//! Tool success rate tracking per agent for capability-aware routing.

use chrono::{DateTime, Utc};
use rusqlite::params;

use flowforge_core::Result;

use super::{parse_datetime, MemoryDb, SqliteExt};

/// A single tool success metric record.
#[derive(Debug, Clone)]
pub struct ToolMetric {
    pub tool_name: String,
    pub agent_name: String,
    pub success_count: u64,
    pub failure_count: u64,
    pub total_duration_ms: i64,
    pub last_updated: DateTime<Utc>,
}

impl ToolMetric {
    /// Compute success rate as a value in [0.0, 1.0].
    /// Returns 0.0 if there are no uses.
    pub fn success_rate(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            self.success_count as f64 / total as f64
        }
    }

    /// Compute average duration per tool use in milliseconds.
    /// Returns 0.0 if there are no uses.
    pub fn avg_duration_ms(&self) -> f64 {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            0.0
        } else {
            self.total_duration_ms as f64 / total as f64
        }
    }
}

impl MemoryDb {
    /// Record a tool use outcome. `agent_name` should be "" for anonymous/unknown agent.
    pub fn record_tool_metric(
        &self,
        tool_name: &str,
        agent_name: &str,
        success: bool,
        duration_ms: Option<i64>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let dur = duration_ms.unwrap_or(0);

        if success {
            self.conn
                .execute(
                    "INSERT INTO tool_success_metrics (tool_name, agent_name, success_count, failure_count, total_duration_ms, last_updated)
                     VALUES (?1, ?2, 1, 0, ?3, ?4)
                     ON CONFLICT(tool_name, agent_name) DO UPDATE SET
                         success_count = success_count + 1,
                         total_duration_ms = total_duration_ms + ?3,
                         last_updated = ?4",
                    params![tool_name, agent_name, dur, now],
                )
                .sq()?;
        } else {
            self.conn
                .execute(
                    "INSERT INTO tool_success_metrics (tool_name, agent_name, success_count, failure_count, total_duration_ms, last_updated)
                     VALUES (?1, ?2, 0, 1, ?3, ?4)
                     ON CONFLICT(tool_name, agent_name) DO UPDATE SET
                         failure_count = failure_count + 1,
                         total_duration_ms = total_duration_ms + ?3,
                         last_updated = ?4",
                    params![tool_name, agent_name, dur, now],
                )
                .sq()?;
        }

        Ok(())
    }

    /// Get success rate for a specific tool, optionally filtered by agent.
    /// Returns `(success_count, failure_count, success_rate)`.
    pub fn get_tool_success_rate(
        &self,
        tool_name: &str,
        agent_name: Option<&str>,
    ) -> Result<Option<(u64, u64, f64)>> {
        let (sql, result) = if let Some(agent) = agent_name {
            let r = self.conn.query_row(
                "SELECT success_count, failure_count FROM tool_success_metrics
                 WHERE tool_name = ?1 AND agent_name = ?2",
                params![tool_name, agent],
                |row| {
                    let s: u64 = row.get(0)?;
                    let f: u64 = row.get(1)?;
                    Ok((s, f))
                },
            );
            ("per-agent", r)
        } else {
            let r = self.conn.query_row(
                "SELECT COALESCE(SUM(success_count), 0), COALESCE(SUM(failure_count), 0)
                 FROM tool_success_metrics WHERE tool_name = ?1",
                params![tool_name],
                |row| {
                    let s: u64 = row.get(0)?;
                    let f: u64 = row.get(1)?;
                    Ok((s, f))
                },
            );
            ("aggregate", r)
        };

        match result {
            Ok((s, f)) => {
                let total = s + f;
                if total == 0 {
                    Ok(None)
                } else {
                    let rate = s as f64 / total as f64;
                    Ok(Some((s, f, rate)))
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => {
                let _ = sql; // suppress unused warning
                Err(flowforge_core::Error::Database {
                    message: e.to_string(),
                    transient: super::is_transient_sqlite(&e),
                })
            }
        }
    }

    /// Get all tool metrics, optionally filtered by agent.
    pub fn list_tool_metrics(&self, agent_name: Option<&str>) -> Result<Vec<ToolMetric>> {
        let mut metrics = Vec::new();

        if let Some(agent) = agent_name {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT tool_name, agent_name, success_count, failure_count, total_duration_ms, last_updated
                     FROM tool_success_metrics WHERE agent_name = ?1
                     ORDER BY (success_count + failure_count) DESC",
                )
                .sq()?;
            let rows = stmt
                .query_map(params![agent], parse_tool_metric_row)
                .sq()?;
            for row in rows {
                metrics.push(row.sq()?);
            }
        } else {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT tool_name, agent_name, success_count, failure_count, total_duration_ms, last_updated
                     FROM tool_success_metrics
                     ORDER BY (success_count + failure_count) DESC",
                )
                .sq()?;
            let rows = stmt.query_map([], parse_tool_metric_row).sq()?;
            for row in rows {
                metrics.push(row.sq()?);
            }
        }

        Ok(metrics)
    }

