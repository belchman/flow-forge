pub mod notification;
pub mod post_tool_use;
pub mod post_tool_use_failure;
pub mod pre_compact;
pub mod pre_tool_use;
pub mod session_end;
pub mod session_start;
pub mod stop;
pub mod subagent_start;
pub mod subagent_stop;
pub mod task_completed;
pub mod teammate_idle;
pub mod user_prompt_submit;

/// Run a hook safely: catch errors AND panics, returning Ok(()) regardless.
/// Any stderr output causes Claude Code to display a hook error in the TUI,
/// so we must suppress everything. On error, emit a valid empty JSON response
/// so Claude Code doesn't treat missing stdout as a hook failure.
pub fn run_safe(
    hook_name: &str,
    f: impl FnOnce() -> flowforge_core::Result<()>,
) -> flowforge_core::Result<()> {
    // Kill-switch: skip all hooks for A/B benchmarking.
    // user_prompt_submit is exempt so work-tracking enforcement always fires.
    if std::env::var("FLOWFORGE_HOOKS_DISABLED").is_ok() && hook_name != "user-prompt-submit" {
        return Ok(());
    }

    let start = std::time::Instant::now();

    let succeeded = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            log_hook_error(hook_name, &format!("Error: {e}"));
            false
        }
        Err(panic_info) => {
            let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            log_hook_error(hook_name, &format!("PANIC: {msg}"));
            false
        }
    };

    // Record hook timing metrics (best-effort, never fail)
    let elapsed_ms = start.elapsed().as_millis() as f64;
    record_hook_timing(hook_name, elapsed_ms, succeeded);

    // If the hook failed without producing output, emit a valid empty response
    // so Claude Code doesn't report "hook error" from missing stdout.
    if !succeeded {
        println!("{{\"hookSpecificOutput\":{{}}}}");
    }

    Ok(())
}

/// Record hook execution timing as session metrics.
/// Best-effort: never fails or blocks the hook pipeline.
fn record_hook_timing(hook_name: &str, elapsed_ms: f64, succeeded: bool) {
    let config = match FlowForgeConfig::load(&FlowForgeConfig::config_path()) {
        Ok(c) => c,
        Err(_) => return,
    };
    let db_path = config.db_path();
    let db = match db_path.exists().then(|| MemoryDb::open(&db_path).ok()).flatten() {
        Some(d) => d,
        None => return,
    };
    let session_id = match db.get_current_session().ok().flatten() {
        Some(s) => s.id,
        None => return,
    };

    let metric = format!("hook_ms:{}", hook_name);
    let _ = db.increment_session_metric(&session_id, &metric, elapsed_ms);
    let _ = db.increment_session_metric(&session_id, &format!("hook_calls:{}", hook_name), 1.0);
    if !succeeded {
        let _ = db.increment_session_metric(&session_id, &format!("hook_errors:{}", hook_name), 1.0);
    }
}

fn log_hook_error(hook_name: &str, msg: &str) {
    use std::io::Write;
    // Write to project-local log if .flowforge/ exists, otherwise /tmp/
    let path = if std::path::Path::new(".flowforge").is_dir() {
        ".flowforge/hook-errors.log".to_string()
    } else {
        "/tmp/flowforge-hook-errors.log".to_string()
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "[{}] {}: {}", chrono::Utc::now(), hook_name, msg);
    }
}

/// Extract a meaningful task pattern from a subject/description string.
/// Filters out stop words and takes up to 5 content words for better DB cache hits.
pub fn extract_task_pattern(text: &str) -> String {
    const STOP_WORDS: &[&str] = &[
        "the", "a", "an", "in", "on", "to", "for", "of", "with", "is", "it", "and", "or", "but",
        "this", "that", "my", "your", "its", "be", "at", "by", "from", "as", "into", "about", "up",
        "out", "if", "not", "no", "so", "do", "can", "will", "just", "should", "would", "could",
        "has", "have", "had", "was", "were", "been", "being", "am", "are",
    ];
    text.to_lowercase()
        .split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
}

use flowforge_core::plugin::LoadedPlugin;
use flowforge_core::plugin_exec::exec_plugin_hook;
use flowforge_core::FlowForgeConfig;
use flowforge_memory::MemoryDb;

