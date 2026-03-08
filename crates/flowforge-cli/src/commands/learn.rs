use colored::Colorize;
use flowforge_core::{FlowForgeConfig, PatternTier, Result};
use flowforge_memory::{MemoryDb, PatternStore};

pub fn store(content: &str, category: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;
    let pattern_store = PatternStore::new(&db, &config.patterns);

    let id = pattern_store.store_short_term(content, category)?;
    println!(
        "{} Stored pattern {} in category '{}'",
        "✓".green(),
        &id[..8],
        category
    );
    Ok(())
}

pub fn search(query: &str, limit: usize) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;
    let store = PatternStore::new(&db, &config.patterns);

    let results = store.search_all_patterns(query, limit)?;

    if results.is_empty() {
        println!("No patterns found for '{query}'");
        return Ok(());
    }

    let long: Vec<_> = results
        .iter()
        .filter(|m| m.tier == PatternTier::Long)
        .collect();
    let short: Vec<_> = results
        .iter()
        .filter(|m| m.tier == PatternTier::Short)
        .collect();

    if !long.is_empty() {
        println!("{}", "Long-term patterns:".bold());
        for p in &long {
            println!(
                "  [{}] {} (conf: {:.0}%, used: {}x, sim: {:.0}%)",
                p.category.cyan(),
                p.content,
                p.confidence * 100.0,
                p.usage_count,
                p.similarity * 100.0
            );
        }
    }

    if !short.is_empty() {
        println!("{}", "Short-term patterns:".bold());
        for p in &short {
            println!(
                "  [{}] {} (conf: {:.0}%, used: {}x, sim: {:.0}%)",
                p.category.cyan(),
                p.content,
                p.confidence * 100.0,
                p.usage_count,
                p.similarity * 100.0
            );
        }
    }

    Ok(())
}

