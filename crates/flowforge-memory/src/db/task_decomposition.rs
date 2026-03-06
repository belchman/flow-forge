//! Predictive task decomposition: analyze historical trajectories to suggest
//! how to break down a new task into phases/subtasks.

use serde::{Deserialize, Serialize};

use flowforge_core::Result;

use super::{MemoryDb, SqliteExt};

/// A predicted phase/subtask for a task decomposition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictedPhase {
    /// Phase name derived from historical tool sequences
    pub name: String,
    /// Predicted primary tools for this phase
    pub tools: Vec<String>,
    /// Suggested agent (if historical data indicates a specialist)
    pub suggested_agent: Option<String>,
    /// Estimated number of steps in this phase
    pub estimated_steps: u32,
    /// Confidence in this prediction (0.0-1.0)
    pub confidence: f64,
}

/// Full task decomposition prediction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDecomposition {
    /// The original task description
    pub task: String,
    /// Predicted phases (ordered)
    pub phases: Vec<PredictedPhase>,
    /// Number of historical trajectories used for prediction
    pub sample_count: u64,
    /// Overall confidence in the decomposition
    pub confidence: f64,
}

impl MemoryDb {
    /// Predict task decomposition based on historical successful trajectories.
    ///
    /// Algorithm:
    /// 1. Find trajectories with similar task descriptions (keyword matching).
    /// 2. Extract tool sequences from those trajectories.
    /// 3. Cluster tool sequences into phases (contiguous groups of related tools).
    /// 4. Aggregate across trajectories to find common phase patterns.
    /// 5. Include agent information from the most successful matches.
    pub fn predict_decomposition(&self, task: &str) -> Result<TaskDecomposition> {
        let keywords: Vec<&str> = task
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();

        if keywords.is_empty() {
            return Ok(TaskDecomposition {
                task: task.to_string(),
                phases: default_phases(),
                sample_count: 0,
                confidence: 0.1,
            });
        }

        // Build LIKE clauses for keyword matching
        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for kw in &keywords {
            param_values.push(Box::new(format!("%{kw}%")));
            conditions.push(format!(
                "LOWER(t.task_description) LIKE LOWER(?{})",
                param_values.len()
            ));
        }
        let where_clause = conditions.join(" OR ");

        // Find matching successful trajectories with their tool sequences
        let sql = format!(
            "SELECT t.id, t.agent_name, t.task_description,
                    GROUP_CONCAT(ts.tool_name, ',') as tool_sequence,
                    COUNT(ts.id) as step_count
             FROM trajectories t
             JOIN trajectory_steps ts ON ts.trajectory_id = t.id
             WHERE t.status IN ('completed', 'judged')
               AND t.task_description IS NOT NULL
               AND ({where_clause})
             GROUP BY t.id
             ORDER BY t.started_at DESC
             LIMIT 20"
        );

        let params_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let rows = stmt
            .query_map(params_slice.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,      // id
                    row.get::<_, Option<String>>(1)?, // agent_name
                    row.get::<_, String>(2)?,       // task_description
                    row.get::<_, String>(3)?,       // tool_sequence (comma-separated)
                    row.get::<_, u32>(4)?,          // step_count
                ))
            })
            .sq()?;

        let mut trajectories = Vec::new();
        for row in rows {
            trajectories.push(row.sq()?);
        }

        if trajectories.is_empty() {
            return Ok(TaskDecomposition {
                task: task.to_string(),
                phases: default_phases(),
                sample_count: 0,
                confidence: 0.1,
            });
        }

        // Extract tool phases from each trajectory
        #[allow(clippy::type_complexity)]
        let mut all_phases: Vec<Vec<(String, Vec<String>, Option<String>)>> = Vec::new();

        for (_id, agent_name, _desc, tool_seq, _count) in &trajectories {
            let tools: Vec<&str> = tool_seq.split(',').collect();
            let phases = extract_phases(&tools);
            let annotated: Vec<(String, Vec<String>, Option<String>)> = phases
                .into_iter()
                .map(|(name, tools)| (name, tools, agent_name.clone()))
                .collect();
            all_phases.push(annotated);
        }

        // Aggregate phases across trajectories
        let merged = merge_phases(&all_phases);
        let sample_count = trajectories.len() as u64;
        let confidence = (sample_count as f64 / 10.0).min(1.0) * 0.8;

        Ok(TaskDecomposition {
            task: task.to_string(),
            phases: merged,
            sample_count,
            confidence,
        })
    }

    /// Get the most common tool sequences for tasks matching keywords.
    /// Returns up to `limit` (tool_name, usage_count) pairs.
    pub fn get_common_tool_patterns(
        &self,
        keywords: &[&str],
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        if keywords.is_empty() {
            return Ok(Vec::new());
        }

        let mut conditions = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        for kw in keywords {
            param_values.push(Box::new(format!("%{kw}%")));
            conditions.push(format!(
                "LOWER(t.task_description) LIKE LOWER(?{})",
                param_values.len()
            ));
        }
        let where_clause = conditions.join(" OR ");

        param_values.push(Box::new(limit as i64));
        let limit_param = param_values.len();

        let sql = format!(
            "SELECT ts.tool_name, COUNT(*) as cnt
             FROM trajectory_steps ts
             JOIN trajectories t ON t.id = ts.trajectory_id
             WHERE t.status IN ('completed', 'judged')
               AND t.task_description IS NOT NULL
               AND ({where_clause})
             GROUP BY ts.tool_name
             ORDER BY cnt DESC
             LIMIT ?{limit_param}"
        );

        let params_slice: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = self.conn.prepare(&sql).sq()?;
        let rows = stmt
            .query_map(params_slice.as_slice(), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
            })
            .sq()?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.sq()?);
        }
        Ok(results)
    }
}

