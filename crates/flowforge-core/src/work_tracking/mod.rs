//! Work tracking abstraction and backend implementations.
//! Supports Claude Tasks, Beads, Kanbus, and FlowForge's internal SQLite.

mod backends;
pub mod claude_tasks;
mod stealing;

use crate::config::WorkTrackingConfig;
use crate::types::{WorkEvent, WorkFilter, WorkItem, WorkStatus};
use crate::Result;

use backends::resolve_backend;
use claude_tasks::{sync_from_claude_tasks, sync_status_to_claude_tasks, sync_to_claude_tasks};

// Re-export public items
pub use backends::detect_backend;
pub use claude_tasks::sync_all_to_claude_tasks;
pub use stealing::{claim_item, detect_stale, list_stealable, release_item, steal_item};

// ── Database trait to decouple from MemoryDb ──

/// Trait for work tracking CRUD operations.
/// Implemented by MemoryDb so we can use it from both CLI and MCP.
pub trait WorkDb {
    fn create_work_item(&self, item: &WorkItem) -> Result<()>;
    fn get_work_item(&self, id: &str) -> Result<Option<WorkItem>>;
    fn get_work_item_by_external_id(&self, external_id: &str) -> Result<Option<WorkItem>>;
    fn update_work_item_status(&self, id: &str, status: WorkStatus) -> Result<()>;
    fn update_work_item_assignee(&self, id: &str, assignee: &str) -> Result<()>;
    fn list_work_items(&self, filter: &WorkFilter) -> Result<Vec<WorkItem>>;
    fn update_work_item_backend(&self, id: &str, backend: &str) -> Result<()>;
    fn update_work_item_external_id(&self, id: &str, external_id: &str) -> Result<()>;
    fn delete_work_item(&self, id: &str) -> Result<()>;
    fn count_work_items_by_status(&self, status: WorkStatus) -> Result<u64>;
    fn record_work_event(&self, event: &WorkEvent) -> Result<i64>;
    fn get_work_events(&self, work_item_id: &str, limit: usize) -> Result<Vec<WorkEvent>>;
    fn get_recent_work_events(&self, limit: usize) -> Result<Vec<WorkEvent>>;
}

/// Trait for work-stealing operations. Extends WorkDb with claim/steal lifecycle.
pub trait WorkStealing: WorkDb {
    fn claim_work_item(&self, id: &str, session_id: &str) -> Result<bool>;
    fn release_work_item(&self, id: &str) -> Result<()>;
    fn update_heartbeat(&self, session_id: &str) -> Result<u64>;
    fn update_progress(&self, id: &str, progress: i32) -> Result<()>;
    fn mark_stale_items_stealable(&self, stale_mins: u64, min_progress: i32) -> Result<u64>;
    fn auto_release_abandoned(&self, abandon_mins: u64) -> Result<u64>;
    fn get_stealable_items(&self, limit: usize) -> Result<Vec<WorkItem>>;
    fn steal_work_item(&self, id: &str, new_session_id: &str) -> Result<bool>;
}

/// Validate a work item status transition.
/// Valid transitions: pending→in_progress, in_progress→completed,
/// in_progress→pending, completed→pending.
/// Returns Ok(()) for valid transitions, Err for invalid ones.
pub fn validate_status_transition(old_status: WorkStatus, new_status: WorkStatus) -> Result<()> {
    use WorkStatus::*;
    let valid = matches!(
        (old_status, new_status),
        (Pending, InProgress)
            | (InProgress, Completed)
            | (InProgress, Pending)
            | (Completed, Pending)
            | (Pending, Blocked)
            | (Blocked, Pending)
            | (Blocked, InProgress)
            | (InProgress, Blocked)
    );
    if valid || old_status == new_status {
        Ok(())
    } else {
        Err(crate::Error::InvalidInput(format!(
            "invalid status transition: {old_status} → {new_status}"
        )))
    }
}

