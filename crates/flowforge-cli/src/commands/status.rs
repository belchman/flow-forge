use colored::Colorize;
use flowforge_agents::AgentRegistry;
use flowforge_core::{FlowForgeConfig, Result, WorkFilter, WorkStatus};
use flowforge_memory::MemoryDb;

pub fn run(json: bool) -> Result<()> {
    let config_path = FlowForgeConfig::config_path();
    let config = FlowForgeConfig::load(&config_path)?;
    let project_dir = FlowForgeConfig::project_dir();

    if json {
        return run_json(&config, &project_dir);
    }

    println!("{}", "FlowForge Status".bold());
    println!("{}", "─".repeat(40));

    if project_dir.exists() {
        println!(
            "Project: {} ({})",
            "initialized".green(),
            project_dir.display()
        );
    } else {
        println!("Project: {}", "not initialized".red());
        println!("Run `flowforge init --project` to set up.");
        return Ok(());
    }

    let db_path = config.db_path();
    if db_path.exists() {
        let db = MemoryDb::open(&db_path)?;

        if let Ok(Some(session)) = db.get_current_session() {
            println!(
                "Session: {} ({} edits, {} commands)",
                session.id[..8].to_string().cyan(),
                session.edits,
                session.commands,
            );
        } else {
            println!("Session: {}", "none active".yellow());
        }

        if let Ok(count) = db.count_kv() {
            println!("Memory: {} entries", count);
        }

        if let Ok(count) = db.count_patterns() {
            println!("Patterns: {} total", count);
        }

        // Work tracking summary
        let pending = db.count_work_items_by_status(WorkStatus::Pending).unwrap_or(0);
        let in_progress = db.count_work_items_by_status(WorkStatus::InProgress).unwrap_or(0);
        let completed = db.count_work_items_by_status(WorkStatus::Completed).unwrap_or(0);
        let blocked = db.count_work_items_by_status(WorkStatus::Blocked).unwrap_or(0);
        println!(
            "Work: {} pending, {} in-progress, {} completed, {} blocked",
            pending,
            in_progress.to_string().yellow(),
            completed.to_string().green(),
            blocked.to_string().red()
        );

        // Trust score
        if let Ok(Some(session)) = db.get_current_session() {
            if let Ok(Some(trust)) = db.get_trust_score(&session.id) {
                println!("Trust: {:.2}", trust.score);
            }
        }
    } else {
        println!("Database: {}", "not found".red());
    }

    if let Ok(registry) = AgentRegistry::load(&config.agents) {
        println!("Agents: {} loaded", registry.len());
    } else {
        println!("Agents: {}", "failed to load".red());
    }

    println!("\nHooks:");
    println!(
        "  Bash validation: {}",
        if config.hooks.bash_validation { "enabled".green() } else { "disabled".yellow() }
    );
    println!(
        "  Edit tracking:   {}",
        if config.hooks.edit_tracking { "enabled".green() } else { "disabled".yellow() }
    );
    println!(
        "  Routing:         {}",
        if config.hooks.routing { "enabled".green() } else { "disabled".yellow() }
    );
    println!(
        "  Learning:        {}",
        if config.hooks.learning { "enabled".green() } else { "disabled".yellow() }
    );

    Ok(())
}

fn run_json(config: &FlowForgeConfig, project_dir: &std::path::Path) -> Result<()> {
    let mut obj = serde_json::json!({
        "initialized": project_dir.exists(),
    });

    let db_path = config.db_path();
    if db_path.exists() {
        let db = MemoryDb::open(&db_path)?;

        let session = db.get_current_session().ok().flatten();
        let session_json = session.as_ref().map(|s| {
            serde_json::json!({
                "id": s.id,
                "edits": s.edits,
                "commands": s.commands,
                "started_at": s.started_at.to_rfc3339(),
            })
        });

        let trust = session
            .as_ref()
            .and_then(|s| db.get_trust_score(&s.id).ok().flatten())
            .map(|t| t.score);

        let pending = db.count_work_items_by_status(WorkStatus::Pending).unwrap_or(0);
        let in_progress = db.count_work_items_by_status(WorkStatus::InProgress).unwrap_or(0);
        let completed = db.count_work_items_by_status(WorkStatus::Completed).unwrap_or(0);
        let blocked = db.count_work_items_by_status(WorkStatus::Blocked).unwrap_or(0);

        let active_items: Vec<serde_json::Value> = db
            .list_work_items(&WorkFilter {
                status: Some(WorkStatus::InProgress),
                limit: Some(5),
                ..Default::default()
            })
            .unwrap_or_default()
            .iter()
            .map(|i| serde_json::json!({"id": i.id, "title": i.title, "type": i.item_type.to_string()}))
            .collect();

        let memory_count = db.count_kv().unwrap_or(0);
        let pattern_count = db.count_patterns().unwrap_or(0);
        let short_count = db.count_patterns_short().unwrap_or(0);
        let long_count = db.count_patterns_long().unwrap_or(0);

        let agent_count = AgentRegistry::load(&config.agents)
            .map(|r| r.len())
            .unwrap_or(0);

        let (error_fingerprints, error_resolutions, error_occurrences) =
            db.get_error_stats().unwrap_or((0, 0, 0));

        obj = serde_json::json!({
            "initialized": true,
            "session": session_json,
            "trust_score": trust,
            "memory_entries": memory_count,
            "patterns": {
                "total": pattern_count,
                "short_term": short_count,
                "long_term": long_count,
            },
            "work": {
                "pending": pending,
                "in_progress": in_progress,
                "completed": completed,
                "blocked": blocked,
                "active_items": active_items,
            },
            "errors": {
                "fingerprints": error_fingerprints,
                "resolutions": error_resolutions,
                "occurrences": error_occurrences,
            },
            "agents": agent_count,
            "hooks": {
                "bash_validation": config.hooks.bash_validation,
                "edit_tracking": config.hooks.edit_tracking,
                "routing": config.hooks.routing,
                "learning": config.hooks.learning,
            },
            "guidance_enabled": config.guidance.enabled,
        });
    }

    println!("{}", serde_json::to_string_pretty(&obj).unwrap_or_default());
    Ok(())
}