/// Classify a tool into a phase category.
fn tool_phase(tool: &str) -> &'static str {
    match tool {
        "Read" | "Glob" | "Grep" | "LSP" => "research",
        "Edit" | "Write" | "NotebookEdit" => "implementation",
        "Bash" => "execution",
        "Task" => "delegation",
        _ => "other",
    }
}

/// Extract phases from a tool sequence by grouping contiguous same-phase tools.
fn extract_phases(tools: &[&str]) -> Vec<(String, Vec<String>)> {
    if tools.is_empty() {
        return Vec::new();
    }

    let mut phases: Vec<(String, Vec<String>)> = Vec::new();
    let mut current_phase = tool_phase(tools[0]).to_string();
    let mut current_tools: Vec<String> = vec![tools[0].to_string()];

    for tool in &tools[1..] {
        let phase = tool_phase(tool);
        if phase == current_phase {
            if !current_tools.contains(&tool.to_string()) {
                current_tools.push(tool.to_string());
            }
        } else {
            phases.push((current_phase.clone(), current_tools.clone()));
            current_phase = phase.to_string();
            current_tools = vec![tool.to_string()];
        }
    }
    phases.push((current_phase, current_tools));

    // Merge consecutive phases of the same type
    let mut merged: Vec<(String, Vec<String>)> = Vec::new();
    for (phase, tools) in phases {
        if let Some(last) = merged.last_mut() {
            if last.0 == phase {
                for t in tools {
                    if !last.1.contains(&t) {
                        last.1.push(t);
                    }
                }
                continue;
            }
        }
        merged.push((phase, tools));
    }

    merged
}

