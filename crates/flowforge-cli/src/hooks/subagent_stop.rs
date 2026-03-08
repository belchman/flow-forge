use flowforge_core::hook::SubagentStopInput;
use flowforge_core::{AgentSessionStatus, Result, TeamMemberStatus};
use flowforge_tmux::TmuxStateManager;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = SubagentStopInput::from_value(&ctx.raw)?;

    let agent_id = input
        .agent_id
        .clone()
        .unwrap_or_else(|| "unknown".to_string());

    // Update tmux state
    let state_mgr = TmuxStateManager::new(flowforge_core::FlowForgeConfig::tmux_state_path());
    let _ = state_mgr.update_member_status(&agent_id, TeamMemberStatus::Completed, None);
    let _ = state_mgr.add_event(format!("{} stopped", agent_id));

    // End agent session and roll up stats to parent
    // Wrapped in a transaction so crash between steps doesn't leave orphans
    ctx.with_db("end_agent_session", |db| {
        db.with_transaction(|| {
            // Roll up agent edits/commands to parent session BEFORE ending
            // (so the statusline reflects cumulative work from all agents)
            db.rollup_agent_stats_to_parent(&agent_id)?;
            db.end_agent_session(&agent_id, AgentSessionStatus::Completed)?;
            Ok(())
        })
    });

    // Log work event for agent stop (C4)
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
                "agent_stopped",
                Some("active"),
                Some("completed"),
                Some(&format!("agent:{}", agent_id)),
            );
        }
    }

    // Extract patterns from agent output if learning is enabled
    if ctx.config.hooks.learning {
        if let Some(message) = &input.last_assistant_message {
            extract_patterns(&ctx, message);
        }
    }

    Ok(())
}

/// Check if a line looks like data/stats rather than an actionable insight.
fn is_data_line(s: &str) -> bool {
    // Lines with lots of numbers, IDs, timestamps, or backtick-wrapped values are data
    let digit_count = s.chars().filter(|c| c.is_ascii_digit()).count();
    let backtick_count = s.chars().filter(|&c| c == '`').count();
    let total = s.len();
    if total == 0 {
        return true;
    }
    // >30% digits = data line
    if digit_count * 100 / total > 30 {
        return true;
    }
    // Backtick-heavy lines are usually code/data references
    if backtick_count >= 4 {
        return true;
    }
    // Common data patterns
    let lower = s.to_lowercase();
    let data_indicators = [
        "calls,", "ms total", "ms)", "count:", "total:", "score:",
        "session id", "started at", "confidence:", "sim:", "(n=",
        "occurrences", "fingerprint", "0x", "uuid", "hash",
    ];
    data_indicators.iter().any(|pat| lower.contains(pat))
}

fn extract_patterns(ctx: &super::HookContext, message: &str) {
    ctx.with_db("extract_patterns", |db| {
        let store = flowforge_memory::PatternStore::new(db, &ctx.config.patterns);
        let mut stored = 0u32;

        for line in message.lines() {
            let trimmed = line.trim();

            // Length gate: too short = not useful, too long = probably a paragraph
            if trimmed.len() < 30 || trimmed.len() > 200 {
                continue;
            }

            // Only capture explicitly marked patterns/insights, not every bullet
            let content = if trimmed.starts_with("Note:") || trimmed.starts_with("Note: ") {
                trimmed.trim_start_matches("Note:").trim_start_matches("Note: ").trim()
            } else if trimmed.starts_with("Pattern:") || trimmed.starts_with("Pattern: ") {
                trimmed.trim_start_matches("Pattern:").trim_start_matches("Pattern: ").trim()
            } else if trimmed.starts_with("Learned:") || trimmed.starts_with("Learned: ") {
                trimmed.trim_start_matches("Learned:").trim_start_matches("Learned: ").trim()
            } else {
                // Skip generic bullets — they're almost always data/stats, not insights
                continue;
            };

            if content.is_empty() || is_data_line(content) {
                continue;
            }

            // Cap at 3 patterns per agent output to prevent flooding
            if stored >= 3 {
                break;
            }

            store.store_short_term(content, "agent-insight")?;
            stored += 1;
        }

        Ok(())
    });
}