// ── Public API ──

/// Create a work item and log to the appropriate backend.
pub fn create_item(db: &dyn WorkDb, config: &WorkTrackingConfig, item: &WorkItem) -> Result<()> {
    // Always log to FlowForge SQLite
    db.create_work_item(item)?;

    // Log creation event
    let event = WorkEvent {
        id: 0,
        work_item_id: item.id.clone(),
        event_type: "created".into(),
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
            db.update_work_item_external_id(&item.id, eid)?;
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
    new_status: WorkStatus,
    actor: &str,
) -> Result<()> {
    let old_item = db.get_work_item(id)?;
    let old_status = old_item
        .as_ref()
        .map(|i| i.status)
        .unwrap_or_default();

    validate_status_transition(old_status, new_status)?;

    db.update_work_item_status(id, new_status)?;

    let status_str = new_status.to_string();
    let event = WorkEvent {
        id: 0,
        work_item_id: id.to_string(),
        event_type: "status_changed".to_string(),
        old_value: Some(old_status.to_string()),
        new_value: Some(status_str.clone()),
        actor: Some(actor.to_string()),
        timestamp: chrono::Utc::now(),
    };
    db.record_work_event(&event)?;

    // Sync to external backend with full field update + comment
    let (backend_name, backend) = resolve_backend(config);
    if let Some(b) = backend {
        // Re-read the item after status update for accurate field sync
        if let Some(updated_item) = db.get_work_item(id)? {
            if let Some(ref ext_id) = updated_item.external_id {
                b.update_item(ext_id, &updated_item)?;
                // Add a comment recording the status change
                let comment = format!(
                    "{} → {} (by {})",
                    old_item.as_ref().map(|i| i.status.to_string()).unwrap_or_else(|| "?".to_string()),
                    status_str,
                    actor
                );
                if let Err(e) = b.add_comment(ext_id, "FlowForge", &comment) {
                    eprintln!("[FlowForge] warning: failed to add backend comment: {e}");
                }
            }
        }
        // Dual-write to Claude Tasks
        sync_status_to_claude_tasks(id, &status_str, config)?;
    } else if backend_name == "claude_tasks" {
        sync_status_to_claude_tasks(id, &status_str, config)?;
    }

    Ok(())
}

/// Add a comment to a work item (syncs to external backend).
pub fn add_comment(
    db: &dyn WorkDb,
    config: &WorkTrackingConfig,
    id: &str,
    author: &str,
    text: &str,
) -> Result<()> {
    let item = db
        .get_work_item(id)?
        .ok_or_else(|| crate::Error::NotFound(format!("Work item '{id}' not found")))?;

    // Record as work event
    let event = WorkEvent {
        id: 0,
        work_item_id: id.to_string(),
        event_type: "comment".to_string(),
        old_value: None,
        new_value: Some(text.to_string()),
        actor: Some(author.to_string()),
        timestamp: chrono::Utc::now(),
    };
    db.record_work_event(&event)?;

    // Sync to external backend
    let (_backend_name, backend) = resolve_backend(config);
    if let Some(b) = backend {
        if let Some(ref ext_id) = item.external_id {
            if let Err(e) = b.add_comment(ext_id, author, text) {
                eprintln!("[FlowForge] warning: failed to add backend comment: {e}");
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
    update_status(db, config, id, WorkStatus::Completed, actor)
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

/// Get or create a work item from a Claude task.
/// Deduplicates by checking `external_id` (Claude task ID) first, then title match.
/// Returns the work item ID.
pub fn get_or_create_from_claude_task(
    db: &dyn WorkDb,
    config: &WorkTrackingConfig,
    claude_task_id: Option<&str>,
    subject: &str,
    description: Option<&str>,
) -> Result<String> {
    // 1. Check by external_id (Claude task ID)
    if let Some(ext_id) = claude_task_id {
        if let Some(existing) = db.get_work_item_by_external_id(ext_id)? {
            return Ok(existing.id);
        }
    }

    // 2. Check by title match
    let filter = WorkFilter {
        status: None,
        ..Default::default()
    };
    let items = db.list_work_items(&filter)?;
    if let Some(existing) = items
        .iter()
        .find(|i| i.title == subject && i.status != WorkStatus::Completed)
    {
        // Link the external_id if we have one and it's not set
        if let (Some(ext_id), None) = (claude_task_id, &existing.external_id) {
            db.update_work_item_external_id(&existing.id, ext_id)?;
        }
        return Ok(existing.id.clone());
    }

    // 3. Create new work item
    let now = chrono::Utc::now();
    let item = WorkItem {
        id: uuid::Uuid::new_v4().to_string(),
        external_id: claude_task_id.map(String::from),
        backend: "claude_tasks".to_string(),
        item_type: "task".to_string(),
        title: subject.to_string(),
        description: description.map(String::from),
        status: WorkStatus::Pending,
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

    create_item(db, config, &item)?;
    Ok(item.id)
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
                        db.update_work_item_external_id(&item.id, eid)?;
                    }
                    if let Err(e) = sync_to_claude_tasks(item, config) {
                        eprintln!("[FlowForge] warning: Claude Tasks dual-write failed: {e}");
                    }
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
            db.update_work_item_backend(&item.id, backend_name)?;
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

    #[test]
    fn test_detect_backend_beads() {
        let config = WorkTrackingConfig {
            backend: "beads".to_string(),
            ..Default::default()
        };
        assert_eq!(detect_backend(&config), "beads");
    }

    #[test]
    fn test_detect_backend_flowforge() {
        let config = WorkTrackingConfig {
            backend: "flowforge".to_string(),
            ..Default::default()
        };
        assert_eq!(detect_backend(&config), "flowforge");
    }

    #[test]
    fn test_detect_backend_claude_tasks() {
        let config = WorkTrackingConfig {
            backend: "claude_tasks".to_string(),
            ..Default::default()
        };
        assert_eq!(detect_backend(&config), "claude_tasks");
    }

    #[test]
    fn test_resolve_backend_kanbus() {
        let config = WorkTrackingConfig {
            backend: "kanbus".to_string(),
            ..Default::default()
        };
        let (name, backend) = resolve_backend(&config);
        assert_eq!(name, "kanbus");
        assert!(backend.is_some());
    }

    #[test]
    fn test_resolve_backend_beads() {
        let config = WorkTrackingConfig {
            backend: "beads".to_string(),
            ..Default::default()
        };
        let (name, backend) = resolve_backend(&config);
        assert_eq!(name, "beads");
        assert!(backend.is_some());
    }

    #[test]
    fn test_valid_status_transitions() {
        use crate::types::WorkStatus::*;
        assert!(validate_status_transition(Pending, InProgress).is_ok());
        assert!(validate_status_transition(InProgress, Completed).is_ok());
        assert!(validate_status_transition(InProgress, Pending).is_ok());
        assert!(validate_status_transition(Completed, Pending).is_ok());
        assert!(validate_status_transition(Pending, Blocked).is_ok());
        assert!(validate_status_transition(Blocked, InProgress).is_ok());
        // Same status is a no-op, should succeed
        assert!(validate_status_transition(Pending, Pending).is_ok());
    }

    #[test]
    fn test_invalid_status_transitions() {
        use crate::types::WorkStatus::*;
        assert!(validate_status_transition(Completed, InProgress).is_err());
        assert!(validate_status_transition(Pending, Completed).is_err());
    }

    #[test]
    fn test_resolve_backend_flowforge_returns_none() {
        let config = WorkTrackingConfig {
            backend: "flowforge".to_string(),
            ..Default::default()
        };
        let (name, backend) = resolve_backend(&config);
        assert_eq!(name, "flowforge");
        assert!(backend.is_none());
    }
}
