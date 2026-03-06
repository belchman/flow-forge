//! Recovery strategies: intelligent suggestions for guidance gate denials/asks.

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// A recovery strategy suggesting an alternative when a guidance gate blocks an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryStrategy {
    pub id: i64,
    pub gate_name: String,
    pub trigger_pattern: String,
    pub suggestion: String,
    pub alternative_command: Option<String>,
    pub success_count: u64,
    pub failure_count: u64,
}

impl RecoveryStrategy {
    /// Confidence score: success / (success + failure), or 0.5 if no data.
    pub fn confidence(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.5
        } else {
            self.success_count as f64 / total as f64
        }
    }
}

impl MemoryDb {
    /// Find recovery strategies matching a gate name and trigger substring.
    /// If `trigger` is non-empty, only strategies whose `trigger_pattern` appears
    /// in the trigger text (case-insensitive) are returned.
    /// Results are ordered by confidence (success_count / total) descending.
    pub fn get_recovery_strategies(
        &self,
        gate_name: &str,
        trigger: &str,
    ) -> Result<Vec<RecoveryStrategy>> {
        let trigger_lower = trigger.to_lowercase();
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, gate_name, trigger_pattern, suggestion, alternative_command,
                        success_count, failure_count
                 FROM recovery_strategies
                 WHERE gate_name = ?1
                 ORDER BY CAST(success_count AS REAL) / MAX(success_count + failure_count, 1) DESC",
            )
            .sq()?;

        let rows = stmt
            .query_map(params![gate_name], |row| {
                Ok(RecoveryStrategy {
                    id: row.get(0)?,
                    gate_name: row.get(1)?,
                    trigger_pattern: row.get(2)?,
                    suggestion: row.get(3)?,
                    alternative_command: row.get(4)?,
                    success_count: row.get::<_, i64>(5).unwrap_or(0) as u64,
                    failure_count: row.get::<_, i64>(6).unwrap_or(0) as u64,
                })
            })
            .sq()?;

        let strategies: Vec<RecoveryStrategy> = rows
            .filter_map(|r| r.ok())
            .filter(|s| {
                if trigger_lower.is_empty() {
                    true
                } else {
                    trigger_lower.contains(&s.trigger_pattern.to_lowercase())
                }
            })
            .collect();

        Ok(strategies)
    }

    /// Upsert a recovery strategy. If (gate_name, trigger_pattern, suggestion) already
    /// exists, updates the alternative_command and last_used timestamp.
    pub fn record_recovery_strategy(
        &self,
        gate_name: &str,
        trigger: &str,
        suggestion: &str,
        alt_command: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO recovery_strategies
                 (gate_name, trigger_pattern, suggestion, alternative_command,
                  success_count, failure_count, created_at, last_used)
                 VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)
                 ON CONFLICT(gate_name, trigger_pattern, suggestion)
                 DO UPDATE SET
                    alternative_command = COALESCE(?4, alternative_command),
                    last_used = ?5",
                params![gate_name, trigger, suggestion, alt_command, now],
            )
            .sq()?;

        Ok(())
    }

    /// Increment success or failure count for a recovery strategy.
    pub fn update_recovery_outcome(&self, id: i64, success: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        if success {
            self.conn
                .execute(
                    "UPDATE recovery_strategies
                     SET success_count = success_count + 1, last_used = ?1
                     WHERE id = ?2",
                    params![now, id],
                )
                .sq()?;
        } else {
            self.conn
                .execute(
                    "UPDATE recovery_strategies
                     SET failure_count = failure_count + 1, last_used = ?1
                     WHERE id = ?2",
                    params![now, id],
                )
                .sq()?;
        }

        Ok(())
    }

    /// List all recovery strategies, optionally filtered by gate_name.
    pub fn list_recovery_strategies(
        &self,
        gate_name: Option<&str>,
    ) -> Result<Vec<RecoveryStrategy>> {
        if let Some(gate) = gate_name {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, gate_name, trigger_pattern, suggestion, alternative_command,
                            success_count, failure_count
                     FROM recovery_strategies
                     WHERE gate_name = ?1
                     ORDER BY CAST(success_count AS REAL) / MAX(success_count + failure_count, 1) DESC",
                )
                .sq()?;

            let rows = stmt
                .query_map(params![gate], |row| {
                    Ok(RecoveryStrategy {
                        id: row.get(0)?,
                        gate_name: row.get(1)?,
                        trigger_pattern: row.get(2)?,
                        suggestion: row.get(3)?,
                        alternative_command: row.get(4)?,
                        success_count: row.get::<_, i64>(5).unwrap_or(0) as u64,
                        failure_count: row.get::<_, i64>(6).unwrap_or(0) as u64,
                    })
                })
                .sq()?;
            let strategies: Vec<RecoveryStrategy> = rows.filter_map(|r| r.ok()).collect();
            Ok(strategies)
        } else {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT id, gate_name, trigger_pattern, suggestion, alternative_command,
                            success_count, failure_count
                     FROM recovery_strategies
                     ORDER BY gate_name, CAST(success_count AS REAL) / MAX(success_count + failure_count, 1) DESC",
                )
                .sq()?;

            let rows = stmt
                .query_map([], |row| {
                    Ok(RecoveryStrategy {
                        id: row.get(0)?,
                        gate_name: row.get(1)?,
                        trigger_pattern: row.get(2)?,
                        suggestion: row.get(3)?,
                        alternative_command: row.get(4)?,
                        success_count: row.get::<_, i64>(5).unwrap_or(0) as u64,
                        failure_count: row.get::<_, i64>(6).unwrap_or(0) as u64,
                    })
                })
                .sq()?;
            let strategies: Vec<RecoveryStrategy> = rows.filter_map(|r| r.ok()).collect();
            Ok(strategies)
        }
    }
}

