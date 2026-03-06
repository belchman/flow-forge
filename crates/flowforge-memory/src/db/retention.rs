use chrono::Utc;
use rusqlite::params;

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    /// Prune data older than `retention_days` from append-only tables.
    /// Tables pruned: gate_decisions, work_events, edits, pattern_effectiveness,
    /// conversation_messages, session_tool_failures, and routing_hit:* meta keys.
    /// Returns the total number of rows deleted.
    ///
    /// Note: tool_success_metrics is NOT pruned here because it uses upserts
    /// (one row per tool+agent pair), not append-only inserts. The table stays
    /// bounded by the number of distinct (tool_name, agent_name) combinations.
    ///
    /// Pass `retention_days = 0` to skip pruning entirely.
    pub fn prune_old_data(&self, retention_days: u64) -> Result<u64> {
        if retention_days == 0 {
            return Ok(0);
        }

        let threshold = Utc::now() - chrono::Duration::days(retention_days as i64);
        let threshold_str = threshold.to_rfc3339();
        let mut total = 0u64;

        // gate_decisions — audit trail, high volume
        let count = self
            .conn
            .execute(
                "DELETE FROM gate_decisions WHERE timestamp < ?1",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // work_events — event log for completed work items
        let count = self
            .conn
            .execute(
                "DELETE FROM work_events WHERE timestamp < ?1
                 AND work_item_id IN (SELECT id FROM work_items WHERE status = 'completed')",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // edits — file edit records (session-id cascades handle some, but orphaned edits remain)
        let count = self
            .conn
            .execute(
                "DELETE FROM edits WHERE timestamp < ?1",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // pattern_effectiveness — feedback records
        let count = self
            .conn
            .execute(
                "DELETE FROM pattern_effectiveness WHERE timestamp < ?1",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // conversation_messages — delete orphaned messages (session_id not in sessions table)
        let count = self
            .conn
            .execute(
                "DELETE FROM conversation_messages
                 WHERE session_id NOT IN (SELECT id FROM sessions)",
                [],
            )
            .sq()?;
        total += count as u64;

        // conversation_messages — delete old messages from ended sessions
        let count = self
            .conn
            .execute(
                "DELETE FROM conversation_messages WHERE timestamp < ?1",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // conversation_messages — cap at 1000 most recent rows
        let count = self
            .conn
            .execute(
                "DELETE FROM conversation_messages WHERE rowid NOT IN (
                     SELECT rowid FROM conversation_messages ORDER BY timestamp DESC LIMIT 1000
                 )",
                [],
            )
            .sq()?;
        total += count as u64;

        // session_tool_failures — per-session failure tracking for loop detection
        let count = self
            .conn
            .execute(
                "DELETE FROM session_tool_failures WHERE timestamp < ?1",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // routing_hit:* meta keys — routing accuracy tracking
        let count = self
            .conn
            .execute(
                "DELETE FROM flowforge_meta WHERE key LIKE 'routing_hit:%'",
                [],
            )
            .sq()?;
        total += count as u64;

        // routing_outcomes — adaptive weight tuning data
        let count = self
            .conn
            .execute(
                "DELETE FROM routing_outcomes WHERE timestamp < ?1",
                params![threshold_str],
            )
            .sq()?;
        total += count as u64;

        // discovered_capabilities — stale entries not seen recently
        let count = self
            .conn
            .execute(
                "DELETE FROM discovered_capabilities WHERE last_seen < ?1",
                params![threshold_str],
            )
            .unwrap_or(0);
        total += count as u64;

        // recovery_strategies — low-usage strategies with stale last_used
        let count = self
            .conn
            .execute(
                "DELETE FROM recovery_strategies
                 WHERE last_used < ?1
                 AND (success_count + failure_count) < 3",
                params![threshold_str],
            )
            .unwrap_or(0);
        total += count as u64;

        if total > 0 {
            tracing::info!(
                retention_days,
                rows_pruned = total,
                "pruned old data from append-only tables"
            );

            // VACUUM to reclaim disk space after significant cleanup
            if total > 1000 {
                let _ = self.conn.execute_batch("VACUUM");
                tracing::info!("vacuumed database after large prune");
            }
        }

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_prune_zero_days_is_noop() {
        let db = test_db();
        let result = db.prune_old_data(0).unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_prune_removes_old_gate_decisions() {
        let db = test_db();

        // Create a session for FK constraints
        let session = flowforge_core::SessionInfo {
            id: "sess-prune-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();

        // Insert an old gate decision (200 days ago)
        let old_ts = (Utc::now() - chrono::Duration::days(200)).to_rfc3339();
        db.conn
            .execute(
                "INSERT INTO gate_decisions (session_id, gate_name, tool_name, action, reason, risk_level, timestamp, hash)
                 VALUES ('sess-prune-1', 'test', 'Bash', 'Allow', 'test', 'low', ?1, 'abc')",
                params![old_ts],
            )
            .unwrap();

        // Insert a recent gate decision (1 day ago)
        let recent_ts = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        db.conn
            .execute(
                "INSERT INTO gate_decisions (session_id, gate_name, tool_name, action, reason, risk_level, timestamp, hash)
                 VALUES ('sess-prune-1', 'test', 'Bash', 'Allow', 'test', 'low', ?1, 'def')",
                params![recent_ts],
            )
            .unwrap();

        let pruned = db.prune_old_data(90).unwrap();
        assert!(pruned >= 1, "should prune old gate decision");

        // Verify recent one survives
        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM gate_decisions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_prune_removes_old_edits() {
        let db = test_db();

        let old_ts = (Utc::now() - chrono::Duration::days(100)).to_rfc3339();
        db.conn
            .execute(
                "INSERT INTO edits (timestamp, file_path, operation) VALUES (?1, '/tmp/foo.rs', 'write')",
                params![old_ts],
            )
            .unwrap();

        let recent_ts = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        db.conn
            .execute(
                "INSERT INTO edits (timestamp, file_path, operation) VALUES (?1, '/tmp/bar.rs', 'write')",
                params![recent_ts],
            )
            .unwrap();

        let pruned = db.prune_old_data(90).unwrap();
        assert!(pruned >= 1);

        let count: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM edits", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_prune_removes_routing_hit_meta_keys() {
        let db = test_db();

        db.set_meta("routing_hit:sess-1", "1").unwrap();
        db.set_meta("routing_hit:sess-2", "0").unwrap();
        db.set_meta("other_key", "keep").unwrap();

        let pruned = db.prune_old_data(90).unwrap();
        assert!(pruned >= 2);

        // routing_hit:* keys should be gone
        assert!(db.get_meta("routing_hit:sess-1").unwrap().is_none());
        assert!(db.get_meta("routing_hit:sess-2").unwrap().is_none());
        // other keys should survive
        assert_eq!(db.get_meta("other_key").unwrap(), Some("keep".to_string()));
    }

    #[test]
    fn test_prune_no_data_returns_zero() {
        let db = test_db();
        let pruned = db.prune_old_data(90).unwrap();
        assert_eq!(pruned, 0);
    }
}
