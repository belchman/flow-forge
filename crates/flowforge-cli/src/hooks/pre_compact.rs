use flowforge_core::hook::PreCompactInput;
use flowforge_core::Result;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let _input = PreCompactInput::from_value(&ctx.raw)?;

    let mut lines: Vec<String> = Vec::new();

    ctx.with_db("compaction_context", |db| {
        // Clear injection cache so post-compaction re-injects full context.
        // After compaction, Claude loses previous hook output, so next prompt
        // must re-inject even if content hasn't changed.
        if let Some(session_id) = ctx.session_id.as_deref() {
            let _ = db.clear_injection_cache(session_id);
            // Clear session reads — after compaction Claude genuinely lost context,
            // so allow re-reads without the dedup warning.
            let _ = db.clear_session_reads(session_id);
        }

        // Active work item — the ONE thing Claude must not forget
        let filter = flowforge_core::WorkFilter {
            status: Some(flowforge_core::WorkStatus::InProgress),
            limit: Some(1),
            ..Default::default()
        };
        if let Ok(items) = db.list_work_items(&filter) {
            if let Some(item) = items.first() {
                lines.push(format!("Task: {}", item.title));
            }
        }

        // Files modified this session — compact list
        if let Some(session) = db.get_current_session().ok().flatten() {
            if let Ok(edits) = db.get_edits_for_session(&session.id) {
                let mut seen = std::collections::HashSet::new();
                let files: Vec<&str> = edits.iter().rev()
                    .filter(|e| seen.insert(e.file_path.as_str()))
                    .take(6)
                    .map(|e| e.file_path.as_str())
                    .collect();
                if !files.is_empty() {
                    let short: Vec<&str> = files.iter()
                        .map(|f| f.rfind('/').map(|i| &f[i+1..]).unwrap_or(f))
                        .collect();
                    lines.push(format!("Modified: {}", short.join(", ")));
                }
            }
        }

        Ok(())
    });

    if lines.is_empty() {
        flowforge_core::hook::ContextOutput::with_context("[FlowForge] Ready.".to_string()).write()?;
    } else {
        let output = format!("[FlowForge] Preserve:\n{}", lines.join("\n"));
        flowforge_core::hook::ContextOutput::with_context(output).write()?;
    }

    Ok(())
}
