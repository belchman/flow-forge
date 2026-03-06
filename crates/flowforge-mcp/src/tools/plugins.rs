use serde_json::{json, Value};

use flowforge_core::FlowForgeConfig;

use crate::params::ParamExt;

pub fn list(config: &FlowForgeConfig) -> flowforge_core::Result<Value> {
    let plugins = flowforge_core::plugin::load_all_plugins(&config.plugins)?;
    let entries: Vec<Value> = plugins
        .iter()
        .map(|p| {
            let disabled = config.plugins.disabled.contains(&p.manifest.plugin.name);
            json!({
                "name": p.manifest.plugin.name,
                "version": p.manifest.plugin.version,
                "description": p.manifest.plugin.description,
                "enabled": !disabled,
                "tools": p.manifest.tools.len(),
                "hooks": p.manifest.hooks.len(),
                "agents": p.manifest.agents.len(),
            })
        })
        .collect();
    Ok(json!({"status": "ok", "count": entries.len(), "plugins": entries}))
}

pub fn info(config: &FlowForgeConfig, p: &Value) -> flowforge_core::Result<Value> {
    let name = p.require_str("name")?;
    let plugins = flowforge_core::plugin::load_all_plugins(&config.plugins)?;
    match plugins.iter().find(|p| p.manifest.plugin.name == name) {
        Some(p) => {
            let disabled = config.plugins.disabled.contains(&p.manifest.plugin.name);
            let tools: Vec<Value> = p
                .manifest
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "timeout": t.timeout
                    })
                })
                .collect();
            let hooks: Vec<Value> = p
                .manifest
                .hooks
                .iter()
                .map(|h| {
                    json!({
                        "event": h.event,
                        "priority": h.priority
                    })
                })
                .collect();
            Ok(json!({
                "status": "ok",
                "name": name,
                "version": p.manifest.plugin.version,
                "description": p.manifest.plugin.description,
                "enabled": !disabled,
                "tools": tools,
                "hooks": hooks
            }))
        }
        None => Ok(json!({"status": "error", "message": format!("plugin '{name}' not found")})),
    }
}
