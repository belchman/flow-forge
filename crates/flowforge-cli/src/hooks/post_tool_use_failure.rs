use flowforge_core::hook::PostToolUseFailureInput;
use flowforge_core::types::error_recovery::classify_error;
use flowforge_core::Result;
use sha2::{Digest, Sha256};

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = PostToolUseFailureInput::from_value(&ctx.raw)?;

    // Error tracking is handled by the error fingerprinting + resolution system below.
    // Raw error messages as patterns were noise — they lack resolutions and duplicate
    // what error_fingerprints/error_resolutions already track.

    // Record trajectory failure step + error fingerprint + failure loop tracking
    let error_msg = input.error.clone();
    let tool_name = input.tool_name.clone();
    let input_json = serde_json::to_string(&input.tool_input).unwrap_or_default();
    let input_hash = format!("{:x}", Sha256::digest(input_json.as_bytes()));

    // Use session_id from hook input as fallback when get_current_session() returns None.
    // This ensures session_tool_failures are always recorded, which is critical for
    // auto_detect_resolutions to find and create error resolutions.
    let hook_session_id = input.common.session_id.clone();

    ctx.with_db("record_trajectory_failure_step", |db| {
        let session_id = db
            .get_current_session()?
            .map(|s| s.id)
            .or(hook_session_id);

        if let Some(ref sid) = session_id {
            if let Some(trajectory) = db.get_active_trajectory(sid)? {
                db.record_trajectory_step(
                    &trajectory.id,
                    &tool_name,
                    Some(&input_hash),
                    flowforge_core::trajectory::StepOutcome::Failure,
                    None,
                )?;
            }

            // Record error fingerprint for resolution tracking
            let mut fp_id: Option<String> = None;
            if let Some(ref err) = error_msg {
                let fingerprint_id = db.record_error_occurrence(&tool_name, err)?;

                // Embed the error fingerprint for semantic search
                if ctx.config.vectors.embed_errors {
                    let category = classify_error(err, &tool_name);
                    let error_preview: String = err.chars().take(200).collect();
                    let content =
                        format!("{}: {} - {}", category, tool_name, &error_preview);
                    let embedder =
                        flowforge_memory::default_embedder(&ctx.config.patterns);
                    let vec = embedder.embed(&content);
                    // Only store if not already vectorized
                    if db.count_vectors_for_source_id("error", &fingerprint_id)? == 0 {
                        db.store_vector("error", &fingerprint_id, &vec)?;
                    }
                }
                fp_id = Some(fingerprint_id);
            }

            // Record tool failure for loop detection in pre_tool_use
            let err_preview: Option<String> = error_msg.as_ref().map(|e| {
                e.chars().take(200).collect()
            });
            db.record_tool_failure(sid, &tool_name, &input_hash, err_preview.as_deref(), fp_id.as_deref())?;

            // ACTIVE LEARNING: Feed tool failure back to routing weights immediately.
            // Every failed tool call after a routing suggestion weakens that route.
            let key = format!("active_routing:{}", sid);
            if let Some(routing_info) = db.get_meta(&key)? {
                if let Some((agent_name, task_pattern)) = routing_info.split_once('|') {
                    let _ = db.record_routing_failure(task_pattern, agent_name);
                }
            }
        }
        Ok(())
    });

    Ok(())
}
