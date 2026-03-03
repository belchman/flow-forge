use chrono::{DateTime, NaiveDate, Utc};
use colored::Colorize;
use uuid::Uuid;

use flowforge_core::work_tracking;
use flowforge_core::{FlowForgeConfig, Result, WorkFilter, WorkItem};
use flowforge_memory::MemoryDb;

fn open_db(config: &FlowForgeConfig) -> Result<MemoryDb> {
    let db_path = config.db_path();
    if !db_path.exists() {
        return Err(flowforge_core::Error::Config(
            "FlowForge not initialized. Run `flowforge init --project` first.".to_string(),
        ));
    }
    MemoryDb::open(&db_path)
}

pub fn create(
    item_type: &str,
    title: &str,
    description: Option<&str>,
    parent: Option<&str>,
    priority: i32,
) -> Result<()> {
    if title.trim().is_empty() {
        return Err(flowforge_core::Error::InvalidInput(
            "Title cannot be empty.".to_string(),
        ));
    }
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let now = Utc::now();
    let item = WorkItem {
        id: Uuid::new_v4().to_string(),
        external_id: None,
        backend: work_tracking::detect_backend(&config.work_tracking).to_string(),
        item_type: item_type.to_string(),
        title: title.to_string(),
        description: description.map(|s| s.to_string()),
        status: "pending".to_string(),
        assignee: None,
        parent_id: parent.map(|s| s.to_string()),
        priority,
        labels: vec![],
        created_at: now,
        updated_at: now,
        completed_at: None,
        session_id: None,
        metadata: None,
        claimed_by: None,
        claimed_at: None,
        last_heartbeat: None,
        progress: 0,
        stealable: false,
    };

    work_tracking::create_item(&db, &config.work_tracking, &item)?;

    println!(
        "{} Created work item: {} ({})",
        "✓".green(),
        item.id.chars().take(8).collect::<String>(),
        title
    );
    println!(
        "  Type: {}, Priority: {}, Backend: {}",
        item_type, priority, item.backend
    );

    Ok(())
}

pub fn list(status: Option<&str>, item_type: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let filter = WorkFilter {
        status: status.map(|s| s.to_string()),
        item_type: item_type.map(|s| s.to_string()),
        limit: Some(50),
        ..Default::default()
    };

    let items = work_tracking::list_items(&db, &filter)?;

    if items.is_empty() {
        println!("No work items found.");
        return Ok(());
    }

    let backend = work_tracking::detect_backend(&config.work_tracking);
    println!("{} ({} backend)\n", "Work Items".bold(), backend);

    for item in &items {
        let status_colored = match item.status.as_str() {
            "completed" => item.status.green(),
            "in_progress" => item.status.yellow(),
            "blocked" => item.status.red(),
            _ => item.status.normal(),
        };

        let short_id: String = item.id.chars().take(8).collect();
        println!(
            "  {} [{}] {} ({})",
            short_id.dimmed(),
            status_colored,
            item.title,
            item.item_type.dimmed(),
        );

        if let Some(ref assignee) = item.assignee {
            println!("    Assignee: {}", assignee);
        }
    }

    println!("\n{} total items", items.len());
    Ok(())
}

pub fn update(id: &str, status: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    // Try to find the item by partial ID match
    let full_id = resolve_id(&db, id)?;

    work_tracking::update_status(&db, &config.work_tracking, &full_id, status, "user")?;

    println!("{} Updated {} → {}", "✓".green(), id, status);
    Ok(())
}

pub fn close(id: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let full_id = resolve_id(&db, id)?;
    work_tracking::close_item(&db, &config.work_tracking, &full_id, "user")?;

    println!("{} Closed {}", "✓".green(), id);
    Ok(())
}

pub fn sync() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let backend = work_tracking::detect_backend(&config.work_tracking);
    println!("Syncing with {} backend...", backend);

    // Pull from external backend
    let pulled = work_tracking::sync_from_backend(&db, &config.work_tracking)?;
    if pulled > 0 {
        println!("  Pulled {} items from {}", pulled, backend);
    }

    // Push FlowForge-only items to external backend
    let pushed = work_tracking::push_to_backend(&db, &config.work_tracking)?;
    if pushed > 0 {
        println!("  Pushed {} items to {}", pushed, backend);
    }

    println!("{} Sync complete (backend: {})", "✓".green(), backend);
    Ok(())
}

