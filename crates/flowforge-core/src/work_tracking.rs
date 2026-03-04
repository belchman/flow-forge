//! Work tracking abstraction and backend implementations.
//! Supports Claude Tasks, Beads, Kanbus, and FlowForge's internal SQLite.

use std::path::{Path, PathBuf};

use tracing::warn;

use crate::config::WorkTrackingConfig;
use crate::types::{WorkEvent, WorkFilter, WorkItem};
use crate::Result;

// ── WorkBackend trait ──

/// Internal trait for external work-tracking backends (kanbus, beads).
/// Claude Tasks is NOT a backend — it's an unconditional dual-write side-effect.
trait WorkBackend {
    /// Create an item in the external backend, returning its external ID if available.
    fn create(&self, item: &WorkItem) -> Result<Option<String>>;
    /// Update an item's status in the external backend.
    fn update_status(&self, external_id: &str, status: &str) -> Result<()>;
    /// Pull items from the external backend into FlowForge SQLite. Returns count synced.
    fn sync_inbound(&self, db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32>;
}

// ── KanbusBackend ──

struct KanbusBackend {
    root: PathBuf,
}

impl KanbusBackend {
    fn new(config: &WorkTrackingConfig) -> Self {
        let root = config
            .kanbus
            .root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self { root }
    }
}

impl WorkBackend for KanbusBackend {
    fn create(&self, item: &WorkItem) -> Result<Option<String>> {
        let request = kanbus::issue_creation::IssueCreationRequest {
            root: self.root.clone(),
            title: item.title.clone(),
            issue_type: Some(item.item_type.clone()),
            priority: Some(item.priority.clamp(1, 4) as u8),
            assignee: item.assignee.clone(),
            parent: item.parent_id.clone(),
            labels: item.labels.clone(),
            description: item.description.clone(),
            local: false,
            validate: false,
        };

        match kanbus::issue_creation::create_issue(&request) {
            Ok(result) => Ok(Some(result.issue.identifier)),
            Err(e) => {
                warn!("kanbus create failed: {e}");
                Ok(None)
            }
        }
    }

    fn update_status(&self, external_id: &str, status: &str) -> Result<()> {
        if status == "completed" {
            if let Err(e) = kanbus::issue_close::close_issue(&self.root, external_id) {
                warn!("kanbus close failed: {e}");
            }
            return Ok(());
        }

        let kanbus_status = match status {
            "pending" => "open",
            "in_progress" => "in_progress",
            "blocked" => "blocked",
            other => other,
        };

        if let Err(e) = kanbus::issue_update::update_issue(
            &self.root,
            external_id,
            None,                // title
            None,                // description
            Some(kanbus_status), // status
            None,                // assignee
            None,                // priority
            false,               // claim
            false,               // validate
            &[],                 // add_labels
            &[],                 // remove_labels
            None,                // set_labels
            None,                // parent
        ) {
            warn!("kanbus status update failed: {e}");
        }
        Ok(())
    }

    fn sync_inbound(&self, db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
        let issues = match kanbus::issue_listing::list_issues(
            &self.root,
            None,  // status (all)
            None,  // issue_type
            None,  // assignee
            None,  // label
            None,  // sort
            None,  // search
            &[],   // project_filter
            false, // include_local
            false, // local_only
        ) {
            Ok(issues) => issues,
            Err(e) => {
                warn!("kanbus list failed: {e}");
                return Ok(0);
            }
        };

        let mut synced = 0u32;
        let now = chrono::Utc::now();

        for issue in &issues {
            let ext_id = &issue.identifier;
            if db.get_work_item_by_external_id(ext_id)?.is_some() {
                continue;
            }

            let status = match issue.status.as_str() {
                "closed" => "completed",
                "open" | "backlog" => "pending",
                "in_progress" => "in_progress",
                "blocked" => "blocked",
                other => other,
            };

            let priority = issue.priority.clamp(1, 4);

            let work_item = WorkItem {
                id: uuid::Uuid::new_v4().to_string(),
                external_id: Some(ext_id.to_string()),
                backend: "kanbus".to_string(),
                item_type: issue.issue_type.clone(),
                title: issue.title.clone(),
                description: if issue.description.is_empty() {
                    None
                } else {
                    Some(issue.description.clone())
                },
                status: status.to_string(),
                assignee: issue.assignee.clone(),
                parent_id: issue.parent.clone(),
                priority,
                labels: issue.labels.clone(),
                created_at: now,
                updated_at: now,
                completed_at: if status == "completed" {
                    Some(now)
                } else {
                    None
                },
                session_id: None,
                metadata: None,
                claimed_by: None,
                claimed_at: None,
                last_heartbeat: None,
                progress: 0,
                stealable: false,
            };

            db.create_work_item(&work_item)?;
            let _ = sync_to_claude_tasks(&work_item, config);
            synced += 1;
        }

        Ok(synced)
    }
}

// ── BeadsBackend ──

struct BeadsBackend;

impl WorkBackend for BeadsBackend {
    fn create(&self, item: &WorkItem) -> Result<Option<String>> {
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
        Ok(None)
    }

