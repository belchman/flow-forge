use serde_json::{json, Value};

use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

pub fn history(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let limit = p.u64_or("limit", 20) as usize;
    let offset = p.u64_or("offset", 0) as usize;
    let total = db.get_conversation_message_count(session_id).unwrap_or(0);
    let msgs = db.get_conversation_messages(session_id, limit, offset)?;
    let entries: Vec<Value> = msgs
        .iter()
        .map(|m| {
            json!({
                "message_index": m.message_index,
                "role": m.role,
                "message_type": m.message_type,
                "content": m.content,
                "model": m.model,
                "timestamp": m.timestamp.to_rfc3339(),
                "source": m.source,
            })
        })
        .collect();
    Ok(json!({"status": "ok", "messages": entries, "total": total}))
}

pub fn search(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let query = p.require_str("query")?;
    let limit = p.u64_or("limit", 10) as usize;
    let msgs = db.search_conversation_messages(session_id, query, limit)?;
    let entries: Vec<Value> = msgs
        .iter()
        .map(|m| {
            json!({
                "message_index": m.message_index,
                "role": m.role,
                "content": m.content,
                "timestamp": m.timestamp.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "results": entries, "count": entries.len()}))
}

pub fn ingest(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let path = p.require_str("transcript_path")?;
    let count = db.ingest_transcript(session_id, path)?;
    Ok(json!({"status": "ok", "ingested": count, "session_id": session_id}))
}

pub fn checkpoint_create(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let name = p.require_str("name")?;
    let description = p.opt_str("description");
    let message_index = db.get_latest_message_index(session_id).unwrap_or(0);
    let cp = flowforge_core::Checkpoint {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        name: name.to_string(),
        message_index,
        description: description.map(|s| s.to_string()),
        git_ref: None,
        created_at: chrono::Utc::now(),
        metadata: None,
    };
    db.create_checkpoint(&cp)?;
    Ok(json!({"status": "ok", "id": cp.id, "name": name, "message_index": message_index}))
}

pub fn checkpoint_list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let cps = db.list_checkpoints(session_id)?;
    let entries: Vec<Value> = cps
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.name,
                "message_index": c.message_index,
                "description": c.description,
                "git_ref": c.git_ref,
                "created_at": c.created_at.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "checkpoints": entries}))
}

pub fn checkpoint_get(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let id = p.opt_str("id");
    let session_id = p.opt_str("session_id");
    let name = p.opt_str("name");
    let cp = if let Some(id) = id {
        db.get_checkpoint(id)?
    } else if let (Some(sid), Some(n)) = (session_id, name) {
        db.get_checkpoint_by_name(sid, n)?
    } else {
        return Ok(json!({"status": "error", "message": "Provide either id or session_id+name"}));
    };
    match cp {
        Some(c) => Ok(json!({
            "status": "ok",
            "checkpoint": {
                "id": c.id,
                "session_id": c.session_id,
                "name": c.name,
                "message_index": c.message_index,
                "description": c.description,
                "git_ref": c.git_ref,
                "created_at": c.created_at.to_rfc3339(),
            }
        })),
        None => Ok(json!({"status": "error", "message": "Checkpoint not found"})),
    }
}

pub fn session_fork(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let checkpoint_name = p.opt_str("checkpoint_name");
    let at_index = p.opt_u32("at_index");
    let reason = p.opt_str("reason");
    let (fork_index, checkpoint_id) = if let Some(cp_name) = checkpoint_name {
        match db.get_checkpoint_by_name(session_id, cp_name)? {
            Some(cp) => (cp.message_index, Some(cp.id)),
            None => {
                return Ok(
                    json!({"status": "error", "message": format!("Checkpoint '{}' not found", cp_name)}),
                )
            }
        }
    } else if let Some(idx) = at_index {
        (idx, None)
    } else {
        let latest = db.get_latest_message_index(session_id).unwrap_or(0);
        (latest.saturating_sub(1), None)
    };

    let new_session_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let new_session = flowforge_core::SessionInfo {
        id: new_session_id.clone(),
        started_at: now,
        ended_at: None,
        cwd: ".".to_string(),
        edits: 0,
        commands: 0,
        summary: Some(format!("Forked from {}", session_id)),
        transcript_path: None,
    };
    db.create_session(&new_session)?;

    let copied = db.fork_conversation(session_id, &new_session_id, fork_index)?;

    let fork = flowforge_core::SessionFork {
        id: uuid::Uuid::new_v4().to_string(),
        source_session_id: session_id.to_string(),
        target_session_id: new_session_id.clone(),
        fork_message_index: fork_index,
        checkpoint_id,
        reason: reason.map(|s| s.to_string()),
        created_at: now,
    };
    db.create_session_fork(&fork)?;

    Ok(json!({
        "status": "ok",
        "fork_id": fork.id,
        "new_session_id": new_session_id,
        "fork_message_index": fork_index,
        "messages_copied": copied,
    }))
}

pub fn session_forks(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let forks = db.get_session_forks(session_id)?;
    let entries: Vec<Value> = forks
        .iter()
        .map(|f| {
            json!({
                "id": f.id,
                "source_session_id": f.source_session_id,
                "target_session_id": f.target_session_id,
                "fork_message_index": f.fork_message_index,
                "checkpoint_id": f.checkpoint_id,
                "reason": f.reason,
                "created_at": f.created_at.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "forks": entries}))
}

pub fn session_lineage(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.require_str("session_id")?;
    let lineage = db.get_session_lineage(session_id)?;
    let entries: Vec<Value> = lineage
        .iter()
        .map(|f| {
            json!({
                "source_session_id": f.source_session_id,
                "target_session_id": f.target_session_id,
                "fork_message_index": f.fork_message_index,
                "created_at": f.created_at.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "lineage": entries, "depth": entries.len()}))
}
