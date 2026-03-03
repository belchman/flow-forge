use flowforge_core::hook::{self, ContextOutput, SubagentStartInput};
use flowforge_core::{AgentSession, AgentSessionStatus, FlowForgeConfig};
use flowforge_tmux::TmuxStateManager;

pub fn run() -> flowforge_core::Result<()> {
    let v = hook::parse_stdin_value()?;
    let input = SubagentStartInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let agent_id = input
        .agent_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Update tmux state
    let state_mgr = TmuxStateManager::new(FlowForgeConfig::tmux_state_path());
    let _ = state_mgr.add_member(&agent_id, input.agent_type.as_deref().unwrap_or("general"));
    let _ = state_mgr.add_event(format!(
        "{} started ({})",
        agent_id,
        input.agent_type.as_deref().unwrap_or("general")
    ));

    // Create agent session in DB and link trajectory to agent
    {
        let db_path = config.db_path();
        if db_path.exists() {
            if let Ok(db) = flowforge_memory::MemoryDb::open(&db_path) {
                let parent_id = db
                    .get_current_session()
                    .ok()
                    .flatten()
                    .map(|s| s.id)
                    .unwrap_or_default();
                let agent_session = AgentSession {
                    id: uuid::Uuid::new_v4().to_string(),
                    parent_session_id: parent_id.clone(),
                    agent_id: agent_id.clone(),
                    agent_type: input
                        .agent_type
                        .clone()
                        .unwrap_or_else(|| "general".to_string()),
                    status: AgentSessionStatus::Active,
                    started_at: chrono::Utc::now(),
                    ended_at: None,
                    edits: 0,
                    commands: 0,
                    task_id: None,
                    transcript_path: input.common.transcript_path.clone(),
                };
                let _ = db.create_agent_session(&agent_session);

                // Link active trajectory to this agent's type
                if let Some(ref agent_type) = input.agent_type {
                    if let Ok(Some(trajectory)) = db.get_active_trajectory(&parent_id) {
                        if trajectory.agent_name.is_none() {
                            let _ = db.set_trajectory_agent_name(&trajectory.id, agent_type);
                        }
                    }
                }
            }
        }
    }

    // Log work event for agent start and update assignee (C4)
    if config.work_tracking.log_all {
        let db_path = config.db_path();
        if db_path.exists() {
            if let Ok(db) = flowforge_memory::MemoryDb::open(&db_path) {
                let event = flowforge_core::WorkEvent {
                    id: 0,
                    work_item_id: agent_id.clone(),
                    event_type: "agent_started".to_string(),
                    old_value: None,
                    new_value: input.agent_type.clone(),
                    actor: Some(format!("agent:{}", agent_id)),
                    timestamp: chrono::Utc::now(),
                };
                let _ = db.record_work_event(&event);

                // Update assignee on any in-progress work items assigned to this agent
                if let Some(ref task_id) = input.common.session_id {
                    let agent_name = input.agent_type.as_deref().unwrap_or(&agent_id);
                    let _ = db.update_work_item_assignee(task_id, agent_name);
                }
            }
        }
    }

    // Inject agent-specific context if we have an agent type match
    let mut context_parts = Vec::new();

    if let Some(agent_type) = &input.agent_type {
        if let Ok(registry) = flowforge_agents::AgentRegistry::load(&config.agents) {
            if let Some(agent) = registry.get(agent_type) {
                if !agent.body.is_empty() {
                    context_parts.push(format!(
                        "[FlowForge] Agent guidance for {}:\n{}",
                        agent.name, agent.body
                    ));
                }
            }
        }
    }

    if context_parts.is_empty() {
        let output = ContextOutput::none();
        hook::write_stdout(&output)?;
    } else {
        let output = ContextOutput::with_context(context_parts.join("\n\n"));
        hook::write_stdout(&output)?;
    }

    Ok(())
}
