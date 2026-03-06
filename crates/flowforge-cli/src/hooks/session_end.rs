use chrono::Utc;
use flowforge_core::hook::SessionEndInput;
use flowforge_core::Result;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let _input = SessionEndInput::from_value(&ctx.raw)?;

    if ctx.db.is_none() {
        return Ok(());
    }

    // Capture session data BEFORE ending it (get_current_session filters by ended_at IS NULL)
    let current_session = ctx.with_db("get_current_session", |db| db.get_current_session());
    let current_session = current_session.flatten();

    // End current session
    if let Some(ref session) = current_session {
        let sid = session.id.clone();
        ctx.with_db("end_session", |db| db.end_session(&sid, Utc::now()));
    }

    // Log session end to work events (C4)
    if ctx.config.work_tracking.log_all {
        if let Some(ref session) = current_session {
            if let Some(wid) = ctx.resolve_work_item_for_task(None) {
                ctx.record_work_event(
                    &wid,
                    "session_ended",
                    None,
                    Some(&format!(
                        "edits: {}, commands: {}",
                        session.edits, session.commands
                    )),
                    Some("hook:session-end"),
                );
            }
        }
    }

    // Push FlowForge-only items to external backend (C4)
    ctx.with_db("push_to_backend", |db| {
        flowforge_core::work_tracking::push_to_backend(db, &ctx.config.work_tracking)
    });

    // Shared learning cleanup: trajectory judgment, pattern consolidation, routing feedback
    if let Some(ref session) = current_session {
        super::run_session_learning(&ctx, session);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    use flowforge_core::{FlowForgeConfig, SessionInfo, WorkItem, WorkStatus};
    use flowforge_memory::MemoryDb;

    use super::super::HookContext;

    fn test_ctx_with_db() -> (HookContext, tempfile::NamedTempFile) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };
        (ctx, tmp)
    }

    #[test]
    fn test_session_end_ends_session() {
        let (ctx, _tmp) = test_ctx_with_db();
        let session = SessionInfo {
            id: "sess-end-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 5,
            commands: 10,
            summary: None,
            transcript_path: None,
        };
        ctx.with_db("create", |db| db.create_session(&session));

        let current = ctx.with_db("get", |db| db.get_current_session()).flatten();
        assert!(current.is_some());

        ctx.with_db("end", |db| db.end_session("sess-end-1", Utc::now()));

        let current = ctx.with_db("get", |db| db.get_current_session()).flatten();
        assert!(current.is_none());
    }

    #[test]
    fn test_session_end_work_event_logging() {
        let (ctx, _tmp) = test_ctx_with_db();

        let item = WorkItem {
            id: "wi-end-1".to_string(),
            external_id: None,
            backend: "flowforge".to_string(),
            item_type: "task".to_string(),
            title: "Test task".to_string(),
            description: None,
            status: WorkStatus::InProgress,
            assignee: None,
            parent_id: None,
            priority: 2,
            labels: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            session_id: None,
            metadata: None,
            claimed_by: None,
            claimed_at: None,
            last_heartbeat: None,
            progress: 0,
            stealable: false,
        };
        ctx.with_db("create_work_item", |db| db.create_work_item(&item));

        // Should succeed with no FK error
        ctx.record_work_event("wi-end-1", "session_ended", None, Some("edits: 5, commands: 10"), Some("hook:session-end"));
    }

    #[test]
    fn test_run_session_learning_no_trajectory() {
        let (ctx, _tmp) = test_ctx_with_db();
        let session = SessionInfo {
            id: "sess-learn-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 0,
            commands: 0,
            summary: None,
            transcript_path: None,
        };
        ctx.with_db("create", |db| db.create_session(&session));

        // Should not panic when no trajectory exists
        super::super::run_session_learning(&ctx, &session);
    }

    #[test]
    fn test_run_session_learning_with_trajectory() {
        let (ctx, _tmp) = test_ctx_with_db();
        let session = SessionInfo {
            id: "sess-learn-2".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp".to_string(),
            edits: 3,
            commands: 15,
            summary: None,
            transcript_path: None,
        };
        ctx.with_db("create", |db| db.create_session(&session));

        // Create a trajectory
        ctx.with_db("create_trajectory", |db| {
            let traj = Trajectory {
                id: "traj-learn-2".to_string(),
                session_id: "sess-learn-2".to_string(),
                work_item_id: None,
                agent_name: None,
                task_description: None,
                status: TrajectoryStatus::Recording,
                started_at: Utc::now(),
                ended_at: None,
                verdict: None,
                confidence: None,
                metadata: None,
                embedding_id: None,
            };
            db.create_trajectory(&traj)
        });

        // Add a step so judgment has data
        ctx.with_db("add_step", |db| {
            db.record_trajectory_step(
                "traj-learn-2",
                "Read",
                None,
                StepOutcome::Success,
                Some(100),
            )?;
            Ok(())
        });

        // Should complete without panicking — judges trajectory + consolidates
        super::super::run_session_learning(&ctx, &session);

        // Verify trajectory was ended
        let active = ctx
            .with_db("check", |db| db.get_active_trajectory("sess-learn-2"))
            .flatten();
        assert!(active.is_none(), "trajectory should be completed after learning");
    }
}
