use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::hook::{ContextOutput, UserPromptSubmitInput};
use flowforge_core::{Result, RoutingContext};
use flowforge_memory::MemoryDb;
use std::collections::HashMap;

pub fn run() -> Result<()> {
    let ctx = super::HookContext::init()?;
    let input = UserPromptSubmitInput::from_value(&ctx.raw)?;

    // Fallback: if SessionStart didn't create a session record, create one now.
    // This ensures session-dependent features (trust, metrics, trajectories) work
    // even if the SessionStart hook failed or Claude Code didn't send a sessionId.
    ensure_session(&ctx, &input.common);

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

    // Prompt-length gate: count content words (skip stop words) to decide if ML is worth it.
    // Trivial prompts ("ok", "yes", "do it", "go ahead") skip embedding/routing entirely.
    let word_count = prompt.split_whitespace().count();
    let is_trivial = word_count < 4;

    // Lazy embedder: only load the ONNX model when we actually need it (non-trivial prompts).
    // This saves ~200ms on trivial prompts that skip routing + semantic search.
    let embedder_cell: std::cell::OnceCell<Box<dyn flowforge_memory::Embedder>> =
        std::cell::OnceCell::new();
    let get_embedder = || -> &dyn flowforge_memory::Embedder {
        &**embedder_cell
            .get_or_init(|| flowforge_memory::default_embedder(&ctx.config.patterns))
    };

    // Pre-compute prompt embedding ONCE and reuse everywhere (saves 5-6 redundant embed calls).
    let prompt_vec_cell: std::cell::OnceCell<Vec<f32>> = std::cell::OnceCell::new();
    let get_prompt_vec = || -> &[f32] {
        prompt_vec_cell.get_or_init(|| get_embedder().embed(&prompt))
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
    // Reuse the trajectory we already fetched above (avoid redundant DB call).
    if let Some(db) = db {
        if let (Some(ref sid), Some(ref tid)) = (&current_session_id, &current_trajectory_id) {
            // Check task_description directly — we already confirmed trajectory exists above
            let needs_description = db
                .get_active_trajectory(sid)
                .ok()
                .flatten()
                .map(|t| t.task_description.is_none())
                .unwrap_or(false);
            if needs_description {
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
                            None,
                        );
                    }
                }
            }
        }
    }


    // Load learned weights once from the shared DB connection (A10)
    // Also pre-compute similarity-based matches for generalization (Fix 5)
    // Skip on trivial prompts — no point routing "ok" or "yes"
    let learned_weights = if !is_trivial {
        if let Some(db) = db {
            load_learned_weights_from_db(db, &prompt, get_prompt_vec())
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };

    // Build routing context from session state
    let routing_context = if let Some(db) = db {
        build_routing_context(db)
    } else {
        None
    };
    // Route the task to suggested agents (skip for trivial prompts)
    if ctx.config.hooks.routing && !is_trivial {
        if let Ok(registry) = AgentRegistry::load(&ctx.config.agents) {
            // Check for adaptive weights — override config if enough data exists
            let routing_config = if let Some(db) = db {
                load_adaptive_routing_config(db, &ctx.config.routing)
            } else {
                ctx.config.routing.clone()
            };
            let router = AgentRouter::new(&routing_config);
            let agents: Vec<&_> = registry.list().into_iter().collect();

            // Compute semantic scores for all agents (reuse pre-computed prompt vector)
            let semantic_scores = if let Some(db) = db {
                compute_semantic_scores(db, get_prompt_vec(), &agents, get_embedder())
            } else {
                None
            };
            let results = router.route(
                &prompt,
                &agents,
                &learned_weights,
                routing_context.as_ref(),
                semantic_scores.as_ref(),
            );

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
                        // Phase 3B: Directive routing — tiered by confidence
                        let conf = top.confidence;
                        let b = &top.breakdown;
                        let breakdown_line = format!(
                            "Signals: pattern={:.0}%, cap={:.0}%, learned={:.0}%, context={:.0}%, semantic={:.0}%",
                            b.pattern_score * 100.0,
                            b.capability_score * 100.0,
                            b.learned_score * 100.0,
                            b.context_score * 100.0,
                            b.semantic_score * 100.0,
                        );

                        let subagent_type = map_to_subagent_type(&top.agent_name);

                        // Tiered directive: higher confidence = stronger instruction
                        let directive = if conf >= 0.7 {
                            format!(
                                "[FlowForge Routing] DISPATCH to `{}` agent via Task tool (subagent_type: \"{}\").\n\
                                 Confidence: {:.0}% — this agent is the best match for this task.\n\
                                 {}",
                                top.agent_name, subagent_type,
                                conf * 100.0, breakdown_line,
                            )
                        } else if conf >= 0.5 {
                            format!(
                                "[FlowForge Routing] Use `{}` agent for this task (subagent_type: \"{}\").\n\
                                 Confidence: {:.0}% — strong match. Delegate via Task tool unless the task is trivial.\n\
                                 {}",
                                top.agent_name, subagent_type,
                                conf * 100.0, breakdown_line,
                            )
                        } else {
                            format!(
                                "[FlowForge Routing] Consider `{}` agent (subagent_type: \"{}\").\n\
                                 Confidence: {:.0}% — moderate match. Use if the task would benefit from specialization.\n\
                                 {}",
                                top.agent_name, subagent_type,
                                conf * 100.0, breakdown_line,
                            )
                        };

                        let mut routing_ctx = directive;

                        // Include agent context for top match
                        if let Some(agent) = registry.get(&top.agent_name) {
                            if ctx.config.hooks.inject_agent_body {
                                if !agent.body.is_empty() {
                                    routing_ctx.push_str(&format!("\n\n{}", agent.body));
                                }
                            } else {
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

                        // Record routing injection with breakdown metadata
                        if let Some(db) = db {
                            if let Some(ref sid) = current_session_id {
                                let metadata = serde_json::to_string(&top.breakdown).ok();
                                let _ = db.record_context_injection(
                                    sid,
                                    current_trajectory_id.as_deref(),
                                    "routing",
                                    Some(&top.agent_name),
                                    Some(top.confidence),
                                    metadata.as_deref(),
                                );

                                // IMMEDIATE LEARNING: Record routing outcome NOW, not at session end.
                                // Outcome is "pending" — post_tool_use will update to success/failure.
                                // This ensures routing_outcomes table is populated for adaptive weights.
                                let task_pattern = super::extract_task_pattern(&prompt);
                                if !task_pattern.is_empty() {
                                    let b = &top.breakdown;
                                    let _ = db.record_routing_outcome(
                                        sid,
                                        &top.agent_name,
                                        &task_pattern,
                                        b.pattern_score,
                                        b.capability_score,
                                        b.learned_score,
                                        b.priority_score,
                                        b.context_score,
                                        b.semantic_score,
                                        "pending",
                                    );

                                    // INSTANT VECTOR CREATION: Embed this task pattern NOW so
                                    // similarity-based generalization works on the very next prompt.
                                    // source_id = "task_pattern::agent_name" (matches migrate_embeddings format)
                                    let vec_source_id = format!("{}::{}", task_pattern, top.agent_name);
                                    // Only create if not already vectorized (dedup by source_id)
                                    if db.count_vectors_for_source_id("routing", &vec_source_id).unwrap_or(1) == 0 {
                                        let vec = get_embedder().embed(&task_pattern);
                                        let _ = db.store_vector("routing", &vec_source_id, &vec);
                                    }
                                }

                                // Store active routing suggestion in KV so post_tool_use
                                // can record success/failure per-tool-call (active learning)
                                let _ = db.set_meta(
                                    &format!("active_routing:{}", sid),
                                    &format!("{}|{}", top.agent_name, task_pattern),
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

    // Inject related past work items from semantic search
    if let Some(db) = db {
        if ctx.config.vectors.embed_work_items && !is_trivial {
            let query_vec = get_prompt_vec();
            if let Ok(results) = db.search_vectors(query_vec, &["work_item"], 3) {
                let completed_filter = flowforge_core::WorkFilter {
                    status: Some(flowforge_core::WorkStatus::Completed),
                    ..Default::default()
                };
                let completed_items = db.list_work_items(&completed_filter).unwrap_or_default();
                let completed_ids: std::collections::HashSet<&str> =
                    completed_items.iter().map(|i| i.id.as_str()).collect();

                let related: Vec<_> = results
                    .iter()
                    .filter(|r| r.similarity > 0.4 && completed_ids.contains(r.source_id.as_str()))
                    .take(2)
                    .collect();

                if !related.is_empty() {
                    let mut work_ctx = String::from("[FlowForge Memory] Related past work:");
                    for r in &related {
                        if let Ok(Some(item)) = db.get_work_item(&r.source_id) {
                            work_ctx.push_str(&format!(
                                "\n- {} (sim: {:.0}%)",
                                item.title,
                                r.similarity * 100.0,
                            ));
                        }
                    }
                    context_parts.push(work_ctx);
                }
            }
        }
    }

    // Codebase file suggestions: search code_index vectors + symbol keywords
    // against the prompt. Inject relevant file paths with descriptions.
    if !is_trivial && ctx.config.vectors.embed_code {
        if let Some(db) = db {
            let mut suggested_files: Vec<(String, String, Vec<String>)> = Vec::new(); // (path, desc, symbols)
            let mut seen_paths = std::collections::HashSet::new();

            // 1. Semantic search against code_index vectors
            let query_vec = get_prompt_vec();
            if let Ok(results) = db.search_vectors(query_vec, &["code_file"], 5) {
                for r in results.iter().filter(|r| r.similarity > 0.35) {
                    if seen_paths.insert(r.source_id.clone()) {
                        if let Ok(Some(entry)) = db.get_code_entry(&r.source_id) {
                            let top_syms: Vec<String> =
                                entry.symbols.iter().take(4).cloned().collect();
                            suggested_files.push((
                                entry.file_path.clone(),
                                entry.description.clone(),
                                top_syms,
                            ));
                        }
                    }
                }
            }

            // 2. Keyword LIKE search on symbols for prompt words >= 4 chars
            let keywords: Vec<&str> = prompt
                .split_whitespace()
                .filter(|w| w.len() >= 4)
                .take(5)
                .collect();
            for kw in &keywords {
                if let Ok(entries) = db.search_code_symbols(kw, 3) {
                    for entry in entries {
                        if seen_paths.insert(entry.file_path.clone()) {
                            let top_syms: Vec<String> =
                                entry.symbols.iter().take(4).cloned().collect();
                            suggested_files.push((
                                entry.file_path.clone(),
                                entry.description.clone(),
                                top_syms,
                            ));
                        }
                    }
                }
            }

            suggested_files.truncate(5);

            if !suggested_files.is_empty() {
                let mut code_ctx = String::from("[FlowForge Codebase] Relevant files:");
                for (path, desc, syms) in &suggested_files {
                    let short_desc = if desc.is_empty() {
                        String::new()
                    } else {
                        let d: String = desc.chars().take(60).collect();
                        format!(" — {d}")
                    };
                    let sym_str = if syms.is_empty() {
                        String::new()
                    } else {
                        format!(" ({})", syms.join(", "))
                    };
                    code_ctx.push_str(&format!("\n  {path}{short_desc}{sym_str}"));
                }
                context_parts.push(code_ctx);

                if let Some(ref sid) = current_session_id {
                    let _ = db.record_context_injection(
                        sid,
                        current_trajectory_id.as_deref(),
                        "codebase_suggestion",
                        None,
                        None,
                        None,
                    );
                }
            }
        }
    }

    // Project intelligence injection: search project_intel vectors against prompt
    if !is_trivial && ctx.config.hooks.intelligence {
        if let Some(db) = db {
            if db.has_intelligence().unwrap_or(false) {
                let query_vec = get_prompt_vec();
                if let Ok(results) = db.search_vectors(query_vec, &["project_intel"], 4) {
                    let relevant: Vec<_> = results
                        .iter()
                        .filter(|r| r.similarity > 0.40)
                        .take(2)
                        .collect();

                    for r in &relevant {
                        if let Ok(Some(section)) =
                            db.get_intelligence_section(&r.source_id)
                        {
                            let truncated: String =
                                section.content.chars().take(500).collect();
                            context_parts.push(format!(
                                "[FlowForge Intel: {}] {}",
                                section.section_title, truncated
                            ));

                            if let Some(ref sid) = current_session_id {
                                let _ = db.record_context_injection(
                                    sid,
                                    current_trajectory_id.as_deref(),
                                    "project_intel",
                                    Some(&r.source_id),
                                    Some(r.similarity as f64),
                                    None,
                                );
                            }
                        }
                    }
                }
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
                                None,
                            );
                        }
                    }

                    let _ = db.mark_messages_read(sid);
                }
            }
        }
    }


    // Search FlowForge memory — use top patterns by confidence/usage instead of
    // expensive HNSW rebuild (which loads 500+ vectors from SQLite on every prompt).
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            // Search patterns relevant to the current prompt via keyword matching.
            // Falls back to top-by-confidence if no keywords match.
            if let Ok(top_patterns) = db.search_patterns_by_keywords(&prompt, 5) {
                let proven: Vec<_> = top_patterns
                    .iter()
                    .filter(|p| {
                        p.confidence >= 0.5
                            && p.category != "agent-output"
                            && p.category != "error_pattern" // Handled by error recovery system
                    })
                    .collect();

                if !proven.is_empty() {
                    let mut pattern_ctx = String::from("[FlowForge Memory] Relevant patterns:");
                    for p in &proven {
                        pattern_ctx.push_str(&format!(
                            "\n- {} (conf: {:.0}%)",
                            p.content,
                            p.confidence * 100.0
                        ));
                        // Record usage so patterns can be promoted to long-term.
                        // Without this, all patterns stay at usage_count=0 and never
                        // meet the promotion criteria (usage >= 1 AND confidence >= 0.5).
                        let _ = db.update_pattern_short_usage(&p.id);
                    }
                    context_parts.push(pattern_ctx);

                    // Record injection for impact tracking
                    if let Some(ref sid) = current_session_id {
                        for p in &proven {
                            let _ = db.record_context_injection(
                                sid,
                                current_trajectory_id.as_deref(),
                                "pattern",
                                Some(&p.id),
                                None,
                                None,
                            );
                        }
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
                                    None,
                                );
                            }
                        }
                    }
                }
            }

            // Trajectory insights are now captured as distilled trajectory patterns in
            // patterns_short (category "trajectory"). The keyword-matched pattern injection
            // above already handles this — no need to also scan raw trajectories.
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
                                    let mut hint = format!(
                                        "  Error: {} → Fix: {} (confidence: {:.0}%)",
                                        error_preview.chars().take(80).collect::<String>(),
                                        best.resolution_summary,
                                        best.confidence() * 100.0
                                    );
                                    if !best.tool_sequence.is_empty() {
                                        hint.push_str(&format!(
                                            "\n    Steps: {}",
                                            best.tool_sequence.join(" → ")
                                        ));
                                    }
                                    if !best.files_changed.is_empty() {
                                        let files: Vec<&str> = best.files_changed.iter().take(3).map(|s| s.as_str()).collect();
                                        hint.push_str(&format!(
                                            "\n    Files: {}",
                                            files.join(", ")
                                        ));
                                    }
                                    resolution_hints.push(hint);
                                }
                            }
                        }
                    }
                    // Semantic fallback: if no exact resolutions found, search by similarity.
                    // Skip entirely if 0 resolutions exist in the DB — saves expensive vector search.
                    let has_any_resolutions = resolution_hints.is_empty()
                        && ctx.config.vectors.embed_errors
                        && db.get_error_stats().map(|(_, r, _)| r > 0).unwrap_or(false);
                    if has_any_resolutions {
                        for (error_preview, _) in &recent_errors {
                            let query_vec = get_embedder().embed(error_preview);
                            if let Ok(semantic_results) =
                                db.find_error_resolutions_semantic(&query_vec, 2)
                            {
                                for (fp, resolutions) in &semantic_results {
                                    if let Some(best) = resolutions.first() {
                                        if best.confidence() > 0.5 {
                                            resolution_hints.push(format!(
                                                "  Similar error ({}): {} → Fix: {} (confidence: {:.0}%)",
                                                fp.category,
                                                fp.error_preview
                                                    .chars()
                                                    .take(60)
                                                    .collect::<String>(),
                                                best.resolution_summary,
                                                best.confidence() * 100.0
                                            ));
                                        }
                                    }
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
                                None,
                            );
                        }
                    }
                }
            }
        }
    }


    // File-aware convention injection: inject relevant conventions based on recently
    // edited files, not just prompt keywords. E.g., editing schema.rs → schema convention.
    if ctx.config.hooks.learning {
        if let Some(db) = db {
            if let Some(ref sid) = current_session_id {
                if let Ok(edits) = db.get_edits_for_session(sid) {
                    // Build a search string from recently edited file names
                    let mut seen = std::collections::HashSet::new();
                    let file_keywords: String = edits
                        .iter()
                        .rev()
                        .filter(|e| seen.insert(e.file_path.as_str()))
                        .take(3)
                        .filter_map(|e| {
                            std::path::Path::new(&e.file_path)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_string())
                        })
                        .collect::<Vec<_>>()
                        .join(" ");

                    if !file_keywords.is_empty() {
                        // Search for patterns matching file names (e.g., "schema" matches schema convention)
                        if let Ok(file_patterns) =
                            db.search_patterns_by_keywords(&file_keywords, 3)
                        {
                            let new_conventions: Vec<_> = file_patterns
                                .iter()
                                .filter(|p| {
                                    p.category == "code_style"
                                        && p.confidence >= 0.5
                                        // Don't duplicate patterns already injected by keyword match
                                        && !context_parts
                                            .iter()
                                            .any(|c| c.contains(&p.content))
                                })
                                .collect();

                            if !new_conventions.is_empty() {
                                let mut conv_ctx = String::from(
                                    "[FlowForge Conventions] For recently edited files:",
                                );
                                for p in &new_conventions {
                                    conv_ctx.push_str(&format!(
                                        "\n- {}",
                                        p.content
                                    ));
                                    let _ = db.update_pattern_short_usage(&p.id);
                                }
                                context_parts.push(conv_ctx);
                            }
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
                                let task_vec = get_embedder().embed(original_task);
                                let similarity = flowforge_memory::cosine_similarity(
                                    &task_vec, get_prompt_vec(),
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

    // Predictive Task Briefing: before Claude starts working, predict which files it
    // will need, warn about known errors for those files, and suggest a proven tool sequence.
    // Only fires for substantive prompts (≥6 words) and when we have trajectory history.
    if ctx.config.hooks.learning && !is_trivial && word_count >= 6 {
        if let Some(db) = db {
            // Extract keywords (reuse same stop-word logic as pattern search)
            const BRIEF_STOP_WORDS: &[&str] = &[
                "the", "this", "that", "with", "from", "into", "about", "have", "been",
                "were", "will", "just", "should", "would", "could", "also", "need", "want",
                "make", "like", "some", "more", "very", "when", "then", "than", "only",
                "each", "does", "done", "here", "there", "what", "your", "they", "them",
                "please", "help", "code", "file", "change",
            ];
            let keywords: Vec<String> = prompt
                .split_whitespace()
                .map(|w| w.to_lowercase())
                .filter(|w| w.len() >= 4 && !BRIEF_STOP_WORDS.contains(&w.as_str()))
                .collect();

            if keywords.len() >= 2 {
                let mut brief_parts: Vec<String> = Vec::new();

                // 1. Predict files from similar successful trajectories
                if let Ok(predicted_files) = db.predict_task_files(&keywords, 15) {
                    // Filter: prefer source code, skip docs/config unless very frequent
                    let relevant: Vec<_> = predicted_files
                        .iter()
                        .filter(|(path, count)| {
                            let is_source = path.ends_with(".rs")
                                || path.ends_with(".ts")
                                || path.ends_with(".py")
                                || path.ends_with(".js")
                                || path.ends_with(".go")
                                || path.ends_with(".toml");
                            // Source files: include if ≥1 session. Non-source: require ≥2.
                            if is_source { *count >= 1 } else { *count >= 2 }
                        })
                        .collect();
                    if !relevant.is_empty() {
                        // Expand via co-edit graph: find related files not already predicted
                        let mut expanded: Vec<(String, String)> = Vec::new(); // (file, reason)
                        let predicted_set: std::collections::HashSet<&str> =
                            relevant.iter().map(|(f, _)| f.as_str()).collect();
                        for (file, _) in relevant.iter().take(3) {
                            if let Ok(related) = db.get_related_files(file, 2) {
                                for dep in &related {
                                    let other = if dep.file_a == *file {
                                        &dep.file_b
                                    } else {
                                        &dep.file_a
                                    };
                                    if !predicted_set.contains(other.as_str())
                                        && dep.co_edit_count >= 2
                                        && !expanded.iter().any(|(f, _)| f == other)
                                    {
                                        expanded.push((
                                            other.clone(),
                                            format!("{}x with {}", dep.co_edit_count, shorten_path(file)),
                                        ));
                                    }
                                }
                            }
                        }
                        // Prefer source code in co-edit expansion too
                        expanded.sort_by(|a, b| {
                            let a_src = a.0.ends_with(".rs") || a.0.ends_with(".ts") || a.0.ends_with(".py");
                            let b_src = b.0.ends_with(".rs") || b.0.ends_with(".ts") || b.0.ends_with(".py");
                            b_src.cmp(&a_src)
                        });
                        expanded.truncate(3);

                        let mut files_section = String::from("FILES:");
                        for (file, count) in relevant.iter().take(5) {
                            let short_path = shorten_path(file);
                            files_section.push_str(&format!(
                                "\n  {} (edited in {} similar session{})",
                                short_path,
                                count,
                                if *count > 1 { "s" } else { "" }
                            ));
                        }
                        for (file, reason) in &expanded {
                            let short_path = shorten_path(file);
                            files_section.push_str(&format!(
                                "\n  {} (co-edited {})",
                                short_path, reason
                            ));
                        }
                        brief_parts.push(files_section);

                        // 2. Error warnings for predicted files
                        let all_files: Vec<&str> = relevant
                            .iter()
                            .map(|(f, _)| f.as_str())
                            .chain(expanded.iter().map(|(f, _)| f.as_str()))
                            .collect();
                        if let Ok(errors) = db.get_errors_for_files(&all_files, 5) {
                            // Skip generic errors that apply to everything
                            let warnings: Vec<_> = errors
                                .iter()
                                .filter(|(preview, _, _)| {
                                    let lower = preview.to_lowercase();
                                    preview.len() > 20
                                        && !lower.starts_with("command failed with exit code")
                                        && !lower.starts_with("exit code")
                                        && !lower.starts_with("error:")
                                })
                                .take(3)
                                .collect();
                            if !warnings.is_empty() {
                                let mut warn_section = String::from("WATCH OUT:");
                                for (preview, tool, resolution) in &warnings {
                                    let short_err: String =
                                        preview.chars().take(80).collect();
                                    if let Some(fix) = resolution {
                                        warn_section.push_str(&format!(
                                            "\n  {} ({}) -> Fix: {}",
                                            short_err, tool, fix
                                        ));
                                    } else {
                                        warn_section.push_str(&format!(
                                            "\n  {} ({})",
                                            short_err, tool
                                        ));
                                    }
                                }
                                brief_parts.push(warn_section);
                            }
                        }
                    }
                }

                // 3. Winning tool sequence from best matching trajectory
                if let Ok(Some(sequence)) = db.get_winning_sequence(&keywords) {
                    if sequence.len() >= 2 {
                        brief_parts.push(format!(
                            "APPROACH: {}",
                            sequence.join(" -> ")
                        ));
                    }
                }

                if !brief_parts.is_empty() {
                    let mut briefing =
                        String::from("[FlowForge Brief] Predicted from similar past sessions:");
                    for part in &brief_parts {
                        briefing.push_str(&format!("\n{}", part));
                    }
                    context_parts.push(briefing);

                    if let Some(ref sid) = current_session_id {
                        let _ = db.record_context_injection(
                            sid,
                            current_trajectory_id.as_deref(),
                            "predictive_brief",
                            None,
                            None,
                            None,
                        );
                    }
                }
            }
        }
    }

    if context_parts.is_empty() {
        ContextOutput::none().write()?;
    } else {
        // Adaptive context budgeting: scale budget based on session age.
        // Early session (< 30 commands): full budget for rich context.
        // Mid session (30-80): 75% budget — Claude has working knowledge.
        // Late session (80+): 50% budget — conserve tokens for actual work.
        let base_budget = ctx.config.hooks.context_budget_chars;
        let budget = if base_budget > 0 {
            let command_count = db
                .and_then(|db| db.get_current_session().ok().flatten())
                .map(|s| s.commands)
                .unwrap_or(0);

            if command_count > 80 {
                base_budget / 2
            } else if command_count > 30 {
                base_budget * 3 / 4
            } else {
                base_budget
            }
        } else {
            0
        };

        let final_output = if budget > 0 {
            let mut total = 0usize;
            let mut budgeted_parts: Vec<String> = Vec::new();
            for part in &context_parts {
                let len = part.len();
                if total + len + 2 > budget {
                    let remaining = budget.saturating_sub(total + 2);
                    if remaining > 80 {
                        let truncated: String = part.chars().take(remaining).collect();
                        budgeted_parts.push(format!("{}...", truncated));
                    }
                    break;
                }
                total += len + 2;
                budgeted_parts.push(part.clone());
            }
            budgeted_parts.join("\n\n")
        } else {
            context_parts.join("\n\n")
        };

        // Injection deduplication: skip re-injecting identical context.
        // Hash the output, compare to last injection. If unchanged and
        // skip_count < 8, suppress the injection entirely to save tokens.
        // After 8 consecutive skips, force a refresh in case Claude lost context.
        let content_hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            final_output.hash(&mut hasher);
            format!("{:016x}", hasher.finish())
        };

        let should_inject = if let Some(session_id) = ctx.session_id.as_deref() {
            if let Some(db) = db {
                match db.get_injection_cache(session_id) {
                    Ok(Some((cached_hash, skip_count))) if cached_hash == content_hash => {
                        if skip_count < 8 {
                            // Same content, skip injection
                            let _ = db.increment_injection_skip(session_id);
                            false
                        } else {
                            // Force refresh after 8 skips
                            let _ = db.set_injection_cache(session_id, &content_hash);
                            true
                        }
                    }
                    _ => {
                        // New or changed content — inject and cache
                        let _ = db.set_injection_cache(session_id, &content_hash);
                        true
                    }
                }
            } else {
                true // no DB, always inject
            }
        } else {
            true // no session, always inject
        };

        if should_inject {
            ContextOutput::with_context(final_output).write()?;
        } else {
            ContextOutput::none().write()?;
        }
    }

    Ok(())
}

/// Fallback session creation: if SessionStart didn't persist a session record
/// (e.g., sessionId wasn't in the payload), create one now so all session-dependent
/// features (trust scoring, trajectory, metrics, statusline) work correctly.
fn ensure_session(ctx: &super::HookContext, common: &flowforge_core::hook::CommonHookFields) {
    let session_id = match ctx.session_id.as_deref().or(common.session_id.as_deref()) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => return, // No session ID available anywhere — can't create
    };

    let db = match ctx.db.as_ref() {
        Some(db) => db,
        None => return,
    };

    // Check if session already exists
    if db.get_current_session().ok().flatten().is_some() {
        return; // Session exists, nothing to do
    }

    // Also check if this specific session_id already exists (ended or otherwise)
    if db.get_session_by_id(&session_id).ok().flatten().is_some() {
        // Session exists but is ended — reopen it
        let _ = db.reopen_session(&session_id);
        return;
    }

    // Create the session record as fallback
    let now = chrono::Utc::now();
    let session = flowforge_core::SessionInfo {
        id: session_id.clone(),
        started_at: now,
        ended_at: None,
        cwd: common.cwd.clone().unwrap_or_else(|| ".".to_string()),
        edits: 0,
        commands: 0,
        summary: None,
        transcript_path: common.transcript_path.clone(),
    };
    let _ = db.create_session(&session);

    // Create trajectory for this session
    let trajectory = flowforge_core::trajectory::Trajectory {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.clone(),
        work_item_id: None,
        agent_name: None,
        task_description: None,
        status: flowforge_core::trajectory::TrajectoryStatus::Recording,
        started_at: now,
        ended_at: None,
        verdict: None,
        confidence: None,
        metadata: None,
        embedding_id: None,
    };
    let _ = db.create_trajectory(&trajectory);

    // Initialize trust score
    let _ = db.create_trust_score(&session_id, ctx.config.guidance.trust_initial_score);
}

/// Map a FlowForge agent name to a Claude Code Task tool subagent_type.
/// Agents that match a built-in subagent type get dispatched directly.
/// All others use "general-purpose" with the agent's context injected.
fn map_to_subagent_type(agent_name: &str) -> &'static str {
    match agent_name {
        // Research/exploration agents → Explore (read-only, fast)
        "code-analyzer" | "code-quality" | "security-auditor" | "security-sentinel"
        | "collective-intelligence" | "dependency-checker" => "Explore",

        // Planning/architecture agents → Plan (read-only, designs)
        "architect" | "architecture" | "code-goal-planner" | "system-design"
        | "specification" | "refinement" => "Plan",

        // Everything else → general-purpose (full tool access)
        _ => "general-purpose",
    }
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

fn load_learned_weights_from_db(
    db: &MemoryDb,
    prompt: &str,
    prompt_vec: &[f32],
) -> HashMap<(String, String), f64> {
    let mut weights = HashMap::new();

    // 1. Load exact matches
    if let Ok(all_weights) = db.get_all_routing_weights() {
        for w in &all_weights {
            weights.insert((w.task_pattern.clone(), w.agent_name.clone()), w.weight);
        }
    }

    // 2. Similarity-based generalization via HNSW search (fast, O(log n))
    // instead of loading ALL routing vectors (was O(n) and very slow)
    if let Ok(results) = db.search_vectors(prompt_vec, &["routing"], 10) {
        for r in &results {
            if r.similarity > 0.55 {
                if let Some((task_pattern, agent_name)) = r.source_id.split_once("::") {
                    let key = (task_pattern.to_string(), agent_name.to_string());
                    if let Some(&original_weight) = weights.get(&key) {
                        let generalized_key = (prompt.to_string(), agent_name.to_string());
                        weights
                            .entry(generalized_key)
                            .or_insert_with(|| original_weight * r.similarity as f64);
                    }
                }
            }
        }
    }

    weights
}

/// Load adaptive routing config from DB if enough data exists.
/// Falls back to the static config if not enough routing outcomes are recorded.
fn load_adaptive_routing_config(
    db: &MemoryDb,
    base: &flowforge_core::config::RoutingConfig,
) -> flowforge_core::config::RoutingConfig {
    // Only use adaptive weights if we have >= 5 computed weights
    let adaptive = match db.get_all_adaptive_weights() {
        Ok(w) if w.len() >= 5 => w,
        _ => return base.clone(),
    };

    flowforge_core::config::RoutingConfig {
        pattern_weight: *adaptive.get("pattern").unwrap_or(&base.pattern_weight),
        capability_weight: *adaptive.get("capability").unwrap_or(&base.capability_weight),
        learned_weight: *adaptive.get("learned").unwrap_or(&base.learned_weight),
        priority_weight: *adaptive.get("priority").unwrap_or(&base.priority_weight),
        context_weight: *adaptive.get("context").unwrap_or(&base.context_weight),
        semantic_weight: *adaptive.get("semantic").unwrap_or(&base.semantic_weight),
        confidence_sharpening: base.confidence_sharpening,
    }
}

/// Shorten a file path for display: strip the common project prefix to show crate-relative paths.
fn shorten_path(path: &str) -> &str {
    // Try to find "crates/" prefix for Rust crate paths
    if let Some(idx) = path.find("crates/") {
        return &path[idx..];
    }
    // Try to find "src/" prefix
    if let Some(idx) = path.find("src/") {
        return &path[idx..];
    }
    // Fall back to just the filename
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}

/// Compute semantic (embedding) similarity scores for all agents against the task.
fn compute_semantic_scores(
    db: &MemoryDb,
    task_vec: &[f32],
    agents: &[&flowforge_core::AgentDef],
    embedder: &dyn flowforge_memory::Embedder,
) -> Option<HashMap<String, f64>> {

    // Try to load cached agent embeddings from DB first (source_type="agent_embed")
    let cached = db.get_vectors_for_source("agent_embed").unwrap_or_default();
    let cached_map: HashMap<&str, &Vec<f32>> = cached
        .iter()
        .map(|(_, source_id, vec)| (source_id.as_str(), vec))
        .collect();

    let mut scores = HashMap::new();
    let mut cache_misses = Vec::new();
    for agent in agents {
        if let Some(cached_vec) = cached_map.get(agent.name.as_str()) {
            let sim = flowforge_memory::cosine_similarity(task_vec, cached_vec);
            scores.insert(agent.name.clone(), (sim as f64).clamp(0.0, 1.0));
        } else {
            // Cache miss: compute and store
            let mut agent_text = agent.description.clone();
            if !agent.capabilities.is_empty() {
                agent_text.push(' ');
                agent_text.push_str(&agent.capabilities.join(" "));
            }
            let agent_vec = embedder.embed(&agent_text);
            let sim = flowforge_memory::cosine_similarity(task_vec, &agent_vec);
            scores.insert(agent.name.clone(), (sim as f64).clamp(0.0, 1.0));
            cache_misses.push((agent.name.clone(), agent_vec));
        }
    }

    // Backfill cache for misses (best-effort)
    for (name, vec) in &cache_misses {
        let _ = db.store_vector("agent_embed", name, vec);
    }

    // Tier 5A: Enhance with historical routing success/failure vectors
    enhance_with_historical_matches(db, task_vec, &mut scores);

    Some(scores)
}

/// Tier 5A: Blend historical routing vectors into semantic scores.
/// Uses HNSW search (O(log n)) instead of loading all vectors (was O(n)).
fn enhance_with_historical_matches(
    db: &MemoryDb,
    task_vec: &[f32],
    scores: &mut HashMap<String, f64>,
) {
    // HNSW search for top-3 similar successful routings
    if let Ok(success_results) = db.search_vectors(task_vec, &["routing_success"], 3) {
        for r in &success_results {
            if r.similarity > 0.5 {
                if let Some(agent_name) = r.source_id.split("::").nth(1) {
                    if let Some(score) = scores.get_mut(agent_name) {
                        *score = 0.6 * *score + 0.4 * r.similarity as f64;
                    }
                }
            }
        }
    }

    // HNSW search for top-3 similar failure routings
    if let Ok(failure_results) = db.search_vectors(task_vec, &["routing_failure"], 3) {
        for r in &failure_results {
            if r.similarity > 0.5 {
                if let Some(agent_name) = r.source_id.split("::").nth(1) {
                    if let Some(score) = scores.get_mut(agent_name) {
                        *score *= 1.0 - 0.3 * r.similarity as f64;
                    }
                }
            }
        }
    }
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
        let emb = flowforge_memory::default_embedder(&flowforge_core::config::PatternsConfig::default());
        let prompt_vec = emb.embed("fix authentication bug");
        let weights = load_learned_weights_from_db(&db, "fix authentication bug", &prompt_vec);
        assert!(weights.is_empty());
    }

    #[test]
    fn test_load_learned_weights_returns_exact_matches() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = MemoryDb::open(tmp.path()).unwrap();
        db.record_routing_success("fix bug", "debugger").unwrap();
        let emb = flowforge_memory::default_embedder(&flowforge_core::config::PatternsConfig::default());
        let prompt_vec = emb.embed("fix bug");
        let weights = load_learned_weights_from_db(&db, "fix bug", &prompt_vec);
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

    #[test]
    fn test_shorten_path() {
        assert_eq!(
            shorten_path("/Users/matt/Projects/flowforge/crates/flowforge-cli/src/main.rs"),
            "crates/flowforge-cli/src/main.rs"
        );
        assert_eq!(shorten_path("/tmp/src/lib.rs"), "src/lib.rs");
        assert_eq!(shorten_path("/tmp/foo.rs"), "foo.rs");
    }
}
