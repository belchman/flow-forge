//! Work tracking abstraction and backend implementations.
//! Supports Claude Tasks, Beads, Kanbus, and FlowForge's internal SQLite.

use std::path::Path;

use tracing::warn;

use crate::config::WorkTrackingConfig;
use crate::types::{WorkEvent, WorkFilter, WorkItem};
use crate::Result;

/// Detect which work tracking backend is active.
pub fn detect_backend(config: &WorkTrackingConfig) -> &str {
    if config.backend != "auto" {
        return &config.backend;
    }

    // Check for Kanbus
    if Path::new(".kanbus.yml").exists() || Path::new(".kanbus").exists() {
        return "kanbus";
    }

    // Check for Beads
    if Path::new(".beads").exists() {
        return "beads";
    }

    // Check for Claude Tasks via environment
    if std::env::var("CLAUDE_CODE_TASK_LIST_ID").is_ok() {
        return "claude_tasks";
    }

    // Check for Claude Tasks directory
    let home = dirs::home_dir().unwrap_or_default();
    if home.join(".claude/tasks").exists() {
        return "claude_tasks";
    }

    "flowforge"
}

/// Create a work item and log to the appropriate backend.
pub fn create_item(db: &dyn WorkDb, config: &WorkTrackingConfig, item: &WorkItem) -> Result<()> {
    // Always log to FlowForge SQLite
    db.create_work_item(item)?;

    // Log creation event
    let event = WorkEvent {
        id: 0,
        work_item_id: item.id.clone(),
        event_type: "created".to_string(),
        old_value: None,
        new_value: Some(item.title.clone()),
        actor: Some("user".to_string()),
        timestamp: chrono::Utc::now(),
    };
    db.record_work_event(&event)?;

    // Forward to external backend
    let backend = detect_backend(config);
    match backend {
        "kanbus" => sync_to_kanbus(item, config),
        "beads" => sync_to_beads(item),
        "claude_tasks" => sync_to_claude_tasks(item, config),
        _ => Ok(()),
    }
}

/// Update a work item's status.
pub fn update_status(
    db: &dyn WorkDb,
    config: &WorkTrackingConfig,
    id: &str,
    new_status: &str,
    actor: &str,
) -> Result<()> {
    let old_item = db.get_work_item(id)?;
    let old_status = old_item
        .as_ref()
        .map(|i| i.status.clone())
        .unwrap_or_default();

    db.update_work_item_status(id, new_status)?;

    let event = WorkEvent {
        id: 0,
        work_item_id: id.to_string(),
        event_type: "status_changed".to_string(),
        old_value: Some(old_status),
        new_value: Some(new_status.to_string()),
        actor: Some(actor.to_string()),
        timestamp: chrono::Utc::now(),
    };
    db.record_work_event(&event)?;

    // Sync status to external backend
    let backend = detect_backend(config);
    if let Some(item) = &old_item {
        match backend {
            "claude_tasks" => sync_status_to_claude_tasks(&item.id, new_status, config)?,
            _ => {
                if let Some(ref ext_id) = item.external_id {
                    match backend {
                        "kanbus" => sync_status_to_kanbus(ext_id, new_status),
                        "beads" => sync_status_to_beads(ext_id, new_status),
                        _ => Ok(()),
                    }?;
                }
            }
        }
    }

    Ok(())
}

/// Close a work item (set to completed).
pub fn close_item(
    db: &dyn WorkDb,
    config: &WorkTrackingConfig,
    id: &str,
    actor: &str,
) -> Result<()> {
    update_status(db, config, id, "completed", actor)
}

/// List work items with optional filter.
pub fn list_items(db: &dyn WorkDb, filter: &WorkFilter) -> Result<Vec<WorkItem>> {
    db.list_work_items(filter)
}

/// Get audit trail for a work item.
pub fn get_events(db: &dyn WorkDb, work_item_id: &str, limit: usize) -> Result<Vec<WorkEvent>> {
    db.get_work_events(work_item_id, limit)
}

/// Get recent events across all work items.
pub fn get_recent_events(db: &dyn WorkDb, limit: usize) -> Result<Vec<WorkEvent>> {
    db.get_recent_work_events(limit)
}

// ── Database trait to decouple from MemoryDb ──