/// Merge phases across multiple trajectory decompositions.
/// Finds the most common phase ordering pattern.
#[allow(clippy::type_complexity)]
fn merge_phases(
    all_phases: &[Vec<(String, Vec<String>, Option<String>)>],
) -> Vec<PredictedPhase> {
    if all_phases.is_empty() {
        return Vec::new();
    }

    // Count phase name frequencies at each position
    use std::collections::HashMap;
    let mut position_counts: HashMap<(usize, String), u32> = HashMap::new();
    let mut position_tools: HashMap<(usize, String), HashMap<String, u32>> = HashMap::new();
    let mut position_agents: HashMap<(usize, String), HashMap<String, u32>> = HashMap::new();
    let mut max_phases = 0usize;

    for trajectory_phases in all_phases {
        max_phases = max_phases.max(trajectory_phases.len());
        for (i, (phase_name, tools, agent)) in trajectory_phases.iter().enumerate() {
            let key = (i, phase_name.clone());
            *position_counts.entry(key.clone()).or_insert(0) += 1;

            let tool_map = position_tools.entry(key.clone()).or_default();
            for tool in tools {
                *tool_map.entry(tool.clone()).or_insert(0) += 1;
            }

            if let Some(agent_name) = agent {
                let agent_map = position_agents.entry(key).or_default();
                *agent_map.entry(agent_name.clone()).or_insert(0) += 1;
            }
        }
    }

    let total = all_phases.len() as f64;

    // Build the most common phase sequence
    let mut result = Vec::new();
    for pos in 0..max_phases.min(6) {
        // Find the most common phase at this position
        let mut best_phase = String::new();
        let mut best_count = 0u32;

        for ((p, name), count) in &position_counts {
            if *p == pos && *count > best_count {
                best_count = *count;
                best_phase = name.clone();
            }
        }

        if best_phase.is_empty() || best_count == 0 {
            continue;
        }

        let key = (pos, best_phase.clone());
        let confidence = best_count as f64 / total;

        // Get top tools for this phase
        let tools = if let Some(tool_map) = position_tools.get(&key) {
            let mut sorted: Vec<_> = tool_map.iter().collect();
            sorted.sort_by(|a, b| b.1.cmp(a.1));
            sorted.into_iter().take(5).map(|(t, _)| t.clone()).collect()
        } else {
            Vec::new()
        };

        // Get suggested agent
        let agent = position_agents
            .get(&key)
            .and_then(|agent_map| {
                agent_map
                    .iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(name, _)| name.clone())
            });

        // Estimate steps from average trajectory length at this phase
        let estimated_steps = (best_count as f64 * 3.0 / total).max(1.0) as u32;

        result.push(PredictedPhase {
            name: format_phase_name(&best_phase, pos),
            tools,
            suggested_agent: agent,
            estimated_steps,
            confidence,
        });
    }

    if result.is_empty() {
        return default_phases();
    }

    result
}

fn format_phase_name(phase: &str, position: usize) -> String {
    let ordinal = match position {
        0 => "1",
        1 => "2",
        2 => "3",
        3 => "4",
        4 => "5",
        _ => "N",
    };
    let label = match phase {
        "research" => "Research & Analysis",
        "implementation" => "Implementation",
        "execution" => "Build & Test",
        "delegation" => "Agent Delegation",
        _ => "Other",
    };
    format!("Phase {ordinal}: {label}")
}

