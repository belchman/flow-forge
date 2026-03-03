use colored::Colorize;
use flowforge_core::config::FlowForgeConfig;
use flowforge_core::plugin::load_all_plugins;
use flowforge_core::Result;

pub fn list() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let plugins_dir = FlowForgeConfig::plugins_dir();

    if !plugins_dir.exists() {
        println!(
            "No plugins directory found. Run {} first.",
            "flowforge init --project".cyan()
        );
        return Ok(());
    }

    let plugins = load_all_plugins(&config.plugins)?;

    if plugins.is_empty() {
        println!("No plugins installed.");
        println!("  Place plugins in {}", plugins_dir.display());
        return Ok(());
    }

    println!("{} ({} plugins)", "Installed Plugins".bold(), plugins.len());
    for p in &plugins {
        let status = if config.plugins.disabled.contains(&p.manifest.plugin.name) {
            "disabled".red()
        } else {
            "enabled".green()
        };
        println!(
            "  {} {} v{} — {} [{}]",
            "•".cyan(),
            p.manifest.plugin.name,
            p.manifest.plugin.version,
            p.manifest.plugin.description,
            status
        );

        if !p.manifest.tools.is_empty() {
            println!(
                "    Tools: {}",
                p.manifest
                    .tools
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !p.manifest.hooks.is_empty() {
            println!(
                "    Hooks: {}",
                p.manifest
                    .hooks
                    .iter()
                    .map(|h| h.event.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !p.manifest.agents.is_empty() {
            println!(
                "    Agents: {}",
                p.manifest
                    .agents
                    .iter()
                    .map(|a| a.path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    Ok(())
}

pub fn info(name: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let plugins = load_all_plugins(&config.plugins)?;

    let plugin = plugins.iter().find(|p| p.manifest.plugin.name == name);

    match plugin {
        Some(p) => {
            println!("{} {}", "Plugin:".bold(), p.manifest.plugin.name);
            println!("  Version: {}", p.manifest.plugin.version);
            println!("  Description: {}", p.manifest.plugin.description);
            println!("  Directory: {}", p.dir.display());

            let is_disabled = config.plugins.disabled.contains(&p.manifest.plugin.name);
            println!(
                "  Status: {}",
                if is_disabled {
                    "disabled".red()
                } else {
                    "enabled".green()
                }
            );

            if !p.manifest.tools.is_empty() {
                println!();
                println!("  Tools:");
                for t in &p.manifest.tools {
                    println!(
                        "    {} — {} (timeout: {}ms)",
                        t.name, t.description, t.timeout
                    );
                }
            }

            if !p.manifest.hooks.is_empty() {
                println!();
                println!("  Hooks:");
                for h in &p.manifest.hooks {
                    println!("    {} — priority {} — {}", h.event, h.priority, h.command);
                }
            }
        }
        None => {
            println!("Plugin '{}' not found.", name);
        }
    }

    Ok(())
}

pub fn enable(name: &str) -> Result<()> {
    let mut config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    config.plugins.disabled.retain(|n| n != name);
    if !config.plugins.enabled.is_empty() && !config.plugins.enabled.contains(&name.to_string()) {
        config.plugins.enabled.push(name.to_string());
    }
    config.save(&FlowForgeConfig::config_path())?;
    println!("{} Plugin '{}' enabled.", "✓".green(), name);
    Ok(())
}

pub fn disable(name: &str) -> Result<()> {
    let mut config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    if !config.plugins.disabled.contains(&name.to_string()) {
        config.plugins.disabled.push(name.to_string());
    }
    config.save(&FlowForgeConfig::config_path())?;
    println!("{} Plugin '{}' disabled.", "✓".green(), name);
    Ok(())
}
