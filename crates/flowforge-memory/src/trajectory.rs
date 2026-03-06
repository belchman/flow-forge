use flowforge_core::config::PatternsConfig;
use flowforge_core::trajectory::{TrajectoryStatus, TrajectoryVerdict};
use flowforge_core::Result;

use crate::db::MemoryDb;
use crate::patterns::PatternStore;

pub struct TrajectoryJudge<'a> {
    db: &'a MemoryDb,
    config: &'a PatternsConfig,
}

/// Result of judging a trajectory.
pub struct JudgmentResult {
    pub verdict: TrajectoryVerdict,
    pub confidence: f64,
    pub reason: String,
}

impl<'a> TrajectoryJudge<'a> {
    pub fn new(db: &'a MemoryDb, config: &'a PatternsConfig) -> Self {
        Self { db, config }
    }

    /// Judge a completed trajectory and persist the verdict.
    pub fn judge(&self, trajectory_id: &str) -> Result<JudgmentResult> {
        let trajectory = self.db.get_trajectory(trajectory_id)?.ok_or_else(|| {
            flowforge_core::Error::NotFound(format!("trajectory {trajectory_id}"))
        })?;

        if trajectory.status == TrajectoryStatus::Judged {
            return Ok(JudgmentResult {
                verdict: trajectory.verdict.unwrap_or(TrajectoryVerdict::Partial),
                confidence: trajectory.confidence.unwrap_or(0.5),
                reason: "already judged".to_string(),
            });
        }

        let steps = self.db.get_trajectory_steps(trajectory_id)?;
        if steps.is_empty() {
            self.db
                .judge_trajectory(trajectory_id, TrajectoryVerdict::Failure, 0.0)?;
            return Ok(JudgmentResult {
                verdict: TrajectoryVerdict::Failure,
                confidence: 0.0,
                reason: "no steps recorded".to_string(),
            });
        }

        // Factor 1: Step success ratio (weight 0.6)
        let success_ratio = self.db.trajectory_success_ratio(trajectory_id)?;

        // Factor 2: Work item outcome (weight 0.3)
        let work_item_factor = if let Some(ref wi_id) = trajectory.work_item_id {
            if let Ok(Some(wi)) = self.db.get_work_item(wi_id) {
                if wi.status == flowforge_core::WorkStatus::Completed {
                    1.0
                } else if wi.status == flowforge_core::WorkStatus::InProgress {
                    0.5
                } else {
                    0.2
                }
            } else {
                0.5 // No work item found, neutral
            }
        } else {
            0.5 // No work item linked, neutral
        };

        // Factor 3: Pattern match bonus (weight 0.1)
        let pattern_factor = if let Some(ref desc) = trajectory.task_description {
            self.pattern_match_score(desc)?
        } else {
            0.5
        };

        let confidence = success_ratio * 0.6 + work_item_factor * 0.3 + pattern_factor * 0.1;

        let verdict = if success_ratio > 0.8 && work_item_factor >= 0.5 {
            TrajectoryVerdict::Success
        } else if success_ratio > 0.5 {
            TrajectoryVerdict::Partial
        } else {
            TrajectoryVerdict::Failure
        };

        let reason = format!(
            "success_ratio={success_ratio:.2}, work_item={work_item_factor:.2}, pattern={pattern_factor:.2}"
        );

        self.db
            .judge_trajectory(trajectory_id, verdict, confidence)?;

        Ok(JudgmentResult {
            verdict,
            confidence,
            reason,
        })
    }

    /// Distill a successful trajectory into a reusable pattern.
    /// Returns the pattern content if one was created.
    pub fn distill(&self, trajectory_id: &str) -> Result<Option<String>> {
        let trajectory = self.db.get_trajectory(trajectory_id)?.ok_or_else(|| {
            flowforge_core::Error::NotFound(format!("trajectory {trajectory_id}"))
        })?;

        // Only distill successful trajectories
        if trajectory.verdict != Some(TrajectoryVerdict::Success) {
            return Ok(None);
        }

        let tool_seq = self.db.trajectory_tool_sequence(trajectory_id)?;
        if tool_seq.is_empty() {
            return Ok(None);
        }

        let desc = trajectory
            .task_description
            .as_deref()
            .unwrap_or("unknown task");

        // Build pattern content: task description + tool sequence
        let seq_str = tool_seq.join(" → ");
        let pattern_content = format!("trajectory:{desc} | {seq_str}");

        // Store as long-lived pattern via PatternStore
        let store = PatternStore::new(self.db, self.config);
        store.store_short_term(&pattern_content, "trajectory")?;

        Ok(Some(pattern_content))
    }

    /// Consolidate trajectories: prune old failures, merge similar successes.
    pub fn consolidate(&self) -> Result<()> {
        // Prune old failed trajectories
        self.db
            .delete_old_failed_trajectories(self.config.trajectory_prune_days)?;

        // Cap total trajectories by deleting oldest judged ones beyond max
        let status_counts = self.db.count_trajectories_by_status()?;
        let total: u64 = status_counts.iter().map(|(_, c)| c).sum();
        if total > self.config.trajectory_max as u64 {
            // Delete excess old failed/partial trajectories first
            let excess = total - self.config.trajectory_max as u64;
            if excess > 0 {
                self.db.delete_old_failed_trajectories(0)?; // Delete all old failed
            }
        }

        Ok(())
    }