/// Shared context for all hooks: parses stdin, loads config, opens DB once.
#[allow(dead_code)]
pub struct HookContext {
    pub raw: serde_json::Value,
    pub config: FlowForgeConfig,
    pub db: Option<MemoryDb>,
    pub session_id: Option<String>,
}

impl HookContext {
    /// Parse stdin, load config, open DB — all in one place.
    pub fn init() -> flowforge_core::Result<Self> {
        let raw = flowforge_core::hook::parse_stdin_value()?;
        let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
        let db_path = config.db_path();
        let db = if db_path.exists() {
            MemoryDb::open(&db_path).ok()
        } else {
            None
        };
        let session_id = std::env::var("CLAUDE_SESSION_ID").ok().or_else(|| {
            raw.get("sessionId")
                .and_then(|v| v.as_str())
                .map(String::from)
        });
        Ok(Self {
            raw,
            config,
            db,
            session_id,
        })
    }

    /// Run a closure with the DB, logging errors instead of swallowing them.
    /// Returns None if no DB is available or if the operation fails.
    pub fn with_db<F, T>(&self, op: &str, f: F) -> Option<T>
    where
        F: FnOnce(&MemoryDb) -> flowforge_core::Result<T>,
    {
        let db = self.db.as_ref()?;
        match f(db) {
            Ok(val) => Some(val),
            Err(e) => {
                log_hook_error(op, &format!("{e}"));
                None
            }
        }
    }

    /// Record a work event — eliminates duplicate WorkEvent constructions across hooks.
    pub fn record_work_event(
        &self,
        work_item_id: &str,
        event_type: &str,
        old_value: Option<&str>,
        new_value: Option<&str>,
        actor: Option<&str>,
    ) {
        self.with_db(&format!("record_work_event:{event_type}"), |db| {
            let event = flowforge_core::WorkEvent {
                id: 0,
                work_item_id: work_item_id.to_string(),
                event_type: event_type.to_string(),
                old_value: old_value.map(String::from),
                new_value: new_value.map(String::from),
                actor: actor.map(String::from),
                timestamp: chrono::Utc::now(),
            };
            db.record_work_event(&event)?;
            Ok(())
        });
    }

    /// Resolve a work item ID for a Claude task.
    /// 1. Try to match `task_subject` to a work item title.
    /// 2. Fall back to any in-progress work item.
    pub fn resolve_work_item_for_task(&self, task_subject: Option<&str>) -> Option<String> {
        // Try title-based match first
        if let Some(subject) = task_subject {
            if let Some(Some(item)) = self.with_db("find_work_item_by_title", |db| {
                db.get_work_item_by_title(subject)
            }) {
                return Some(item.id);
            }
        }

        // Fall back to any in-progress item
        let found = self.with_db("find_active_work_item", |db| {
            let filter = flowforge_core::WorkFilter {
                status: Some(flowforge_core::WorkStatus::InProgress),
                ..Default::default()
            };
            let items = db.list_work_items(&filter)?;
            Ok(items.into_iter().next().map(|i| i.id))
        });
        found.flatten()
    }

    /// Record routing outcome — used in session_end and task_completed.
    #[allow(dead_code)]
    pub fn record_routing_outcome(&self, task_desc: &str, agent: &str, success: bool) {
        if !self.config.hooks.learning {
            return;
        }
        self.with_db("record_routing_outcome", |db| {
            if success {
                db.record_routing_success(task_desc, agent)
            } else {
                db.record_routing_failure(task_desc, agent)
            }
        });
    }
}

