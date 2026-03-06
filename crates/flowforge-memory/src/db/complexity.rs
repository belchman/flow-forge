use rusqlite::params;

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// Estimated complexity for a task based on historical data.
#[derive(Debug, Clone, Default)]
pub struct ComplexityEstimate {
    /// Predicted number of files to be edited
    pub estimated_files: f64,
    /// Predicted number of tool calls
    pub estimated_tool_calls: f64,
    /// Predicted error rate (0.0-1.0)
    pub estimated_error_rate: f64,
    /// Number of similar historical tasks used for estimation
    pub sample_count: u64,
    /// Complexity tier: "simple", "moderate", "complex", "very_complex"
    pub tier: String,
}

impl ComplexityEstimate {
    /// Compute a single complexity score (0.0-1.0).
    pub fn score(&self) -> f64 {
        // Weighted combination of factors
        let file_factor = (self.estimated_files / 10.0).min(1.0);
        let tool_factor = (self.estimated_tool_calls / 50.0).min(1.0);
        let error_factor = self.estimated_error_rate;

        (0.4 * file_factor + 0.4 * tool_factor + 0.2 * error_factor).clamp(0.0, 1.0)
    }

    fn compute_tier(score: f64) -> &'static str {
        if score < 0.2 {
            "simple"
        } else if score < 0.5 {
            "moderate"
        } else if score < 0.8 {
            "complex"
        } else {
            "very_complex"
        }
    }
}

impl MemoryDb {
    /// Estimate task complexity based on historical trajectory data.
    ///
    /// Looks at completed trajectories with similar task patterns to predict:
    /// - Number of files likely to be edited
    /// - Number of tool calls needed
    /// - Error rate probability
    pub fn estimate_complexity(&self, task_keywords: &[&str]) -> Result<ComplexityEstimate> {
        if task_keywords.is_empty() {
            return Ok(ComplexityEstimate {
                tier: "moderate".to_string(),
                ..Default::default()
            });
        }

        // Build LIKE clauses for keyword matching against task descriptions
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for kw in task_keywords {
            param_values.push(Box::new(format!("%{kw}%")));
            conditions.push(format!(
                "LOWER(t.task_description) LIKE LOWER(?{})",
                param_values.len()
            ));
        }

        let where_clause = conditions.join(" OR ");

        // Query: for matching trajectories, get avg tool calls and error rate
        let sql = format!(
            "SELECT
                COUNT(DISTINCT t.id) as trajectory_count,
                AVG(step_counts.total_steps) as avg_steps,
                AVG(step_counts.error_steps * 1.0 / NULLIF(step_counts.total_steps, 0)) as avg_error_rate
             FROM trajectories t
             JOIN (
                SELECT trajectory_id,
                       COUNT(*) as total_steps,
                       SUM(CASE WHEN outcome != 'success' THEN 1 ELSE 0 END) as error_steps
                FROM trajectory_steps
                GROUP BY trajectory_id
             ) step_counts ON step_counts.trajectory_id = t.id
             WHERE t.status IN ('completed', 'judged')
               AND t.task_description IS NOT NULL
               AND ({where_clause})"
        );

        let params_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let (count, avg_steps, avg_error_rate): (u64, f64, f64) = self
            .conn
            .query_row(&sql, params_slice.as_slice(), |row| {
                Ok((
                    row.get::<_, u64>(0).unwrap_or(0),
                    row.get::<_, f64>(1).unwrap_or(0.0),
                    row.get::<_, f64>(2).unwrap_or(0.0),
                ))
            })
            .sq()?;

        // Query: avg distinct files edited in sessions that ran those trajectories
        let files_sql = format!(
            "SELECT AVG(file_count) FROM (
                SELECT COUNT(DISTINCT e.file_path) as file_count
                FROM trajectories t
                JOIN edits e ON e.session_id = t.session_id
                WHERE t.status IN ('completed', 'judged')
                  AND t.task_description IS NOT NULL
                  AND ({where_clause})
                GROUP BY t.id
             )"
        );

        let avg_files: f64 = self
            .conn
            .query_row(&files_sql, params_slice.as_slice(), |row| {
                Ok(row.get::<_, f64>(0).unwrap_or(0.0))
            })
            .unwrap_or(0.0);

        if count == 0 {
            // No historical data — return moderate defaults
            return Ok(ComplexityEstimate {
                estimated_files: 3.0,
                estimated_tool_calls: 15.0,
                estimated_error_rate: 0.1,
                sample_count: 0,
                tier: "moderate".to_string(),
            });
        }

        let mut estimate = ComplexityEstimate {
            estimated_files: avg_files,
            estimated_tool_calls: avg_steps,
            estimated_error_rate: avg_error_rate.clamp(0.0, 1.0),
            sample_count: count,
            tier: String::new(),
        };
        estimate.tier = ComplexityEstimate::compute_tier(estimate.score()).to_string();

