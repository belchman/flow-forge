use colored::Colorize;
use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::{FlowForgeConfig, Result};
use flowforge_memory::MemoryDb;
use std::collections::HashMap;

pub fn run(task: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let registry = AgentRegistry::load(&config.agents)?;
    let router = AgentRouter::new(&config.routing);
    let agents: Vec<&_> = registry.list().into_iter().collect();

    // Load learned weights if available
    let mut learned_weights = HashMap::new();
    if let Ok(db) = MemoryDb::open(&config.db_path()) {
        if let Ok(all_weights) = db.get_all_routing_weights() {
            for w in all_weights {
                learned_weights.insert((w.task_pattern, w.agent_name), w.weight);
            }
        }
    }

    let results = router.route(task, &agents, &learned_weights, None);

    println!("{} \"{}\"", "Routing:".bold(), task);
    println!("{}", "─".repeat(60));

    if results.is_empty() {
        println!("No agents available for routing");
        return Ok(());
    }

    let top_5: Vec<_> = results.iter().take(5).collect();
    for (i, result) in top_5.iter().enumerate() {
        let marker = if i == 0 {
            "→".green().to_string()
        } else {
            " ".to_string()
        };
        println!(
            "{} {:<20} {:.0}%  (pattern: {:.0}%, cap: {:.0}%, learned: {:.0}%, context: {:.0}%, priority: {:.0}%)",
            marker,
            result.agent_name.cyan(),
            result.confidence * 100.0,
            result.breakdown.pattern_score * 100.0,
            result.breakdown.capability_score * 100.0,
            result.breakdown.learned_score * 100.0,
            result.breakdown.context_score * 100.0,
            result.breakdown.priority_score * 100.0,
        );
    }

    Ok(())
}