pub fn stats(json: bool) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let short_count = db.count_patterns_short()?;
    let long_count = db.count_patterns_long()?;
    let weights_count = db.count_routing_weights()?;

    if json {
        let traj_counts = db.count_trajectories_by_status()?;
        let cluster_count = db.get_all_clusters()?.len();
        let outlier_count = db.count_outlier_vectors()?;
        let (routing_hits, routing_total) = db.routing_accuracy_stats().unwrap_or((0, 0));
        let (pattern_successes, pattern_total) = db.pattern_hit_rate().unwrap_or((0, 0));
        let (with_conf, without_conf, with_count, without_count) =
            db.context_effectiveness_stats().unwrap_or((0.0, 0.0, 0, 0));

        let obj = serde_json::json!({
            "short_term_patterns": short_count,
            "short_term_max": config.patterns.short_term_max,
            "long_term_patterns": long_count,
            "long_term_max": config.patterns.long_term_max,
            "routing_weights": weights_count,
            "trajectories": traj_counts,
            "clusters": cluster_count,
            "outlier_vectors": outlier_count,
            "routing_accuracy": { "hits": routing_hits, "total": routing_total },
            "pattern_hit_rate": { "successes": pattern_successes, "total": pattern_total },
            "context_effectiveness": {
                "with_context": { "avg_confidence": with_conf, "count": with_count },
                "without_context": { "avg_confidence": without_conf, "count": without_count },
            },
            "embedder": if config.patterns.semantic_embeddings { "semantic" } else { "hash" },
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
        return Ok(());
    }

    println!("{}", "Learning Statistics".bold());
    println!(
        "Short-term patterns: {} / {} max",
        short_count, config.patterns.short_term_max
    );
    println!(
        "Long-term patterns:  {} / {} max",
        long_count, config.patterns.long_term_max
    );
    println!("Routing weights:     {}", weights_count);

    println!("\nConfig:");
    println!(
        "  Promotion threshold: {}x usage, {:.0}% confidence",
        config.patterns.promotion_min_usage,
        config.patterns.promotion_min_confidence * 100.0
    );
    println!(
        "  Decay rate: {:.1}%/hour",
        config.patterns.decay_rate_per_hour * 100.0
    );
    println!(
        "  Dedup threshold: {:.0}%",
        config.patterns.dedup_similarity_threshold * 100.0
    );

    let traj_counts = db.count_trajectories_by_status()?;
    if !traj_counts.is_empty() {
        println!();
        println!("{}", "Trajectories:".bold());
        for (status, count) in &traj_counts {
            println!("  {}: {}", status, count);
        }
    }

    let cluster_count = db.get_all_clusters()?.len();
    let outlier_count = db.count_outlier_vectors()?;
    if cluster_count > 0 || outlier_count > 0 {
        println!();
        println!("{}", "Clusters:".bold());
        println!("  Topic clusters: {}", cluster_count);
        println!("  Outlier vectors: {}", outlier_count);
    }

    // Context Effectiveness
    let (routing_hits, routing_total) = db.routing_accuracy_stats().unwrap_or((0, 0));
    let (pattern_successes, pattern_total) = db.pattern_hit_rate().unwrap_or((0, 0));
    let (with_conf, without_conf, with_count, without_count) =
        db.context_effectiveness_stats().unwrap_or((0.0, 0.0, 0, 0));

    if routing_total > 0 || pattern_total > 0 || with_count > 0 {
        println!();
        println!("{}", "Context Effectiveness:".bold());
        if routing_total > 0 {
            println!(
                "  Routing accuracy: {}/{} ({:.0}%)",
                routing_hits,
                routing_total,
                if routing_total > 0 {
                    routing_hits as f64 / routing_total as f64 * 100.0
                } else {
                    0.0
                }
            );
        }
        if pattern_total > 0 {
            println!(
                "  Pattern hit rate: {}/{} ({:.0}%)",
                pattern_successes,
                pattern_total,
                if pattern_total > 0 {
                    pattern_successes as f64 / pattern_total as f64 * 100.0
                } else {
                    0.0
                }
            );
        }
        if with_count > 0 || without_count > 0 {
            println!(
                "  Avg confidence: with ctx={:.2} (n={}) vs without={:.2} (n={})",
                with_conf, with_count, without_conf, without_count
            );
            if with_count > 5 && without_count > 5 {
                let lift = with_conf - without_conf;
                println!("  Context lift: {:+.2}", lift);
            }
        }
    }

    println!();
    println!(
        "Embedder: {}",
        if config.patterns.semantic_embeddings {
            "semantic (AllMiniLM-L6-v2Q, 384-dim)"
        } else {
            "hash (xxhash n-gram, 128-dim)"
        }
    );

    Ok(())
}

pub fn trajectories(session_id: Option<&str>, status: Option<&str>, limit: usize) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let trajectories = db.list_trajectories(session_id, status, limit)?;

    if trajectories.is_empty() {
        println!("No trajectories found.");
        return Ok(());
    }

    println!("{} ({} entries)", "Trajectories".bold(), trajectories.len());
    for t in &trajectories {
        let status_str = format!("{}", t.status);
        let verdict_str = t
            .verdict
            .as_ref()
            .map(|v| format!(" → {v}"))
            .unwrap_or_default();
        let desc = t.task_description.as_deref().unwrap_or("(no description)");
        let desc_short: String = desc.chars().take(60).collect();
        println!(
            "  {} [{}{}] {} — {}",
            &t.id[..8.min(t.id.len())],
            status_str,
            verdict_str,
            t.started_at.format("%Y-%m-%d %H:%M"),
            desc_short
        );
    }

    Ok(())
}

pub fn trajectory(id: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let t = db
        .get_trajectory(id)?
        .ok_or_else(|| flowforge_core::Error::NotFound(format!("trajectory {id}")))?;

    println!("{} {}", "Trajectory".bold(), &t.id[..8.min(t.id.len())]);
    println!("  Session: {}", &t.session_id[..8.min(t.session_id.len())]);
    println!("  Status: {}", t.status);
    if let Some(ref v) = t.verdict {
        println!("  Verdict: {}", v);
    }
    if let Some(c) = t.confidence {
        println!("  Confidence: {:.2}", c);
    }
    if let Some(ref desc) = t.task_description {
        println!("  Task: {}", desc);
    }
    println!("  Started: {}", t.started_at.format("%Y-%m-%d %H:%M:%S"));
    if let Some(ended) = t.ended_at {
        println!("  Ended: {}", ended.format("%Y-%m-%d %H:%M:%S"));
    }

    let steps = db.get_trajectory_steps(&t.id)?;
    if !steps.is_empty() {
        println!();
        println!("  Steps ({}):", steps.len());
        for s in &steps {
            let outcome = format!("{}", s.outcome);
            let dur = s
                .duration_ms
                .map(|d| format!(" ({d}ms)"))
                .unwrap_or_default();
            println!("    {}. {} [{}]{}", s.step_index, s.tool_name, outcome, dur);
        }
    }

    let ratio = db.trajectory_success_ratio(&t.id)?;
    println!();
    println!("  Success ratio: {:.1}%", ratio * 100.0);

    Ok(())
}