/// Trait for work tracking database operations.
/// Implemented by MemoryDb so we can use it from both CLI and MCP.
pub trait WorkDb {
    fn create_work_item(&self, item: &WorkItem) -> Result<()>;
    fn get_work_item(&self, id: &str) -> Result<Option<WorkItem>>;
    fn get_work_item_by_external_id(&self, external_id: &str) -> Result<Option<WorkItem>>;
    fn update_work_item_status(&self, id: &str, status: &str) -> Result<()>;
    fn update_work_item_assignee(&self, id: &str, assignee: &str) -> Result<()>;
    fn list_work_items(&self, filter: &WorkFilter) -> Result<Vec<WorkItem>>;
    fn update_work_item_backend(&self, id: &str, backend: &str) -> Result<()>;
    fn delete_work_item(&self, id: &str) -> Result<()>;
    fn count_work_items_by_status(&self, status: &str) -> Result<u64>;
    fn record_work_event(&self, event: &WorkEvent) -> Result<i64>;
    fn get_work_events(&self, work_item_id: &str, limit: usize) -> Result<Vec<WorkEvent>>;
    fn get_recent_work_events(&self, limit: usize) -> Result<Vec<WorkEvent>>;

    // Work-stealing methods
    fn claim_work_item(&self, id: &str, session_id: &str) -> Result<bool>;
    fn release_work_item(&self, id: &str) -> Result<()>;
    fn update_heartbeat(&self, session_id: &str) -> Result<u64>;
    fn update_progress(&self, id: &str, progress: i32) -> Result<()>;
    fn mark_stale_items_stealable(&self, stale_mins: u64, min_progress: i32) -> Result<u64>;
    fn auto_release_abandoned(&self, abandon_mins: u64) -> Result<u64>;
    fn get_stealable_items(&self, limit: usize) -> Result<Vec<WorkItem>>;
    fn steal_work_item(&self, id: &str, new_session_id: &str) -> Result<bool>;
}

// ── Work-stealing functions ──

/// Claim a work item for a session.
pub fn claim_item(db: &dyn WorkDb, id: &str, session_id: &str) -> Result<bool> {
    db.claim_work_item(id, session_id)
}

/// Release a claimed work item.
pub fn release_item(db: &dyn WorkDb, id: &str) -> Result<()> {
    db.release_work_item(id)
}

/// Steal a stealable work item for a new session.
pub fn steal_item(db: &dyn WorkDb, id: &str, new_session_id: &str) -> Result<bool> {
    db.steal_work_item(id, new_session_id)
}

