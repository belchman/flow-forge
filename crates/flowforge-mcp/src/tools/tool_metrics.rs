use serde_json::{json, Value};

use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

/// List tool success/failure metrics, optionally filtered by agent.
pub fn list(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let agent_name = p.opt_str("agent_name");
    let metrics = db.list_tool_metrics(agent_name)?;
    let items: Vec<Value> = metrics
        .iter()
        .map(|m| {
            json!({
                "tool_name": m.tool_name,
                "agent_name": m.agent_name,
                "success_count": m.success_count,
                "failure_count": m.failure_count,
                "success_rate": format!("{:.1}%", m.success_rate() * 100.0),
                "avg_duration_ms": m.avg_duration_ms(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "metrics": items, "count": items.len()}))
}

/// Get the best agents for a specific tool based on success rate.
pub fn best(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let tool_name = p.require_str("tool_name")?;
    let limit = p.u64_or("limit", 5) as usize;
    let best = db.get_best_agents_for_tool(tool_name, limit)?;
    let items: Vec<Value> = best
        .iter()
        .map(|(name, rate, total)| {
            json!({
                "agent_name": name,
                "success_rate": format!("{:.1}%", rate * 100.0),
                "total_uses": total,
            })
        })
        .collect();
    Ok(json!({"status": "ok", "tool_name": tool_name, "best_agents": items}))
}

/// Get session cost metrics (tool calls, bytes, errors) for current or specified session.
pub fn session_cost(db: &MemoryDb, p: &Value) -> flowforge_core::Result<Value> {
    let session_id = p.opt_str("session_id");
    let sid = match session_id {
        Some(s) => s.to_string(),
        None => db
            .get_current_session()?
            .map(|s| s.id)
            .ok_or_else(|| flowforge_core::Error::NotFound("No active session".into()))?,
    };
    let metrics = db.get_session_metrics(&sid)?;
    Ok(json!({"status": "ok", "session_id": sid, "metrics": metrics}))
}