/// Seed default recovery strategies into the database.
/// Uses INSERT OR IGNORE so this is idempotent.
pub fn seed_default_strategies(db: &MemoryDb) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    let defaults = [
        (
            "destructive_ops",
            "rm -rf",
            "Consider moving files to .trash/ first, or use git stash to preserve changes",
            Some("mv <target> .trash/"),
        ),
        (
            "destructive_ops",
            "git reset --hard",
            "Use git stash instead to preserve changes, or create a backup branch first",
            Some("git stash"),
        ),
        (
            "destructive_ops",
            "git push --force",
            "Use --force-with-lease instead for safer force push",
            Some("git push --force-with-lease"),
        ),
        (
            "destructive_ops",
            "drop table",
            "Consider renaming the table first, or create a backup",
            Some("ALTER TABLE <name> RENAME TO <name>_backup"),
        ),
        (
            "secrets_detection",
            "secret",
            "Use environment variables or .env files instead of hardcoding secrets",
            None,
        ),
    ];

    for (gate, trigger, suggestion, alt_cmd) in &defaults {
        db.conn
            .execute(
                "INSERT OR IGNORE INTO recovery_strategies
                 (gate_name, trigger_pattern, suggestion, alternative_command,
                  success_count, failure_count, created_at, last_used)
                 VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)",
                params![gate, trigger, suggestion, alt_cmd, now],
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
    fn test_seed_default_strategies() {
        let db = test_db();
        seed_default_strategies(&db).unwrap();

        let strategies = db.list_recovery_strategies(None).unwrap();
        assert_eq!(strategies.len(), 5);

        // Verify destructive_ops strategies
        let destructive = db
            .list_recovery_strategies(Some("destructive_ops"))
            .unwrap();
        assert_eq!(destructive.len(), 4);

        // Verify secrets_detection strategies
        let secrets = db
            .list_recovery_strategies(Some("secrets_detection"))
            .unwrap();
        assert_eq!(secrets.len(), 1);

        // Seeding again should be idempotent
        seed_default_strategies(&db).unwrap();
        let strategies2 = db.list_recovery_strategies(None).unwrap();
        assert_eq!(strategies2.len(), 5);
    }

    #[test]
    fn test_get_recovery_strategies_by_gate() {
        let db = test_db();
        seed_default_strategies(&db).unwrap();

        // Get strategies for destructive_ops gate
        let strategies = db
            .get_recovery_strategies("destructive_ops", "rm -rf /important")
            .unwrap();
        assert!(!strategies.is_empty());
        assert!(strategies
            .iter()
            .any(|s| s.trigger_pattern == "rm -rf"));

        // Get strategies for a trigger that doesn't match
        let strategies = db
            .get_recovery_strategies("destructive_ops", "echo hello")
            .unwrap();
        assert!(strategies.is_empty());

        // Get strategies for secrets_detection
        let strategies = db
            .get_recovery_strategies("secrets_detection", "contains a secret key")
            .unwrap();
        assert_eq!(strategies.len(), 1);
        assert!(strategies[0].suggestion.contains("environment variables"));
    }

    #[test]
    fn test_record_recovery_strategy_upserts() {
        let db = test_db();

        // DB open seeds 5 default strategies. Count baseline.
        let baseline = db.list_recovery_strategies(None).unwrap().len();

        // Record a new strategy (unique gate+trigger+suggestion combo)
        db.record_recovery_strategy(
            "destructive_ops",
            "chmod 777",
            "Use more restrictive permissions like 755",
            Some("chmod 755"),
        )
        .unwrap();

        let strategies = db.list_recovery_strategies(None).unwrap();
        assert_eq!(strategies.len(), baseline + 1);

        // Find the newly added one
        let new_strat = strategies
            .iter()
            .find(|s| s.trigger_pattern == "chmod 777")
            .expect("should find new strategy");
        assert_eq!(new_strat.success_count, 0);
        assert_eq!(new_strat.failure_count, 0);

        // Record the same strategy again (should upsert, not duplicate)
        db.record_recovery_strategy(
            "destructive_ops",
            "chmod 777",
            "Use more restrictive permissions like 755",
            Some("chmod 700"),
        )
        .unwrap();

        let strategies2 = db.list_recovery_strategies(None).unwrap();
        assert_eq!(strategies2.len(), baseline + 1); // Same count — no duplicate

        // Alternative command should be updated
        let updated = strategies2
            .iter()
            .find(|s| s.trigger_pattern == "chmod 777")
            .expect("should still find strategy");
        assert_eq!(
            updated.alternative_command.as_deref(),
            Some("chmod 700")
        );
    }

    #[test]
    fn test_update_recovery_outcome() {
        let db = test_db();

        db.record_recovery_strategy(
            "destructive_ops",
            "rm -rf",
            "Move to trash instead",
            None,
        )
        .unwrap();

        let strategies = db.list_recovery_strategies(None).unwrap();
        let id = strategies[0].id;

        // Record successes
        db.update_recovery_outcome(id, true).unwrap();
        db.update_recovery_outcome(id, true).unwrap();
        db.update_recovery_outcome(id, true).unwrap();

        // Record a failure
        db.update_recovery_outcome(id, false).unwrap();

        let strategies = db.list_recovery_strategies(None).unwrap();
        assert_eq!(strategies[0].success_count, 3);
        assert_eq!(strategies[0].failure_count, 1);

        // Confidence should be 3/4 = 0.75
        let confidence = strategies[0].confidence();
        assert!((confidence - 0.75).abs() < 0.001);
    }

    #[test]
    fn test_recovery_strategies_integrated_in_deny_message() {
        let db = test_db();
        seed_default_strategies(&db).unwrap();

        // Simulate what pre_tool_use does: look up strategies for a deny reason
        let reason = "[destructive_ops] Dangerous command: rm -rf detected";
        let strategies = db
            .get_recovery_strategies("destructive_ops", reason)
            .unwrap();

        assert!(!strategies.is_empty());

        // Format suggestions like the hook would
        let suggestions: Vec<String> = strategies
            .iter()
            .map(|s| {
                if let Some(ref alt) = s.alternative_command {
                    format!("{} (try: {})", s.suggestion, alt)
                } else {
                    s.suggestion.clone()
                }
            })
            .collect();

        let formatted = format!(
            "{}\n\nSuggested alternatives:\n{}",
            reason,
            suggestions
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        assert!(formatted.contains("Suggested alternatives:"));
        assert!(formatted.contains("Consider moving files to .trash/"));
    }
}
