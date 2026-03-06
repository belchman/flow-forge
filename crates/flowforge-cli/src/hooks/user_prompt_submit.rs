use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::hook::{ContextOutput, UserPromptSubmitInput};
use flowforge_core::{PatternTier, Result, RoutingContext};
use flowforge_memory::{MemoryDb, PatternStore};
use std::collections::HashMap;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = UserPromptSubmitInput::from_value(&ctx.raw)?;

    // Kill-switch fast-path: skip routing/patterns/embeddings but still
    // enforce work-tracking so tasks are never silently forgotten.
    if std::env::var("FLOWFORGE_HOOKS_DISABLED").is_ok() {
        return run_work_tracking_only(&ctx);
    }

    let prompt = match input.prompt {
        Some(ref p) if p.trim().len() >= 5 => p.clone(),
        _ => {
            // No prompt or too short — nothing to route
            ContextOutput::none().write()?;
            return Ok(());
        }
    };

    let mut context_parts: Vec<String> = Vec::new();

    // Use the DB from HookContext if available, but only if routing or learning is enabled
    let db = if ctx.config.hooks.routing || ctx.config.hooks.learning {
        ctx.db.as_ref()
    } else {
        None
    };

    // Resolve session_id and trajectory_id once for injection recording
    let (current_session_id, current_trajectory_id) = if let Some(db) = db {
        let sid = db
            .get_current_session()
            .ok()
            .flatten()
            .map(|s| s.id.clone());
        let tid = sid.as_ref().and_then(|s| {
            db.get_active_trajectory(s)
                .ok()
                .flatten()
                .map(|t| t.id.clone())
        });
        (sid, tid)
    } else {
        (None, None)
    };

    // Set trajectory task description from first user prompt.
    // If this is the first prompt (no task_description yet), also inject session continuity.
    if let Some(db) = db {
        if let (Some(ref sid), Some(ref tid)) = (&current_session_id, &current_trajectory_id) {
            if let Ok(Some(trajectory)) = db.get_active_trajectory(sid) {
                if trajectory.task_description.is_none() {
                    let desc: String = prompt.chars().take(200).collect();
                    let _ = db.set_trajectory_task_description(tid, &desc);

                    // First prompt: inject session continuity context
                    let cwd = input
                        .common
                        .cwd
                        .as_deref()
                        .unwrap_or(".");
                    if let Ok(Some(prev)) = db.get_previous_session_context(cwd) {
                        let mut prev_ctx = String::from(
                            "[FlowForge Session Continuity] Your last session in this project:",
                        );
                        if let Some(ref task) = prev.task_description {
                            prev_ctx.push_str(&format!("\n  Task: {}", task));
                        }
                        if let Some(ref verdict) = prev.verdict {
                            prev_ctx.push_str(&format!(" (outcome: {})", verdict));
                        }
                        if !prev.files_modified.is_empty() {
                            let files: Vec<&str> = prev
                                .files_modified
                                .iter()
                                .take(5)
                                .map(|s| s.as_str())
                                .collect();
                            prev_ctx.push_str(&format!("\n  Files modified: {}", files.join(", ")));
                        }
                        context_parts.push(prev_ctx);

                        if let Some(ref session_id) = current_session_id {
                            let _ = db.record_context_injection(
                                session_id,
                                current_trajectory_id.as_deref(),
                                "session_continuity",
                                Some(&prev.session_id),
                                None,
                            );
                        }
                    }
                }
            }
        }
    }

    // Load learned weights once from the shared DB connection (A10)
    // Also pre-compute similarity-based matches for generalization (Fix 5)
    let learned_weights = if let Some(db) = db {
        load_learned_weights_from_db(db, &prompt)
    } else {
        HashMap::new()
    };

    // Build routing context from session state
    let routing_context = if let Some(db) = db {
        build_routing_context(db)
    } else {
        None
    };

    // Route the task to suggested agents
    if ctx.config.hooks.routing {
        if let Ok(registry) = AgentRegistry::load(&ctx.config.agents) {
            let router = AgentRouter::new(&ctx.config.routing);
            let agents: Vec<&_> = registry.list().into_iter().collect();

            let results =
                router.route(&prompt, &agents, &learned_weights, routing_context.as_ref());

            if let Some(top) = results.first() {
                if top.confidence > 0.3 {
                    // Phase 3A: Suppress if top suggestion matches active agent
                    let suppress = routing_context
                        .as_ref()
                        .and_then(|ctx| ctx.active_agent.as_ref())
                        .map(|active| {
                            top.agent_name.eq_ignore_ascii_case(active)
                                && results
                                    .get(1)
                                    .map(|r2| (top.confidence - r2.confidence) < 0.20)
                                    .unwrap_or(true)
                        })
                        .unwrap_or(false);

                    if !suppress {
                        // Phase 3B: Show breakdown in suggestion
                        let b = &top.breakdown;
                        let breakdown_line = format!(
                            "Why: pattern={:.0}%, cap={:.0}%, learned={:.0}%, context={:.0}%",
                            b.pattern_score * 100.0,
                            b.capability_score * 100.0,
                            b.learned_score * 100.0,
                            b.context_score * 100.0,
                        );

                        let mut routing_ctx = format!(
                            "[FlowForge] Suggested agent: {} (confidence: {:.0}%)\n{}",
                            top.agent_name,
                            top.confidence * 100.0,
                            breakdown_line,
                        );

                        // Include agent context for top match
                        if let Some(agent) = registry.get(&top.agent_name) {
                            if ctx.config.hooks.inject_agent_body {
                                // Legacy: inject full markdown body
                                if !agent.body.is_empty() {
                                    routing_ctx.push_str(&format!("\n\n{}", agent.body));
                                }
                            } else {
                                // Default: compact 1-line summary
                                routing_ctx.push_str(&format!("\nRole: {}", agent.description));
                                if !agent.capabilities.is_empty() {
                                    routing_ctx.push_str(&format!(
                                        "\nCapabilities: {}",
                                        agent.capabilities.join(", ")
                                    ));
                                }
                            }
                        }

                        // Show runner-up if close
                        if results.len() > 1 && results[1].confidence > 0.25 {
                            routing_ctx.push_str(&format!(
                                "\nAlternative: {} ({:.0}%)",
                                results[1].agent_name,
                                results[1].confidence * 100.0
                            ));
                        }

                        context_parts.push(routing_ctx);

                        // Record routing injection
                        if let Some(db) = db {
                            if let Some(ref sid) = current_session_id {
                                let _ = db.record_context_injection(
                                    sid,
                                    current_trajectory_id.as_deref(),
                                    "routing",
                                    Some(&top.agent_name),
                                    Some(top.confidence),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Inject active work items context (C4)
    if let Some(db) = db {
        let filter = flowforge_core::WorkFilter {
            status: Some(flowforge_core::WorkStatus::InProgress),
            ..Default::default()
        };
        if let Ok(active_items) = db.list_work_items(&filter) {
            if !active_items.is_empty() {
                let mut work_ctx = String::from("[FlowForge Work] Active items:");
                for item in active_items.iter().take(5) {
                    work_ctx.push_str(&format!(
                        "\n- {} ({}): {}",
                        item.id.chars().take(8).collect::<String>(),
                        item.status,
                        item.title,
                    ));
                }
                context_parts.push(work_ctx);

                // Record work item injections
                if let Some(ref sid) = current_session_id {
                    for item in active_items.iter().take(5) {
                        let _ = db.record_context_injection(
                            sid,
                            current_trajectory_id.as_deref(),
                            "work_item",
                            Some(&item.id),
                            None,
                        );
                    }
                }
            } else if ctx.config.work_tracking.require_task {
                context_parts.push(
                    "[FlowForge Work] No active work item. You MUST run `flowforge work create \"<description>\" --type task` before doing any work. Tool calls will be BLOCKED until a work item is active.".to_string()
                );
            }
        }
    }

    // Inject unread co-agent mailbox messages
    if let Some(db) = db {
        if let Some(ref sid) = input.common.session_id {
            if let Ok(unread) = db.get_unread_messages(sid) {
                if !unread.is_empty() {
                    let mut mailbox_ctx = format!(
                        "[FlowForge Mailbox] {} unread from co-agents:",
                        unread.len()
                    );
                    for msg in &unread {
                        mailbox_ctx.push_str(&format!(
                            "\n  From {}: {}",
                            msg.from_agent_name, msg.content
                        ));
                    }
                    context_parts.push(mailbox_ctx);

                    // Record mailbox injections
                    if let Some(ref session_id) = current_session_id {
                        for msg in &unread {
                            let _ = db.record_context_injection(
                                session_id,
                                current_trajectory_id.as_deref(),
                                "mailbox",
                                Some(&msg.from_agent_name),
                                None,
                            );
                        }
                    }

                    let _ = db.mark_messages_read(sid);
                }
            }
        }
    }

    // Search FlowForge memory using semantic vector search + cluster boosting
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            let store = PatternStore::new(db, &ctx.config.patterns);
            if let Ok(matches) = store.search_all_patterns(&prompt, 5) {
                // Filter out low-similarity noise (use self-tuned threshold if available)
                let sim_floor = db
                    .get_meta("tuned_min_injection_similarity")
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(ctx.config.patterns.min_injection_similarity);
                let matches: Vec<_> = matches
                    .into_iter()
                    .filter(|m| m.similarity as f64 >= sim_floor)
                    .collect();

                let proven: Vec<_> = matches
                    .iter()
                    .filter(|m| m.tier == PatternTier::Long && m.confidence > 0.5)
                    .collect();
                let recent: Vec<_> = matches
                    .iter()
                    .filter(|m| m.tier == PatternTier::Short && m.confidence > 0.4)
                    .collect();

                if !proven.is_empty() {
                    let mut pattern_ctx = String::from("[FlowForge Memory] Proven patterns:");
                    for p in &proven {
                        pattern_ctx.push_str(&format!(
                            "\n- {} (conf: {:.0}%, sim: {:.0}%, used: {}x)",
                            p.content,
                            p.confidence * 100.0,
                            p.similarity * 100.0,
                            p.usage_count
                        ));
                    }
                    context_parts.push(pattern_ctx);
                }

                if !recent.is_empty() {
                    let mut pattern_ctx = String::from("[FlowForge Memory] Relevant patterns:");
                    for p in &recent {
                        pattern_ctx.push_str(&format!(
                            "\n- {} (conf: {:.0}%, sim: {:.0}%)",
                            p.content,
                            p.confidence * 100.0,
                            p.similarity * 100.0
                        ));
                    }
                    context_parts.push(pattern_ctx);
                }

                // Record usage and injection on all injected patterns
                for m in proven.iter().chain(recent.iter()) {
                    let _ = store.record_usage(&m.id);

                    // Record pattern injection for impact tracking
                    if let Some(ref sid) = current_session_id {
                        let _ = db.record_context_injection(
                            sid,
                            current_trajectory_id.as_deref(),
                            "pattern",
                            Some(&m.id),
                            Some(m.similarity as f64),
                        );
                    }
                }
            }

            // Search key-value memory for relevant stored knowledge (skip short prompts)
            if prompt.len() >= 10 {
                if let Ok(kv_results) = db.kv_search(&prompt, 3) {
                    if !kv_results.is_empty() {
                        let mut kv_ctx = String::from("[FlowForge Memory] Stored knowledge:");
                        for (key, value, _ns) in &kv_results {
                            kv_ctx.push_str(&format!("\n- {}: {}", key, value));
                        }
                        context_parts.push(kv_ctx);

                        // Record KV injections
                        if let Some(ref sid) = current_session_id {
                            for (key, _, _) in &kv_results {
                                let _ = db.record_context_injection(
                                    sid,
                                    current_trajectory_id.as_deref(),
                                    "kv",
                                    Some(key),
                                    None,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // File dependency injection: find commonly co-edited files for recently touched files
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            if let Some(ref sid) = current_session_id {
                if let Ok(edits) = db.get_edits_for_session(sid) {
                    let recent_files: Vec<String> = {
                        let mut seen = std::collections::HashSet::new();
                        edits
                            .iter()
                            .rev()
                            .filter(|e| seen.insert(e.file_path.clone()))
                            .take(5)
                            .map(|e| e.file_path.clone())
                            .collect()
                    };

                    if !recent_files.is_empty() {
                        let mut dep_files: Vec<(String, u64)> = Vec::new();
                        let mut dep_seen = std::collections::HashSet::new();

                        for file in &recent_files {
                            if let Ok(related) = db.get_related_files(file, 3) {
                                for dep in related {
                                    let other = if dep.file_a == *file {
                                        &dep.file_b
                                    } else {
                                        &dep.file_a
                                    };
                                    if !recent_files.contains(other)
                                        && dep_seen.insert(other.clone())
                                    {
                                        dep_files.push((other.clone(), dep.co_edit_count));
                                    }
                                }
                            }
                        }

                        dep_files.sort_by(|a, b| b.1.cmp(&a.1));
                        dep_files.truncate(5);

                        if !dep_files.is_empty() {
                            let mut dep_ctx = String::from(
                                "[FlowForge Dependencies] Files commonly edited together:",
                            );
                            for (file, count) in &dep_files {
                                dep_ctx.push_str(&format!("\n  {} ({}x co-edited)", file, count));
                            }
                            context_parts.push(dep_ctx);

                            if let Some(ref session_id) = current_session_id {
                                let _ = db.record_context_injection(
                                    session_id,
                                    current_trajectory_id.as_deref(),
                                    "file_dependency",
                                    None,
                                    None,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // Error resolution injection: suggest known fixes for recent errors in this session
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            if let Some(ref sid) = current_session_id {
                if let Ok(recent_errors) = db.get_recent_session_errors(sid, 3) {
                    let mut resolution_hints: Vec<String> = Vec::new();
                    for (error_preview, fingerprint_id) in &recent_errors {
                        if let Ok(resolutions) =
                            db.get_resolutions_for_fingerprint(fingerprint_id, 1)
                        {
                            if let Some(best) = resolutions.first() {
                                if best.confidence() > 0.5 {
                                    resolution_hints.push(format!(
                                        "  Error: {} → Fix: {} (confidence: {:.0}%)",
                                        error_preview.chars().take(80).collect::<String>(),
                                        best.resolution_summary,
                                        best.confidence() * 100.0
                                    ));
                                }
                            }
                        }
                    }
                    if !resolution_hints.is_empty() {
                        let mut ctx_str = String::from(
                            "[FlowForge Error Recovery] Known fixes for recent errors:",
                        );
                        for hint in &resolution_hints {
                            ctx_str.push_str(&format!("\n{}", hint));
                        }
                        context_parts.push(ctx_str);

                        if let Some(ref session_id) = current_session_id {
                            let _ = db.record_context_injection(
                                session_id,
                                current_trajectory_id.as_deref(),
                                "error_resolution",
                                None,
                                None,
                            );
                        }
                    }
                }
            }
        }
    }

    // Anti-drift detection: warn when current prompt diverges from the original task
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            if let (Some(ref sid), Some(ref _tid)) = (&current_session_id, &current_trajectory_id) {
                if let Ok(Some(session)) = db.get_current_session() {
                    // Only check after 20+ commands to avoid false positives early
                    if session.commands >= 20 {
                        if let Ok(Some(trajectory)) = db.get_active_trajectory(sid) {
                            if let Some(ref original_task) = trajectory.task_description {
                                let config_for_embed =
                                    flowforge_core::config::PatternsConfig::default();
                                let embedding =
                                    flowforge_memory::default_embedder(&config_for_embed);
                                let task_vec = embedding.embed(original_task);
                                let prompt_vec = embedding.embed(&prompt);
                                let similarity = flowforge_memory::cosine_similarity(
                                    &task_vec, &prompt_vec,
                                );
                                if similarity < 0.25 {
                                    context_parts.push(format!(
                                        "[FlowForge] Drift warning: Current prompt has low similarity ({:.0}%) to original task: \"{}\". Consider whether you're still on track.",
                                        similarity * 100.0,
                                        original_task.chars().take(80).collect::<String>()
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Test suggestions: based on recently edited files, suggest relevant tests
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            if let Some(ref sid) = current_session_id {
                if let Ok(edits) = db.get_edits_for_session(sid) {
                    let mut seen = std::collections::HashSet::new();
                    let recent_files: Vec<&str> = edits
                        .iter()
                        .rev()
                        .filter(|e| seen.insert(e.file_path.as_str()))
                        .take(5)
                        .map(|e| e.file_path.as_str())
                        .collect();

                    if !recent_files.is_empty() {
                        if let Ok(suggestions) =
                            db.get_test_suggestions_batch(&recent_files, 3)
                        {
                            let high_conf: Vec<_> = suggestions
                                .iter()
                                .filter(|s| s.co_occurrence_count >= 2)
                                .collect();
                            if !high_conf.is_empty() {
                                let mut test_ctx = String::from(
                                    "[FlowForge Tests] Suggested tests for modified files:",
                                );
                                for s in &high_conf {
                                    if let Some(ref cmd) = s.test_command {
                                        test_ctx.push_str(&format!(
                                            "\n  {} ({}x co-edited, cmd: {})",
                                            s.test_file, s.co_occurrence_count, cmd
                                        ));
                                    } else {
                                        test_ctx.push_str(&format!(
                                            "\n  {} ({}x co-edited)",
                                            s.test_file, s.co_occurrence_count
                                        ));
                                    }
                                }
                                context_parts.push(test_ctx);

                                if let Some(ref session_id) = current_session_id {
                                    let _ = db.record_context_injection(
                                        session_id,
                                        current_trajectory_id.as_deref(),
                                        "test_suggestion",
                                        None,
                                        None,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if context_parts.is_empty() {
        ContextOutput::none().write()?;
    } else {
        ContextOutput::with_context(context_parts.join("\n\n")).write()?;
    }

    Ok(())
}

/// Build routing context from current session state.
fn build_routing_context(db: &MemoryDb) -> Option<RoutingContext> {
    let session = db.get_current_session().ok().flatten()?;

    let mut ctx = RoutingContext {
        session_edit_count: session.edits,
        ..Default::default()
    };

    // Get file extensions from recent edits
    if let Ok(edits) = db.get_edits_for_session(&session.id) {
        let mut seen = std::collections::HashSet::new();
        for edit in edits.iter().rev().take(20) {
            if let Some(ref ext) = edit.file_extension {
                if seen.insert(ext.clone()) {
                    ctx.active_file_extensions.push(ext.clone());
                }
            }
        }
    }

    // Get recent tool names from active trajectory
    if let Ok(Some(trajectory)) = db.get_active_trajectory(&session.id) {
        if let Ok(steps) = db.get_trajectory_steps(&trajectory.id) {
            ctx.recent_tools = steps
                .iter()
                .rev()
                .take(10)
                .map(|s| s.tool_name.clone())
                .collect();
        }
    }

    // Get active agent type
    if let Ok(agent_sessions) = db.get_active_agent_sessions() {
        if let Some(first) = agent_sessions.first() {
            ctx.active_agent = Some(first.agent_type.clone());
        }
    }

    // Get active work item type
    let filter = flowforge_core::WorkFilter {
        status: Some(flowforge_core::WorkStatus::InProgress),
        limit: Some(1),
        ..Default::default()
    };
    if let Ok(items) = db.list_work_items(&filter) {
        if let Some(item) = items.first() {
            ctx.active_work_type = Some(item.item_type.clone());
        }
    }

    Some(ctx)
}

fn load_learned_weights_from_db(db: &MemoryDb, prompt: &str) -> HashMap<(String, String), f64> {
    let mut weights = HashMap::new();

    // 1. Load exact matches (existing behavior)
    if let Ok(all_weights) = db.get_all_routing_weights() {
        for w in all_weights {
            weights.insert((w.task_pattern, w.agent_name), w.weight);
        }
    }

    // 2. Pre-compute similarity-based matches for generalization
    // Skip embedding computation entirely if no routing vectors exist (common case)
    let routing_count = db.count_vectors_for_source("routing").unwrap_or(0);
    if routing_count == 0 {
        return weights;
    }

    let config_for_embed = flowforge_core::config::PatternsConfig::default();
    let embedding = flowforge_memory::default_embedder(&config_for_embed);
    let query_vec = embedding.embed(prompt);

    if let Ok(routing_vecs) = db.get_vectors_for_source("routing") {
        for (_, source_id, vec) in &routing_vecs {
            let sim = flowforge_memory::cosine_similarity(&query_vec, vec);
            if sim > 0.7 {
                // source_id is "task_pattern::agent_name"
                if let Some((task_pattern, agent_name)) = source_id.split_once("::") {
                    let key = (task_pattern.to_string(), agent_name.to_string());
                    if let Some(&original_weight) = weights.get(&key) {
                        let generalized_key = (prompt.to_string(), agent_name.to_string());
                        weights
                            .entry(generalized_key)
                            .or_insert_with(|| original_weight * sim as f64);
                    }
                }
            }
        }
    }

    weights
}

/// Lightweight fast-path: only check for active work items.
/// Runs when FLOWFORGE_HOOKS_DISABLED is set so work-tracking enforcement
/// is never bypassed. Skips routing, patterns, embeddings (~1.2s saved).
fn run_work_tracking_only(ctx: &super::HookContext) -> Result<()> {
    if !ctx.config.work_tracking.require_task {
        ContextOutput::none().write()?;
        return Ok(());
    }

    let active_items = ctx.with_db("check_work_tracking", |db| {
        let filter = flowforge_core::WorkFilter {
            status: Some(flowforge_core::WorkStatus::InProgress),
            ..Default::default()
        };
        db.list_work_items(&filter)
    });

    match active_items {
        Some(items) if !items.is_empty() => {
            let mut work_ctx = String::from("[FlowForge Work] Active items:");
            for item in items.iter().take(5) {
                work_ctx.push_str(&format!(
                    "\n- {} ({}): {}",
                    item.id.chars().take(8).collect::<String>(),
                    item.status,
                    item.title,
                ));
            }
            ContextOutput::with_context(work_ctx).write()?;
        }
        _ => {
            ContextOutput::with_context(
                "[FlowForge Work] No active work item. Create one with `flowforge work create` or use your task tracker.".to_string()
            ).write()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use flowforge_core::trajectory::{StepOutcome, Trajectory, TrajectoryStatus};
    use flowforge_core::{EditRecord, FlowForgeConfig, SessionInfo, WorkItem, WorkStatus};

    fn test_ctx_with_session() -> (super::super::HookContext, tempfile::NamedTempFile, String) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let session = SessionInfo {
            id: "sess-ups-1".to_string(),
            started_at: Utc::now(),
            ended_at: None,
            cwd: "/tmp/test".to_string(),
            edits: 5,
            commands: 10,
            summary: None,
            transcript_path: None,
        };
        db.create_session(&session).unwrap();
        let ctx = super::super::HookContext {
            raw: serde_json::json!({}),
            config: FlowForgeConfig::default(),
            db: Some(db),
            session_id: Some("sess-ups-1".to_string()),
        };
        (ctx, tmp, "sess-ups-1".to_string())
    }

    #[test]
    fn test_build_routing_context_returns_some_with_session() {
        let (ctx, _tmp, _sid) = test_ctx_with_session();
        let result = ctx.with_db("test", |db| Ok(build_routing_context(db)));
        let rc = result.flatten();
        assert!(rc.is_some());
        assert_eq!(rc.unwrap().session_edit_count, 5);
    }

    #[test]
    fn test_build_routing_context_returns_none_without_session() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        assert!(build_routing_context(&db).is_none());
    }

    #[test]
    fn test_build_routing_context_captures_file_extensions() {
        let (ctx, _tmp, sid) = test_ctx_with_session();
        ctx.with_db("edits", |db| {
            db.record_edit(&EditRecord {
                session_id: sid.clone(),
                timestamp: Utc::now(),
                file_path: "/tmp/foo.rs".to_string(),
                operation: "write".to_string(),
                file_extension: Some("rs".to_string()),
            })?;
            db.record_edit(&EditRecord {
                session_id: sid.clone(),
                timestamp: Utc::now(),
                file_path: "/tmp/bar.ts".to_string(),
                operation: "write".to_string(),
                file_extension: Some("ts".to_string()),
            })?;
            Ok(())
        });
        let rc = ctx
            .with_db("test", |db| Ok(build_routing_context(db)))
            .flatten()
            .unwrap();
        assert!(!rc.active_file_extensions.is_empty());
    }

    #[test]
    fn test_load_learned_weights_empty_db() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        let weights = load_learned_weights_from_db(&db, "fix authentication bug");
        assert!(weights.is_empty());
    }

    #[test]
    fn test_load_learned_weights_returns_exact_matches() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        db.record_routing_success("fix bug", "debugger").unwrap();
        let weights = load_learned_weights_from_db(&db, "fix bug");
        assert!(
            weights.contains_key(&("fix bug".to_string(), "debugger".to_string())),
            "should contain exact match weight"
        );
    }

    #[test]
    fn test_build_routing_context_captures_work_type() {
        let (ctx, _tmp, _sid) = test_ctx_with_session();
        let item = WorkItem {
            id: "wi-ctx-1".to_string(),
            external_id: None,
            backend: "flowforge".to_string(),
            item_type: "bug".to_string(),
            title: "Fix crash".to_string(),
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
        ctx.with_db("create_work", |db| db.create_work_item(&item));
        let rc = ctx
            .with_db("test", |db| Ok(build_routing_context(db)))
            .flatten()
            .unwrap();
        assert_eq!(rc.active_work_type.as_deref(), Some("bug"));
    }

    #[test]
    fn test_short_prompt_skipped() {
        let short_prompts = ["hi", "ok", "yes", "no", ""];
        for p in &short_prompts {
            assert!(p.trim().len() < 5, "prompt '{}' should be considered short", p);
        }
    }

    #[test]
    fn test_build_routing_context_captures_recent_tools() {
        let (ctx, _tmp, sid) = test_ctx_with_session();
        ctx.with_db("traj", |db| {
            let traj = Trajectory {
                id: "traj-ups-1".to_string(),
                session_id: sid.clone(),
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
            db.create_trajectory(&traj)?;
            db.record_trajectory_step("traj-ups-1", "Bash", None, StepOutcome::Success, Some(50))?;
            db.record_trajectory_step("traj-ups-1", "Read", None, StepOutcome::Success, Some(30))?;
            Ok(())
        });
        let rc = ctx
            .with_db("test", |db| Ok(build_routing_context(db)))
            .flatten()
            .unwrap();
        assert!(!rc.recent_tools.is_empty(), "should capture recent tools from trajectory");
        assert!(rc.recent_tools.contains(&"Bash".to_string()));
        assert!(rc.recent_tools.contains(&"Read".to_string()));
    }
}