        Ok(estimate)
    }

    /// Get complexity stats for a specific agent (how complex are tasks they handle?).
    pub fn get_agent_complexity_profile(
        &self,
        agent_name: &str,
    ) -> Result<ComplexityEstimate> {
        let (count, avg_steps, avg_error_rate): (u64, f64, f64) = self
            .conn
            .query_row(
                "SELECT
                    COUNT(DISTINCT t.id),
                    AVG(step_counts.total_steps),
                    AVG(step_counts.error_steps * 1.0 / NULLIF(step_counts.total_steps, 0))
                 FROM trajectories t
                 JOIN (
                    SELECT trajectory_id,
                           COUNT(*) as total_steps,
                           SUM(CASE WHEN outcome != 'success' THEN 1 ELSE 0 END) as error_steps
                    FROM trajectory_steps
                    GROUP BY trajectory_id
                 ) step_counts ON step_counts.trajectory_id = t.id
                 WHERE t.agent_name = ?1
                   AND t.status IN ('completed', 'judged')",
                params![agent_name],
                |row| {
                    Ok((
                        row.get::<_, u64>(0).unwrap_or(0),
                        row.get::<_, f64>(1).unwrap_or(0.0),
                        row.get::<_, f64>(2).unwrap_or(0.0),
                    ))
                },
            )
            .sq()?;

        let avg_files: f64 = self
            .conn
            .query_row(
                "SELECT AVG(file_count) FROM (
                    SELECT COUNT(DISTINCT e.file_path) as file_count
                    FROM trajectories t
                    JOIN edits e ON e.session_id = t.session_id
                    WHERE t.agent_name = ?1
                      AND t.status IN ('completed', 'judged')
                    GROUP BY t.id
                 )",
                params![agent_name],
                |row| Ok(row.get::<_, f64>(0).unwrap_or(0.0)),
            )
            .unwrap_or(0.0);

        let mut estimate = ComplexityEstimate {
            estimated_files: avg_files,
            estimated_tool_calls: avg_steps,
            estimated_error_rate: avg_error_rate.clamp(0.0, 1.0),
            sample_count: count,
            tier: String::new(),
        };
        estimate.tier = ComplexityEstimate::compute_tier(estimate.score()).to_string();

        Ok(estimate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemoryDb;
    use chrono::Utc;
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};

    fn test_db() -> MemoryDb {
        MemoryDb::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn create_trajectory_with_steps(
        db: &MemoryDb,
        id: &str,
        session_id: &str,
        task_desc: &str,
        agent: &str,
        step_count: usize,
        error_count: usize,
    ) {
        // Create session first
        db.conn
            .execute(
                "INSERT OR IGNORE INTO sessions (id, started_at) VALUES (?1, ?2)",
                params![session_id, Utc::now().to_rfc3339()],
            )
            .unwrap();

        let t = Trajectory {
            id: id.to_string(),
            session_id: session_id.to_string(),
            work_item_id: None,
            agent_name: Some(agent.to_string()),
            task_description: Some(task_desc.to_string()),
            status: TrajectoryStatus::Completed,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&t).unwrap();

        for i in 0..step_count {
            let outcome = if i < error_count {
                StepOutcome::Failure
            } else {
                StepOutcome::Success
            };
            db.record_trajectory_step(id, "Edit", None, outcome, Some(100))
                .unwrap();
        }
    }

    #[test]
    fn test_estimate_complexity_no_data() {
        let db = test_db();
        let est = db.estimate_complexity(&["refactor"]).unwrap();
        // No historical data → moderate defaults
        assert_eq!(est.sample_count, 0);
        assert_eq!(est.tier, "moderate");
        assert!(est.estimated_files > 0.0);
    }

    #[test]
    fn test_estimate_complexity_with_data() {
        let db = test_db();

        create_trajectory_with_steps(&db, "t1", "s1", "refactor the auth module", "coder", 20, 2);
        create_trajectory_with_steps(&db, "t2", "s2", "refactor the database layer", "coder", 30, 5);

        // Add some edits for file count estimation
        for path in &["/src/auth.rs", "/src/db.rs", "/src/main.rs"] {
            db.conn
                .execute(
                    "INSERT INTO edits (session_id, timestamp, file_path, operation) VALUES ('s1', ?1, ?2, 'write')",
                    params![Utc::now().to_rfc3339(), path],
                )
                .unwrap();
        }

        let est = db.estimate_complexity(&["refactor"]).unwrap();
        assert_eq!(est.sample_count, 2);
        assert!(est.estimated_tool_calls > 0.0);
        assert!(est.estimated_error_rate >= 0.0 && est.estimated_error_rate <= 1.0);
        assert!(!est.tier.is_empty());
    }

    #[test]
    fn test_estimate_complexity_empty_keywords() {
        let db = test_db();
        let est = db.estimate_complexity(&[]).unwrap();
        assert_eq!(est.sample_count, 0);
        assert_eq!(est.tier, "moderate");
    }

    #[test]
    fn test_agent_complexity_profile() {
        let db = test_db();

        create_trajectory_with_steps(&db, "t1", "s1", "fix bugs", "tester", 10, 1);
        create_trajectory_with_steps(&db, "t2", "s2", "write tests", "tester", 15, 3);

        let profile = db.get_agent_complexity_profile("tester").unwrap();
        assert_eq!(profile.sample_count, 2);
        assert!(profile.estimated_tool_calls > 0.0);
    }

    #[test]
    fn test_agent_complexity_profile_unknown_agent() {
        let db = test_db();
        let profile = db.get_agent_complexity_profile("unknown").unwrap();
        assert_eq!(profile.sample_count, 0);
    }

    #[test]
    fn test_complexity_score_ranges() {
        let simple = ComplexityEstimate {
            estimated_files: 1.0,
            estimated_tool_calls: 5.0,
            estimated_error_rate: 0.0,
            sample_count: 10,
            tier: "simple".to_string(),
        };
        assert!(simple.score() < 0.2);

        let complex = ComplexityEstimate {
            estimated_files: 8.0,
            estimated_tool_calls: 40.0,
            estimated_error_rate: 0.3,
            sample_count: 10,
            tier: "complex".to_string(),
        };
        assert!(complex.score() > 0.5);
    }

    #[test]
    fn test_complexity_tier() {
        assert_eq!(ComplexityEstimate::compute_tier(0.1), "simple");
        assert_eq!(ComplexityEstimate::compute_tier(0.3), "moderate");
        assert_eq!(ComplexityEstimate::compute_tier(0.6), "complex");
        assert_eq!(ComplexityEstimate::compute_tier(0.9), "very_complex");
    }
}