    /// Check if task description matches known successful patterns (both tiers).
    fn pattern_match_score(&self, task_description: &str) -> Result<f64> {
        let store = PatternStore::new(self.db, self.config);
        let results = store.search_all_patterns(task_description, 3)?;

        if results.is_empty() {
            return Ok(0.5); // Neutral if no matches
        }

        // Average similarity of top matches that are trajectory patterns
        let trajectory_matches: Vec<f32> = results
            .iter()
            .filter(|m| m.category == "trajectory")
            .map(|m| m.similarity)
            .collect();

        if trajectory_matches.is_empty() {
            return Ok(0.5);
        }

        let avg: f32 = trajectory_matches.iter().sum::<f32>() / trajectory_matches.len() as f32;
        Ok(avg as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use flowforge_core::config::PatternsConfig;
    use flowforge_core::trajectory::{
        StepOutcome, Trajectory, TrajectoryStatus, TrajectoryVerdict,
    };
    use flowforge_core::types::SessionInfo;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    fn setup_session(db: &MemoryDb) {
        db.create_session(&SessionInfo {
            id: "sess-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        })
        .unwrap();
    }

    fn create_test_trajectory(db: &MemoryDb, id: &str) {
        db.create_trajectory(&Trajectory {
            id: id.to_string(),
            session_id: "sess-1".to_string(),
            work_item_id: None,
            agent_name: None,
            task_description: Some("test task".to_string()),
            status: TrajectoryStatus::Recording,
            started_at: Utc::now(),
            ended_at: None,
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        })
        .unwrap();
    }

    #[test]
    fn test_judge_empty_trajectory() {
        let db = test_db();
        setup_session(&db);
        create_test_trajectory(&db, "traj-1");
        db.end_trajectory("traj-1", TrajectoryStatus::Completed)
            .unwrap();

        let config = PatternsConfig::default();
        let judge = TrajectoryJudge::new(&db, &config);
        let result = judge.judge("traj-1").unwrap();
        assert_eq!(result.verdict, TrajectoryVerdict::Failure);
        assert_eq!(result.confidence, 0.0);
        assert!(result.reason.contains("no steps"));
    }

    #[test]
    fn test_judge_all_success_steps() {
        let db = test_db();
        setup_session(&db);
        create_test_trajectory(&db, "traj-1");

        for tool in &["Read", "Edit", "Write", "Grep", "Bash"] {
            db.record_trajectory_step("traj-1", tool, None, StepOutcome::Success, None)
                .unwrap();
        }
        db.end_trajectory("traj-1", TrajectoryStatus::Completed)
            .unwrap();

        let config = PatternsConfig::default();
        let judge = TrajectoryJudge::new(&db, &config);
        let result = judge.judge("traj-1").unwrap();
        assert_eq!(result.verdict, TrajectoryVerdict::Success);
        assert!(result.confidence > 0.5);
    }

    #[test]
    fn test_judge_mixed_steps() {
        let db = test_db();
        setup_session(&db);
        create_test_trajectory(&db, "traj-1");

        // 3 success + 2 failure = 0.6 ratio → Partial (> 0.5 but not > 0.8)
        for _ in 0..3 {
            db.record_trajectory_step("traj-1", "Read", None, StepOutcome::Success, None)
                .unwrap();
        }
        for _ in 0..2 {
            db.record_trajectory_step("traj-1", "Bash", None, StepOutcome::Failure, None)
                .unwrap();
        }
        db.end_trajectory("traj-1", TrajectoryStatus::Completed)
            .unwrap();

        let config = PatternsConfig::default();
        let judge = TrajectoryJudge::new(&db, &config);
        let result = judge.judge("traj-1").unwrap();
        assert_eq!(result.verdict, TrajectoryVerdict::Partial);
    }

    #[test]
    fn test_judge_all_failure_steps() {
        let db = test_db();
        setup_session(&db);
        create_test_trajectory(&db, "traj-1");

        for _ in 0..5 {
            db.record_trajectory_step("traj-1", "Bash", None, StepOutcome::Failure, None)
                .unwrap();
        }
        db.end_trajectory("traj-1", TrajectoryStatus::Completed)
            .unwrap();

        let config = PatternsConfig::default();
        let judge = TrajectoryJudge::new(&db, &config);
        let result = judge.judge("traj-1").unwrap();
        assert_eq!(result.verdict, TrajectoryVerdict::Failure);
    }

    #[test]
    fn test_distill_creates_pattern() {
        let db = test_db();
        setup_session(&db);
        create_test_trajectory(&db, "traj-1");

        for tool in &["Read", "Edit", "Bash"] {
            db.record_trajectory_step("traj-1", tool, None, StepOutcome::Success, None)
                .unwrap();
        }
        db.end_trajectory("traj-1", TrajectoryStatus::Completed)
            .unwrap();

        let config = PatternsConfig::default();
        let judge = TrajectoryJudge::new(&db, &config);

        // Judge first to set verdict to Success
        let result = judge.judge("traj-1").unwrap();
        assert_eq!(result.verdict, TrajectoryVerdict::Success);

        // Now distill
        let pattern = judge.distill("traj-1").unwrap();
        assert!(pattern.is_some());
        let content = pattern.unwrap();
        assert!(content.contains("test task"));
        assert!(content.contains("Read"));
        assert!(content.contains("Edit"));
        assert!(content.contains("Bash"));
    }

    #[test]
    fn test_consolidate_runs() {
        let db = test_db();
        setup_session(&db);

        let config = PatternsConfig::default();
        let judge = TrajectoryJudge::new(&db, &config);
        // Should not error on empty DB
        judge.consolidate().unwrap();
    }
}
