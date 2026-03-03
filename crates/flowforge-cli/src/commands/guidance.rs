use colored::Colorize;
use flowforge_core::config::FlowForgeConfig;
use flowforge_core::Result;
use flowforge_memory::MemoryDb;
use sha2::{Digest, Sha256};

fn open_db(config: &FlowForgeConfig) -> Result<MemoryDb> {
    MemoryDb::open(&config.db_path())
}

pub fn rules() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;

    println!("{}", "Guidance Rules & Gates".bold());
    println!();

    println!(
        "  {} Destructive operations gate: {}",
        "•".cyan(),
        if config.guidance.destructive_ops_gate {
            "enabled".green()
        } else {
            "disabled".red()
        }
    );
    println!(
        "  {} File scope gate: {}",
        "•".cyan(),
        if config.guidance.file_scope_gate {
            "enabled".green()
        } else {
            "disabled".red()
        }
    );
    println!(
        "  {} Diff size gate: {} (max {} lines)",
        "•".cyan(),
        if config.guidance.diff_size_gate {
            "enabled".green()
        } else {
            "disabled".red()
        },
        config.guidance.max_diff_lines
    );
    println!(
        "  {} Secrets detection gate: {}",
        "•".cyan(),
        if config.guidance.secrets_gate {
            "enabled".green()
        } else {
            "disabled".red()
        }
    );

    if !config.guidance.custom_rules.is_empty() {
        println!();
        println!("  Custom rules:");
        for rule in &config.guidance.custom_rules {
            let status = if rule.enabled {
                "✓".green()
            } else {
                "✗".red()
            };
            println!(
                "    {} [{}] {} — {} ({})",
                status, rule.scope, rule.pattern, rule.description, rule.risk_level
            );
        }
    }

    if !config.guidance.protected_paths.is_empty() {
        println!();
        println!("  Protected paths:");
        for path in &config.guidance.protected_paths {
            println!("    {} {}", "•".yellow(), path);
        }
    }

    println!();
    println!(
        "  Trust: initial={:.2}, ask_threshold={:.2}, decay={:.4}/hr",
        config.guidance.trust_initial_score,
        config.guidance.trust_ask_threshold,
        config.guidance.trust_decay_per_hour
    );

    Ok(())
}

pub fn trust(session_id: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let sid = match session_id {
        Some(s) => s.to_string(),
        None => db
            .get_current_session()?
            .map(|s| s.id)
            .unwrap_or_else(|| "unknown".to_string()),
    };

    match db.get_trust_score(&sid)? {
        Some(trust) => {
            println!(
                "{} Trust Score for session {}",
                "ℹ".blue(),
                &sid[..8.min(sid.len())]
            );
            println!("  Score: {:.4}", trust.score);
            println!(
                "  Checks: {} total ({} allows, {} asks, {} denials)",
                trust.total_checks, trust.allows, trust.asks, trust.denials
            );
            println!(
                "  Last updated: {}",
                trust.last_updated.format("%Y-%m-%d %H:%M:%S")
            );
        }
        None => {
            println!(
                "No trust score found for session {}",
                &sid[..8.min(sid.len())]
            );
        }
    }

    Ok(())
}

pub fn audit(session_id: Option<&str>, limit: usize) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let sid = match session_id {
        Some(s) => s.to_string(),
        None => db
            .get_current_session()?
            .map(|s| s.id)
            .unwrap_or_else(|| "unknown".to_string()),
    };

    let decisions = db.get_gate_decisions(&sid, limit)?;

    if decisions.is_empty() {
        println!(
            "No gate decisions found for session {}",
            &sid[..8.min(sid.len())]
        );
        return Ok(());
    }

    println!(
        "{} Gate Decisions for session {} ({} entries)",
        "ℹ".blue(),
        &sid[..8.min(sid.len())],
        decisions.len()
    );
    for d in &decisions {
        let action_str = match d.action {
            flowforge_core::types::GateAction::Deny => "DENY".red().to_string(),
            flowforge_core::types::GateAction::Ask => "ASK".yellow().to_string(),
            flowforge_core::types::GateAction::Allow => "ALLOW".green().to_string(),
        };
        println!(
            "  {} [{}] {} on {} — {} (trust: {:.2}→{:.2})",
            d.timestamp.format("%H:%M:%S"),
            action_str,
            d.gate_name,
            d.tool_name,
            d.reason,
            d.trust_before,
            d.trust_after
        );
    }

    Ok(())
}

pub fn verify(session_id: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let sid = match session_id {
        Some(s) => s.to_string(),
        None => db
            .get_current_session()?
            .map(|s| s.id)
            .unwrap_or_else(|| "unknown".to_string()),
    };

    let decisions = db.get_gate_decisions(&sid, 10000)?;

    if decisions.is_empty() {
        println!("No audit entries to verify.");
        return Ok(());
    }

    let mut prev_hash = String::new();
    let mut valid = 0u32;
    let mut invalid = 0u32;

    for d in &decisions {
        let expected_input = format!("{}{}{}{}", d.session_id, d.tool_name, d.reason, prev_hash);
        let expected_hash = format!("{:x}", Sha256::digest(expected_input.as_bytes()));

        if d.hash == expected_hash && d.prev_hash == prev_hash {
            valid += 1;
        } else {
            invalid += 1;
            println!("  {} Entry #{}: hash mismatch", "✗".red(), d.id);
        }
        prev_hash = d.hash.clone();
    }

    if invalid == 0 {
        println!(
            "{} Audit chain verified: {} entries, all valid",
            "✓".green(),
            valid
        );
    } else {
        println!(
            "{} Audit chain broken: {} valid, {} invalid",
            "✗".red(),
            valid,
            invalid
        );
    }

    Ok(())
}
