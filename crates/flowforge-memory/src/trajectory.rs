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
                if wi.status == "completed" {
                    1.0
                } else if wi.status == "in_progress" {
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

    /// Check if task description matches known successful patterns.
    fn pattern_match_score(&self, task_description: &str) -> Result<f64> {
        let store = PatternStore::new(self.db, self.config);
        let results = store.search_patterns(task_description, 3)?;

        if results.is_empty() {
            return Ok(0.5); // Neutral if no matches
        }

        // Average similarity of top matches that are trajectory patterns
        let trajectory_matches: Vec<f32> = results
            .iter()
            .filter(|(p, _)| p.category == "trajectory")
            .map(|(_, sim)| *sim)
            .collect();

        if trajectory_matches.is_empty() {
            return Ok(0.5);
        }

        let avg: f32 = trajectory_matches.iter().sum::<f32>() / trajectory_matches.len() as f32;
        Ok(avg as f64)
    }
}
