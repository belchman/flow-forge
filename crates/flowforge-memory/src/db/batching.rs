//! Tool call batching analysis: detect sequential calls that could be parallelized.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// A detected batching opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchingOpportunity {
    /// The tool name that was called sequentially
    pub tool_name: String,
    /// Number of consecutive calls detected
    pub consecutive_count: u32,
    /// Number of times this pattern was observed
    pub occurrence_count: u64,
}

impl MemoryDb {
    /// Analyze trajectory steps to detect sequential same-tool calls that could be batched.
    /// Returns tools where consecutive runs of >= `min_consecutive` calls were observed.
    pub fn detect_batching_opportunities(
        &self,
        session_id: &str,
        min_consecutive: u32,
    ) -> Result<Vec<BatchingOpportunity>> {
        // Get the tool sequence for all trajectories in this session
        let mut stmt = self
            .conn
            .prepare(
                "SELECT ts.tool_name
                 FROM trajectory_steps ts
                 JOIN trajectories t ON t.id = ts.trajectory_id
                 WHERE t.session_id = ?1
                 ORDER BY ts.trajectory_id, ts.step_index",
            )
            .sq()?;

        let tools: Vec<String> = stmt
            .query_map(params![session_id], |row| row.get(0))
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        find_consecutive_runs(&tools, min_consecutive)
    }

    /// Analyze ALL trajectories to find global batching patterns.
    /// Returns the most common sequential same-tool patterns across all history.
    pub fn get_global_batching_stats(
        &self,
        min_consecutive: u32,
        limit: usize,
    ) -> Result<Vec<BatchingOpportunity>> {
        // Get tool sequences per trajectory
        let mut stmt = self
            .conn
            .prepare(
                "SELECT t.id, ts.tool_name
                 FROM trajectory_steps ts
                 JOIN trajectories t ON t.id = ts.trajectory_id
                 WHERE t.status IN ('completed', 'judged')
                 ORDER BY t.id, ts.step_index",
            )
            .sq()?;

        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .sq()?
            .filter_map(|r| r.ok())
            .collect();

        // Group by trajectory
        use std::collections::HashMap;
        let mut trajectories: HashMap<String, Vec<String>> = HashMap::new();
        for (tid, tool) in rows {
            trajectories.entry(tid).or_default().push(tool);
        }

        // Aggregate batching opportunities across all trajectories
        let mut totals: HashMap<String, u64> = HashMap::new();
        let mut max_consecutive: HashMap<String, u32> = HashMap::new();

        for tools in trajectories.values() {
            if let Ok(opps) = find_consecutive_runs(tools, min_consecutive) {
                for opp in opps {
                    *totals.entry(opp.tool_name.clone()).or_insert(0) += opp.occurrence_count;
                    let max = max_consecutive.entry(opp.tool_name.clone()).or_insert(0);
                    if opp.consecutive_count > *max {
                        *max = opp.consecutive_count;
                    }
                }
            }
        }

        let mut results: Vec<BatchingOpportunity> = totals
            .into_iter()
            .map(|(tool, count)| BatchingOpportunity {
                consecutive_count: *max_consecutive.get(&tool).unwrap_or(&0),
                tool_name: tool,
                occurrence_count: count,
            })
            .collect();

        results.sort_by(|a, b| b.occurrence_count.cmp(&a.occurrence_count));
        results.truncate(limit);

        Ok(results)
    }
}

