use flowforge_core::hook::{ContextOutput, SubagentStartInput};
use flowforge_core::{AgentSession, AgentSessionStatus, FlowForgeConfig};
use flowforge_tmux::TmuxStateManager;

pub fn run() -> flowforge_core::Result<()> {
    let ctx = super::HookContext::init()?;
    let input = SubagentStartInput::from_value(&ctx.raw)?;

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
    let fallback_session_id = ctx
        .session_id
        .clone()
        .or_else(|| input.common.session_id.clone());
    ctx.with_db("create_agent_session", |db| {
        let parent_id = db
            .get_current_session()?
            .map(|s| s.id)
            .or(fallback_session_id)
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
        db.create_agent_session(&agent_session)?;

        // Link active trajectory to this agent's type
        if let Some(ref agent_type) = input.agent_type {
            if let Some(trajectory) = db.get_active_trajectory(&parent_id)? {
                if trajectory.agent_name.is_none() {
                    db.set_trajectory_agent_name(&trajectory.id, agent_type)?;
                }
            }
        }
        Ok(())
    });

    // Log work event for agent start and update assignee (C4)
    if ctx.config.work_tracking.log_all {
        // Find actual in-progress work item to avoid FK errors (agent_id != work_item_id)
        let work_item_id = ctx.with_db("find_active_work_item", |db| {
            let filter = flowforge_core::WorkFilter {
                status: Some(flowforge_core::WorkStatus::InProgress),
                ..Default::default()
            };
            let items = db.list_work_items(&filter)?;
            Ok(items.into_iter().next().map(|i| i.id))
        });
        if let Some(Some(wid)) = work_item_id {
            ctx.record_work_event(
                &wid,
                "agent_started",
                None,
                input.agent_type.as_deref(),
                Some(&format!("agent:{}", agent_id)),
            );
        }

        // Update assignee on the actual work item (not session_id)
        if let Some(wid) = ctx.resolve_work_item_for_task(None) {
            let agent_name = input.agent_type.as_deref().unwrap_or(&agent_id).to_string();
            ctx.with_db("update_work_item_assignee", |db| {
                db.update_work_item_assignee(&wid, &agent_name)
            });
        }
    }

    // Inject agent-specific context if we have an agent type match
    let mut context_parts = Vec::new();

    if let Some(agent_type) = &input.agent_type {
        if let Ok(registry) = flowforge_agents::AgentRegistry::load(&ctx.config.agents) {
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
        output.write()?;
    } else {
        let output = ContextOutput::with_context(context_parts.join("\n\n"));
        output.write()?;
    }

    Ok(())
}
