use flowforge_core::hook::TaskCompletedInput;
use flowforge_core::Result;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = TaskCompletedInput::from_value(&ctx.raw)?;

    // Update routing weights based on task completion
    if ctx.config.hooks.learning {
        if let (Some(subject), Some(teammate)) = (&input.task_subject, &input.teammate_name) {
            update_routing_weight(&ctx, subject, teammate);
        }
    }

    // Resolve the work item for this Claude task:
    // 1. Match task_subject → work_item.title
    // 2. Fall back to any in-progress work item
    let resolved_wid = ctx.resolve_work_item_for_task(input.task_subject.as_deref());

    // Log work event for task completion (C4)
    if ctx.config.work_tracking.log_all {
        let actor = input
            .teammate_name
            .as_deref()
            .map(|n| format!("agent:{n}"))
            .unwrap_or_else(|| "hook:task-completed".to_string());

        if let Some(ref wid) = resolved_wid {
            ctx.record_work_event(
                wid,
                "completed",
                Some("in_progress"),
                Some("completed"),
                Some(&actor),
            );
        }
    }

    // Release work item claim on completion
    if ctx.config.work_tracking.work_stealing.enabled {
        // Release by resolved work item ID (not Claude task_id)
        if let Some(ref wid) = resolved_wid {
            ctx.with_db("release_work_item", |db| db.release_work_item(wid));
        }
    }

    // Link trajectory to resolved work item
    if let Some(ref wid) = resolved_wid {
        ctx.with_db("link_trajectory_work_item", |db| {
            if let Some(session) = db.get_current_session()? {
                if let Some(trajectory) = db.get_active_trajectory(&session.id)? {
                    db.link_trajectory_work_item(&trajectory.id, wid)?;
                }
            }
            Ok(())
        });
    }

    Ok(())
}

fn update_routing_weight(ctx: &super::HookContext, task_subject: &str, agent_name: &str) {
    let task_pattern = super::extract_task_pattern(task_subject);

    // Record a success for this agent on this task pattern (old system)
    ctx.with_db("record_routing_success", |db| {
        db.record_routing_success(&task_pattern, agent_name)
    });

    // New system: record_routing_outcome with stored breakdown if available
    ctx.with_db("record_routing_outcome", |db| {
        let session = db.get_current_session()?.ok_or_else(|| {
            flowforge_core::Error::Config("no current session".to_string())
        })?;
        let injections = db.get_injections_for_session(&session.id)?;
        if let Some(routing_inj) = injections.iter().find(|i| i.injection_type == "routing") {
            if let Some(ref metadata) = routing_inj.metadata {
                if let Ok(breakdown) = serde_json::from_str::<flowforge_core::RoutingBreakdown>(metadata) {
                    return db.record_routing_outcome(
                        &session.id, agent_name, &task_pattern,
                        breakdown.pattern_score, breakdown.capability_score,
                        breakdown.learned_score, breakdown.priority_score,
                        breakdown.context_score, breakdown.semantic_score,
                        "success",
                    );
                }
            }
        }
        // Fallback: record with zero scores
        db.record_routing_outcome(
            &session.id, agent_name, &task_pattern,
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, "success",
        )
    });

    // Store routing embedding for similarity-based generalization
    ctx.with_db("store_routing_vector", |db| {
        let config_for_embed = flowforge_core::config::PatternsConfig::default();
        let embedding = flowforge_memory::default_embedder(&config_for_embed);
        let vec = embedding.embed(&task_pattern);
        let source_id = format!("{}::{}", task_pattern, agent_name);
        db.store_vector("routing", &source_id, &vec)?;
        // Also store as routing_success for few-shot lookup
        db.store_vector("routing_success", &source_id, &vec)
    });
}
