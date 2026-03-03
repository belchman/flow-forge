use flowforge_core::hook::{self, SubagentStopInput};
use flowforge_core::{AgentSessionStatus, FlowForgeConfig, Result, TeamMemberStatus};
use flowforge_memory::MemoryDb;
use flowforge_tmux::TmuxStateManager;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let input = SubagentStopInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let agent_id = input
        .agent_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Update tmux state
    let state_mgr = TmuxStateManager::new(FlowForgeConfig::tmux_state_path());
    let _ = state_mgr.update_member_status(&agent_id, TeamMemberStatus::Completed, None);
    let _ = state_mgr.add_event(format!("{} stopped", agent_id));

    // Ingest agent transcript and end agent session in DB
    {
        let db_path = config.db_path();
        if db_path.exists() {
            if let Ok(db) = MemoryDb::open(&db_path) {
                // Ingest transcript if available
                if let Some(ref path) = input.common.transcript_path {
                    let _ = db.ingest_transcript(&agent_id, path);
                }
                let _ = db.end_agent_session(&agent_id, AgentSessionStatus::Completed);
            }
        }
    }

    // Log work event for agent stop (C4)
    if config.work_tracking.log_all {
        let db_path = config.db_path();
        if db_path.exists() {
            if let Ok(db) = MemoryDb::open(&db_path) {
                let event = flowforge_core::WorkEvent {
                    id: 0,
                    work_item_id: agent_id.clone(),
                    event_type: "agent_stopped".to_string(),
                    old_value: Some("active".to_string()),
                    new_value: Some("completed".to_string()),
                    actor: Some(format!("agent:{}", agent_id)),
                    timestamp: chrono::Utc::now(),
                };
                let _ = db.record_work_event(&event);
            }
        }
    }

    // Extract patterns from agent output if learning is enabled
    if config.hooks.learning {
        if let Some(message) = &input.last_assistant_message {
            extract_patterns(&config, message)?;
        }
    }

    Ok(())
}

fn extract_patterns(config: &FlowForgeConfig, message: &str) -> Result<()> {
    let db_path = config.db_path();
    if !db_path.exists() {
        return Ok(());
    }

    let db = MemoryDb::open(&db_path)?;
    let store = flowforge_memory::PatternStore::new(&db, &config.patterns);

    for line in message.lines() {
        let trimmed = line.trim();

        if trimmed.len() < 20 || trimmed.len() > 200 {
            continue;
        }

        if trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with("Note:")
            || trimmed.starts_with("Pattern:")
            || trimmed.starts_with("Learned:")
        {
            let content = trimmed
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .trim_start_matches("Note: ")
                .trim_start_matches("Pattern: ")
                .trim_start_matches("Learned: ");

            if !content.is_empty() {
                let _ = store.store_short_term(content, "agent-output");
            }
        }
    }

    Ok(())
}