    /// Get the best agents for a given tool, sorted by success rate (min 3 uses).
    /// Returns `Vec<(agent_name, success_rate, total_uses)>`.
    pub fn get_best_agents_for_tool(
        &self,
        tool_name: &str,
        limit: usize,
    ) -> Result<Vec<(String, f64, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT agent_name, success_count, failure_count
                 FROM tool_success_metrics
                 WHERE tool_name = ?1 AND agent_name != ''
                   AND (success_count + failure_count) >= 3
                 ORDER BY CAST(success_count AS REAL) / (success_count + failure_count) DESC
                 LIMIT ?2",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![tool_name, limit as i64], |row| {
                let agent: String = row.get(0)?;
                let s: u64 = row.get(1)?;
                let f: u64 = row.get(2)?;
                let total = s + f;
                let rate = if total > 0 {
                    s as f64 / total as f64
                } else {
                    0.0
                };
                Ok((agent, rate, total))
            })
            .sq()?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.sq()?);
        }
        Ok(results)
    }

    /// Get the most problematic tools for an agent (lowest success rate, min 3 uses).
    /// Returns `Vec<(tool_name, success_rate, total_uses)>`.
    pub fn get_weakest_tools(
        &self,
        agent_name: &str,
        limit: usize,
    ) -> Result<Vec<(String, f64, u64)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tool_name, success_count, failure_count
                 FROM tool_success_metrics
                 WHERE agent_name = ?1
                   AND (success_count + failure_count) >= 3
                 ORDER BY CAST(success_count AS REAL) / (success_count + failure_count) ASC
                 LIMIT ?2",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![agent_name, limit as i64], |row| {
                let tool: String = row.get(0)?;
                let s: u64 = row.get(1)?;
                let f: u64 = row.get(2)?;
                let total = s + f;
                let rate = if total > 0 {
                    s as f64 / total as f64
                } else {
                    0.0
                };
                Ok((tool, rate, total))
            })
            .sq()?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.sq()?);
        }
        Ok(results)
    }
}