fn default_phases() -> Vec<PredictedPhase> {
    vec![
        PredictedPhase {
            name: "Phase 1: Research & Analysis".to_string(),
            tools: vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()],
            suggested_agent: None,
            estimated_steps: 5,
            confidence: 0.3,
        },
        PredictedPhase {
            name: "Phase 2: Implementation".to_string(),
            tools: vec!["Edit".to_string(), "Write".to_string()],
            suggested_agent: None,
            estimated_steps: 10,
            confidence: 0.3,
        },
        PredictedPhase {
            name: "Phase 3: Build & Test".to_string(),
            tools: vec!["Bash".to_string()],
            suggested_agent: None,
            estimated_steps: 3,
            confidence: 0.3,
        },
    ]
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

    fn seed_trajectory(
        db: &MemoryDb,
        id: &str,
        session_id: &str,
        task_desc: &str,
        agent: &str,
        tools: &[&str],
    ) {
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

        for tool in tools {
            db.record_trajectory_step(id, tool, None, StepOutcome::Success, Some(100))
                .unwrap();
        }
    }

    #[test]
    fn test_predict_decomposition_no_data() {
        let db = test_db();
        let decomp = db.predict_decomposition("refactor the auth module").unwrap();
        assert_eq!(decomp.sample_count, 0);
        assert_eq!(decomp.phases.len(), 3); // default phases
        assert!(decomp.confidence < 0.2);
    }

    #[test]
    fn test_predict_decomposition_empty_task() {
        let db = test_db();
        let decomp = db.predict_decomposition("a b c").unwrap();
        // All words <= 3 chars, falls back to defaults
        assert_eq!(decomp.sample_count, 0);
    }

    #[test]
    fn test_predict_decomposition_with_data() {
        let db = test_db();

        // Seed trajectory: research → implementation → testing
        seed_trajectory(
            &db, "t1", "s1",
            "refactor the authentication module",
            "rust-expert",
            &["Read", "Grep", "Glob", "Edit", "Write", "Edit", "Bash", "Bash"],
        );

        seed_trajectory(
            &db, "t2", "s2",
            "refactor the database layer",
            "rust-expert",
            &["Read", "Read", "Grep", "Edit", "Edit", "Write", "Bash"],
        );

        let decomp = db.predict_decomposition("refactor the user module").unwrap();
        assert_eq!(decomp.sample_count, 2);
        assert!(!decomp.phases.is_empty());
        assert!(decomp.confidence > 0.0);

        // Should have research phase first
        assert!(decomp.phases[0].name.contains("Research"));
    }

    #[test]
    fn test_predict_decomposition_preserves_agent() {
        let db = test_db();

        seed_trajectory(
            &db, "t1", "s1",
            "fix compile errors in auth",
            "compiler-specialist",
            &["Read", "Grep", "Edit", "Bash"],
        );

        let decomp = db.predict_decomposition("fix compile errors").unwrap();
        // At least one phase should suggest the specialist agent
        let has_agent = decomp.phases.iter().any(|p| p.suggested_agent.is_some());
        assert!(has_agent);
    }

    #[test]
    fn test_extract_phases() {
        let tools = &["Read", "Grep", "Glob", "Edit", "Write", "Bash", "Bash"];
        let phases = extract_phases(tools);

        assert_eq!(phases.len(), 3);
        assert_eq!(phases[0].0, "research");
        assert_eq!(phases[1].0, "implementation");
        assert_eq!(phases[2].0, "execution");
    }

    #[test]
    fn test_extract_phases_mixed() {
        // research → impl → research → test pattern
        let tools = &["Read", "Edit", "Read", "Grep", "Bash"];
        let phases = extract_phases(tools);

        assert_eq!(phases.len(), 4);
        assert_eq!(phases[0].0, "research");
        assert_eq!(phases[1].0, "implementation");
        assert_eq!(phases[2].0, "research");
        assert_eq!(phases[3].0, "execution");
    }

    #[test]
    fn test_extract_phases_empty() {
        let phases = extract_phases(&[]);
        assert!(phases.is_empty());
    }

    #[test]
    fn test_get_common_tool_patterns() {
        let db = test_db();

        seed_trajectory(
            &db, "t1", "s1",
            "refactor the auth module",
            "coder",
            &["Read", "Read", "Grep", "Edit", "Edit", "Edit", "Bash"],
        );

        let patterns = db.get_common_tool_patterns(&["refactor"], 5).unwrap();
        assert!(!patterns.is_empty());

        // Edit should be most common (3 uses)
        assert_eq!(patterns[0].0, "Edit");
        assert_eq!(patterns[0].1, 3);
    }

    #[test]
    fn test_get_common_tool_patterns_empty_keywords() {
        let db = test_db();
        let patterns = db.get_common_tool_patterns(&[], 5).unwrap();
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_default_phases() {
        let phases = default_phases();
        assert_eq!(phases.len(), 3);
        assert!(phases[0].name.contains("Research"));
        assert!(phases[1].name.contains("Implementation"));
        assert!(phases[2].name.contains("Test"));
    }
}
