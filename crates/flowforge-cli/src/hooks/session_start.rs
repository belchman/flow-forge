use chrono::Utc;
use flowforge_core::hook::{ContextOutput, SessionStartInput};
use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
use flowforge_core::{Result, SessionInfo};

pub fn run() -> Result<()> {
    let mut ctx = super::HookContext::init()?;
    let input = SessionStartInput::from_value(&ctx.raw)?;

    // Create DB state that session_end/other hooks expect
    if let Some(ref session_id) = input.session_id.or(input.common.session_id.clone()) {
        // If DB doesn't exist yet, ensure parent dir and try opening
        if ctx.db.is_none() {
            let db_path = ctx.config.db_path();
            let _ = std::fs::create_dir_all(db_path.parent().unwrap_or(".".as_ref()));
            ctx.db = flowforge_memory::MemoryDb::open(&db_path).ok();
        }

        ctx.with_db("session_init", |db| {
            let now = Utc::now();

            // Create session record (INSERT OR IGNORE to preserve existing data on resume)
            let session = SessionInfo {
                id: session_id.clone(),
                started_at: now,
                ended_at: None,
                cwd: input.common.cwd.clone().unwrap_or_else(|| ".".to_string()),
                edits: 0,
                commands: 0,
                summary: None,
                transcript_path: input.common.transcript_path.clone(),
            };
            db.create_session(&session)?;

            // If session exists but was ended (resume scenario), reopen it
            db.reopen_session(session_id)?;

            // Create trajectory for this session
            let trajectory = Trajectory {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session_id.clone(),
                work_item_id: None,
                agent_name: None,
                task_description: None,
                status: TrajectoryStatus::Recording,
                started_at: now,
                ended_at: None,
                verdict: None,
                confidence: None,
                metadata: None,
                embedding_id: None,
            };
            db.create_trajectory(&trajectory)?;

            // Clean up orphaned agent sessions from previous crashed sessions
            let orphans = db.cleanup_orphaned_agent_sessions()?;
            if orphans > 0 {
                eprintln!("[FlowForge] Cleaned up {} orphaned agent sessions", orphans);
            }

            // Initialize trust score
            db.create_trust_score(session_id, ctx.config.guidance.trust_initial_score)?;

            // Auto-release stale/abandoned work items from previous sessions
            if ctx.config.work_tracking.work_stealing.enabled {
                let (marked, released) = flowforge_core::work_tracking::detect_stale(
                    db, &ctx.config.work_tracking,
                ).unwrap_or((0, 0));
                if marked > 0 || released > 0 {
                    eprintln!(
                        "[FlowForge] Cleaned up stale work items: {} marked stealable, {} released",
                        marked, released
                    );
                }
            }

            // Sync work items from external backend and write to Claude Tasks
            let _ = flowforge_core::work_tracking::sync_from_backend(db, &ctx.config.work_tracking);
            let _ = flowforge_core::work_tracking::sync_all_to_claude_tasks(
                db,
                &ctx.config.work_tracking,
            );

            // Log session start event (only if there's an active work item to attach to)
            // Uses resolve_work_item_for_task to find a work item by title or fallback
            if ctx.config.work_tracking.log_all {
                if let Some(wid) = ctx.resolve_work_item_for_task(None) {
                    let event = flowforge_core::WorkEvent {
                        id: 0,
                        work_item_id: wid,
                        event_type: "session_started".to_string(),
                        old_value: None,
                        new_value: Some(format!("source: {:?}", input.source)),
                        actor: Some("hook:session-start".to_string()),
                        timestamp: now,
                    };
                    db.record_work_event(&event)?;
                }
            }

            Ok(())
        });
    }

    // Auto-clear stale hook-errors.log (older than 24 hours)
    let log_path = flowforge_core::FlowForgeConfig::project_dir().join("hook-errors.log");
    if log_path.exists() {
        if let Ok(meta) = std::fs::metadata(&log_path) {
            if let Ok(modified) = meta.modified() {
                let age = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                if age > std::time::Duration::from_secs(24 * 3600) {
                    let _ = std::fs::remove_file(&log_path);
                }
            }
        }
    }

    // Validate work backend and report status on startup
    let mut ready_msg = String::from("[FlowForge] Ready.");
    let backend = flowforge_core::work_tracking::detect_backend(&ctx.config.work_tracking);
    if backend == "kanbus" {
        let kanbus_ok = std::path::Path::new(".kanbus.yml").exists()
            || std::path::Path::new(".kanbus").exists();
        if !kanbus_ok {
            ready_msg.push_str("\n\u{26a0} Kanbus backend configured but .kanbus.yml not found.");
        }
    }

    // Session continuity: inject previous session context
    if let Some(ref db) = ctx.db {
        let cwd = input
            .common
            .cwd
            .as_deref()
            .unwrap_or(".");
        if let Ok(Some(prev)) = db.get_previous_session_context(cwd) {
            let mut prev_ctx = String::from("\n[FlowForge] Previous session:");
            if let Some(ref task) = prev.task_description {
                prev_ctx.push_str(&format!(" Task: {}", task));
            }
            if let Some(ref verdict) = prev.verdict {
                prev_ctx.push_str(&format!(" ({})", verdict));
            }
            if prev.duration_minutes > 0 {
                prev_ctx.push_str(&format!(
                    "\n  Duration: {}m, {} edits, {} commands",
                    prev.duration_minutes, prev.edits_count, prev.commands_count
                ));
            }
            if !prev.files_modified.is_empty() {
                let files: Vec<&str> = prev.files_modified.iter().take(5).map(|s| s.as_str()).collect();
                prev_ctx.push_str(&format!("\n  Files: {}", files.join(", ")));
            }
            ready_msg.push_str(&prev_ctx);
        }
    }

    // Report active work items count at startup
    if let Some(active) = ctx.with_db("list_active_work_items", |db| {
        let filter = flowforge_core::WorkFilter {
            status: Some(flowforge_core::WorkStatus::InProgress),
            ..Default::default()
        };
        db.list_work_items(&filter)
    }) {
        if active.is_empty() && ctx.config.work_tracking.require_task {
            ready_msg.push_str(&format!(
                "\n[{backend}] No active work items. Run `flowforge work create --title \"<desc>\" --type task` before starting work."
            ));
        } else if !active.is_empty() {
            ready_msg.push_str(&format!("\n[{backend}] {} active item(s).", active.len()));
        }
    }

    let output = ContextOutput::with_context(ready_msg);
    output.write()?;
    Ok(())
}