fn parse_tool_metric_row(row: &rusqlite::Row) -> rusqlite::Result<ToolMetric> {
    Ok(ToolMetric {
        tool_name: row.get(0)?,
        agent_name: row.get(1)?,
        success_count: row.get(2)?,
        failure_count: row.get(3)?,
        total_duration_ms: row.get(4)?,
        last_updated: parse_datetime(row.get(5)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_record_and_get_tool_metric() {
        let db = test_db();
        db.record_tool_metric("Bash", "debugger", true, Some(150))
            .unwrap();

        let rate = db
            .get_tool_success_rate("Bash", Some("debugger"))
            .unwrap();
        assert!(rate.is_some());
        let (s, f, r) = rate.unwrap();
        assert_eq!(s, 1);
        assert_eq!(f, 0);
        assert!((r - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_success_rate_calculation() {
        let db = test_db();
        // 3 successes, 1 failure => 75%
        db.record_tool_metric("Edit", "coder", true, None).unwrap();
        db.record_tool_metric("Edit", "coder", true, None).unwrap();
        db.record_tool_metric("Edit", "coder", true, None).unwrap();
        db.record_tool_metric("Edit", "coder", false, None)
            .unwrap();

        let rate = db.get_tool_success_rate("Edit", Some("coder")).unwrap();
        let (s, f, r) = rate.unwrap();
        assert_eq!(s, 3);
        assert_eq!(f, 1);
        assert!((r - 0.75).abs() < f64::EPSILON);

        // Aggregate (no agent filter) should return the same
        let agg = db.get_tool_success_rate("Edit", None).unwrap();
        let (as_, af, ar) = agg.unwrap();
        assert_eq!(as_, 3);
        assert_eq!(af, 1);
        assert!((ar - 0.75).abs() < f64::EPSILON);
    }

    #[test]
    fn test_best_agents_for_tool() {
        let db = test_db();
        // Agent "ace" has 5/5 = 100% on Bash
        for _ in 0..5 {
            db.record_tool_metric("Bash", "ace", true, None).unwrap();
        }
        // Agent "ok" has 3/5 = 60% on Bash
        for _ in 0..3 {
            db.record_tool_metric("Bash", "ok", true, None).unwrap();
        }
        for _ in 0..2 {
            db.record_tool_metric("Bash", "ok", false, None).unwrap();
        }
        // Agent "newbie" has 1/1 = 100% but below min threshold of 3
        db.record_tool_metric("Bash", "newbie", true, None)
            .unwrap();

        let best = db.get_best_agents_for_tool("Bash", 10).unwrap();
        assert_eq!(best.len(), 2); // "newbie" excluded (< 3 uses)
        assert_eq!(best[0].0, "ace");
        assert!((best[0].1 - 1.0).abs() < f64::EPSILON);
        assert_eq!(best[1].0, "ok");
        assert!((best[1].1 - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn test_weakest_tools() {
        let db = test_db();
        // "coder" is great at Edit (5/5) but bad at Bash (1/4)
        for _ in 0..5 {
            db.record_tool_metric("Edit", "coder", true, None).unwrap();
        }
        db.record_tool_metric("Bash", "coder", true, None).unwrap();
        for _ in 0..3 {
            db.record_tool_metric("Bash", "coder", false, None)
                .unwrap();
        }

        let weak = db.get_weakest_tools("coder", 10).unwrap();
        assert_eq!(weak.len(), 2);
        // Bash (25%) should be first (weakest)
        assert_eq!(weak[0].0, "Bash");
        assert!((weak[0].1 - 0.25).abs() < f64::EPSILON);
        assert_eq!(weak[1].0, "Edit");
        assert!((weak[1].1 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_metric_upsert() {
        let db = test_db();
        // Record twice for the same tool+agent
        db.record_tool_metric("Read", "reviewer", true, Some(100))
            .unwrap();
        db.record_tool_metric("Read", "reviewer", false, Some(200))
            .unwrap();

        let rate = db
            .get_tool_success_rate("Read", Some("reviewer"))
            .unwrap();
        let (s, f, r) = rate.unwrap();
        assert_eq!(s, 1);
        assert_eq!(f, 1);
        assert!((r - 0.5).abs() < f64::EPSILON);

        // Also check that duration accumulated
        let metrics = db.list_tool_metrics(Some("reviewer")).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].total_duration_ms, 300);
        assert!((metrics[0].avg_duration_ms() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_list_tool_metrics_with_agent_filter() {
        let db = test_db();
        db.record_tool_metric("Edit", "coder", true, None).unwrap();
        db.record_tool_metric("Bash", "coder", true, None).unwrap();
        db.record_tool_metric("Edit", "reviewer", true, None)
            .unwrap();

        // Filter by "coder" — should return 2
        let coder_metrics = db.list_tool_metrics(Some("coder")).unwrap();
        assert_eq!(coder_metrics.len(), 2);

        // Filter by "reviewer" — should return 1
        let reviewer_metrics = db.list_tool_metrics(Some("reviewer")).unwrap();
        assert_eq!(reviewer_metrics.len(), 1);

        // No filter — should return all 3
        let all_metrics = db.list_tool_metrics(None).unwrap();
        assert_eq!(all_metrics.len(), 3);
    }

    #[test]
    fn test_get_tool_success_rate_nonexistent() {
        let db = test_db();
        let rate = db
            .get_tool_success_rate("NonexistentTool", None)
            .unwrap();
        assert!(rate.is_none());
    }

    #[test]
    fn test_anonymous_agent() {
        let db = test_db();
        // Empty string agent (anonymous)
        db.record_tool_metric("Bash", "", true, None).unwrap();

        let rate = db.get_tool_success_rate("Bash", Some("")).unwrap();
        assert!(rate.is_some());
        let (s, _, _) = rate.unwrap();
        assert_eq!(s, 1);

        // get_best_agents_for_tool should exclude anonymous agents (agent_name = '')
        let best = db.get_best_agents_for_tool("Bash", 10).unwrap();
        assert!(best.is_empty());
    }
}
