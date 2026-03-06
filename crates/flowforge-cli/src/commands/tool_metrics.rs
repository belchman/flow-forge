use colored::Colorize;
use flowforge_core::config::FlowForgeConfig;
use flowforge_core::Result;
use flowforge_memory::MemoryDb;

fn open_db(config: &FlowForgeConfig) -> Result<MemoryDb> {
    MemoryDb::open(&config.db_path())
}

pub fn list(agent: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;
    let metrics = db.list_tool_metrics(agent)?;

    if metrics.is_empty() {
        println!("No tool metrics recorded yet");
        return Ok(());
    }

    println!(
        "{:<20} {:<20} {:<8} {:<8} {:<10}",
        "Tool", "Agent", "OK", "Fail", "Rate"
    );
    println!("{}", "\u{2500}".repeat(66));

    for m in &metrics {
        let agent_display = if m.agent_name.is_empty() {
            "(anonymous)"
        } else {
            &m.agent_name
        };
        let rate = m.success_rate() * 100.0;
        let rate_colored = if rate >= 80.0 {
            format!("{:.1}%", rate).green().to_string()
        } else if rate >= 50.0 {
            format!("{:.1}%", rate).yellow().to_string()
        } else {
            format!("{:.1}%", rate).red().to_string()
        };
        println!(
            "{:<20} {:<20} {:<8} {:<8} {}",
            m.tool_name, agent_display, m.success_count, m.failure_count, rate_colored
        );
    }
    Ok(())
}

pub fn best(tool_name: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;
    let best = db.get_best_agents_for_tool(tool_name, 10)?;

    if best.is_empty() {
        println!(
            "No agents with enough data for tool '{}'",
            tool_name.cyan()
        );
        return Ok(());
    }

    println!("Best agents for '{}':", tool_name.cyan());
    println!("{:<20} {:<12} {:<10}", "Agent", "Rate", "Uses");
    println!("{}", "\u{2500}".repeat(42));

    for (name, rate, total) in &best {
        let rate_pct = rate * 100.0;
        let rate_colored = if rate_pct >= 80.0 {
            format!("{:.1}%", rate_pct).green().to_string()
        } else if rate_pct >= 50.0 {
            format!("{:.1}%", rate_pct).yellow().to_string()
        } else {
            format!("{:.1}%", rate_pct).red().to_string()
        };
        println!("{:<20} {:<12} {}", name, rate_colored, total);
    }
    Ok(())
}
