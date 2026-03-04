use chrono::Utc;
use flowforge_core::hook::{self, SessionEndInput};
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let _input = SessionEndInput::from_value(&v)?;
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    let db_path = config.db_path();
    if !db_path.exists() {
        return Ok(());
    }

    let db = MemoryDb::open(&db_path)?;

    // Capture session data BEFORE ending it (get_current_session filters by ended_at IS NULL)
    let current_session = db.get_current_session().ok().flatten();

    // Ingest transcript before ending session
    if let Some(ref session) = current_session {
        // Try transcript_path from session record, or from hook input
        let transcript = session
            .transcript_path
            .as_deref()
            .or(_input.common.transcript_path.as_deref());
        if let Some(path) = transcript {
            let _ = db.ingest_transcript(&session.id, path);
        }
    }

    // End current session
    if let Some(ref session) = current_session {
        db.end_session(&session.id, Utc::now())?;
    }

    // Log session end to work events (C4) — uses captured session data
    if config.work_tracking.log_all {
        if let Some(ref session) = current_session {
            let event = flowforge_core::WorkEvent {
                id: 0,
                work_item_id: session.id.clone(),
                event_type: "session_ended".to_string(),
                old_value: None,
                new_value: Some(format!(
                    "edits: {}, commands: {}",
                    session.edits, session.commands
                )),
                actor: Some("hook:session-end".to_string()),
                timestamp: chrono::Utc::now(),
            };
            let _ = db.record_work_event(&event);
        }
    }

    // Push FlowForge-only items to external backend (C4)
    let _ = flowforge_core::work_tracking::push_to_backend(&db, &config.work_tracking);

    // Close active trajectory, judge it, distill if successful
    if let Some(ref session) = current_session {
        if let Ok(Some(trajectory)) = db.get_active_trajectory(&session.id) {
            use flowforge_core::trajectory::TrajectoryStatus;
            let _ = db.end_trajectory(&trajectory.id, TrajectoryStatus::Completed);

            // Judge and distill, then feed verdict back to routing weights
            use flowforge_memory::trajectory::TrajectoryJudge;
            let judge = TrajectoryJudge::new(&db, &config.patterns);
            if let Ok(result) = judge.judge(&trajectory.id) {
                if result.verdict == flowforge_core::trajectory::TrajectoryVerdict::Success {
                    let _ = judge.distill(&trajectory.id);
                }

                // Effectiveness feedback: routing accuracy + pattern confidence boost
                if let Ok(injections) = db.get_injections_for_session(&session.id) {
                    // Routing accuracy: compare suggested agent vs actual
                    for inj in injections.iter().filter(|i| i.injection_type == "routing") {
                        let hit = trajectory
                            .agent_name
                            .as_ref()
                            .map(|a| a.eq_ignore_ascii_case(&inj.reference_id))
                            .unwrap_or(false);
                        let _ = db.set_meta(
                            &format!("routing_hit:{}", session.id),
                            if hit { "1" } else { "0" },
                        );
                    }

                    // Pattern confidence boost on success
                    if result.verdict == flowforge_core::trajectory::TrajectoryVerdict::Success {
                        let store = flowforge_memory::PatternStore::new(&db, &config.patterns);
                        for inj in injections.iter().filter(|i| i.injection_type == "pattern") {
                            let _ = store.record_feedback(&inj.reference_id, true);
                        }
                    }
                }

                // Auto-rate all context injections based on trajectory verdict
                {
                    use flowforge_core::trajectory::TrajectoryVerdict;
                    let rating = match result.verdict {
                        TrajectoryVerdict::Success => "correlated_success",
                        TrajectoryVerdict::Failure => "correlated_failure",
                        TrajectoryVerdict::Partial => "correlated_partial",
                    };
                    let _ = db.rate_session_injections(&session.id, rating);
                }

                // Feed verdict back to routing weights (close the learning loop)
                if let (Some(ref agent_name), Some(ref task_desc)) =
                    (&trajectory.agent_name, &trajectory.task_description)
                {
                    let pattern = crate::hooks::extract_task_pattern(task_desc);
                    if !pattern.is_empty() {
                        use flowforge_core::trajectory::TrajectoryVerdict;
                        match result.verdict {
                            TrajectoryVerdict::Success => {
                                let _ = db.record_routing_success(&pattern, agent_name);
                            }
                            TrajectoryVerdict::Failure => {
                                let _ = db.record_routing_failure(&pattern, agent_name);
                            }
                            TrajectoryVerdict::Partial => {} // avoid noise
                        }
                        // Store routing embedding for similarity-based generalization
                        let config_for_embed = flowforge_core::config::PatternsConfig::default();
                        let embedding = flowforge_memory::default_embedder(&config_for_embed);
                        let vec = embedding.embed(&pattern);
                        let source_id = format!("{}::{}", pattern, agent_name);
                        let _ = db.store_vector("routing", &source_id, &vec);
                    }
                }
            }

            // Consolidate old trajectories
            let _ = judge.consolidate();
        }
    }

    // Run pattern consolidation
    if config.hooks.learning {
        let store = flowforge_memory::PatternStore::new(&db, &config.patterns);
        let _ = store.consolidate();
    }

    Ok(())
}
