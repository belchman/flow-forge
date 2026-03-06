use serde_json::{json, Value};

use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

pub fn list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let gate_name = p.opt_str("gate_name");
    let trigger = p.str_or("trigger", "");

    let strategies = if let Some(gate) = gate_name {
        if !trigger.is_empty() {
            db.get_recovery_strategies(gate, trigger)?
        } else {
            db.list_recovery_strategies(Some(gate))?
        }
    } else {
        db.list_recovery_strategies(None)?
    };

    let entries: Vec<Value> = strategies
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "gate_name": s.gate_name,
                "trigger_pattern": s.trigger_pattern,
                "suggestion": s.suggestion,
                "alternative_command": s.alternative_command,
                "success_count": s.success_count,
                "failure_count": s.failure_count,
                "confidence": (s.confidence() * 100.0).round() / 100.0,
            })
        })
        .collect();

    Ok(json!({
        "status": "ok",
        "count": entries.len(),
        "strategies": entries,
    }))
}