pub fn patterns(mine: bool, min_occurrences: u32) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let patterns = db.list_failure_patterns()?;

    if patterns.is_empty() && !mine {
        println!("No failure patterns found.");
        return Ok(());
    }

    if !patterns.is_empty() {
        println!(
            "{} ({} patterns)",
            "Failure Patterns".bold(),
            patterns.len()
        );
        for p in &patterns {
            println!(
                "  {} [{}]",
                p.pattern_name.cyan(),
                p.trigger_tools.dimmed()
            );
            println!("    {}", p.description);
            println!("    Hint: {}", p.prevention_hint);
            println!(
                "    Occurrences: {}, Prevented: {}",
                p.occurrence_count, p.prevented_count
            );
        }
    }

    if mine {
        println!();
        println!(
            "{} (min_occurrences={})",
            "Mining failure patterns from trajectories...".bold(),
            min_occurrences
        );
        let mined = db.mine_failure_patterns(min_occurrences)?;

        if mined.is_empty() {
            println!("  No common failure sequences found.");
        } else {
            println!("  Found {} candidate sequences:", mined.len());
            for (seq, count) in &mined {
                println!("    {} ({}x)", seq.cyan(), count);
            }
        }
    }

    Ok(())
}

pub fn download_model() -> Result<()> {
    #[cfg(feature = "semantic")]
    {
        println!("Downloading semantic embedding model (AllMiniLM-L6-v2 quantized)...");
        let _embedder = flowforge_memory::SemanticEmbedder::new_with_progress();
        println!("{} Model downloaded and ready", "✓".green());
    }
    #[cfg(not(feature = "semantic"))]
    {
        println!("Semantic embeddings not enabled (compile with --features semantic)");
    }
    Ok(())
}

pub fn clusters() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let clusters = db.get_all_clusters()?;
    if clusters.is_empty() {
        println!("No clusters found. Run consolidation to generate clusters.");
        return Ok(());
    }

    let outlier_count = db.count_outlier_vectors()?;

    println!("{} ({} clusters)", "Topic Clusters".bold(), clusters.len());
    for c in &clusters {
        println!(
            "  Cluster #{}: {} members, p95={:.2}, avg_conf={:.0}%",
            c.id,
            c.member_count,
            c.p95_distance,
            c.avg_confidence * 100.0
        );
    }
    println!("  Outliers: {} unclustered patterns", outlier_count);

    Ok(())
}

pub fn tune_clusters() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    use flowforge_memory::clustering::ClusterManager;
    let mgr = ClusterManager::new(&db, &config.patterns);
    let result = mgr.tune()?;

    if result.vector_count == 0 {
        println!("No pattern vectors found. Store some patterns first.");
        return Ok(());
    }

    println!("{}", "DBSCAN Parameter Tuning".bold());
    println!();
    println!("  Vectors analyzed: {}", result.vector_count);
    println!(
        "  Current epsilon:    {:.3} (cosine distance)",
        config.patterns.clustering_epsilon
    );
    println!(
        "  Suggested epsilon:  {:.3} (cosine distance)",
        result.suggested_epsilon
    );
    println!(
        "  Current min_points: {}",
        config.patterns.clustering_min_points
    );
    println!("  Suggested min_points: {}", result.suggested_min_points);
    println!("  Elbow index: {}", result.elbow_index);

    if !result.k_distances.is_empty() {
        println!();
        println!("{}", "K-distance distribution:".bold());
        let top_n = 10.min(result.k_distances.len());
        println!("  Top {} (largest distances):", top_n);
        for (i, d) in result.k_distances.iter().take(top_n).enumerate() {
            let marker = if i == result.elbow_index {
                " <-- elbow"
            } else {
                ""
            };
            println!("    [{:3}] {:.4}{}", i, d, marker);
        }
        let bottom_start = result.k_distances.len().saturating_sub(10);
        if bottom_start > top_n {
            println!("  ...");
            println!("  Bottom 10 (smallest distances):");
            for (i, d) in result.k_distances.iter().enumerate().skip(bottom_start) {
                let marker = if i == result.elbow_index {
                    " <-- elbow"
                } else {
                    ""
                };
                println!("    [{:3}] {:.4}{}", i, d, marker);
            }
        }
    }

    println!();
    println!("{}", "To apply, add to .flowforge/config.toml:".dimmed());
    println!(
        "{}",
        format!(
            "[patterns]\nclustering_epsilon = {:.3}\nclustering_min_points = {}",
            result.suggested_epsilon, result.suggested_min_points
        )
        .dimmed()
    );

    Ok(())
}

