//! Claude Tasks dual-write layer (not a backend).
//! Syncs work items to/from Claude Code's task file format for visibility.

use tracing::warn;

use crate::config::WorkTrackingConfig;
use crate::types::{WorkFilter, WorkItem, WorkStatus};
use crate::Result;

use super::WorkDb;

pub fn sync_to_claude_tasks(item: &WorkItem, config: &WorkTrackingConfig) -> Result<()> {
    let tasks_dir = claude_tasks_dir(config);
    if std::fs::create_dir_all(&tasks_dir).is_err() {
        return Ok(()); // Best effort
    }

    // Map blocked → pending (Claude Code doesn't have a blocked status)
    let claude_status = if item.status == WorkStatus::Blocked {
        "pending".to_string()
    } else {
        item.status.to_string()
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
        if item.status != WorkStatus::Completed {
            sync_to_claude_tasks(item, config)?;
            synced += 1;
        }
    }
    Ok(synced)
}

pub(crate) fn sync_from_claude_tasks(db: &dyn WorkDb, config: &WorkTrackingConfig) -> Result<u32> {
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
            status: item["status"]
                .as_str()
                .unwrap_or("pending")
                .parse()
                .unwrap_or(WorkStatus::Pending),
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
