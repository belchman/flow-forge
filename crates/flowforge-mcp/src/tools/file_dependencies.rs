use serde_json::{json, Value};

use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

pub fn list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let file = p.opt_str("file");
    let limit = p.u64_or("limit", 20) as usize;

    if let Some(file_path) = file {
        let deps = db.get_related_files(file_path, limit)?;
        let entries: Vec<Value> = deps
            .iter()
            .map(|d| {
                let other = if d.file_a == file_path {
                    &d.file_b
                } else {
                    &d.file_a
                };
                json!({
                    "file": other,
                    "co_edit_count": d.co_edit_count,
                    "last_seen": d.last_seen,
                })
            })
            .collect();

        Ok(json!({
            "status": "ok",
            "file": file_path,
            "count": entries.len(),
            "dependencies": entries,
        }))
    } else {
        let deps = db.get_dependency_graph(1, limit)?;
        let entries: Vec<Value> = deps
            .iter()
            .map(|d| {
                json!({
                    "file_a": d.file_a,
                    "file_b": d.file_b,
                    "co_edit_count": d.co_edit_count,
                    "last_seen": d.last_seen,
                })
            })
            .collect();

        Ok(json!({
            "status": "ok",
            "count": entries.len(),
            "edges": entries,
        }))
    }
}
