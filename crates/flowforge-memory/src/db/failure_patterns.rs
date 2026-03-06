//! Failure pattern detection and prevention.
//!
//! Detects when an agent is about to repeat a known failure pattern
//! (based on historical trajectory data) and injects a warning to prevent it.

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// A failure pattern detected from historical trajectories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailurePattern {
    pub id: i64,
    pub pattern_name: String,
    pub description: String,
    /// Comma-separated tool names that form the trigger sequence.
    pub trigger_tools: String,
    pub prevention_hint: String,
    pub occurrence_count: u64,
    pub prevented_count: u64,
}

impl MemoryDb {
    /// Given the last N tool calls in the current trajectory, check if any match
    /// known failure patterns. Uses the trigger_tools sequence as a suffix match.
    pub fn check_failure_pattern(&self, recent_tools: &[&str]) -> Result<Vec<FailurePattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, pattern_name, description, trigger_tools, prevention_hint,
                        occurrence_count, prevented_count
                 FROM failure_patterns
                 ORDER BY occurrence_count DESC",
            )
            .sq()?;

        let rows = stmt
            .query_map([], |row| {
                Ok(FailurePattern {
                    id: row.get(0)?,
                    pattern_name: row.get(1)?,
                    description: row.get(2)?,
                    trigger_tools: row.get(3)?,
                    prevention_hint: row.get(4)?,
                    occurrence_count: row.get::<_, i64>(5).unwrap_or(0) as u64,
                    prevented_count: row.get::<_, i64>(6).unwrap_or(0) as u64,
                })
            })
            .sq()?;

        let patterns: Vec<FailurePattern> = rows.filter_map(|r| r.ok()).collect();

        // Suffix-match: check if the recent tool sequence ends with the trigger sequence
        let recent_joined = recent_tools.join(",");
        let matched: Vec<FailurePattern> = patterns
            .into_iter()
            .filter(|p| {
                let trigger = &p.trigger_tools;
                // Single tool trigger: check if the last tool matches
                if !trigger.contains(',') {
                    recent_tools.last().map(|t| *t == trigger).unwrap_or(false)
                } else {
                    // Multi-tool trigger: suffix match on comma-joined sequence
                    recent_joined.ends_with(trigger.as_str())
                }
            })
            .collect();

        Ok(matched)
    }

    /// Upsert a failure pattern. If pattern_name already exists, updates counts.
    pub fn record_failure_pattern(
        &self,
        name: &str,
        description: &str,
        trigger_tools: &str,
        hint: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO failure_patterns
                 (pattern_name, description, trigger_tools, prevention_hint,
                  occurrence_count, prevented_count, created_at, last_triggered)
                 VALUES (?1, ?2, ?3, ?4, 1, 0, ?5, ?5)
                 ON CONFLICT(pattern_name)
                 DO UPDATE SET
                    occurrence_count = occurrence_count + 1,
                    last_triggered = ?5,
                    description = COALESCE(?2, description),
                    trigger_tools = COALESCE(?3, trigger_tools),
                    prevention_hint = COALESCE(?4, prevention_hint)",
                params![name, description, trigger_tools, hint, now],
            )
            .sq()?;

        Ok(())
    }

    /// Increment the prevented count when a warning averted a failure.
    pub fn increment_pattern_prevented(&self, id: i64) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        self.conn
            .execute(
                "UPDATE failure_patterns
                 SET prevented_count = prevented_count + 1, last_triggered = ?1
                 WHERE id = ?2",
                params![now, id],
            )
            .sq()?;

        Ok(())
    }

    /// Analyze failed trajectories to find common tool sequences that precede failures.
    /// Returns (tool_sequence, count) pairs sorted by frequency descending.
    pub fn mine_failure_patterns(&self, min_occurrences: u32) -> Result<Vec<(String, u32)>> {
        // Get all failed/judged-failure trajectories
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.id FROM trajectories t
                 WHERE t.status IN ('failed', 'judged')
                   AND (t.verdict IS NULL OR t.verdict = 'failure')",
            )
            .sq()?;

        let trajectory_ids: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        // Collect the last 3 tools from each failed trajectory as a sequence
        let mut sequence_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        for tid in &trajectory_ids {
            let mut step_stmt = self
                .conn
                .prepare(
                    "SELECT tool_name FROM trajectory_steps
                     WHERE trajectory_id = ?1
                     ORDER BY step_index DESC
                     LIMIT 3",
                )
                .sq()?;

            let tools: Vec<String> = step_stmt
                .query_map(params![tid], |row| row.get::<_, String>(0))
                .sq()?
                .filter_map(|r| r.ok())
                .collect();

            if tools.is_empty() {
                continue;
            }

            // Reverse to get chronological order (we fetched DESC)
            let mut tools_ordered = tools;
            tools_ordered.reverse();

            // Record subsequences of length 1..=3
            for len in 1..=tools_ordered.len() {
                let start = tools_ordered.len() - len;
                let seq = tools_ordered[start..].join(",");
                *sequence_counts.entry(seq).or_insert(0) += 1;
            }
        }

        // Filter by min_occurrences and sort by count descending
        let mut results: Vec<(String, u32)> = sequence_counts
            .into_iter()
            .filter(|(_, count)| *count >= min_occurrences)
            .collect();

        results.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(results)
    }

    /// List all failure patterns.
    pub fn list_failure_patterns(&self) -> Result<Vec<FailurePattern>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, pattern_name, description, trigger_tools, prevention_hint,
                        occurrence_count, prevented_count
                 FROM failure_patterns
                 ORDER BY occurrence_count DESC",
            )
            .sq()?;

        let rows = stmt
            .query_map([], |row| {
                Ok(FailurePattern {
                    id: row.get(0)?,
                    pattern_name: row.get(1)?,
                    description: row.get(2)?,
                    trigger_tools: row.get(3)?,
                    prevention_hint: row.get(4)?,
                    occurrence_count: row.get::<_, i64>(5).unwrap_or(0) as u64,
                    prevented_count: row.get::<_, i64>(6).unwrap_or(0) as u64,
                })
            })
            .sq()?;

        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }
}