pub fn dependencies(file: Option<&str>, limit: usize) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    if let Some(file_path) = file {
        let deps = db.get_related_files(file_path, limit)?;
        if deps.is_empty() {
            println!("No file dependencies found for '{}'", file_path);
            return Ok(());
        }

        println!(
            "{} for {}",
            "File Dependencies".bold(),
            file_path.cyan()
        );
        for dep in &deps {
            let other = if dep.file_a == file_path {
                &dep.file_b
            } else {
                &dep.file_a
            };
            println!(
                "  {} ({}x co-edited, last: {})",
                other, dep.co_edit_count, dep.last_seen
            );
        }
    } else {
        let deps = db.get_dependency_graph(1, limit)?;
        if deps.is_empty() {
            println!("No file dependencies recorded yet.");
            println!("Dependencies are recorded at session end when files are edited together.");
            return Ok(());
        }

        println!(
            "{} ({} edges)",
            "File Dependency Graph".bold(),
            deps.len()
        );
        for dep in &deps {
            println!(
                "  {} <-> {} ({}x co-edited)",
                dep.file_a, dep.file_b, dep.co_edit_count
            );
        }
    }

    Ok(())
}

pub fn vectorize(source: Option<&str>, limit: usize, dry_run: bool) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;
    let embedder = flowforge_memory::default_embedder(&config.patterns);

    let sources: Vec<&str> = match source {
        Some(s) => vec![s],
        None => vec!["error", "work_item", "trajectory", "conversation", "code_file", "project_intel"],
    };

    let mut total_vectorized = 0u64;

    for src in &sources {
        match *src {
            "error" => {
                let count = db.count_unvectorized_errors()?;
                if dry_run {
                    println!("  {} errors to vectorize: {}", "→".dimmed(), count);
                    continue;
                }
                if count == 0 {
                    continue;
                }
                // Get unvectorized error fingerprints
                let fps = db.list_error_fingerprints(limit)?;
                let mut embedded = 0u64;
                for fp in &fps {
                    if db.count_vectors_for_source_id("error", &fp.id)? == 0 {
                        let tool = fp.tool_name.as_deref().unwrap_or("unknown");
                        let content = format!(
                            "{}: {} - {}",
                            fp.category, tool, fp.error_preview
                        );
                        let vec = embedder.embed(&content);
                        let _ = db.store_vector("error", &fp.id, &vec);
                        embedded += 1;
                        if embedded as usize >= limit {
                            break;
                        }
                    }
                }
                if embedded > 0 {
                    println!(
                        "  {} Vectorized {} error fingerprints",
                        "✓".green(),
                        embedded
                    );
                    total_vectorized += embedded;
                }
            }
            "work_item" => {
                let count = db.count_unvectorized_work_items()?;
                if dry_run {
                    println!("  {} work items to vectorize: {}", "→".dimmed(), count);
                    continue;
                }
                if count == 0 {
                    continue;
                }
                let filter = flowforge_core::WorkFilter::default();
                let items = db.list_work_items(&filter)?;
                let mut embedded = 0u64;
                for item in &items {
                    if db.count_vectors_for_source_id("work_item", &item.id)? == 0 {
                        let _ = db.store_work_item_vector(
                            &item.id,
                            &item.title,
                            item.description.as_deref(),
                            embedder.as_ref(),
                        );
                        embedded += 1;
                        if embedded as usize >= limit {
                            break;
                        }
                    }
                }
                if embedded > 0 {
                    println!(
                        "  {} Vectorized {} work items",
                        "✓".green(),
                        embedded
                    );
                    total_vectorized += embedded;
                }
            }
            "trajectory" => {
                let count = db.count_unvectorized_trajectories()?;
                if dry_run {
                    println!("  {} trajectories to vectorize: {}", "→".dimmed(), count);
                    continue;
                }
                if count == 0 {
                    continue;
                }
                let trajectories = db.list_trajectories(None, None, limit)?;
                let mut embedded = 0u64;
                for t in &trajectories {
                    if !matches!(t.status.to_string().as_str(), "completed" | "judged") {
                        continue;
                    }
                    if db.count_vectors_for_source_id("trajectory", &t.id)? == 0 {
                        if let Ok(Some(summary)) = db.build_trajectory_summary(&t.id) {
                            let vec = embedder.embed(&summary);
                            let _ = db.store_vector("trajectory", &t.id, &vec);
                            embedded += 1;
                            if embedded as usize >= limit {
                                break;
                            }
                        }
                    }
                }
                if embedded > 0 {
                    println!(
                        "  {} Vectorized {} trajectories",
                        "✓".green(),
                        embedded
                    );
                    total_vectorized += embedded;
                }
            }
            "conversation" => {
                let count = db.count_unvectorized_conversations()?;
                if dry_run {
                    println!(
                        "  {} conversation messages to vectorize: {}",
                        "→".dimmed(),
                        count
                    );
                    continue;
                }
                if count == 0 {
                    continue;
                }
                // Get recent sessions and their user messages
                let sessions = db.list_sessions(20)?;
                let mut embedded = 0u64;
                let min_len = config.vectors.conversation_min_length;
                let max_per_session = config.vectors.conversation_max_per_session;
                for session in &sessions {
                    let msgs =
                        db.get_conversation_messages(&session.id, max_per_session, 0)?;
                    for msg in &msgs {
                        if msg.role != "user" || msg.content.len() <= min_len {
                            continue;
                        }
                        let source_id =
                            format!("{}:{}", session.id, msg.message_index);
                        if db.count_vectors_for_source_id("conversation", &source_id)?
                            == 0
                        {
                            let content: String = format!(
                                "user: {}",
                                msg.content.chars().take(500).collect::<String>()
                            );
                            let vec = embedder.embed(&content);
                            let _ = db.store_vector("conversation", &source_id, &vec);
                            embedded += 1;
                            if embedded as usize >= limit {
                                break;
                            }
                        }
                    }
                    if embedded as usize >= limit {
                        break;
                    }
                }
                if embedded > 0 {
                    println!(
                        "  {} Vectorized {} conversation messages",
                        "✓".green(),
                        embedded
                    );
                    total_vectorized += embedded;
                }
            }
            "code_file" => {
                let count = db.count_unvectorized_code_entries()?;
                if dry_run {
                    println!(
                        "  {} code files to vectorize: {}",
                        "→".dimmed(),
                        count
                    );
                    continue;
                }
                if count == 0 {
                    continue;
                }
                let entries = db.list_code_entries(limit)?;
                let mut embedded = 0u64;
                for entry in &entries {
                    if entry.embedding_id.is_some() {
                        continue;
                    }
                    let vec = embedder.embed(&entry.summary);
                    if let Ok(eid) = db.store_vector("code_file", &entry.file_path, &vec) {
                        let _ = db.update_code_entry_embedding(&entry.file_path, eid);
                        embedded += 1;
                        if embedded as usize >= limit {
                            break;
                        }
                    }
                }
                if embedded > 0 {
                    println!(
                        "  {} Vectorized {} code files",
                        "✓".green(),
                        embedded
                    );
                    total_vectorized += embedded;
                }
            }
            "project_intel" => {
                let count = db.count_unvectorized_intelligence()?;
                if dry_run {
                    println!(
                        "  {} intelligence sections to vectorize: {}",
                        "→".dimmed(),
                        count
                    );
                    continue;
                }
                if count == 0 {
                    continue;
                }
                let sections = db.list_intelligence_sections()?;
                let mut embedded = 0u64;
                for section in &sections {
                    if section.embedding_id.is_some() {
                        continue;
                    }
                    let content = format!("{}: {}", section.section_title, section.content);
                    let vec = embedder.embed(&content);
                    if let Ok(eid) = db.store_vector("project_intel", &section.section_key, &vec) {
                        let _ = db.update_intelligence_embedding(&section.section_key, eid);
                        embedded += 1;
                        if embedded as usize >= limit {
                            break;
                        }
                    }
                }
                if embedded > 0 {
                    println!(
                        "  {} Vectorized {} intelligence sections",
                        "✓".green(),
                        embedded
                    );
                    total_vectorized += embedded;
                }
            }
            other => {
                return Err(flowforge_core::Error::InvalidInput(format!(
                    "Unknown source type '{}'. Use: error, work_item, trajectory, conversation, code_file, project_intel",
                    other
                )));
            }
        }
    }

    if dry_run {
        println!("{}", "\nDry run — no vectors stored.".dimmed());
    } else if total_vectorized > 0 {
        let total_vecs = db.count_vectors()?;
        println!(
            "\n{} Backfilled {} vectors (total: {})",
            "✓".green(),
            total_vectorized,
            total_vecs
        );
    } else {
        println!("All records already vectorized.");
    }

    Ok(())
}

