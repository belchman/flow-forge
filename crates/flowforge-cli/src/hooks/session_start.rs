use chrono::Utc;
use flowforge_core::hook::{self, ContextOutput, SessionStartInput};
use flowforge_core::trajectory::{Trajectory, TrajectoryStatus};
use flowforge_core::{FlowForgeConfig, Result, SessionInfo};
use flowforge_memory::MemoryDb;

pub fn run() -> Result<()> {
    // Drain stdin (required — Claude Code sends JSON on stdin)
    let v = hook::parse_stdin_value()?;
    let input = SessionStartInput::from_value(&v)?;

    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db_path = config.db_path();

    // Create DB state that session_end/other hooks expect
    if let Some(ref session_id) = input.session_id.or(input.common.session_id.clone()) {
        if db_path.exists()
            || std::fs::create_dir_all(db_path.parent().unwrap_or(".".as_ref())).is_ok()
        {
            if let Ok(db) = MemoryDb::open(&db_path) {
                let now = Utc::now();

                // Create session record
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
                let _ = db.create_session(&session);

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
                let _ = db.create_trajectory(&trajectory);

                // Initialize trust score
                let _ = db.create_trust_score(session_id, config.guidance.trust_initial_score);

                // Sync work items from external backend and write to Claude Tasks
                let _ =
                    flowforge_core::work_tracking::sync_from_backend(&db, &config.work_tracking);
                let _ = flowforge_core::work_tracking::sync_all_to_claude_tasks(
                    &db,
                    &config.work_tracking,
                );

                // Log session start event
                if config.work_tracking.log_all {
                    let event = flowforge_core::WorkEvent {
                        id: 0,
                        work_item_id: session_id.clone(),
                        event_type: "session_started".to_string(),
                        old_value: None,
                        new_value: Some(format!("source: {:?}", input.source)),
                        actor: Some("hook:session-start".to_string()),
                        timestamp: now,
                    };
                    let _ = db.record_work_event(&event);
                }
            }
        }
    }

    let output = ContextOutput::with_context("[FlowForge] Ready.".to_string());
    output.write()?;
    Ok(())
}
