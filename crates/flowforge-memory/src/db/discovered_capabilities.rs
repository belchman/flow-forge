use chrono::Utc;
use rusqlite::params;

use flowforge_core::{DiscoveredCapability, Result};

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    /// Record a discovered capability (upserts by agent_name + task_pattern).
    /// On success, increments success_count; on failure, increments failure_count.
    /// Confidence is recomputed as success_count / (success_count + failure_count).
    pub fn record_discovered_capability(
        &self,
        agent_name: &str,
        task_pattern: &str,
        success: bool,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();

        if success {
            self.conn
                .execute(
                    "INSERT INTO discovered_capabilities
                        (agent_name, capability, task_pattern, success_count, failure_count, confidence, last_seen, created_at)
                     VALUES (?1, ?2, ?2, 1, 0, 1.0, ?3, ?3)
                     ON CONFLICT(agent_name, task_pattern) DO UPDATE SET
                        success_count = success_count + 1,
                        confidence = CAST((success_count + 1) AS REAL) / CAST((success_count + 1 + failure_count) AS REAL),
                        last_seen = ?3",
                    params![agent_name, task_pattern, now],
                )
                .sq()?;
        } else {
            self.conn
                .execute(
                    "INSERT INTO discovered_capabilities
                        (agent_name, capability, task_pattern, success_count, failure_count, confidence, last_seen, created_at)
                     VALUES (?1, ?2, ?2, 0, 1, 0.0, ?3, ?3)
                     ON CONFLICT(agent_name, task_pattern) DO UPDATE SET
                        failure_count = failure_count + 1,
                        confidence = CAST(success_count AS REAL) / CAST((success_count + failure_count + 1) AS REAL),
                        last_seen = ?3",
                    params![agent_name, task_pattern, now],
                )
                .sq()?;
        }

        Ok(())
    }

    /// Get discovered capabilities for an agent, sorted by confidence descending.
    pub fn get_discovered_capabilities(
        &self,
        agent_name: &str,
    ) -> Result<Vec<DiscoveredCapability>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT agent_name, capability, task_pattern, success_count, failure_count, confidence
                 FROM discovered_capabilities
                 WHERE agent_name = ?1
                 ORDER BY confidence DESC, (success_count + failure_count) DESC",
            )
            .sq()?;

        let rows = stmt
            .query_map(params![agent_name], |row| {
                Ok(DiscoveredCapability {
                    agent_name: row.get(0)?,
                    capability: row.get(1)?,
                    task_pattern: row.get(2)?,
                    success_count: row.get::<_, i64>(3)? as u64,
                    failure_count: row.get::<_, i64>(4)? as u64,
                    confidence: row.get(5)?,
                })
            })
            .sq()?;

        let mut caps = Vec::new();
        for row in rows {
            caps.push(row.sq()?);
        }
        Ok(caps)
    }

    /// Get top agents for a task pattern (by confidence), requiring at least 3 total uses.
    /// Returns (agent_name, confidence) pairs sorted by confidence descending.
    pub fn get_top_agents_for_pattern(
        &self,
        task_pattern: &str,
    ) -> Result<Vec<(String, f64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT agent_name, confidence
                 FROM discovered_capabilities
                 WHERE task_pattern = ?1
                   AND (success_count + failure_count) >= 3
                 ORDER BY confidence DESC",
            )
            .sq()?;

        let rows = stmt
            .query_map(params![task_pattern], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .sq()?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.sq()?);
        }
        Ok(results)
    }

    /// Get all discovered capabilities keyed by agent name, for routing context.
    /// Returns a map of agent_name -> Vec<(task_pattern, confidence)>.
    pub fn get_all_discovered_capabilities(
        &self,
    ) -> Result<std::collections::HashMap<String, Vec<(String, f64)>>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT agent_name, task_pattern, confidence
                 FROM discovered_capabilities
                 WHERE (success_count + failure_count) >= 2
                 ORDER BY confidence DESC",
            )
            .sq()?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .sq()?;

        let mut map: std::collections::HashMap<String, Vec<(String, f64)>> =
            std::collections::HashMap::new();
        for row in rows {
            let (agent, pattern, confidence) = row.sq()?;
            map.entry(agent).or_default().push((pattern, confidence));
        }
        Ok(map)
    }

    /// Prune capabilities with low confidence (< 0.3) and few total uses (< 5).
    /// Returns the number of rows deleted.
    pub fn prune_weak_capabilities(&self) -> Result<u32> {
        let count = self
            .conn
            .execute(
                "DELETE FROM discovered_capabilities
                 WHERE confidence < 0.3 AND (success_count + failure_count) < 5",
                [],
            )
            .sq()?;
        Ok(count as u32)
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
    fn test_record_discovered_capability_creates_entry() {
        let db = test_db();
        db.record_discovered_capability("rust-expert", "fix compile error", true)
            .unwrap();

        let caps = db.get_discovered_capabilities("rust-expert").unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].agent_name, "rust-expert");
        assert_eq!(caps[0].task_pattern, "fix compile error");
        assert_eq!(caps[0].success_count, 1);
        assert_eq!(caps[0].failure_count, 0);
        assert!((caps[0].confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_record_discovered_capability_updates_on_success() {
        let db = test_db();
        db.record_discovered_capability("rust-expert", "fix compile error", true)
            .unwrap();
        db.record_discovered_capability("rust-expert", "fix compile error", true)
            .unwrap();
        db.record_discovered_capability("rust-expert", "fix compile error", true)
            .unwrap();

        let caps = db.get_discovered_capabilities("rust-expert").unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].success_count, 3);
        assert_eq!(caps[0].failure_count, 0);
        assert!((caps[0].confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_record_discovered_capability_updates_on_failure() {
        let db = test_db();
        // Start with a success
        db.record_discovered_capability("agent-a", "deploy service", true)
            .unwrap();
        // Then record a failure
        db.record_discovered_capability("agent-a", "deploy service", false)
            .unwrap();

        let caps = db.get_discovered_capabilities("agent-a").unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].success_count, 1);
        assert_eq!(caps[0].failure_count, 1);
        // confidence = 1 / (1 + 1) = 0.5
        assert!((caps[0].confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_discovered_capabilities_returns_sorted() {
        let db = test_db();
        // High confidence: 3 success, 0 failure
        db.record_discovered_capability("agent-a", "pattern-high", true)
            .unwrap();
        db.record_discovered_capability("agent-a", "pattern-high", true)
            .unwrap();
        db.record_discovered_capability("agent-a", "pattern-high", true)
            .unwrap();

        // Low confidence: 1 success, 2 failure
        db.record_discovered_capability("agent-a", "pattern-low", true)
            .unwrap();
        db.record_discovered_capability("agent-a", "pattern-low", false)
            .unwrap();
        db.record_discovered_capability("agent-a", "pattern-low", false)
            .unwrap();

        let caps = db.get_discovered_capabilities("agent-a").unwrap();
        assert_eq!(caps.len(), 2);
        // Sorted by confidence DESC
        assert!(caps[0].confidence > caps[1].confidence);
        assert_eq!(caps[0].task_pattern, "pattern-high");
        assert_eq!(caps[1].task_pattern, "pattern-low");
    }

    #[test]
    fn test_get_top_agents_for_pattern_min_uses() {
        let db = test_db();
        // Agent with 3 uses (meets threshold)
        db.record_discovered_capability("agent-a", "fix bug", true)
            .unwrap();
        db.record_discovered_capability("agent-a", "fix bug", true)
            .unwrap();
        db.record_discovered_capability("agent-a", "fix bug", true)
            .unwrap();

        // Agent with 2 uses (below threshold)
        db.record_discovered_capability("agent-b", "fix bug", true)
            .unwrap();
        db.record_discovered_capability("agent-b", "fix bug", true)
            .unwrap();

        let top = db.get_top_agents_for_pattern("fix bug").unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, "agent-a");
    }

    #[test]
    fn test_prune_weak_capabilities() {
        let db = test_db();
        // Weak: 1 success, 2 failures = confidence 0.33, total 3 (< 5)
        db.record_discovered_capability("agent-a", "weak-pattern", true)
            .unwrap();
        db.record_discovered_capability("agent-a", "weak-pattern", false)
            .unwrap();
        db.record_discovered_capability("agent-a", "weak-pattern", false)
            .unwrap();

        // Strong: 3 successes, 0 failures = confidence 1.0
        db.record_discovered_capability("agent-b", "strong-pattern", true)
            .unwrap();
        db.record_discovered_capability("agent-b", "strong-pattern", true)
            .unwrap();
        db.record_discovered_capability("agent-b", "strong-pattern", true)
            .unwrap();

        // Verify weak cap confidence < 0.3 threshold
        let _caps = db.get_discovered_capabilities("agent-a").unwrap();
        // confidence = 1/3 = 0.333... which is > 0.3, so need to make it lower
        // Let's add more failures
        db.record_discovered_capability("agent-a", "weak-pattern", false)
            .unwrap();
        // Now: 1 success, 3 failures = 0.25 confidence, 4 total uses (< 5)

        let pruned = db.prune_weak_capabilities().unwrap();
        assert_eq!(pruned, 1);

        // Strong pattern should survive
        let caps = db.get_discovered_capabilities("agent-b").unwrap();
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn test_discovered_capability_confidence_formula() {
        let db = test_db();
        // Record 3 successes and 1 failure
        db.record_discovered_capability("agent-x", "test pattern", true)
            .unwrap();
        db.record_discovered_capability("agent-x", "test pattern", true)
            .unwrap();
        db.record_discovered_capability("agent-x", "test pattern", true)
            .unwrap();
        db.record_discovered_capability("agent-x", "test pattern", false)
            .unwrap();

        let caps = db.get_discovered_capabilities("agent-x").unwrap();
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].success_count, 3);
        assert_eq!(caps[0].failure_count, 1);
        // confidence = 3 / (3 + 1) = 0.75
        assert!((caps[0].confidence - 0.75).abs() < f64::EPSILON);
    }
}