pub fn judge(id: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    use flowforge_memory::trajectory::TrajectoryJudge;
    let judge = TrajectoryJudge::new(&db, &config.patterns);
    let result = judge.judge(id)?;

    println!(
        "{} Trajectory {} judged",
        "✓".green(),
        &id[..8.min(id.len())]
    );
    println!("  Verdict: {}", result.verdict);
    println!("  Confidence: {:.2}", result.confidence);
    println!("  Reason: {}", result.reason);

    // Distill strategy patterns from successful trajectories
    if result.verdict == flowforge_core::trajectory::TrajectoryVerdict::Success {
        if let Ok(Some(pattern)) = judge.distill(id) {
            println!("  Distilled: {}", pattern.chars().take(80).collect::<String>());
        }
    }

    Ok(())
}

pub fn judge_all() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    use flowforge_memory::trajectory::TrajectoryJudge;
    let judge_tool = TrajectoryJudge::new(&db, &config.patterns);

    let trajectories = db.list_trajectories(None, Some("completed"), 1000)?;
    if trajectories.is_empty() {
        println!("No completed trajectories to judge.");
        return Ok(());
    }

    println!("Judging {} completed trajectory(ies)...", trajectories.len());

    let mut judged = 0u32;
    let mut distilled = 0u32;
    for t in &trajectories {
        if let Ok(result) = judge_tool.judge(&t.id) {
            judged += 1;
            if result.verdict == flowforge_core::trajectory::TrajectoryVerdict::Success {
                if let Ok(Some(_)) = judge_tool.distill(&t.id) {
                    distilled += 1;
                }
            }
        }
    }

    println!("{} Judged {} trajectory(ies), distilled {} pattern(s)", "✓".green(), judged, distilled);
    Ok(())
}

pub fn distill_all() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    use flowforge_memory::trajectory::TrajectoryJudge;
    let judge_tool = TrajectoryJudge::new(&db, &config.patterns);

    // Process already-judged successful trajectories
    let trajectories = db.list_trajectories(None, Some("judged"), 1000)?;
    let successful: Vec<_> = trajectories
        .iter()
        .filter(|t| t.verdict == Some(flowforge_core::trajectory::TrajectoryVerdict::Success))
        .collect();

    if successful.is_empty() {
        println!("No successful trajectories to distill.");
        return Ok(());
    }

    println!("Distilling {} successful trajectory(ies)...", successful.len());
    let mut distilled = 0u32;
    let mut skipped = 0u32;
    for t in &successful {
        match judge_tool.distill(&t.id) {
            Ok(Some(content)) => {
                println!("  {} {}", "✓".green(), content.chars().take(100).collect::<String>());
                distilled += 1;
            }
            Ok(None) => skipped += 1,
            Err(_) => skipped += 1,
        }
    }

    println!(
        "\n{} Distilled {} pattern(s), skipped {} (trivial/no-task)",
        "✓".green(),
        distilled,
        skipped
    );
    Ok(())
}
