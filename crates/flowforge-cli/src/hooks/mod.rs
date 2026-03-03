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

/// Run a hook safely: catch errors AND panics, returning Ok(()) regardless.
/// Any stderr output causes Claude Code to display a hook error in the TUI,
/// so we must suppress everything. On error, emit a valid empty JSON response
/// so Claude Code doesn't treat missing stdout as a hook failure.
pub fn run_safe(
    hook_name: &str,
    f: impl FnOnce() -> flowforge_core::Result<()>,
) -> flowforge_core::Result<()> {
    let succeeded = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            log_hook_error(hook_name, &format!("Error: {e}"));
            false
        }
        Err(panic_info) => {
            let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic".to_string()
            };
            log_hook_error(hook_name, &format!("PANIC: {msg}"));
            false
        }
    };

    // If the hook failed without producing output, emit a valid empty response
    // so Claude Code doesn't report "hook error" from missing stdout.
    if !succeeded {
        println!("{{\"hookSpecificOutput\":{{}}}}");
    }

    Ok(())
}

fn log_hook_error(hook_name: &str, msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/flowforge-hook-errors.log")
    {
        let _ = writeln!(f, "[{}] {}: {}", chrono::Utc::now(), hook_name, msg);
    }
}

/// Extract a meaningful task pattern from a subject/description string.
/// Filters out stop words and takes up to 5 content words for better DB cache hits.
pub fn extract_task_pattern(text: &str) -> String {
    const STOP_WORDS: &[&str] = &[
        "the", "a", "an", "in", "on", "to", "for", "of", "with", "is", "it", "and", "or", "but",
        "this", "that", "my", "your", "its", "be", "at", "by", "from", "as", "into", "about", "up",
        "out", "if", "not", "no", "so", "do", "can", "will", "just", "should", "would", "could",
        "has", "have", "had", "was", "were", "been", "being", "am", "are",
    ];
    text.to_lowercase()
        .split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .take(5)
        .collect::<Vec<_>>()
        .join(" ")
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
