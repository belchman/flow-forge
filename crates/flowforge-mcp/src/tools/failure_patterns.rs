use serde_json::{json, Value};

use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

pub fn list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let mine = p.bool_or("mine", false);

    let patterns = db.list_failure_patterns()?;

    let entries: Vec<Value> = patterns
        .iter()
        .map(|fp| {
            json!({
                "id": fp.id,
                "pattern_name": fp.pattern_name,
                "description": fp.description,
                "trigger_tools": fp.trigger_tools,
                "prevention_hint": fp.prevention_hint,
                "occurrence_count": fp.occurrence_count,
                "prevented_count": fp.prevented_count,
            })
        })
        .collect();

    let mut result = json!({
        "status": "ok",
        "count": entries.len(),
        "patterns": entries,
    });

    if mine {
        let min_occ = p.u64_or("min_occurrences", 2) as u32;
        let mined = db.mine_failure_patterns(min_occ)?;
        let mined_entries: Vec<Value> = mined
            .iter()
            .map(|(seq, count)| {
                json!({
                    "tool_sequence": seq,
                    "occurrence_count": count,
                })
            })
            .collect();
        result["mined"] = json!(mined_entries);
        result["mined_count"] = json!(mined_entries.len());
    }

    Ok(result)
}