/// Seed default failure patterns into the database.
/// Uses INSERT OR IGNORE so this is idempotent.
/// Also removes obsolete single-tool patterns that caused log flooding.
pub fn seed_default_failure_patterns(db: &MemoryDb) -> Result<()> {
    // Remove obsolete single-tool patterns that fire on every Bash/Edit call
    db.conn
        .execute(
            "DELETE FROM failure_patterns WHERE pattern_name IN ('force_push_without_backup', 'delete_without_backup')",
            [],
        )
        .sq()?;
    // Update edit_without_read from single-tool "Edit" to multi-tool "Bash,Edit"
    db.conn
        .execute(
            "UPDATE failure_patterns SET trigger_tools = 'Bash,Edit', occurrence_count = 0 WHERE pattern_name = 'edit_without_read' AND trigger_tools = 'Edit'",
            [],
        )
        .sq()?;

    let now = Utc::now().to_rfc3339();

    let defaults = [
        (
            "edit_without_read",
            "Editing a file without reading it first",
            "Bash,Edit",
            "Read the file before editing to understand the current state",
        ),
        (
            "repeated_bash_failure",
            "Running the same bash command after it already failed",
            "Bash,Bash,Bash",
            "Try a different approach or fix the underlying issue before retrying",
        ),
        (
            "edit_then_edit_no_test",
            "Multiple edits without running tests",
            "Edit,Edit,Edit",
            "Run tests between edit cycles to catch regressions early",
        ),
    ];

    for (name, description, trigger_tools, hint) in &defaults {
        db.conn
            .execute(
                "INSERT OR IGNORE INTO failure_patterns
                 (pattern_name, description, trigger_tools, prevention_hint,
                  occurrence_count, prevented_count, created_at)
                 VALUES (?1, ?2, ?3, ?4, 0, 0, ?5)",
                params![name, description, trigger_tools, hint, now],
            )
            .sq()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_seed_defaults() {
        let db = test_db();
        // seed_default_failure_patterns is called during schema init,
        // but call it again to verify idempotency
        seed_default_failure_patterns(&db).unwrap();

        let patterns = db.list_failure_patterns().unwrap();
        assert_eq!(patterns.len(), 3);

        // Verify specific patterns exist (multi-tool triggers, not single-tool)
        assert!(patterns.iter().any(|p| p.pattern_name == "edit_without_read"
            && p.trigger_tools == "Bash,Edit"));
        assert!(patterns
            .iter()
            .any(|p| p.pattern_name == "repeated_bash_failure"
                && p.trigger_tools == "Bash,Bash,Bash"));
        assert!(patterns
            .iter()
            .any(|p| p.pattern_name == "edit_then_edit_no_test"
                && p.trigger_tools == "Edit,Edit,Edit"));

        // Seeding again should be idempotent
        seed_default_failure_patterns(&db).unwrap();
        let patterns2 = db.list_failure_patterns().unwrap();
        assert_eq!(patterns2.len(), 3);
    }

    #[test]
    fn test_check_failure_pattern_match() {
        let db = test_db();

        // The default "edit_without_read" pattern triggers on "Bash,Edit" suffix
        let recent = vec!["Read", "Bash", "Edit"];
        let matches = db.check_failure_pattern(&recent).unwrap();
        assert!(
            matches.iter().any(|m| m.pattern_name == "edit_without_read"),
            "Should match edit_without_read when Bash,Edit is the suffix"
        );
    }

    #[test]
    fn test_check_failure_pattern_no_match() {
        let db = test_db();

        // "Read,Edit" should NOT match "Bash,Edit" trigger
        let recent = vec!["Read", "Edit"];
        let matches = db.check_failure_pattern(&recent).unwrap();
        assert!(
            !matches.iter().any(|m| m.pattern_name == "edit_without_read"),
            "Should not match when sequence is Read,Edit not Bash,Edit"
        );

        // "Read,Glob" should not match any default pattern
        let recent2 = vec!["Read", "Glob"];
        let matches2 = db.check_failure_pattern(&recent2).unwrap();
        assert!(matches2.is_empty(), "Read,Glob should match no patterns");
    }

    #[test]
    fn test_record_and_increment() {
        let db = test_db();

        // Record a new pattern
        db.record_failure_pattern(
            "test_pattern",
            "A test failure pattern",
            "Bash,Edit",
            "Don't do that",
        )
        .unwrap();

        let patterns = db.list_failure_patterns().unwrap();
        let p = patterns
            .iter()
            .find(|p| p.pattern_name == "test_pattern")
            .expect("should find test_pattern");
        assert_eq!(p.occurrence_count, 1);
        assert_eq!(p.prevented_count, 0);

        // Record again (upsert) — should increment occurrence_count
        db.record_failure_pattern(
            "test_pattern",
            "A test failure pattern",
            "Bash,Edit",
            "Don't do that",
        )
        .unwrap();

        let patterns = db.list_failure_patterns().unwrap();
        let p = patterns
            .iter()
            .find(|p| p.pattern_name == "test_pattern")
            .expect("should find test_pattern");
        assert_eq!(p.occurrence_count, 2);

        // Increment prevented count
        db.increment_pattern_prevented(p.id).unwrap();

        let patterns = db.list_failure_patterns().unwrap();
        let p = patterns
            .iter()
            .find(|p| p.pattern_name == "test_pattern")
            .expect("should find test_pattern");
        assert_eq!(p.prevented_count, 1);
    }

    #[test]
    fn test_mine_failure_patterns() {
        let db = test_db();

        // Create a session for FK compliance
        db.conn
            .execute(
                "INSERT INTO sessions (id, started_at) VALUES ('sess-1', datetime('now'))",
                [],
            )
            .unwrap();

        // Create two failed trajectories with the same tool sequence
        for i in 0..3 {
            let tid = format!("traj-{i}");
            db.conn
                .execute(
                    "INSERT INTO trajectories (id, session_id, status, verdict, started_at)
                     VALUES (?1, 'sess-1', 'failed', 'failure', datetime('now'))",
                    params![tid],
                )
                .unwrap();

            // Each trajectory: Bash -> Edit -> Bash (ending in failure)
            for (idx, tool) in ["Bash", "Edit", "Bash"].iter().enumerate() {
                db.conn
                    .execute(
                        "INSERT INTO trajectory_steps (trajectory_id, step_index, tool_name, outcome, timestamp)
                         VALUES (?1, ?2, ?3, 'failure', datetime('now'))",
                        params![tid, idx as i64, tool],
                    )
                    .unwrap();
            }
        }

        let mined = db.mine_failure_patterns(2).unwrap();
        assert!(
            !mined.is_empty(),
            "Should find patterns from failed trajectories"
        );

        // The sequence "Bash" should appear at least 3 times (3 trajectories * last tool)
        assert!(
            mined.iter().any(|(seq, count)| seq == "Bash" && *count >= 3),
            "Should find 'Bash' as a common failure tool. Found: {:?}",
            mined
        );

        // With high min_occurrences, should return empty
        let mined_high = db.mine_failure_patterns(100).unwrap();
        assert!(mined_high.is_empty());
    }

    #[test]
    fn test_check_multi_tool_trigger() {
        let db = test_db();

        // Record a multi-tool trigger pattern
        db.record_failure_pattern(
            "bash_then_edit",
            "Running bash then editing without read",
            "Bash,Edit",
            "Read first",
        )
        .unwrap();

        // Should match when recent tools end with "Bash,Edit"
        let recent = vec!["Read", "Bash", "Edit"];
        let matches = db.check_failure_pattern(&recent).unwrap();
        assert!(
            matches.iter().any(|m| m.pattern_name == "bash_then_edit"),
            "Should match multi-tool trigger"
        );

        // Should NOT match when sequence doesn't end with "Bash,Edit"
        let recent2 = vec!["Bash", "Edit", "Read"];
        let matches2 = db.check_failure_pattern(&recent2).unwrap();
        assert!(
            !matches2.iter().any(|m| m.pattern_name == "bash_then_edit"),
            "Should not match when sequence doesn't end with trigger"
        );
    }
}