/// Shared session cleanup logic: trajectory judgment, pattern consolidation,
/// routing feedback, file co-edits. Called from both session_end and stop hooks.
pub fn run_session_learning(ctx: &HookContext, session: &flowforge_core::SessionInfo) {
    let sid = session.id.clone();

    // Trajectory judgment
    ctx.with_db("trajectory_judgment", |db| {
        let trajectory = match db.get_active_trajectory(&sid)? {
            Some(t) => t,
            None => return Ok(()),
        };

        use flowforge_core::trajectory::TrajectoryStatus;
        db.end_trajectory(&trajectory.id, TrajectoryStatus::Completed)?;

        use flowforge_memory::trajectory::TrajectoryJudge;
        let judge = TrajectoryJudge::new(db, &ctx.config.patterns);
        let result = match judge.judge(&trajectory.id) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };

        if result.verdict == flowforge_core::trajectory::TrajectoryVerdict::Success {
            let _ = judge.distill(&trajectory.id);
        }

        // Auto-detect error resolutions from trajectory steps
        let _ = db.auto_detect_resolutions(&sid, &trajectory.id);

        // Effectiveness feedback: routing accuracy + pattern confidence
        if let Ok(injections) = db.get_injections_for_session(&sid) {
            for inj in injections.iter().filter(|i| i.injection_type == "routing") {
                let hit = trajectory
                    .agent_name
                    .as_ref()
                    .map(|a| a.eq_ignore_ascii_case(&inj.reference_id))
                    .unwrap_or(false);
                let _ =
                    db.set_meta(&format!("routing_hit:{}", sid), if hit { "1" } else { "0" });
            }

            use flowforge_core::trajectory::TrajectoryVerdict;
            let store = flowforge_memory::PatternStore::new(db, &ctx.config.patterns);
            match result.verdict {
                TrajectoryVerdict::Success => {
                    for inj in injections.iter().filter(|i| i.injection_type == "pattern") {
                        let _ = store.record_feedback(&inj.reference_id, true);
                    }
                }
                TrajectoryVerdict::Failure => {
                    for inj in injections.iter().filter(|i| i.injection_type == "pattern") {
                        let _ = store.record_feedback(&inj.reference_id, false);
                    }
                }
                TrajectoryVerdict::Partial => {}
            }
        }

        // Auto-rate context injections
        {
            use flowforge_core::trajectory::TrajectoryVerdict;
            let rating = match result.verdict {
                TrajectoryVerdict::Success => "correlated_success",
                TrajectoryVerdict::Failure => "correlated_failure",
                TrajectoryVerdict::Partial => "correlated_partial",
            };
            let _ = db.rate_session_injections(&sid, rating);
        }

        // Feed verdict back to routing weights (old + new systems)
        if let (Some(ref agent_name), Some(ref task_desc)) =
            (&trajectory.agent_name, &trajectory.task_description)
        {
            let pattern = extract_task_pattern(task_desc);
            if !pattern.is_empty() {
                use flowforge_core::trajectory::TrajectoryVerdict;

                // Old system: scalar routing weights (backward compat)
                match result.verdict {
                    TrajectoryVerdict::Success => {
                        let _ = db.record_routing_success(&pattern, agent_name);
                    }
                    TrajectoryVerdict::Failure => {
                        let _ = db.record_routing_failure(&pattern, agent_name);
                    }
                    TrajectoryVerdict::Partial => {}
                }

                // New system: record_routing_outcome with full signal breakdown
                let outcome_str = match result.verdict {
                    TrajectoryVerdict::Success => "success",
                    TrajectoryVerdict::Failure => "failure",
                    TrajectoryVerdict::Partial => "partial",
                };

                // Look up stored RoutingBreakdown from context_injection metadata
                if let Ok(injections) = db.get_injections_for_session(&sid) {
                    if let Some(routing_inj) = injections.iter().find(|i| i.injection_type == "routing") {
                        if let Some(ref metadata) = routing_inj.metadata {
                            if let Ok(breakdown) = serde_json::from_str::<flowforge_core::RoutingBreakdown>(metadata) {
                                let _ = db.record_routing_outcome(
                                    &sid,
                                    agent_name,
                                    &pattern,
                                    breakdown.pattern_score,
                                    breakdown.capability_score,
                                    breakdown.learned_score,
                                    breakdown.priority_score,
                                    breakdown.context_score,
                                    breakdown.semantic_score,
                                    outcome_str,
                                );
                            }
                        } else {
                            // No breakdown stored — record with zero scores
                            let _ = db.record_routing_outcome(
                                &sid, agent_name, &pattern,
                                0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                                outcome_str,
                            );
                        }
                    }
                }

                // Tier 5B: Store outcome-aware routing vectors
                let config_for_embed = flowforge_core::config::PatternsConfig::default();
                let embedding = flowforge_memory::default_embedder(&config_for_embed);
                let vec = embedding.embed(&pattern);
                let source_id = format!("{}::{}", pattern, agent_name);
                let _ = db.store_vector("routing", &source_id, &vec);

                // Store as routing_success or routing_failure for few-shot lookup
                let outcome_source = match result.verdict {
                    TrajectoryVerdict::Success => "routing_success",
                    TrajectoryVerdict::Failure => "routing_failure",
                    TrajectoryVerdict::Partial => "routing",
                };
                if outcome_source != "routing" {
                    let _ = db.store_vector(outcome_source, &source_id, &vec);
                }

                // Tier 1C: Trigger adaptive weight recomputation if enough data
                if let Ok(count) = db.count_routing_outcomes() {
                    if count >= 10 {
                        let _ = db.compute_adaptive_weights(30);
                    }
                }
            }
        }

        let _ = judge.consolidate();
        Ok(())
    });

    // Pattern consolidation
    if ctx.config.hooks.learning {
        ctx.with_db("pattern_consolidation", |db| {
            db.with_transaction(|| {
                let store = flowforge_memory::PatternStore::new(db, &ctx.config.patterns);
                store.consolidate()
            })
        });

        // Record file co-edit pairs
        let sid2 = session.id.clone();
        ctx.with_db("record_file_co_edits", |db| {
            db.record_file_co_edits(&sid2).map(|_| ())
        });

        // Retention pruning: remove old data from append-only tables
        let retention_days = ctx.config.memory.retention_days;
        if retention_days > 0 {
            ctx.with_db("retention_pruning", |db| {
                db.prune_old_data(retention_days).map(|_| ())
            });
        }

        // Self-tuning: analyze pattern injection effectiveness across sessions
        // and auto-adjust the similarity threshold if data warrants it
        ctx.with_db("self_tune_injection_threshold", |db| {
            let stats = db.get_injection_effectiveness_stats("pattern")?;
            let mut successes = 0u64;
            let mut failures = 0u64;
            for (rating, count) in &stats {
                match rating.as_str() {
                    "correlated_success" => successes += count,
                    "correlated_failure" => failures += count,
                    _ => {}
                }
            }
            let total = successes + failures;
            if total < 20 {
                return Ok(()); // Not enough data to tune
            }
            let success_rate = successes as f64 / total as f64;

            // If pattern injections are mostly failing, raise the similarity threshold
            // If they're succeeding well, lower it slightly to capture more matches
            let current = ctx.config.patterns.min_injection_similarity;
            let adjusted = if success_rate < 0.4 {
                (current + 0.05).min(0.85) // Tighten: fewer, higher-quality injections
            } else if success_rate > 0.75 {
                (current - 0.02).max(0.30) // Loosen: capture more matches
            } else {
                return Ok(()); // In sweet spot, no change needed
            };

            if (adjusted - current).abs() > 0.001 {
                db.set_meta("tuned_min_injection_similarity", &adjusted.to_string())?;
            }
            Ok(())
        });
    }
}