pub fn status() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let backend = work_tracking::detect_backend(&config.work_tracking);

    let pending = db.count_work_items_by_status("pending").unwrap_or(0);
    let in_progress = db.count_work_items_by_status("in_progress").unwrap_or(0);
    let completed = db.count_work_items_by_status("completed").unwrap_or(0);
    let blocked = db.count_work_items_by_status("blocked").unwrap_or(0);

    println!("{}", "Work Tracking Status".bold());
    println!("  Backend: {}", backend);
    println!("  Pending: {}", pending);
    println!("  In Progress: {}", in_progress.to_string().yellow());
    println!("  Blocked: {}", blocked.to_string().red());
    println!("  Completed: {}", completed.to_string().green());
    println!("  Total: {}", pending + in_progress + completed + blocked);

    // Show recent active items
    let filter = WorkFilter {
        status: Some("in_progress".to_string()),
        limit: Some(5),
        ..Default::default()
    };
    if let Ok(active) = db.list_work_items(&filter) {
        if !active.is_empty() {
            println!("\n{}", "Active Items:".bold());
            for item in &active {
                let short_id: String = item.id.chars().take(8).collect();
                println!(
                    "  {} [{}] {}",
                    short_id.dimmed(),
                    item.item_type,
                    item.title
                );
            }
        }
    }

    Ok(())
}

pub fn log(limit: usize, since: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let events = if let Some(since_str) = since {
        let since_dt = parse_since_date(since_str)?;
        db.get_recent_work_events_since(since_dt, limit)?
    } else {
        db.get_recent_work_events(limit)?
    };

    if events.is_empty() {
        println!("No work events found.");
        return Ok(());
    }

    println!("{}\n", "Work Event Log".bold());
    for event in &events {
        let short_id: String = event.work_item_id.chars().take(8).collect();
        let actor = event.actor.as_deref().unwrap_or("system");
        let detail = match (event.old_value.as_deref(), event.new_value.as_deref()) {
            (Some(old), Some(new)) => format!("{} → {}", old, new),
            (None, Some(new)) => new.to_string(),
            (Some(old), None) => format!("(was: {})", old),
            (None, None) => String::new(),
        };

        println!(
            "  {} {} {} {} {}",
            event.timestamp.format("%m-%d %H:%M").to_string().dimmed(),
            short_id.dimmed(),
            event.event_type.cyan(),
            detail,
            format!("({})", actor).dimmed(),
        );
    }

    Ok(())
}

/// Parse a date string like "2024-01-15", "1d", "7d", "1w", "1h" into a DateTime<Utc>.
fn parse_since_date(s: &str) -> Result<DateTime<Utc>> {
    let s = s.trim();

    // Try relative durations: "1d", "7d", "1w", "2h"
    if let Some(num_str) = s.strip_suffix('d') {
        if let Ok(days) = num_str.parse::<i64>() {
            if days < 0 {
                return Err(flowforge_core::Error::InvalidInput(
                    "Duration cannot be negative.".to_string(),
                ));
            }
            return Ok(Utc::now() - chrono::Duration::days(days));
        }
    }
    if let Some(num_str) = s.strip_suffix('w') {
        if let Ok(weeks) = num_str.parse::<i64>() {
            if weeks < 0 {
                return Err(flowforge_core::Error::InvalidInput(
                    "Duration cannot be negative.".to_string(),
                ));
            }
            return Ok(Utc::now() - chrono::Duration::weeks(weeks));
        }
    }
    if let Some(num_str) = s.strip_suffix('h') {
        if let Ok(hours) = num_str.parse::<i64>() {
            if hours < 0 {
                return Err(flowforge_core::Error::InvalidInput(
                    "Duration cannot be negative.".to_string(),
                ));
            }
            return Ok(Utc::now() - chrono::Duration::hours(hours));
        }
    }

    // Try YYYY-MM-DD date format
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        if let Some(dt) = date.and_hms_opt(0, 0, 0) {
            return Ok(dt.and_utc());
        }
    }

    Err(flowforge_core::Error::InvalidInput(format!(
        "Invalid date '{}'. Use YYYY-MM-DD, Nd (days), Nw (weeks), or Nh (hours).",
        s
    )))
}

