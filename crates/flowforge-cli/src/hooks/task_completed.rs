use flowforge_core::hook::{self, TaskCompletedInput};
use flowforge_core::{FlowForgeConfig, Result, WorkEvent};
use flowforge_memory::MemoryDb;

pub fn run() -> Result<()> {
    let input: TaskCompletedInput = hook::parse_stdin()?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    // Update routing weights based on task completion
    if config.hooks.learning {
        if let (Some(subject), Some(teammate)) = (&input.task_subject, &input.teammate_name) {
            update_routing_weight(&config, subject, teammate)?;
        }
    }

    // Log work event for task completion (C4)
    if config.work_tracking.log_all {
        if let Some(task_id) = &input.task_id {
            let db_path = config.db_path();
            if db_path.exists() {
                if let Ok(db) = MemoryDb::open(&db_path) {
                    let actor = input
                        .teammate_name
                        .as_deref()
                        .map(|n| format!("agent:{n}"))
                        .unwrap_or_else(|| "hook:task-completed".to_string());

                    let event = WorkEvent {
                        id: 0,
                        work_item_id: task_id.clone(),
                        event_type: "completed".to_string(),
                        old_value: Some("in_progress".to_string()),
                        new_value: Some("completed".to_string()),
                        actor: Some(actor),
                        timestamp: chrono::Utc::now(),
                    };
                    let _ = db.record_work_event(&event);

                    // Also update the work item status if it exists
                    let _ = db.update_work_item_status(task_id, "completed");
                }
            }
        }
    }

    // Release work item claim on completion
    if config.work_tracking.work_stealing.enabled {
        if let Some(task_id) = &input.task_id {
            let db_path = config.db_path();
            if db_path.exists() {
                if let Ok(db) = MemoryDb::open(&db_path) {
                    let _ = db.release_work_item(task_id);
                }
            }
        }
    }

    // Link trajectory to completed work item
    if let Some(task_id) = &input.task_id {
        let db_path = config.db_path();
        if db_path.exists() {
            if let Ok(db) = MemoryDb::open(&db_path) {
                if let Ok(Some(session)) = db.get_current_session() {
                    if let Ok(Some(trajectory)) = db.get_active_trajectory(&session.id) {
                        let _ = db.link_trajectory_work_item(&trajectory.id, task_id);
                    }
                }
            }
        }
    }

    Ok(())
}

fn update_routing_weight(
    config: &FlowForgeConfig,
    task_subject: &str,
    agent_name: &str,
) -> Result<()> {
    let db_path = config.db_path();
    if !db_path.exists() {
        return Ok(());
    }

    let db = MemoryDb::open(&db_path)?;

    // Extract a simple task pattern from the subject
    let task_pattern = task_subject
        .to_lowercase()
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");

    // Record a success for this agent on this task pattern
    db.record_routing_success(&task_pattern, agent_name)?;

    Ok(())
}
