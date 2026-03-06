use colored::Colorize;
use flowforge_core::config::FlowForgeConfig;
use flowforge_core::Result;
use flowforge_memory::MemoryDb;

fn open_db(config: &FlowForgeConfig) -> Result<MemoryDb> {
    MemoryDb::open(&config.db_path())
}

pub fn list(limit: usize) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let fingerprints = db.list_error_fingerprints(limit)?;

    if fingerprints.is_empty() {
        println!("No error patterns recorded yet.");
        return Ok(());
    }

    println!(
        "{} Known Error Patterns ({} total)",
        "ℹ".blue(),
        fingerprints.len()
    );
    println!();

    for fp in &fingerprints {
        let category = format!("[{}]", fp.category).cyan();
        let count = format!("{}x", fp.occurrence_count).yellow();
        let tool = fp
            .tool_name
            .as_deref()
            .unwrap_or("unknown")
            .dimmed()
            .to_string();

        println!("  {} {} {} ({})", fp.id.dimmed(), category, count, tool);

        // Show preview truncated to 80 chars
        let preview: String = fp.error_preview.chars().take(80).collect();
        println!("    {}", preview.dimmed());

        // Show resolutions if any
        let resolutions = db.get_resolutions_for_fingerprint(&fp.id, 3)?;
        if !resolutions.is_empty() {
            for r in &resolutions {
                let conf = format!("{:.0}%", r.confidence() * 100.0);
                let conf_colored = if r.confidence() >= 0.8 {
                    conf.green()
                } else if r.confidence() >= 0.5 {
                    conf.yellow()
                } else {
                    conf.red()
                };
                println!("    {} {} {}", "→".green(), conf_colored, r.resolution_summary);
            }
        }
        println!();
    }

    Ok(())
}

pub fn find(error_text: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    match db.find_error_resolutions(error_text, 5)? {
        Some((fp, resolutions)) => {
            println!(
                "{} Found matching error pattern: {} (seen {}x)",
                "ℹ".blue(),
                fp.id,
                fp.occurrence_count
            );
            println!(
                "  Category: {}",
                format!("{}", fp.category).cyan()
            );
            println!();

            if resolutions.is_empty() {
                println!("  No known resolutions yet.");
            } else {
                println!("  {} Known Resolutions:", "→".green());
                for r in &resolutions {
                    let conf = format!("{:.0}%", r.confidence() * 100.0);
                    let conf_colored = if r.confidence() >= 0.8 {
                        conf.green()
                    } else if r.confidence() >= 0.5 {
                        conf.yellow()
                    } else {
                        conf.red()
                    };
                    println!(
                        "    {} {} (success: {}, fail: {})",
                        conf_colored, r.resolution_summary, r.success_count, r.failure_count
                    );
                    if !r.tool_sequence.is_empty() {
                        println!("      Tools: {}", r.tool_sequence.join(" → "));
                    }
                    if !r.files_changed.is_empty() {
                        println!("      Files: {}", r.files_changed.join(", "));
                    }
                }
            }
        }
        None => {
            println!("No matching error pattern found.");
        }
    }

    Ok(())
}

pub fn stats() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let (fingerprints, resolutions, total_occurrences) = db.get_error_stats()?;

    println!("{} Error Recovery Stats", "ℹ".blue());
    println!("  Unique error patterns: {}", fingerprints);
    println!("  Known resolutions:     {}", resolutions);
    println!("  Total occurrences:     {}", total_occurrences);

    Ok(())
}
