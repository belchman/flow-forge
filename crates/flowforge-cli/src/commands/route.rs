use colored::Colorize;
use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;
use std::collections::HashMap;

pub fn run(task: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let registry = AgentRegistry::load(&config.agents)?;
    let agents: Vec<&_> = registry.list().into_iter().collect();

    // Load learned weights and adaptive config if available
    let mut learned_weights = HashMap::new();
    let mut routing_config = config.routing.clone();
    let db_opt = MemoryDb::open(&config.db_path()).ok();
    if let Some(ref db) = db_opt {
        if let Ok(all_weights) = db.get_all_routing_weights() {
            for w in all_weights {
                learned_weights.insert((w.task_pattern, w.agent_name), w.weight);
            }
        }
        // Use adaptive weights if available
        if let Ok(adaptive) = db.get_all_adaptive_weights() {
            if adaptive.len() >= 5 {
                routing_config.pattern_weight = *adaptive.get("pattern").unwrap_or(&routing_config.pattern_weight);
                routing_config.capability_weight = *adaptive.get("capability").unwrap_or(&routing_config.capability_weight);
                routing_config.learned_weight = *adaptive.get("learned").unwrap_or(&routing_config.learned_weight);
                routing_config.priority_weight = *adaptive.get("priority").unwrap_or(&routing_config.priority_weight);
                routing_config.context_weight = *adaptive.get("context").unwrap_or(&routing_config.context_weight);
                routing_config.semantic_weight = *adaptive.get("semantic").unwrap_or(&routing_config.semantic_weight);
            }
        }
    }

    let router = AgentRouter::new(&routing_config);

    // Compute semantic scores
    let semantic_scores = {
        let config_for_embed = flowforge_core::config::PatternsConfig::default();
        let embedding = flowforge_memory::default_embedder(&config_for_embed);
        let task_vec = embedding.embed(task);
        let mut scores = HashMap::new();
        for agent in &agents {
            let mut agent_text = agent.description.clone();
            if !agent.capabilities.is_empty() {
                agent_text.push(' ');
                agent_text.push_str(&agent.capabilities.join(" "));
            }
            let agent_vec = embedding.embed(&agent_text);
            let sim = flowforge_memory::cosine_similarity(&task_vec, &agent_vec);
            scores.insert(agent.name.clone(), (sim as f64).clamp(0.0, 1.0));
        }
        scores
    };

    let results = router.route(task, &agents, &learned_weights, None, Some(&semantic_scores));

    println!("{} \"{}\"", "Routing:".bold(), task);
    println!("{}", "─".repeat(70));

    if results.is_empty() {
        println!("No agents available for routing");
        return Ok(());
    }

    let total_agents = agents.len();
    let filtered_count = results.len();
    if filtered_count < total_agents {
        println!(
            "Scored {}/{} agents (filtered by routing category)",
            filtered_count, total_agents
        );
        println!("{}", "─".repeat(70));
    }

    let top_5: Vec<_> = results.iter().take(5).collect();
    for (i, result) in top_5.iter().enumerate() {
        let marker = if i == 0 {
            "→".green().to_string()
        } else {
            " ".to_string()
        };
        println!(
            "{} {:<20} {:.0}%  (pattern: {:.0}%, cap: {:.0}%, learned: {:.0}%, ctx: {:.0}%, semantic: {:.0}%, pri: {:.0}%)",
            marker,
            result.agent_name.cyan(),
            result.confidence * 100.0,
            result.breakdown.pattern_score * 100.0,
            result.breakdown.capability_score * 100.0,
            result.breakdown.learned_score * 100.0,
            result.breakdown.context_score * 100.0,
            result.breakdown.semantic_score * 100.0,
            result.breakdown.priority_score * 100.0,
        );
    }

    Ok(())
}
