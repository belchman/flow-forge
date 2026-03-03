use chrono::Utc;
use flowforge_core::hook::{self, ContextOutput, SessionStartInput};
use flowforge_core::{FlowForgeConfig, Result, SessionInfo};
use flowforge_memory::MemoryDb;
use uuid::Uuid;

pub fn run() -> Result<()> {
    let input: SessionStartInput = hook::parse_stdin()?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let db_path = config.db_path();
    if !db_path.exists() {
        // Not initialized, just return context
        let output = ContextOutput::with_context(
            "[FlowForge] Not initialized. Run `flowforge init --project` to set up.".to_string(),
        );
        hook::write_stdout(&output)?;
        return Ok(());
    }

    let db = MemoryDb::open(&db_path)?;

    // Create new session
    let session_id = input
        .session_id
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());

    let session = SessionInfo {
        id: session_id.clone(),
        started_at: Utc::now(),
        ended_at: None,
        cwd,
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: input.common.transcript_path.clone(),
    };

    db.create_session(&session)?;

    // Build context with session info and stats
    let mut context_parts = vec![format!("[FlowForge] Session {} started.", &session_id[..8])];

    // Include stats from previous sessions
    if let Ok(sessions) = db.list_sessions(5) {
        if sessions.len() > 1 {
            let total_edits: u64 = sessions.iter().map(|s| s.edits).sum();
            context_parts.push(format!(
                "Recent activity: {} sessions, {} total edits.",
                sessions.len(),
                total_edits
            ));
        }
    }

    // Sync from external backend before showing context (C4)
    let _ = flowforge_core::work_tracking::sync_from_backend(&db, &config.work_tracking);

    // Include active work items (C4)
    let work_filter = flowforge_core::WorkFilter {
        status: Some("in_progress".to_string()),
        ..Default::default()
    };
    if let Ok(active_items) = db.list_work_items(&work_filter) {
        if !active_items.is_empty() {
            let mut work_ctx = format!("{} active work items:", active_items.len());
            for item in active_items.iter().take(3) {
                work_ctx.push_str(&format!(" [{}] {}", item.item_type, item.title));
            }
            context_parts.push(work_ctx);
        }
    }

    // Include pattern count
    if let Ok(count) = db.count_patterns() {
        if count > 0 {
            context_parts.push(format!("{count} learned patterns available."));
        }
    }

    let output = ContextOutput::with_context(context_parts.join(" "));
    hook::write_stdout(&output)?;

    Ok(())
}
