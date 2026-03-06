use flowforge_core::hook::PostToolUseFailureInput;
use flowforge_core::Result;
use sha2::{Digest, Sha256};

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = PostToolUseFailureInput::from_value(&ctx.raw)?;

    // Record failed tool use for error pattern tracking
    if ctx.config.hooks.learning {
        ctx.with_db("store_error_pattern", |db| {
            let error_msg = input.error.as_deref().unwrap_or("unknown error");
            let truncated: String = error_msg.chars().take(100).collect();
            let pattern = format!("tool_failure:{} - {}", input.tool_name, truncated);
            let store = flowforge_memory::PatternStore::new(db, &ctx.config.patterns);
            store.store_short_term(&pattern, "error_pattern")
        });
    }

    // Record trajectory failure step + error fingerprint + failure loop tracking
    let error_msg = input.error.clone();
    let tool_name = input.tool_name.clone();
    let input_json = serde_json::to_string(&input.tool_input).unwrap_or_default();
    let input_hash = format!("{:x}", Sha256::digest(input_json.as_bytes()));

    ctx.with_db("record_trajectory_failure_step", |db| {
        if let Some(session) = db.get_current_session()? {
            if let Some(trajectory) = db.get_active_trajectory(&session.id)? {
                db.record_trajectory_step(
                    &trajectory.id,
                    &tool_name,
                    Some(&input_hash),
                    flowforge_core::trajectory::StepOutcome::Failure,
                    None,
                )?;
            }

            // Record error fingerprint for resolution tracking
            if let Some(ref err) = error_msg {
                let _ = db.record_error_occurrence(&tool_name, err);
            }

            // Record tool failure for loop detection in pre_tool_use
            let err_preview = error_msg.as_deref().map(|e| {
                if e.len() > 200 { &e[..200] } else { e }
            });
            db.record_tool_failure(&session.id, &tool_name, &input_hash, err_preview)?;
        }
        Ok(())
    });

    Ok(())
}