/// Detect and mark stale items, auto-release abandoned ones.
pub fn detect_stale(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<(u64, u64)> {
    let ws = &config.work_stealing;
    if !ws.enabled {
        return Ok((0, 0));
    }
    let marked = db.mark_stale_items_stealable(ws.stale_threshold_mins, ws.stale_min_progress)?;
    let released = db.auto_release_abandoned(ws.abandon_threshold_mins)?;
    Ok((marked, released))
}

/// List stealable work items.
pub fn list_stealable(db: &dyn WorkDb, limit: usize) -> Result<Vec<WorkItem>> {
    db.get_stealable_items(limit)
}

// ── External backend sync (best-effort, CLI-based) ──

fn sync_to_kanbus(item: &WorkItem, config: &WorkTrackingConfig) -> Result<()> {
    let project_key = config.kanbus.project_key.as_deref().unwrap_or("");

    let mut cmd = std::process::Command::new("kanbus");
    cmd.arg("create")
        .arg("--title")
        .arg(&item.title)
        .arg("--type")
        .arg(&item.item_type)
        .arg("--json");

    if !project_key.is_empty() {
        cmd.arg("--project").arg(project_key);
    }

    // Best effort — don't fail the whole operation if kanbus isn't installed
    match cmd.output() {
        Ok(o) if !o.status.success() => {
            warn!(
                "kanbus create failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => warn!("kanbus not available: {e}"),
        _ => {}
    }
    Ok(())
}

fn sync_to_beads(item: &WorkItem) -> Result<()> {
    let mut cmd = std::process::Command::new("bd");
    cmd.arg("create").arg(&item.title);

    if let Some(ref desc) = item.description {
        cmd.arg("--description").arg(desc);
    }

    match cmd.output() {
        Ok(o) if !o.status.success() => {
            warn!("bd create failed: {}", String::from_utf8_lossy(&o.stderr));
        }
        Err(e) => warn!("bd not available: {e}"),
        _ => {}
    }
    Ok(())
}

fn sync_status_to_kanbus(external_id: &str, status: &str) -> Result<()> {
    let kanbus_status = match status {
        "completed" => "done",
        "in_progress" => "in-progress",
        "blocked" => "blocked",
        _ => status,
    };

    if let Err(e) = std::process::Command::new("kanbus")
        .arg("update")
        .arg(external_id)
        .arg("--status")
        .arg(kanbus_status)
        .arg("--json")
        .output()
    {
        warn!("kanbus status update failed: {e}");
    }
    Ok(())
}

fn sync_status_to_beads(external_id: &str, status: &str) -> Result<()> {
    let result = match status {
        "completed" => std::process::Command::new("bd")
            .arg("close")
            .arg(external_id)
            .output(),
        _ => std::process::Command::new("bd")
            .arg("update")
            .arg(external_id)
            .arg("--status")
            .arg(status)
            .output(),
    };
    if let Err(e) = result {
        warn!("bd status update failed: {e}");
    }
    Ok(())
}

fn sync_to_claude_tasks(item: &WorkItem, config: &WorkTrackingConfig) -> Result<()> {
    let tasks_dir = claude_tasks_dir(config);
    if std::fs::create_dir_all(&tasks_dir).is_err() {
        return Ok(()); // Best effort
    }

    let task_file = tasks_dir.join(format!("{}.json", item.id));
    let task_json = serde_json::json!({
        "id": item.id,
        "subject": item.title,
        "description": item.description,
        "status": item.status,
        "owner": item.assignee,
        "metadata": {
            "flowforge_backend": item.backend,
            "item_type": item.item_type,
            "priority": item.priority,
            "parent_id": item.parent_id,
        }
    });

    if let Err(e) = std::fs::write(
        &task_file,
        serde_json::to_string_pretty(&task_json).unwrap_or_default(),
    ) {
        warn!("claude tasks write failed: {e}");
    }
    Ok(())
}

fn sync_status_to_claude_tasks(
    item_id: &str,
    status: &str,
    config: &WorkTrackingConfig,
) -> Result<()> {
    let task_file = claude_tasks_dir(config).join(format!("{}.json", item_id));
    if let Ok(content) = std::fs::read_to_string(&task_file) {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
            json["status"] = serde_json::Value::String(status.to_string());
            if let Err(e) = std::fs::write(
                &task_file,
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            ) {
                warn!("claude tasks status update write failed: {e}");
            }
        }
    }
    Ok(())
}

fn claude_tasks_dir(config: &WorkTrackingConfig) -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_default();
    let list_id = config
        .claude_tasks
        .list_id
        .as_deref()
        .unwrap_or("flowforge");
    home.join(".claude").join("tasks").join(list_id)
}

// ── Inbound sync: pull items from external backends into FlowForge SQLite ──

/// Sync work items from the active external backend into the FlowForge DB.
/// Returns the number of items synced.
pub fn sync_from_backend(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let backend = detect_backend(config);
    match backend {
        "kanbus" => sync_from_kanbus(db, config),
        "beads" => sync_from_beads(db),
        "claude_tasks" => sync_from_claude_tasks(db, config),
        _ => Ok(0),
    }
}

fn sync_from_kanbus(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let mut cmd = std::process::Command::new("kanbus");
    cmd.arg("list").arg("--json");

    if let Some(ref key) = config.kanbus.project_key {
        cmd.arg("--project").arg(key);
    }

    let output = match cmd.output() {
        Ok(o) if o.status.success() => o,
        _ => return Ok(0),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let items: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => return Ok(0),
    };

    let mut synced = 0u32;
    let now = chrono::Utc::now();

    for item in &items {
        let ext_id = item["id"].as_str().unwrap_or_default();
        if ext_id.is_empty() {
            continue;
        }

        // Skip if already synced
        if db.get_work_item_by_external_id(ext_id)?.is_some() {
            continue;
        }

        let status = match item["status"].as_str().unwrap_or("pending") {
            "done" | "closed" => "completed",
            "in-progress" => "in_progress",
            other => other,
        };

        let work_item = WorkItem {
            id: uuid::Uuid::new_v4().to_string(),
            external_id: Some(ext_id.to_string()),
            backend: "kanbus".to_string(),
            item_type: item["type"].as_str().unwrap_or("task").to_string(),
            title: item["title"].as_str().unwrap_or("(untitled)").to_string(),
            description: item["description"].as_str().map(|s| s.to_string()),
            status: status.to_string(),
            assignee: item["assignee"].as_str().map(|s| s.to_string()),
            parent_id: None,
            priority: item["priority"].as_i64().unwrap_or(2) as i32,
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

        db.create_work_item(&work_item)?;
        synced += 1;
    }

    Ok(synced)
}

fn sync_from_beads(db: &dyn WorkDb) -> Result<u32> {
    let beads_file = std::path::Path::new(".beads/issues.jsonl");
    if !beads_file.exists() {
        return Ok(0);
    }

    let content = match std::fs::read_to_string(beads_file) {
        Ok(c) => c,
        Err(_) => return Ok(0),
    };

    let mut synced = 0u32;
    let now = chrono::Utc::now();

    for line in content.lines() {
        let item: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ext_id = item["id"].as_str().unwrap_or_default();
        if ext_id.is_empty() {
            continue;
        }

        if db.get_work_item_by_external_id(ext_id)?.is_some() {
            continue;
        }

        let status = match item["status"].as_str().unwrap_or("open") {
            "closed" | "done" => "completed",
            "open" => "pending",
            other => other,
        };

        let work_item = WorkItem {
            id: uuid::Uuid::new_v4().to_string(),
            external_id: Some(ext_id.to_string()),
            backend: "beads".to_string(),
            item_type: "task".to_string(),
            title: item["title"].as_str().unwrap_or("(untitled)").to_string(),
            description: item["body"].as_str().map(|s| s.to_string()),
            status: status.to_string(),
            assignee: None,
            parent_id: None,
            priority: 2,
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

        db.create_work_item(&work_item)?;
        synced += 1;
    }

    Ok(synced)
}

fn sync_from_claude_tasks(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let tasks_dir = claude_tasks_dir(config);
    if !tasks_dir.exists() {
        return Ok(0);
    }

    let entries = match std::fs::read_dir(&tasks_dir) {
        Ok(e) => e,
        Err(_) => return Ok(0),
    };

    let mut synced = 0u32;
    let now = chrono::Utc::now();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let item: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let ext_id = item["id"].as_str().unwrap_or_default();
        if ext_id.is_empty() {
            continue;
        }

        // Skip if this is a FlowForge-managed task (already in DB by primary ID)
        if db.get_work_item(ext_id)?.is_some() {
            continue;
        }
        if db.get_work_item_by_external_id(ext_id)?.is_some() {
            continue;
        }

        let work_item = WorkItem {
            id: uuid::Uuid::new_v4().to_string(),
            external_id: Some(ext_id.to_string()),
            backend: "claude_tasks".to_string(),
            item_type: item["metadata"]["item_type"]
                .as_str()
                .unwrap_or("task")
                .to_string(),
            title: item["subject"]
                .as_str()
                .or_else(|| item["title"].as_str())
                .unwrap_or("(untitled)")
                .to_string(),
            description: item["description"].as_str().map(|s| s.to_string()),
            status: item["status"].as_str().unwrap_or("pending").to_string(),
            assignee: item["owner"].as_str().map(|s| s.to_string()),
            parent_id: None,
            priority: 2,
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

        db.create_work_item(&work_item)?;
        synced += 1;
    }

    Ok(synced)
}

/// Push FlowForge-only items to the active external backend.
/// Items with backend="flowforge" get synced outward on session end.
/// After pushing, updates the item's backend field so it won't be pushed again.
pub fn push_to_backend(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let backend = detect_backend(config);
    if backend == "flowforge" {
        return Ok(0); // No external backend to push to
    }

    let filter = WorkFilter {
        backend: Some("flowforge".to_string()),
        ..Default::default()
    };
    let items = db.list_work_items(&filter)?;

    let mut pushed = 0u32;
    for item in &items {
        let sync_result = match backend {
            "kanbus" => sync_to_kanbus(item, config),
            "beads" => sync_to_beads(item),
            "claude_tasks" => sync_to_claude_tasks(item, config),
            _ => Ok(()),
        };

        if sync_result.is_ok() {
            // Mark as pushed by updating backend to the actual backend name
            let _ = db.update_work_item_backend(&item.id, backend);
            pushed += 1;
        }
    }

    Ok(pushed)
}
