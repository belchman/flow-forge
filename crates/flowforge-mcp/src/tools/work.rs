use std::collections::HashMap;

use serde_json::{json, Value};

use flowforge_core::{FlowForgeConfig, WorkStatus};
use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

pub fn create(db: &MemoryDb, config: &FlowForgeConfig, p: &Value) -> flowforge_core::Result<Value> {
    let title = p.require_str("title")?;
    let item_type = p.str_or("type", "task");
    let description = p.opt_str("description");
    let parent_id = p.opt_str("parent_id");
    let priority = (p.i64_or("priority", 2) as i32).clamp(0, 4);
    let now = chrono::Utc::now();
    let backend = flowforge_core::work_tracking::detect_backend(&config.work_tracking).to_string();
    let item = flowforge_core::WorkItem {
        id: uuid::Uuid::new_v4().to_string(),
        external_id: None,
        backend,
        item_type: item_type.to_string(),
        title: title.to_string(),
        description: description.map(|s| s.to_string()),
        status: WorkStatus::Pending,
        assignee: None,
        parent_id: parent_id.map(|s| s.to_string()),
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
    flowforge_core::work_tracking::create_item(db, &config.work_tracking, &item)?;
    Ok(json!({"status": "ok", "id": item.id, "title": title}))
}

pub fn list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let status: Option<WorkStatus> = p.opt_str("status").and_then(|s| s.parse().ok());
    let item_type = p.opt_str("type");
    let limit = p.u64_or("limit", 20) as usize;
    let filter = flowforge_core::WorkFilter {
        status,
        item_type: item_type.map(|s| s.to_string()),
        limit: Some(limit),
        ..Default::default()
    };
    let items = db.list_work_items(&filter)?;
    let entries: Vec<Value> = items
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "title": i.title,
                "type": i.item_type,
                "status": i.status,
                "assignee": i.assignee,
                "priority": i.priority,
                "backend": i.backend,
                "created_at": i.created_at.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "items": entries, "count": entries.len()}))
}

pub fn update(db: &MemoryDb, config: &FlowForgeConfig, p: &Value) -> flowforge_core::Result<Value> {
    let id = p.require_str("id")?;
    let new_status_str = p.require_str("status")?;
    let new_status: WorkStatus = new_status_str
        .parse()
        .map_err(|e: String| flowforge_core::Error::Config(e))?;
    flowforge_core::work_tracking::update_status(db, &config.work_tracking, id, new_status, "mcp")?;
    Ok(json!({"status": "ok", "id": id, "new_status": new_status_str}))
}

pub fn log(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let work_item_id = p.opt_str("work_item_id");
    let limit = p.u64_or("limit", 20) as usize;
    let events = if let Some(id) = work_item_id {
        db.get_work_events(id, limit)?
    } else {
        db.get_recent_work_events(limit)?
    };
    let entries: Vec<Value> = events
        .iter()
        .map(|e| {
            json!({
                "work_item_id": e.work_item_id,
                "event_type": e.event_type,
                "old_value": e.old_value,
                "new_value": e.new_value,
                "actor": e.actor,
                "timestamp": e.timestamp.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "events": entries}))
}

pub fn close(db: &MemoryDb, config: &FlowForgeConfig, p: &Value) -> flowforge_core::Result<Value> {
    let id = p.require_str("id")?;
    flowforge_core::work_tracking::close_item(db, &config.work_tracking, id, "mcp")?;
    Ok(json!({"status": "ok", "id": id}))
}

pub fn sync(db: &MemoryDb, config: &FlowForgeConfig) -> flowforge_core::Result<Value> {
    let pulled = flowforge_core::work_tracking::sync_from_backend(db, &config.work_tracking)?;
    let pushed = flowforge_core::work_tracking::push_to_backend(db, &config.work_tracking)?;
    let backend = flowforge_core::work_tracking::detect_backend(&config.work_tracking).to_string();
    Ok(json!({
        "status": "ok",
        "pulled": pulled,
        "pushed": pushed,
        "backend": backend
    }))
}

pub fn load(db: &MemoryDb) -> flowforge_core::Result<Value> {
    let filter = flowforge_core::WorkFilter {
        status: Some(WorkStatus::InProgress),
        limit: Some(1000),
        ..Default::default()
    };
    let items = db.list_work_items(&filter)?;
    let mut by_agent: HashMap<String, Vec<Value>> = HashMap::new();
    for item in &items {
        let agent = item
            .assignee
            .clone()
            .or_else(|| item.claimed_by.clone())
            .unwrap_or_else(|| "unassigned".to_string());
        by_agent.entry(agent).or_default().push(json!({
            "id": item.id,
            "title": item.title,
            "type": item.item_type,
            "priority": item.priority,
            "progress": item.progress,
        }));
    }
    let agents: Vec<Value> = by_agent
        .into_iter()
        .map(|(name, items)| json!({"name": name, "items": items}))
        .collect();
    let total = items.len();
    Ok(json!({"status": "ok", "agents": agents, "total": total}))
}

pub fn status(db: &MemoryDb) -> flowforge_core::Result<Value> {
    let pending = db.count_work_items_by_status(WorkStatus::Pending).unwrap_or(0);
    let in_progress = db.count_work_items_by_status(WorkStatus::InProgress).unwrap_or(0);
    let completed = db.count_work_items_by_status(WorkStatus::Completed).unwrap_or(0);
    let blocked = db.count_work_items_by_status(WorkStatus::Blocked).unwrap_or(0);
    let total = pending + in_progress + completed + blocked;
    Ok(json!({
        "status": "ok",
        "pending": pending,
        "in_progress": in_progress,
        "completed": completed,
        "blocked": blocked,
        "total": total
    }))
}
