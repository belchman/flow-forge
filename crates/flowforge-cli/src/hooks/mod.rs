pub mod notification;
pub mod post_tool_use;
pub mod post_tool_use_failure;
pub mod pre_compact;
pub mod pre_tool_use;
pub mod session_end;
pub mod session_start;
pub mod stop;
pub mod subagent_start;
pub mod subagent_stop;
pub mod task_completed;
pub mod teammate_idle;
pub mod user_prompt_submit;

use std::io::Write;

/// Log a hook error to .flowforge/hook-errors.log instead of crashing.
fn log_hook_error(hook_name: &str, error: &dyn std::fmt::Display) {
    let log_path = flowforge_core::FlowForgeConfig::project_dir().join("hook-errors.log");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let _ = writeln!(file, "[{}] {}: {}", timestamp, hook_name, error);
    }
    tracing::error!("Hook {} failed: {}", hook_name, error);
}

/// Run a hook safely: catch errors, log them, and return Ok(()) regardless.
pub fn run_safe(
    hook_name: &str,
    f: impl FnOnce() -> flowforge_core::Result<()>,
) -> flowforge_core::Result<()> {
    match f() {
        Ok(()) => Ok(()),
        Err(e) => {
            log_hook_error(hook_name, &e);
            Ok(())
        }
    }
}

use flowforge_core::plugin::LoadedPlugin;
use flowforge_core::plugin_exec::exec_plugin_hook;

/// Run plugin hooks for a given event. Returns first deny/ask response if any.
pub fn run_plugin_hooks(
    event: &str,
    raw_input: &serde_json::Value,
    plugins: &[LoadedPlugin],
    _plugin_dir: &std::path::Path,
) -> Option<serde_json::Value> {
    let mut hooks: Vec<_> = plugins
        .iter()
        .flat_map(|p| {
            p.manifest.hooks.iter().filter_map(move |h| {
                if h.event.eq_ignore_ascii_case(event) {
                    Some((h.priority, &h.command, &p.dir))
                } else {
                    None
                }
            })
        })
        .collect();
    hooks.sort_by_key(|(pri, _, _)| *pri);

    for (_, command, dir) in hooks {
        if let Some(response) = exec_plugin_hook(command, dir, raw_input, 5000) {
            // Check if response indicates deny or ask
            if let Some(action) = response.get("action").and_then(|v| v.as_str()) {
                if action == "deny" || action == "ask" {
                    return Some(response);
                }
            }
        }
    }
    None
}