/// Find consecutive runs of the same tool in a sequence.
fn find_consecutive_runs(
    tools: &[String],
    min_consecutive: u32,
) -> Result<Vec<BatchingOpportunity>> {
    use std::collections::HashMap;

    let mut results: HashMap<String, (u32, u64)> = HashMap::new(); // tool -> (max_consecutive, count)

    if tools.is_empty() {
        return Ok(Vec::new());
    }

    let batchable = ["Read", "Glob", "Grep", "WebFetch", "WebSearch"];

    let mut current_tool = &tools[0];
    let mut run_length = 1u32;

    for tool in &tools[1..] {
        if tool == current_tool {
            run_length += 1;
        } else {
            if run_length >= min_consecutive && batchable.contains(&current_tool.as_str()) {
                let entry = results.entry(current_tool.clone()).or_insert((0, 0));
                if run_length > entry.0 {
                    entry.0 = run_length;
                }
                entry.1 += 1;
            }
            current_tool = tool;
            run_length = 1;
        }
    }

    // Handle the last run
    if run_length >= min_consecutive && batchable.contains(&current_tool.as_str()) {
        let entry = results.entry(current_tool.clone()).or_insert((0, 0));
        if run_length > entry.0 {
            entry.0 = run_length;
        }
        entry.1 += 1;
    }

    Ok(results
        .into_iter()
        .map(|(tool, (max, count))| BatchingOpportunity {
            tool_name: tool,
            consecutive_count: max,
            occurrence_count: count,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MemoryDb;
    use chrono::Utc;
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    use rusqlite::params;
    use std::path::Path;

    fn test_db() -> MemoryDb {
        MemoryDb::open(Path::new(":memory:")).unwrap()
    }

    fn seed_trajectory(db: &MemoryDb, id: &str, session_id: &str, tools: &[&str]) {
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
            agent_name: Some("test-agent".to_string()),
            task_description: Some("test task".to_string()),
            status: TrajectoryStatus::Completed,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            verdict: None,
            confidence: None,
            metadata: None,
            embedding_id: None,
        };
        db.create_trajectory(&t).unwrap();

        for tool in tools {
            db.record_trajectory_step(id, tool, None, StepOutcome::Success, Some(100))
                .unwrap();
        }
    }

    #[test]
    fn test_detect_batching_sequential_reads() {
        let db = test_db();
        seed_trajectory(
            &db, "t1", "s1",
            &["Read", "Read", "Read", "Edit", "Bash"],
        );

        let opps = db.detect_batching_opportunities("s1", 2).unwrap();
        assert!(!opps.is_empty());

        let read_opp = opps.iter().find(|o| o.tool_name == "Read").unwrap();
        assert_eq!(read_opp.consecutive_count, 3);
    }

    #[test]
    fn test_detect_batching_no_runs() {
        let db = test_db();
        seed_trajectory(
            &db, "t1", "s1",
            &["Read", "Edit", "Read", "Edit", "Bash"],
        );

        let opps = db.detect_batching_opportunities("s1", 2).unwrap();
        assert!(opps.is_empty());
    }

    #[test]
    fn test_detect_batching_grep_runs() {
        let db = test_db();
        seed_trajectory(
            &db, "t1", "s1",
            &["Grep", "Grep", "Grep", "Grep", "Edit"],
        );

        let opps = db.detect_batching_opportunities("s1", 3).unwrap();
        assert!(!opps.is_empty());
        let grep_opp = opps.iter().find(|o| o.tool_name == "Grep").unwrap();
        assert_eq!(grep_opp.consecutive_count, 4);
    }

    #[test]
    fn test_detect_batching_non_batchable_ignored() {
        let db = test_db();
        // Edit calls are sequential but not batchable
        seed_trajectory(
            &db, "t1", "s1",
            &["Edit", "Edit", "Edit"],
        );

        let opps = db.detect_batching_opportunities("s1", 2).unwrap();
        assert!(opps.is_empty()); // Edit is not in batchable list
    }

    #[test]
    fn test_global_batching_stats() {
        let db = test_db();
        seed_trajectory(&db, "t1", "s1", &["Read", "Read", "Read", "Edit"]);
        seed_trajectory(&db, "t2", "s2", &["Read", "Read", "Grep", "Grep"]);

        let stats = db.get_global_batching_stats(2, 10).unwrap();
        assert!(!stats.is_empty());

        // Read should have highest occurrence count (2 trajectories)
        let read_stat = stats.iter().find(|s| s.tool_name == "Read").unwrap();
        assert_eq!(read_stat.occurrence_count, 2);
    }

    #[test]
    fn test_find_consecutive_runs_empty() {
        let result = find_consecutive_runs(&[], 2).unwrap();
        assert!(result.is_empty());
    }
}
