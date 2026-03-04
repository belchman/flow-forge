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

pub fn stats() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = MemoryDb::open(&config.db_path())?;

    let short_count = db.count_patterns_short()?;
    let long_count = db.count_patterns_long()?;
    let weights_count = db.count_routing_weights()?;

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

    Ok(())
}
