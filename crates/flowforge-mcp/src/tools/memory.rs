use serde_json::{json, Value};

use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

pub fn get(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let key = p.require_str("key")?;
    let namespace = p.str_or("namespace", "default");
    let value = db.kv_get(key, namespace)?;
    Ok(json!({"status": "ok", "key": key, "value": value}))
}

pub fn set(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let key = p.require_str("key")?;
    let value = p.require_str("value")?;
    let namespace = p
        .opt_str("namespace")
        .or_else(|| p.opt_str("category"))
        .unwrap_or("default");
    db.kv_set(key, value, namespace)?;
    Ok(json!({"status": "ok", "key": key, "stored": true}))
}

pub fn search(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let query = p.require_str("query")?;
    let limit = p.u64_or("limit", 10) as usize;
    let results = db.kv_search(query, limit)?;
    let entries: Vec<Value> = results
        .iter()
        .map(|(k, v, ns)| json!({"key": k, "value": v, "namespace": ns}))
        .collect();
    Ok(json!({"status": "ok", "query": query, "results": entries}))
}

pub fn delete(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let key = p.require_str("key")?;
    let namespace = p.str_or("namespace", "default");
    db.kv_delete(key, namespace)?;
    Ok(json!({"status": "ok", "key": key, "deleted": true}))
}

pub fn list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let namespace = p.str_or("category", "default");
    let limit = p.u64_or("limit", 50) as usize;
    let entries = db.kv_list_limited(namespace, limit)?;
    let entries: Vec<Value> = entries
        .iter()
        .map(|(k, v)| json!({"key": k, "value": v}))
        .collect();
    Ok(json!({"status": "ok", "entries": entries}))
}

pub fn import(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let entries = match p.get("entries").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => {
            return Ok(json!({"status": "error", "message": "missing entries array"}));
        }
    };
    let total = entries.len();
    let mut imported = 0usize;
    for entry in &entries {
        let key = entry.str_or("key", "");
        let value = entry.str_or("value", "");
        let namespace = entry
            .opt_str("namespace")
            .or_else(|| entry.opt_str("category"))
            .unwrap_or("default");
        if db.kv_set(key, value, namespace).is_ok() {
            imported += 1;
        }
    }
    Ok(json!({"status": "ok", "imported": imported, "total": total}))
}
