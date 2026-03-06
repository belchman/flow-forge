use std::collections::HashMap;

use serde_json::{json, Value};

use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::FlowForgeConfig;
use flowforge_memory::MemoryDb;

use crate::params::ParamExt;

fn agents_to_json(registry: &AgentRegistry, source_filter: Option<&str>) -> Vec<Value> {
    registry
        .list()
        .iter()
        .filter(|a| {
            source_filter
                .map(|s| {
                    format!("{:?}", a.source)
                        .to_lowercase()
                        .contains(&s.to_lowercase())
                })
                .unwrap_or(true)
        })
        .map(|a| {
            json!({
                "name": a.name,
                "description": a.description,
                "capabilities": a.capabilities,
                "source": format!("{:?}", a.source),
            })
        })
        .collect()
}

/// List agents using a pre-cached AgentRegistry (no disk I/O).
pub fn list_cached(registry: Option<&AgentRegistry>, p: &Value) -> Value {
    match registry {
        Some(reg) => {
            let source_filter = p.opt_str("source");
            let agents = agents_to_json(reg, source_filter);
            json!({"status": "ok", "agents": agents})
        }
        None => json!({"status": "error", "message": "Agent registry not available"}),
    }
}

/// Route using a pre-cached AgentRegistry.
pub fn route_cached(
    db: &MemoryDb,
    config: &FlowForgeConfig,
    p: &Value,
    registry: Option<&AgentRegistry>,
) -> flowforge_core::Result<Value> {
    let task = p.require_str("task")?;
    let top_k = p.u64_or("top_k", 3) as usize;
    let reg = match registry {
        Some(r) => r,
        None => return Ok(json!({"status": "error", "message": "Agent registry not available"})),
    };
    let router = AgentRouter::new(&config.routing);
    let weights_vec = db.get_all_routing_weights()?;
    let mut learned_weights: HashMap<(String, String), f64> = HashMap::new();
    for w in &weights_vec {
        learned_weights.insert((w.task_pattern.clone(), w.agent_name.clone()), w.weight);
    }
    let agent_refs: Vec<&_> = reg.list();
    let results = router.route(task, &agent_refs, &learned_weights, None);
    let candidates: Vec<Value> = results
        .iter()
        .take(top_k)
        .map(|r| {
            json!({
                "agent_name": r.agent_name,
                "confidence": r.confidence,
                "breakdown": {
                    "pattern_score": r.breakdown.pattern_score,
                    "capability_score": r.breakdown.capability_score,
                    "learned_score": r.breakdown.learned_score,
                    "context_score": r.breakdown.context_score,
                    "priority_score": r.breakdown.priority_score,
                },
            })
        })
        .collect();
    Ok(json!({"status": "ok", "candidates": candidates}))
}

/// Get agent info using a pre-cached AgentRegistry.
pub fn info_cached(registry: Option<&AgentRegistry>, p: &Value) -> Value {
    let name = p.opt_str("name").unwrap_or("");
    match registry {
        Some(reg) => match reg.get(name) {
            Some(agent) => json!({
                "status": "ok",
                "agent": {
                    "name": agent.name,
                    "description": agent.description,
                    "capabilities": agent.capabilities,
                    "patterns": agent.patterns,
                    "priority": format!("{:?}", agent.priority),
                    "source": format!("{:?}", agent.source),
                    "body": agent.body,
                },
            }),
            None => json!({"status": "error", "message": "Agent not found"}),
        },
        None => json!({"status": "error", "message": "Agent registry not available"}),
    }
}
