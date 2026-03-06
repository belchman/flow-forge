use colored::Colorize;
use flowforge_agents::AgentRegistry;
use flowforge_core::{FlowForgeConfig, Result};

pub fn list() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let registry = AgentRegistry::load(&config.agents)?;

    let mut agents: Vec<_> = registry.list();
    agents.sort_by(|a, b| a.name.cmp(&b.name));

    if agents.is_empty() {
        println!("No agents loaded");
        return Ok(());
    }

    println!(
        "{:<25} {:<10} {:<40} Source",
        "Name", "Priority", "Description"
    );
    println!("{}", "─".repeat(90));

    for agent in &agents {
        let priority = format!("{:?}", agent.priority);
        let desc = if agent.description.chars().count() > 38 {
            let truncated: String = agent.description.chars().take(35).collect();
            format!("{truncated}...")
        } else {
            agent.description.clone()
        };
        let source = format!("{:?}", agent.source);

        println!(
            "{:<25} {:<10} {:<40} {}",
            agent.name.cyan(),
            priority,
            desc,
            source.dimmed()
        );
    }

    println!("\n{} agents loaded", agents.len());
    Ok(())
}

pub fn info(name: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let registry = AgentRegistry::load(&config.agents)?;

    match registry.get(name) {
        Some(agent) => {
            println!("{}", agent.name.bold());
            println!("{}", "─".repeat(40));
            println!("Description:  {}", agent.description);
            println!("Priority:     {:?}", agent.priority);
            println!("Source:       {:?}", agent.source);
            if let Some(color) = &agent.color {
                println!("Color:        {}", color);
            }
            if !agent.capabilities.is_empty() {
                println!("Capabilities: {}", agent.capabilities.join(", "));
            }
            if !agent.patterns.is_empty() {
                println!("Patterns:     {}", agent.patterns.join(", "));
            }
            if !agent.body.is_empty() {
                println!("\n{}", agent.body);
            }
        }
        None => {
            eprintln!("{}: agent '{}' not found", "Error".red(), name);
            std::process::exit(1);
        }
    }
    Ok(())
}

pub fn search(query: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let registry = AgentRegistry::load(&config.agents)?;

    let results = registry.search(query);
    if results.is_empty() {
        println!("No agents found matching '{query}'");
        return Ok(());
    }

    for agent in &results {
        println!("{}: {}", agent.name.cyan(), agent.description);
    }

    Ok(())
}