/// Run plugin hooks for a given event. Returns first deny/ask response if any.
pub fn run_plugin_hooks(
    event: &str,
    raw_input: &serde_json::Value,
    plugins: &[LoadedPlugin],
    _plugin_dir: &std::path::Path,
) -> Option<serde_json::Value> {
    let mut hooks: Vec<_> = plugins
        .iter()
        .flat_map(|p| {
            p.manifest.hooks.iter().filter_map(move |h| {
                if h.event.eq_ignore_ascii_case(event) {
                    Some((h.priority, &h.command, &p.dir))
                } else {
                    None
                }
            })
        })
        .collect();
    hooks.sort_by_key(|(pri, _, _)| *pri);

    for (_, command, dir) in hooks {
        if let Some(response) = exec_plugin_hook(command, dir, raw_input, 5000) {
            // Check if response indicates deny or ask
            if let Some(action) = response.get("action").and_then(|v| v.as_str()) {
                if action == "deny" || action == "ask" {
                    return Some(response);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_task_pattern tests ──

    #[test]
    fn test_extract_task_pattern_filters_stop_words() {
        assert_eq!(
            extract_task_pattern("fix the bug in the parser"),
            "fix bug parser"
        );
    }

    #[test]
    fn test_extract_task_pattern_limits_to_5_words() {
        assert_eq!(
            extract_task_pattern("implement new feature for user authentication system"),
            "implement new feature user authentication"
        );
    }

    #[test]
    fn test_extract_task_pattern_empty_input() {
        assert_eq!(extract_task_pattern(""), "");
    }

    #[test]
    fn test_extract_task_pattern_all_stop_words() {
        assert_eq!(extract_task_pattern("the a an in on to"), "");
    }

    #[test]
    fn test_extract_task_pattern_preserves_case_lowered() {
        assert_eq!(
            extract_task_pattern("Fix Authentication Bug"),
            "fix authentication bug"
        );
    }

    // ── HookContext::with_db tests ──

    #[test]
    fn test_with_db_returns_none_when_db_is_none() {
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: None,
            session_id: None,
        };
        let result = ctx.with_db("test_op", |_db| Ok(42));
        assert!(result.is_none());
    }

    #[test]
    fn test_with_db_returns_value_on_success() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };
        let result = ctx.with_db("test_op", |_db| Ok(42));
        assert_eq!(result, Some(42));
    }

    #[test]
    fn test_with_db_returns_none_on_error() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };
        let result: Option<i32> = ctx.with_db("test_op", |_db| {
            Err(flowforge_core::Error::Config("test error".to_string()))
        });
        assert!(result.is_none());
    }

    // ── HookContext::record_work_event test ──

    #[test]
    fn test_record_work_event_with_db() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };
        // Should not panic, event is recorded
        ctx.record_work_event("item-1", "test_event", None, Some("value"), Some("actor"));
    }

    #[test]
    fn test_record_work_event_without_db() {
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: None,
            session_id: None,
        };
        // Should not panic when db is None
        ctx.record_work_event("item-1", "test_event", None, Some("value"), Some("actor"));
    }

    // ── HookContext session_id extraction tests ──

    #[test]
    fn test_session_id_from_raw_json() {
        let ctx = HookContext {
            raw: serde_json::json!({"sessionId": "test-session-123"}),
            config: FlowForgeConfig::default(),
            db: None,
            session_id: Some("test-session-123".to_string()),
        };
        assert_eq!(ctx.session_id.as_deref(), Some("test-session-123"));
    }

    #[test]
    fn test_session_id_none_when_absent() {
        let ctx = HookContext {
            raw: serde_json::json!({"other": "data"}),
            config: FlowForgeConfig::default(),
            db: None,
            session_id: None,
        };
        assert!(ctx.session_id.is_none());
    }

    // ── run_plugin_hooks test ──

    #[test]
    fn test_run_plugin_hooks_empty_plugins() {
        let result = run_plugin_hooks(
            "PreToolUse",
            &serde_json::json!({"tool_name": "Read"}),
            &[],
            std::path::Path::new("/tmp"),
        );
        assert!(result.is_none());
    }

    // ── run_safe tests ──

    #[test]
    fn test_run_safe_catches_errors() {
        let result = run_safe("test-hook", || {
            Err(flowforge_core::Error::Config("test error".into()))
        });
        assert!(result.is_ok()); // run_safe always returns Ok
    }

    #[test]
    fn test_run_safe_catches_panics() {
        let result = run_safe("test-hook", || {
            panic!("test panic");
        });
        assert!(result.is_ok()); // run_safe catches panics too
    }

    // ── record_routing_outcome test ──

    #[test]
    fn test_record_routing_outcome_skips_when_learning_disabled() {
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(), // learning defaults to true
            db: None,
            session_id: None,
        };
        // Should not panic — just returns early since db is None
        ctx.record_routing_outcome("fix bug", "debugger", true);
    }

    // ── Phase 1: FK error fix tests ──

    #[test]
    fn test_record_work_event_with_valid_work_item() {
        use flowforge_core::{WorkFilter, WorkItem};
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        // Create a real work item so FK won't fail
        let item = WorkItem {
            id: "wi-test-fk".to_string(),
            external_id: None,
            backend: "flowforge".to_string(),
            item_type: "task".to_string(),
            title: "Test task".to_string(),
            description: None,
            status: flowforge_core::WorkStatus::InProgress,
            assignee: None,
            parent_id: None,
            priority: 2,
            labels: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
            session_id: None,
            metadata: None,
            claimed_by: None,
            claimed_at: None,
            last_heartbeat: None,
            progress: 0,
            stealable: false,
        };
        db.create_work_item(&item).unwrap();

        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };

        // Look up the work item (simulating what the fixed subagent hooks do)
        let work_item_id = ctx.with_db("find_active_work_item", |db| {
            let filter = WorkFilter {
                status: Some(flowforge_core::WorkStatus::InProgress),
                ..Default::default()
            };
            let items = db.list_work_items(&filter)?;
            Ok(items.into_iter().next().map(|i| i.id))
        });
        assert_eq!(work_item_id, Some(Some("wi-test-fk".to_string())));

        // Now record the event with the correct work_item_id (should succeed, no FK error)
        if let Some(Some(wid)) = work_item_id {
            ctx.record_work_event(
                &wid,
                "agent_started",
                None,
                Some("general"),
                Some("agent:test"),
            );
        }
    }

    // ── resolve_work_item_for_task tests ──

    #[test]
    fn test_resolve_work_item_by_title_match() {
        use flowforge_core::WorkItem;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();

        // Create a work item with a specific title
        let mut item = WorkItem {
            id: "wi-resolve-1".to_string(),
            external_id: None,
            backend: "flowforge".to_string(),
            item_type: "task".to_string(),
            title: "Implement dark mode".to_string(),
            description: None,
            status: flowforge_core::WorkStatus::InProgress,
            assignee: None,
            parent_id: None,
            priority: 2,
            labels: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
            session_id: None,
            metadata: None,
            claimed_by: None,
            claimed_at: None,
            last_heartbeat: None,
            progress: 0,
            stealable: false,
        };
        db.create_work_item(&item).unwrap();

        // Create another in-progress item (fallback candidate)
        item.id = "wi-resolve-2".to_string();
        item.title = "Fix tests".to_string();
        db.create_work_item(&item).unwrap();

        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };

        // Title match should find the specific item
        let resolved = ctx.resolve_work_item_for_task(Some("Implement dark mode"));
        assert_eq!(resolved, Some("wi-resolve-1".to_string()));
    }

    #[test]
    fn test_resolve_work_item_falls_back_to_in_progress() {
        use flowforge_core::WorkItem;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();

        let item = WorkItem {
            id: "wi-fallback-1".to_string(),
            external_id: None,
            backend: "flowforge".to_string(),
            item_type: "task".to_string(),
            title: "Some other task".to_string(),
            description: None,
            status: flowforge_core::WorkStatus::InProgress,
            assignee: None,
            parent_id: None,
            priority: 2,
            labels: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            completed_at: None,
            session_id: None,
            metadata: None,
            claimed_by: None,
            claimed_at: None,
            last_heartbeat: None,
            progress: 0,
            stealable: false,
        };
        db.create_work_item(&item).unwrap();

        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };

        // No title match → falls back to any in-progress item
        let resolved = ctx.resolve_work_item_for_task(Some("Non-existent task"));
        assert_eq!(resolved, Some("wi-fallback-1".to_string()));
    }

    #[test]
    fn test_resolve_work_item_none_when_empty_db() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();

        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };

        let resolved = ctx.resolve_work_item_for_task(Some("Any task"));
        assert!(resolved.is_none());
    }

    #[test]
    fn test_with_db_logs_errors() {
        // with_db already logs errors via log_hook_error — verify it returns None on error
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };
        let result: Option<i32> = ctx.with_db("test_error_logging", |_db| {
            Err(flowforge_core::Error::Config(
                "intentional test error".to_string(),
            ))
        });
        assert!(result.is_none()); // Error was logged and None returned
    }

    #[test]
    fn test_record_work_event_skips_when_no_work_item() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let ctx = HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: None,
        };

        // No work items in DB — lookup returns None, so no event is recorded
        let work_item_id = ctx.with_db("find_active_work_item", |db| {
            let filter = flowforge_core::WorkFilter {
                status: Some(flowforge_core::WorkStatus::InProgress),
                ..Default::default()
            };
            let items = db.list_work_items(&filter)?;
            Ok(items.into_iter().next().map(|i| i.id))
        });
        assert_eq!(work_item_id, Some(None));
        // No event recorded — no FK error
    }
}
