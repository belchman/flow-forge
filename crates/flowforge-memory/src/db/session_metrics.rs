use std::collections::HashMap;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

impl MemoryDb {
    /// Increment a session metric by a given delta.
    /// Uses INSERT ON CONFLICT to atomically upsert.
    pub fn increment_session_metric(
        &self,
        session_id: &str,
        metric_name: &str,
        delta: f64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO session_metrics (session_id, metric_name, metric_value, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(session_id, metric_name) DO UPDATE SET
                     metric_value = metric_value + ?3,
                     updated_at = ?4",
                params![session_id, metric_name, delta, now],
            )
            .sq()?;
        Ok(())
    }

    /// Get a specific session metric value.
    pub fn get_session_metric(
        &self,
        session_id: &str,
        metric_name: &str,
    ) -> Result<Option<f64>> {
        self.conn
            .query_row(
                "SELECT metric_value FROM session_metrics
                 WHERE session_id = ?1 AND metric_name = ?2",
                params![session_id, metric_name],
                |row| row.get(0),
            )
            .optional()
            .sq()
    }

    /// Get all metrics for a session.
    pub fn get_session_metrics(&self, session_id: &str) -> Result<HashMap<String, f64>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT metric_name, metric_value FROM session_metrics
                 WHERE session_id = ?1",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .sq()?;
        let mut map = HashMap::new();
        for row in rows {
            let (name, value) = row.sq()?;
            map.insert(name, value);
        }
        Ok(map)
    }

    /// Get aggregate metrics across all sessions.
    pub fn get_global_metrics(&self) -> Result<HashMap<String, f64>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT metric_name, SUM(metric_value) FROM session_metrics
                 GROUP BY metric_name",
            )
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .sq()?;
        let mut map = HashMap::new();
        for row in rows {
            let (name, value) = row.sq()?;
            map.insert(name, value);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_increment_metric() {
        let db = test_db();
        db.increment_session_metric("sess-1", "tool_calls", 1.0)
            .unwrap();
        let val = db.get_session_metric("sess-1", "tool_calls").unwrap();
        assert_eq!(val, Some(1.0));
    }

    #[test]
    fn test_get_nonexistent_metric() {
        let db = test_db();
        let val = db
            .get_session_metric("sess-none", "tool_calls")
            .unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_get_all_session_metrics() {
        let db = test_db();
        db.increment_session_metric("sess-2", "tool_calls", 5.0)
            .unwrap();
        db.increment_session_metric("sess-2", "errors_count", 2.0)
            .unwrap();
        db.increment_session_metric("sess-2", "tool_input_bytes", 1024.0)
            .unwrap();

        let metrics = db.get_session_metrics("sess-2").unwrap();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics["tool_calls"], 5.0);
        assert_eq!(metrics["errors_count"], 2.0);
        assert_eq!(metrics["tool_input_bytes"], 1024.0);
    }

    #[test]
    fn test_multiple_increments_accumulate() {
        let db = test_db();
        db.increment_session_metric("sess-3", "tool_calls", 1.0)
            .unwrap();
        db.increment_session_metric("sess-3", "tool_calls", 1.0)
            .unwrap();
        db.increment_session_metric("sess-3", "tool_calls", 3.0)
            .unwrap();

        let val = db.get_session_metric("sess-3", "tool_calls").unwrap();
        assert_eq!(val, Some(5.0));
    }

    #[test]
    fn test_global_metrics_aggregation() {
        let db = test_db();
        // Session A
        db.increment_session_metric("sess-a", "tool_calls", 10.0)
            .unwrap();
        db.increment_session_metric("sess-a", "errors_count", 2.0)
            .unwrap();
        // Session B
        db.increment_session_metric("sess-b", "tool_calls", 7.0)
            .unwrap();
        db.increment_session_metric("sess-b", "errors_count", 3.0)
            .unwrap();
        db.increment_session_metric("sess-b", "tool_output_bytes", 512.0)
            .unwrap();

        let global = db.get_global_metrics().unwrap();
        assert_eq!(global["tool_calls"], 17.0);
        assert_eq!(global["errors_count"], 5.0);
        assert_eq!(global["tool_output_bytes"], 512.0);
    }
}