pub fn claim(id: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let session_id = db
        .get_current_session()?
        .map(|s| s.id)
        .unwrap_or_else(|| "unknown".to_string());

    if db.claim_work_item(id, &session_id)? {
        println!(
            "{} Claimed work item {}",
            "✓".green(),
            &id[..8.min(id.len())]
        );
    } else {
        println!(
            "{} Could not claim work item {} (already claimed?)",
            "✗".red(),
            &id[..8.min(id.len())]
        );
    }
    Ok(())
}

pub fn release(id: &str) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    db.release_work_item(id)?;
    println!(
        "{} Released work item {}",
        "✓".green(),
        &id[..8.min(id.len())]
    );
    Ok(())
}

pub fn stealable() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let items = db.get_stealable_items(20)?;

    if items.is_empty() {
        println!("No stealable work items.");
        return Ok(());
    }

    println!("{} ({} items)", "Stealable Work Items".bold(), items.len());
    for item in &items {
        println!(
            "  {} {} — {} (priority: {}, progress: {}%)",
            "•".yellow(),
            &item.id[..8.min(item.id.len())],
            item.title,
            item.priority,
            item.progress
        );
        if let Some(ref claimed) = item.claimed_by {
            println!("    Claimed by: {}", &claimed[..8.min(claimed.len())]);
        }
    }
    Ok(())
}

pub fn steal(id: Option<&str>) -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    let session_id = db
        .get_current_session()?
        .map(|s| s.id)
        .unwrap_or_else(|| "unknown".to_string());

    let target_id = match id {
        Some(id) => id.to_string(),
        None => {
            let items = db.get_stealable_items(1)?;
            match items.first() {
                Some(item) => item.id.clone(),
                None => {
                    println!("No stealable work items available.");
                    return Ok(());
                }
            }
        }
    };

    if db.steal_work_item(&target_id, &session_id)? {
        println!(
            "{} Stole work item {}",
            "✓".green(),
            &target_id[..8.min(target_id.len())]
        );
    } else {
        println!(
            "{} Could not steal work item {}",
            "✗".red(),
            &target_id[..8.min(target_id.len())]
        );
    }
    Ok(())
}

pub fn load() -> Result<()> {
    let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())?;
    let db = open_db(&config)?;

    // Show work distribution
    let in_progress = db.list_work_items(&WorkFilter {
        status: Some("in_progress".to_string()),
        ..Default::default()
    })?;

    println!("{}", "Work Distribution".bold());

    if in_progress.is_empty() {
        println!("  No in-progress work items.");
        return Ok(());
    }

    // Group by assignee/claimed_by
    let mut by_agent: std::collections::HashMap<String, Vec<&WorkItem>> =
        std::collections::HashMap::new();
    for item in &in_progress {
        let agent = item
            .claimed_by
            .as_deref()
            .or(item.assignee.as_deref())
            .unwrap_or("unassigned");
        by_agent.entry(agent.to_string()).or_default().push(item);
    }

    for (agent, items) in &by_agent {
        let label = if agent.len() > 8 { &agent[..8] } else { agent };
        println!("  {} ({} items):", label.cyan(), items.len());
        for item in items {
            let stale = if item.stealable {
                " [stealable]".yellow().to_string()
            } else {
                String::new()
            };
            println!(
                "    • {} — {} ({}%){}",
                &item.id[..8.min(item.id.len())],
                item.title,
                item.progress,
                stale
            );
        }
    }

    // Show stealable count
    let stealable_count = db.get_stealable_items(100)?.len();
    if stealable_count > 0 {
        println!();
        println!(
            "  {} {} stealable items available",
            "⚠".yellow(),
            stealable_count
        );
    }

    Ok(())
}

/// Resolve a partial ID to a full work item ID.
fn resolve_id(db: &MemoryDb, partial: &str) -> Result<String> {
    // Try exact match first
    if let Ok(Some(item)) = db.get_work_item(partial) {
        return Ok(item.id);
    }

    // Try prefix match
    let all = db.list_work_items(&WorkFilter {
        limit: Some(1000),
        ..Default::default()
    })?;

    let matches: Vec<_> = all.iter().filter(|i| i.id.starts_with(partial)).collect();

    match matches.len() {
        0 => Err(flowforge_core::Error::NotFound(format!(
            "No work item matching '{}'",
            partial
        ))),
        1 => Ok(matches[0].id.clone()),
        _ => Err(flowforge_core::Error::InvalidInput(format!(
            "Ambiguous ID '{}' matches {} items. Use more characters.",
            partial,
            matches.len()
        ))),
    }
}
