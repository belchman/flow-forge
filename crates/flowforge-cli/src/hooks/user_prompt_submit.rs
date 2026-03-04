use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::hook::{self, ContextOutput, UserPromptSubmitInput};
use flowforge_core::{FlowForgeConfig, PatternTier, Result, RoutingContext};
use flowforge_memory::{MemoryDb, PatternStore};
use std::collections::HashMap;

pub fn run() -> Result<()> {
    let v = hook::parse_stdin_value()?;
    let input = UserPromptSubmitInput::from_value(&v)?;

    let prompt = match input.prompt {
        Some(ref p) if p.trim().len() >= 5 => p.clone(),
        _ => {
            // No prompt or too short — nothing to route
            ContextOutput::none().write()?;
            return Ok(());
        }
    };

    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let mut context_parts: Vec<String> = Vec::new();

    // Single DB connection for the entire hook (A10)
    let db = if config.hooks.routing || config.hooks.learning {
        let db_path = config.db_path();
        if db_path.exists() {
            MemoryDb::open(&db_path).ok()
        } else {
            None
        }
    } else {
        None
    };

    // Set trajectory task description from first user prompt
    if let Some(ref db) = db {
        if let Ok(Some(session)) = db.get_current_session() {
            if let Ok(Some(trajectory)) = db.get_active_trajectory(&session.id) {
                if trajectory.task_description.is_none() {
                    // Use first ~200 chars of prompt as task description
                    let desc: String = prompt.chars().take(200).collect();
                    let _ = db.set_trajectory_task_description(&trajectory.id, &desc);
                }
            }
        }
    }

    // Load learned weights once from the shared DB connection (A10)
    // Also pre-compute similarity-based matches for generalization (Fix 5)
    let learned_weights = if let Some(ref db) = db {
        load_learned_weights_from_db(db, &prompt)
    } else {
        HashMap::new()
    };

    // Build routing context from session state
    let routing_context = if let Some(ref db) = db {
        build_routing_context(db)
    } else {
        None
    };

    // Route the task to suggested agents
    if config.hooks.routing {
        if let Ok(registry) = AgentRegistry::load(&config.agents) {
            let router = AgentRouter::new(&config.routing);
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

                        // Include agent body for top match
                        if let Some(agent) = registry.get(&top.agent_name) {
                            if !agent.body.is_empty() {
                                routing_ctx.push_str(&format!("\n\n{}", agent.body));
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
                    }
                }
            }
        }
    }

    // Inject active work items context (C4)
    if let Some(ref db) = db {
        let filter = flowforge_core::WorkFilter {
            status: Some("in_progress".to_string()),
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
            } else if config.work_tracking.require_task {
                context_parts.push(
                    "[FlowForge Work] No active work item. Create one with `flowforge work create` or use your task tracker.".to_string()
                );
            }
        }
    }

    // Inject unread co-agent mailbox messages
    if let Some(ref db) = db {
        if let Some(ref sid) = input.common.session_id {
            if let Ok(unread) = db.get_unread_messages(sid) {
                if !unread.is_empty() {
                    let mut ctx = format!(
                        "[FlowForge Mailbox] {} unread from co-agents:",
                        unread.len()
                    );
                    for msg in &unread {
                        ctx.push_str(&format!(
                            "\n  From {}: {}",
                            msg.from_agent_name, msg.content
                        ));
                    }
                    context_parts.push(ctx);
                    let _ = db.mark_messages_read(sid);
                }
            }
        }
    }

    // Search FlowForge memory using semantic vector search + cluster boosting
    if config.hooks.learning {
        if let Some(ref db) = db {
            let store = PatternStore::new(db, &config.patterns);
            if let Ok(matches) = store.search_all_patterns(&prompt, 5) {
                let proven: Vec<_> = matches
                    .iter()
                    .filter(|m| m.tier == PatternTier::Long && m.confidence > 0.5)
                    .collect();
                let recent: Vec<_> = matches
                    .iter()
                    .filter(|m| m.tier == PatternTier::Short && m.confidence > 0.4)
                    .collect();

                if !proven.is_empty() {
                    let mut ctx = String::from("[FlowForge Memory] Proven patterns:");
                    for p in &proven {
                        ctx.push_str(&format!(
                            "\n- {} (conf: {:.0}%, sim: {:.0}%, used: {}x)",
                            p.content,
                            p.confidence * 100.0,
                            p.similarity * 100.0,
                            p.usage_count
                        ));
                    }
                    context_parts.push(ctx);
                }

                if !recent.is_empty() {
                    let mut ctx = String::from("[FlowForge Memory] Relevant patterns:");
                    for p in &recent {
                        ctx.push_str(&format!(
                            "\n- {} (conf: {:.0}%, sim: {:.0}%)",
                            p.content,
                            p.confidence * 100.0,
                            p.similarity * 100.0
                        ));
                    }
                    context_parts.push(ctx);
                }
            }

            // Search key-value memory for relevant stored knowledge
            if let Ok(kv_results) = db.kv_search(&prompt, 3) {
                if !kv_results.is_empty() {
                    let mut kv_ctx = String::from("[FlowForge Memory] Stored knowledge:");
                    for (key, value, _ns) in &kv_results {
                        kv_ctx.push_str(&format!("\n- {}: {}", key, value));
                    }
                    context_parts.push(kv_ctx);
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
        status: Some("in_progress".to_string()),
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

    // 2. Pre-compute similarity-based matches for generalization (Fix 5)
    // Embed the incoming prompt, search routing vectors, and inject similar weights
    let config_for_embed = flowforge_core::config::PatternsConfig::default();
    let embedding = flowforge_memory::default_embedder(&config_for_embed);
    let query_vec = embedding.embed(prompt);

    if let Ok(routing_vecs) = db.get_vectors_for_source("routing") {
        for (_, source_id, vec) in &routing_vecs {
            let sim = flowforge_memory::cosine_similarity(&query_vec, vec);
            if sim > 0.7 {
                // source_id is "task_pattern::agent_name"
                if let Some((task_pattern, agent_name)) = source_id.split_once("::") {
                    // Look up the actual routing weight for this pair
                    let key = (task_pattern.to_string(), agent_name.to_string());
                    if let Some(&original_weight) = weights.get(&key) {
                        // Insert with the prompt text as key so router's substring match hits
                        let generalized_key = (prompt.to_string(), agent_name.to_string());
                        // Scale weight by similarity (only insert if not already present)
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
