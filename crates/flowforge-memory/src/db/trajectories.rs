use chrono::Utc;
use rusqlite::params;

use flowforge_core::{
    trajectory::{StepOutcome, Trajectory, TrajectoryStatus, TrajectoryStep, TrajectoryVerdict},
    Result,
};

use super::row_parsers::parse_trajectory_row;
use super::{parse_datetime, MemoryDb, SqliteExt};

use rusqlite::OptionalExtension;

impl MemoryDb {
    // ── Trajectories ──

    pub fn create_trajectory(&self, trajectory: &Trajectory) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO trajectories
                 (id, session_id, work_item_id, agent_name, task_description, status,
                  started_at, ended_at, verdict, confidence, metadata, embedding_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    trajectory.id,
                    trajectory.session_id,
                    trajectory.work_item_id,
                    trajectory.agent_name,
                    trajectory.task_description,
                    trajectory.status.to_string(),
                    trajectory.started_at.to_rfc3339(),
                    trajectory.ended_at.map(|t| t.to_rfc3339()),
                    trajectory.verdict.map(|v| v.to_string()),
                    trajectory.confidence,
                    trajectory.metadata,
                    trajectory.embedding_id,
                ],
            )
            .sq()?;
        Ok(())
    }

    pub fn get_trajectory(&self, id: &str) -> Result<Option<Trajectory>> {
        self.conn
            .query_row(
                "SELECT id, session_id, work_item_id, agent_name, task_description, status,
                        started_at, ended_at, verdict, confidence, metadata, embedding_id
                 FROM trajectories WHERE id = ?1",
                params![id],
                |row| Ok(parse_trajectory_row(row)),
            )
            .optional()
            .sq()
    }

    pub fn get_active_trajectory(&self, session_id: &str) -> Result<Option<Trajectory>> {
        self.conn
            .query_row(
                "SELECT id, session_id, work_item_id, agent_name, task_description, status,
                        started_at, ended_at, verdict, confidence, metadata, embedding_id
                 FROM trajectories WHERE session_id = ?1 AND status = 'recording'
                 ORDER BY started_at DESC LIMIT 1",
                params![session_id],
                |row| Ok(parse_trajectory_row(row)),
            )
            .optional()
            .sq()
    }

    pub fn end_trajectory(&self, id: &str, status: TrajectoryStatus) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE trajectories SET ended_at = ?1, status = ?2 WHERE id = ?3",
                params![now, status.to_string(), id],
            )
            .sq()?;
        Ok(())
    }

    pub fn judge_trajectory(
        &self,
        id: &str,
        verdict: TrajectoryVerdict,
        confidence: f64,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "UPDATE trajectories SET status = 'judged', verdict = ?1, confidence = ?2, ended_at = COALESCE(ended_at, ?3)
                 WHERE id = ?4",
                params![verdict.to_string(), confidence, now, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn list_trajectories(
        &self,
        session_id: Option<&str>,
        status: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Trajectory>> {
        let mut sql = String::from(
            "SELECT id, session_id, work_item_id, agent_name, task_description, status,
                    started_at, ended_at, verdict, confidence, metadata, embedding_id
             FROM trajectories WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(sid) = session_id {
            param_values.push(Box::new(sid.to_string()));
            sql.push_str(&format!(" AND session_id = ?{}", param_values.len()));
        }
        if let Some(st) = status {
            param_values.push(Box::new(st.to_string()));
            sql.push_str(&format!(" AND status = ?{}", param_values.len()));
        }
        param_values.push(Box::new(limit as i64));
        sql.push_str(&format!(
            " ORDER BY started_at DESC LIMIT ?{}",
            param_values.len()
        ));

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let params_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(params_slice.as_slice(), |row| Ok(parse_trajectory_row(row)))
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn set_trajectory_task_description(&self, id: &str, task_description: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE trajectories SET task_description = ?1 WHERE id = ?2",
                params![task_description, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn set_trajectory_agent_name(&self, id: &str, agent_name: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE trajectories SET agent_name = ?1 WHERE id = ?2",
                params![agent_name, id],
            )
            .sq()?;
        Ok(())
    }

    pub fn link_trajectory_work_item(&self, trajectory_id: &str, work_item_id: &str) -> Result<()> {
        self.conn
            .execute(
                "UPDATE trajectories SET work_item_id = ?1 WHERE id = ?2",
                params![work_item_id, trajectory_id],
            )
            .sq()?;
        Ok(())
    }

    // ── Trajectory Steps ──

    pub fn record_trajectory_step(
        &self,
        trajectory_id: &str,
        tool_name: &str,
        tool_input_hash: Option<&str>,
        outcome: StepOutcome,
        duration_ms: Option<i64>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        self.conn
            .execute(
                "INSERT INTO trajectory_steps
                 (trajectory_id, step_index, tool_name, tool_input_hash, outcome, duration_ms, timestamp)
                 VALUES (?1, (SELECT COALESCE(MAX(step_index), -1) + 1 FROM trajectory_steps WHERE trajectory_id = ?1),
                         ?2, ?3, ?4, ?5, ?6)",
                params![
                    trajectory_id,
                    tool_name,
                    tool_input_hash,
                    outcome.to_string(),
                    duration_ms,
                    now,
                ],
            )
            .sq()?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_trajectory_steps(&self, trajectory_id: &str) -> Result<Vec<TrajectoryStep>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, trajectory_id, step_index, tool_name, tool_input_hash, outcome, duration_ms, timestamp
                 FROM trajectory_steps WHERE trajectory_id = ?1 ORDER BY step_index ASC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![trajectory_id], |row| {
                Ok(TrajectoryStep {
                    id: row.get(0)?,
                    trajectory_id: row.get(1)?,
                    step_index: row.get(2)?,
                    tool_name: row.get(3)?,
                    tool_input_hash: row.get(4)?,
                    outcome: row
                        .get::<_, String>(5)?
                        .parse()
                        .unwrap_or(StepOutcome::Success),
                    duration_ms: row.get(6)?,
                    timestamp: parse_datetime(row.get::<_, String>(7)?),
                })
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    pub fn trajectory_success_ratio(&self, trajectory_id: &str) -> Result<f64> {
        let (total, successes): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), SUM(CASE WHEN outcome = 'success' THEN 1 ELSE 0 END)
                 FROM trajectory_steps WHERE trajectory_id = ?1",
                params![trajectory_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .sq()?;
        if total == 0 {
            return Ok(0.0);
        }
        Ok(successes as f64 / total as f64)
    }

    /// Get the last N tool names from a trajectory, in chronological order.
    /// Efficient: only fetches tool_name column with LIMIT.
    pub fn get_recent_trajectory_tools(
        &self,
        trajectory_id: &str,
        limit: usize,
    ) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tool_name FROM (
                     SELECT tool_name, step_index
                     FROM trajectory_steps
                     WHERE trajectory_id = ?1
                     ORDER BY step_index DESC
                     LIMIT ?2
                 ) sub ORDER BY step_index ASC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![trajectory_id, limit as i64], |row| row.get(0))
            .sq()?;
        rows.collect::<std::result::Result<Vec<String>, _>>().sq()
    }

    pub fn trajectory_tool_sequence(&self, trajectory_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT tool_name FROM trajectory_steps WHERE trajectory_id = ?1 ORDER BY step_index ASC",
            )
            .sq()?;
        let rows = stmt
            .query_map(params![trajectory_id], |row| row.get(0))
            .sq()?;
        rows.collect::<std::result::Result<Vec<String>, _>>().sq()
    }

    pub fn delete_old_failed_trajectories(&self, older_than_days: u64) -> Result<u32> {
        let threshold = Utc::now() - chrono::Duration::days(older_than_days as i64);
        // Steps are cascade-deleted via FK
        let count = self
            .conn
            .execute(
                "DELETE FROM trajectories WHERE status = 'failed' AND started_at < ?1",
                params![threshold.to_rfc3339()],
            )
            .sq()?;
        Ok(count as u32)
    }

    pub fn count_trajectories_by_status(&self) -> Result<Vec<(String, u64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT status, COUNT(*) FROM trajectories GROUP BY status")
            .sq()?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }
    /// Find similar trajectories using vector search, with LIKE fallback.
    pub fn find_similar_trajectories_semantic(
        &self,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<flowforge_core::trajectory::TrajectoryInsight>> {
        use flowforge_core::trajectory::TrajectoryInsight;

        let results = self.search_vectors(query_vec, &["trajectory"], k)?;

        let mut output = Vec::new();
        for result in &results {
            if result.similarity < 0.3 {
                continue;
            }
            if let Some(t) = self.get_trajectory(&result.source_id)? {
                let step_count = self.get_trajectory_steps(&t.id)?.len() as u64;
                let success_rate = self.trajectory_success_ratio(&t.id).unwrap_or(0.0);
                output.push(TrajectoryInsight {
                    task_description: t.task_description.unwrap_or_default(),
                    agent_name: t.agent_name,
                    verdict: t.verdict.map(|v| format!("{v}")),
                    confidence: t.confidence.unwrap_or(0.0),
                    total_steps: step_count,
                    success_rate,
                });
            }
        }
        Ok(output)
    }

    /// Build a trajectory summary string for embedding.
    pub fn build_trajectory_summary(&self, trajectory_id: &str) -> Result<Option<String>> {
        let t = match self.get_trajectory(trajectory_id)? {
            Some(t) => t,
            None => return Ok(None),
        };

        let task_desc = t.task_description.as_deref().unwrap_or("unknown task");
        let tools = self.trajectory_tool_sequence(trajectory_id)?;
        let mut unique_tools: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for tool in tools {
            if seen.insert(tool.clone()) {
                unique_tools.push(tool);
            }
        }
        let verdict_str = t
            .verdict
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let step_count = self.get_trajectory_steps(trajectory_id)?.len();

        Ok(Some(format!(
            "{} | tools: {} | verdict: {} | {} steps",
            task_desc,
            unique_tools.join(", "),
            verdict_str,
            step_count
        )))
    }

    /// Find trajectories with similar task descriptions for cross-session knowledge transfer.
    pub fn find_similar_trajectories(
        &self,
        keywords: &[&str],
        limit: usize,
    ) -> Result<Vec<flowforge_core::trajectory::TrajectoryInsight>> {
        use flowforge_core::trajectory::TrajectoryInsight;

        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        // Build a LIKE query for keyword matching
        let conditions: Vec<String> = keywords
            .iter()
            .map(|kw| format!("t.task_description LIKE '%{}%'", kw.replace('\'', "''")))
            .collect();
        let where_clause = conditions.join(" OR ");

        let sql = format!(
            "SELECT t.task_description, t.agent_name, t.verdict, t.confidence,
                    (SELECT COUNT(*) FROM trajectory_steps ts WHERE ts.trajectory_id = t.id) as step_count,
                    CASE WHEN t.verdict = 'success' THEN 1.0 ELSE 0.0 END as success_rate
             FROM trajectories t
             WHERE t.task_description IS NOT NULL
               AND t.status IN ('completed', 'judged')
               AND ({where_clause})
             ORDER BY t.ended_at DESC
             LIMIT ?1"
        );

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let rows = stmt
            .query_map(params![limit], |row| {
                Ok(TrajectoryInsight {
                    task_description: row.get(0)?,
                    agent_name: row.get(1)?,
                    verdict: row.get(2)?,
                    confidence: row.get::<_, f64>(3).unwrap_or(0.0),
                    total_steps: row.get::<_, i64>(4).unwrap_or(0) as u64,
                    success_rate: row.get::<_, f64>(5).unwrap_or(0.0),
                })
            })
            .sq()?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.sq()?);
        }
        Ok(results)
    }

    /// Predict which files will likely need editing for a task, based on files edited
    /// in past successful trajectories with similar task descriptions.
    /// Returns (file_path, session_count) sorted by frequency.
    pub fn predict_task_files(
        &self,
        keywords: &[String],
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        let conditions: Vec<String> = keywords
            .iter()
            .enumerate()
            .map(|(i, _)| format!("LOWER(t.task_description) LIKE ?{}", i + 1))
            .collect();
        let where_clause = conditions.join(" OR ");

        let sql = format!(
            "SELECT e.file_path, COUNT(DISTINCT e.session_id) as session_count
             FROM edits e
             JOIN trajectories t ON t.session_id = e.session_id
             WHERE t.task_description IS NOT NULL
               AND t.verdict = 'success'
               AND ({})
             GROUP BY e.file_path
             ORDER BY session_count DESC
             LIMIT ?{}",
            where_clause,
            keywords.len() + 1
        );

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let like_params: Vec<String> = keywords.iter().map(|k| format!("%{}%", k)).collect();
        let mut params_vec: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
        for p in &like_params {
            params_vec.push(p as &dyn rusqlite::types::ToSql);
        }
        let limit_val = limit as i64;
        params_vec.push(&limit_val);

        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })
            .sq()?;
        rows.collect::<std::result::Result<Vec<_>, _>>().sq()
    }

    /// Get the tool sequence from the best matching successful trajectory.
    /// Returns the condensed sequence (e.g., ["Read", "Edit", "Bash"]) for strategy injection.
    pub fn get_winning_sequence(
        &self,
        keywords: &[String],
    ) -> Result<Option<Vec<String>>> {
        if keywords.is_empty() {
            return Ok(None);
        }

        let conditions: Vec<String> = keywords
            .iter()
            .enumerate()
            .map(|(i, _)| format!("LOWER(t.task_description) LIKE ?{}", i + 1))
            .collect();
        let where_clause = conditions.join(" OR ");

        // Find the best successful trajectory (highest confidence) matching keywords
        let sql = format!(
            "SELECT t.id FROM trajectories t
             WHERE t.task_description IS NOT NULL
               AND t.verdict = 'success'
               AND ({})
             ORDER BY t.confidence DESC, t.ended_at DESC
             LIMIT 1",
            where_clause
        );

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let like_params: Vec<String> = keywords.iter().map(|k| format!("%{}%", k)).collect();
        let mut params_vec: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
        for p in &like_params {
            params_vec.push(p as &dyn rusqlite::types::ToSql);
        }

        let traj_id: Option<String> = stmt
            .query_map(rusqlite::params_from_iter(params_vec), |row| {
                row.get::<_, String>(0)
            })
            .sq()?
            .next()
            .and_then(|r| r.ok());

        match traj_id {
            Some(id) => {
                let tools = self.trajectory_tool_sequence(&id)?;
                if tools.is_empty() {
                    return Ok(None);
                }
                // Condense consecutive duplicates
                let mut condensed: Vec<String> = Vec::new();
                for tool in &tools {
                    if condensed.last().map_or(true, |last| last != tool) {
                        condensed.push(tool.clone());
                    }
                }
                // Deduplicate while preserving order (keep unique tools only)
                let mut seen = std::collections::HashSet::new();
                condensed.retain(|t| seen.insert(t.clone()));
                condensed.truncate(10);
                Ok(Some(condensed))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    fn setup_session_and_trajectory(db: &MemoryDb) -> (String, String) {
        let session = flowforge_core::SessionInfo {
            id: "sess-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();

        let traj = Trajectory {
            id: "traj-1".to_string(),
            session_id: "sess-1".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: Some("Test task".to_string()),
            status: TrajectoryStatus::Recording,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();
        ("sess-1".to_string(), "traj-1".to_string())
    }

    #[test]
    fn test_get_recent_trajectory_tools_empty() {
        let db = test_db();
        let (_sid, tid) = setup_session_and_trajectory(&db);
        let tools = db.get_recent_trajectory_tools(&tid, 5).unwrap();
        assert!(tools.is_empty());
    }

    #[test]
    fn test_get_recent_trajectory_tools_returns_chronological() {
        let db = test_db();
        let (_sid, tid) = setup_session_and_trajectory(&db);

        // Record 7 steps
        for (i, tool) in ["Read", "Glob", "Edit", "Bash", "Write", "Grep", "Read"]
            .iter()
            .enumerate()
        {
            db.record_trajectory_step(&tid, tool, None, StepOutcome::Success, None)
                .unwrap();
            // Ensure distinct step_index by using the auto-increment
            let _ = i;
        }

        // Get last 5 — should be Edit, Bash, Write, Grep, Read (chronological)
        let recent = db.get_recent_trajectory_tools(&tid, 5).unwrap();
        assert_eq!(recent.len(), 5);
        assert_eq!(recent, vec!["Edit", "Bash", "Write", "Grep", "Read"]);
    }

    #[test]
    fn test_get_recent_trajectory_tools_limit_larger_than_steps() {
        let db = test_db();
        let (_sid, tid) = setup_session_and_trajectory(&db);

        db.record_trajectory_step(&tid, "Read", None, StepOutcome::Success, None)
            .unwrap();
        db.record_trajectory_step(&tid, "Edit", None, StepOutcome::Success, None)
            .unwrap();

        let recent = db.get_recent_trajectory_tools(&tid, 10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent, vec!["Read", "Edit"]);
    }

    #[test]
    fn test_predict_task_files() {
        let db = test_db();

        // Create a session with a successful trajectory about "routing"
        let session = flowforge_core::SessionInfo {
            id: "sess-pred-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();

        let traj = Trajectory {
            id: "traj-pred-1".to_string(),
            session_id: "sess-pred-1".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: Some("improve the routing system for agents".to_string()),
            status: TrajectoryStatus::Completed,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            verdict: Some(flowforge_core::trajectory::TrajectoryVerdict::Success),
            confidence: Some(0.8),
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();

        // Record edits in that session
        let edit = flowforge_core::EditRecord {
            session_id: "sess-pred-1".to_string(),
            timestamp: Utc::now(),
            file_path: "src/routing.rs".to_string(),
            operation: "write".to_string(),
            file_extension: Some("rs".to_string()),
        };
        db.record_edit(&edit).unwrap();
        let edit2 = flowforge_core::EditRecord {
            session_id: "sess-pred-1".to_string(),
            timestamp: Utc::now(),
            file_path: "src/agents.rs".to_string(),
            operation: "write".to_string(),
            file_extension: Some("rs".to_string()),
        };
        db.record_edit(&edit2).unwrap();

        // Predict files for a "routing" task
        let keywords = vec!["routing".to_string()];
        let predicted = db.predict_task_files(&keywords, 5).unwrap();
        assert!(!predicted.is_empty(), "should predict files from similar sessions");
        let file_paths: Vec<&str> = predicted.iter().map(|(f, _)| f.as_str()).collect();
        assert!(file_paths.contains(&"src/routing.rs"));
        assert!(file_paths.contains(&"src/agents.rs"));
    }

    #[test]
    fn test_predict_task_files_empty_keywords() {
        let db = test_db();
        let result = db.predict_task_files(&[], 5).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_get_winning_sequence() {
        let db = test_db();

        // Create session + trajectory
        let session = flowforge_core::SessionInfo {
            id: "sess-win-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();

        let traj = Trajectory {
            id: "traj-win-1".to_string(),
            session_id: "sess-win-1".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: Some("fix the authentication module".to_string()),
            status: TrajectoryStatus::Completed,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            verdict: Some(flowforge_core::trajectory::TrajectoryVerdict::Success),
            confidence: Some(0.9),
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&traj).unwrap();

        // Record tool sequence: Read, Read, Edit, Bash, Edit, Bash
        for tool in &["Read", "Read", "Edit", "Bash", "Edit", "Bash"] {
            db.record_trajectory_step("traj-win-1", tool, None, StepOutcome::Success, None)
                .unwrap();
        }

        // Get winning sequence for "authentication" task
        let keywords = vec!["authentication".to_string()];
        let seq = db.get_winning_sequence(&keywords).unwrap();
        assert!(seq.is_some(), "should find a matching sequence");
        let tools = seq.unwrap();
        // Consecutive dupes removed, then deduped: Read, Edit, Bash
        assert_eq!(tools, vec!["Read", "Edit", "Bash"]);
    }

    #[test]
    fn test_get_winning_sequence_no_match() {
        let db = test_db();
        let keywords = vec!["nonexistent".to_string()];
        let result = db.get_winning_sequence(&keywords).unwrap();
        assert!(result.is_none());
    }
}