    fn update_status(&self, external_id: &str, status: &str) -> Result<()> {
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

    fn sync_inbound(&self, db: &dyn WorkDb, _config: &WorkTrackingConfig) -> Result<u32> {
        let beads_file = Path::new(".beads/issues.jsonl");
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
}

// ── Backend resolution ──

/// Resolve the active backend name and trait object.
/// Returns ("backend_name", Some(impl)) for kanbus/beads, or ("name", None) for others.
fn resolve_backend(config: &WorkTrackingConfig) -> (&str, Option<Box<dyn WorkBackend>>) {
    let name = detect_backend(config);
    match name {
        "kanbus" => (name, Some(Box::new(KanbusBackend::new(config)))),
        "beads" => (name, Some(Box::new(BeadsBackend))),
        other => (other, None),
    }
}

// ── Public API ──

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
    let (backend_name, backend) = resolve_backend(config);
    if let Some(b) = backend {
        let ext_id = b.create(item)?;
        if let Some(ref eid) = ext_id {
            let _ = db.update_work_item_external_id(&item.id, eid);
        }
        // Dual-write to Claude Tasks for visibility
        sync_to_claude_tasks(item, config)?;
    } else if backend_name == "claude_tasks" {
        sync_to_claude_tasks(item, config)?;
    }

    Ok(())
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
    let (backend_name, backend) = resolve_backend(config);
    if let Some(item) = &old_item {
        if let Some(b) = backend {
            if let Some(ref ext_id) = item.external_id {
                b.update_status(ext_id, new_status)?;
            }
            // Dual-write to Claude Tasks
            sync_status_to_claude_tasks(&item.id, new_status, config)?;
        } else if backend_name == "claude_tasks" {
            sync_status_to_claude_tasks(&item.id, new_status, config)?;
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

/// Push FlowForge-only items to the active external backend.
/// Items with backend="flowforge" get synced outward on session end.
/// After pushing, updates the item's backend field so it won't be pushed again.
pub fn push_to_backend(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let (backend_name, backend) = resolve_backend(config);
    if backend_name == "flowforge" {
        return Ok(0); // No external backend to push to
    }

    let filter = WorkFilter {
        backend: Some("flowforge".to_string()),
        ..Default::default()
    };
    let items = db.list_work_items(&filter)?;

    let mut pushed = 0u32;
    for item in &items {
        let ok = if let Some(ref b) = backend {
            match b.create(item) {
                Ok(ext_id) => {
                    if let Some(ref eid) = ext_id {
                        let _ = db.update_work_item_external_id(&item.id, eid);
                    }
                    let _ = sync_to_claude_tasks(item, config);
                    true
                }
                Err(_) => false,
            }
        } else if backend_name == "claude_tasks" {
            sync_to_claude_tasks(item, config).is_ok()
        } else {
            true
        };

        if ok {
            let _ = db.update_work_item_backend(&item.id, backend_name);
            pushed += 1;
        }
    }

    Ok(pushed)
}

/// Sync work items from the active external backend into the FlowForge DB.
/// Returns the number of items synced.
pub fn sync_from_backend(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let (backend_name, backend) = resolve_backend(config);
    if let Some(b) = backend {
        b.sync_inbound(db, config)
    } else if backend_name == "claude_tasks" {
        sync_from_claude_tasks(db, config)
    } else {
        Ok(0)
    }
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
    fn update_work_item_external_id(&self, id: &str, external_id: &str) -> Result<()>;
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

// ── Claude Tasks (dual-write layer, not a backend) ──

pub fn sync_to_claude_tasks(item: &WorkItem, config: &WorkTrackingConfig) -> Result<()> {
    let tasks_dir = claude_tasks_dir(config);
    if std::fs::create_dir_all(&tasks_dir).is_err() {
        return Ok(()); // Best effort
    }

    // Map blocked → pending (Claude Code doesn't have a blocked status)
    let claude_status = match item.status.as_str() {
        "blocked" => "pending",
        s => s,
    };

    let task_file = tasks_dir.join(format!("{}.json", item.id));
    let task_json = serde_json::json!({
        "id": item.id,
        "subject": item.title,
        "description": item.description,
        "status": claude_status,
        "owner": item.assignee,
        "blocks": [],
        "blockedBy": [],
        "metadata": {
            "flowforge_backend": item.backend,
            "item_type": item.item_type,
            "priority": item.priority,
            "parent_id": item.parent_id,
            "external_id": item.external_id,
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

pub fn sync_status_to_claude_tasks(
    item_id: &str,
    status: &str,
    config: &WorkTrackingConfig,
) -> Result<()> {
    // Map blocked → pending for Claude Code
    let claude_status = match status {
        "blocked" => "pending",
        s => s,
    };

    let task_file = claude_tasks_dir(config).join(format!("{}.json", item_id));
    if let Ok(content) = std::fs::read_to_string(&task_file) {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
            json["status"] = serde_json::Value::String(claude_status.to_string());
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

/// Write all non-completed work items to Claude Tasks for visibility.
/// Called on session start to pre-populate Claude Code's task view.
pub fn sync_all_to_claude_tasks(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
    let items = db.list_work_items(&WorkFilter::default())?;
    let mut synced = 0u32;
    for item in &items {
        if item.status != "completed" {
            sync_to_claude_tasks(item, config)?;
            synced += 1;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_backend_explicit() {
        let config = WorkTrackingConfig {
            backend: "kanbus".to_string(),
            ..Default::default()
        };
        assert_eq!(detect_backend(&config), "kanbus");
    }

    #[test]
    fn test_detect_backend_auto_fallback() {
        // When no external backend files exist, auto should fall back to
        // either claude_tasks or flowforge depending on environment
        let config = WorkTrackingConfig::default();
        let result = detect_backend(&config);
        assert!(!result.is_empty());
    }
}
