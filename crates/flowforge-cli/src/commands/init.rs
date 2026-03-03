use colored::Colorize;
use flowforge_core::{FlowForgeConfig, Result};
use std::path::Path;

pub fn run(project: bool, global: bool) -> Result<()> {
    if !project && !global {
        // Default to project init
        return init_project();
    }
    if project {
        init_project()?;
    }
    if global {
        init_global()?;
    }
    Ok(())
}

fn init_project() -> Result<()> {
    let project_dir = FlowForgeConfig::project_dir();
    std::fs::create_dir_all(&project_dir)?;
    std::fs::create_dir_all(project_dir.join("agents"))?;
    std::fs::create_dir_all(".flowforge/plugins")?;

    // Create default config
    let config = FlowForgeConfig::default();
    config.save(&FlowForgeConfig::config_path())?;
    println!(
        "{} Created {}",
        "✓".green(),
        FlowForgeConfig::config_path().display()
    );

    // Create database
    let db_path = config.db_path();
    let _db = flowforge_memory::MemoryDb::open(&db_path)?;
    println!("{} Created {}", "✓".green(), db_path.display());

    // Create/update .claude/settings.json with hooks (A7: merge, don't overwrite)
    write_settings_json()?;
    println!(
        "{} Updated .claude/settings.json with FlowForge hooks",
        "✓".green()
    );

    // Create .mcp.json for auto-registration (B3)
    write_mcp_json()?;
    println!(
        "{} Created .mcp.json for MCP auto-registration",
        "✓".green()
    );

    // Create CLAUDE.md additions
    write_claude_md()?;
    println!(
        "{} Created/updated CLAUDE.md with FlowForge instructions",
        "✓".green()
    );

    println!("\n{}", "FlowForge initialized!".green().bold());
    println!("Start a new Claude Code session to activate hooks.");

    Ok(())
}

fn init_global() -> Result<()> {
    let global_dir = FlowForgeConfig::global_dir();
    std::fs::create_dir_all(&global_dir)?;
    std::fs::create_dir_all(global_dir.join("agents"))?;

    let config_path = global_dir.join("config.toml");
    if !config_path.exists() {
        let config = FlowForgeConfig::default();
        config.save(&config_path)?;
    }

    println!(
        "{} Global FlowForge initialized at {}",
        "✓".green(),
        global_dir.display()
    );
    Ok(())
}

/// Resolve the absolute path to the flowforge binary.
/// Prefers the currently running executable, falls back to ~/.cargo/bin/flowforge.
fn flowforge_bin_path() -> String {
    // Use the path of the currently running binary if available
    if let Ok(exe) = std::env::current_exe() {
        if let Some(path) = exe.to_str() {
            return path.to_string();
        }
    }
    // Fallback: ~/.cargo/bin/flowforge
    if let Some(home) = dirs::home_dir() {
        return home
            .join(".cargo")
            .join("bin")
            .join("flowforge")
            .to_string_lossy()
            .to_string();
    }
    // Last resort: bare command name
    "flowforge".to_string()
}

/// Build the FlowForge hooks settings as a JSON Value.
/// Uses the absolute binary path so hooks work even if ~/.cargo/bin isn't on PATH.
fn flowforge_hooks() -> serde_json::Value {
    let bin = flowforge_bin_path();
    serde_json::json!({
        "PreToolUse": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook pre-tool-use"),
                    "timeout": 3000
                }]
            }
        ],
        "PostToolUse": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook post-tool-use"),
                    "timeout": 3000
                }]
            }
        ],
        "PostToolUseFailure": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook post-tool-use-failure"),
                    "timeout": 3000
                }]
            }
        ],
        "Notification": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook notification"),
                    "timeout": 3000
                }]
            }
        ],
        "UserPromptSubmit": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook user-prompt-submit"),
                    "timeout": 5000
                }]
            }
        ],
        "SessionStart": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook session-start"),
                    "timeout": 10000
                }]
            }
        ],
        "SessionEnd": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook session-end")
                }]
            }
        ],
        "Stop": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook stop")
                }]
            }
        ],
        "PreCompact": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook pre-compact")
                }]
            }
        ],
        "SubagentStart": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook subagent-start")
                }]
            }
        ],
        "SubagentStop": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook subagent-stop")
                }]
            }
        ],
        "TeammateIdle": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook teammate-idle")
                }]
            }
        ],
        "TaskCompleted": [
            {
                "hooks": [{
                    "type": "command",
                    "command": format!("{bin} hook task-completed")
                }]
            }
        ]
    })
}

/// Merge FlowForge hooks into existing settings.json, don't overwrite (A7).
fn write_settings_json() -> Result<()> {
    let settings_dir = Path::new(".claude");
    std::fs::create_dir_all(settings_dir)?;

    let settings_path = settings_dir.join("settings.json");

    // Load existing settings if present
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Ensure settings is a JSON object
    if !settings.is_object() {
        settings = serde_json::json!({});
    }

    // Ensure env section exists with teams enabled
    let env = settings
        .as_object_mut()
        .expect("settings is guaranteed to be an object")
        .entry("env")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(env_obj) = env.as_object_mut() {
        env_obj
            .entry("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS")
            .or_insert_with(|| serde_json::json!("1"));
    }

    // Deep-merge FlowForge hooks into existing hooks
    let ff_hooks = flowforge_hooks();
    let hooks = settings
        .as_object_mut()
        .expect("settings is guaranteed to be an object")
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));

    if let (Some(existing_hooks), Some(ff_hooks_obj)) =
        (hooks.as_object_mut(), ff_hooks.as_object())
    {
        for (event_name, ff_hook_array) in ff_hooks_obj {
            if let Some(existing_arr) = existing_hooks
                .get_mut(event_name)
                .and_then(|v| v.as_array_mut())
            {
                // Remove any existing FlowForge hook entries so we can replace them
                // with the current absolute-path version
                existing_arr.retain(|entry| {
                    !entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|hooks| {
                            hooks.iter().any(|h| {
                                h.get("command")
                                    .and_then(|c| c.as_str())
                                    .map(|c| c.starts_with("flowforge") || c.contains("/flowforge"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
                });
                // Append fresh FlowForge hooks with absolute path
                if let Some(ff_arr) = ff_hook_array.as_array() {
                    for item in ff_arr {
                        existing_arr.push(item.clone());
                    }
                }
            } else {
                // No existing hooks for this event, insert directly
                existing_hooks.insert(event_name.clone(), ff_hook_array.clone());
            }
        }
    }

    // Update statusLine to use absolute binary path
    let bin = flowforge_bin_path();
    settings
        .as_object_mut()
        .expect("settings is guaranteed to be an object")
        .insert(
            "statusLine".to_string(),
            serde_json::json!({
                "type": "command",
                "command": format!("{bin} statusline")
            }),
        );

    let content = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&settings_path, content)?;

    Ok(())
}

/// Create .mcp.json for MCP server auto-registration (B3).
fn write_mcp_json() -> Result<()> {
    let mcp_path = Path::new(".mcp.json");

    // Load existing .mcp.json if present, merge
    let mut mcp: serde_json::Value = if mcp_path.exists() {
        let content = std::fs::read_to_string(mcp_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !mcp.is_object() {
        mcp = serde_json::json!({});
    }
    let servers = mcp
        .as_object_mut()
        .expect("mcp is guaranteed to be an object")
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));

    if let Some(servers_obj) = servers.as_object_mut() {
        servers_obj.entry("flowforge").or_insert_with(|| {
            serde_json::json!({
                "command": flowforge_bin_path(),
                "args": ["mcp", "serve"]
            })
        });
    }

    let content = serde_json::to_string_pretty(&mcp)?;
    std::fs::write(mcp_path, content)?;

    Ok(())
}

fn write_claude_md() -> Result<()> {
    let claude_md_path = Path::new("CLAUDE.md");
    let mut content = String::new();

    if claude_md_path.exists() {
        content = std::fs::read_to_string(claude_md_path)?;
        if content.contains("[FlowForge]") {
            return Ok(()); // Already has FlowForge section
        }
        content.push_str("\n\n");
    }

    content.push_str(
        r#"## [FlowForge] Agent Orchestration

This project uses FlowForge for intelligent agent orchestration.

### Agent Teams
- For multi-file changes, use agent teams (TeamCreate + Task)
- FlowForge will route tasks to specialized agents and provide context
- Maximum 6-8 agents per team for optimal coordination
- Use the anti-drift swarm pattern (hierarchical topology, raft consensus)

### Dual Memory System
FlowForge uses BOTH a fast Rust-based memory system AND Claude's native auto-memory:

**FlowForge Memory (fast, structured, searchable):**
- SQLite + HNSW vector search for sub-millisecond pattern retrieval
- Stores learned patterns, routing weights, session history, edit records
- Use `flowforge memory set <key> <value>` for project-specific knowledge
- Use `flowforge memory search <query>` to recall stored knowledge
- Use `flowforge learn store "<pattern>" --category <cat>` for reusable patterns
- Automatically learns from agent outcomes (routing weights, success rates)
- MCP tools available: `memory_get`, `memory_set`, `memory_search`, `learning_store`

**Claude's Auto-Memory (semantic, cross-session, natural language):**
- Claude's built-in MEMORY.md and topic files for high-level insights
- Best for architectural decisions, user preferences, project conventions
- Persists across all sessions automatically
- Natural language — good for nuanced context

**When to use which:**
- FlowForge memory: routing weights, patterns, metrics, structured data, fast lookup
- Claude memory: design decisions, workflow preferences, project philosophy
- Use BOTH for critical knowledge — redundancy improves recall

### Work Tracking
- FlowForge tracks all work items (epics, tasks, bugs) automatically via hooks
- Every task completion, agent assignment, and status change is logged
- Use `flowforge work status` to see active work
- Use `flowforge work create` to create tracked items
- Supported backends: Claude Tasks, Beads, Kanbus (auto-detected)
- MCP tools: `work_create`, `work_list`, `work_update`, `work_log`

### tmux Monitor
- Run `flowforge tmux start` for real-time team monitoring
- The monitor updates automatically via hooks

### Available Agents
- Run `flowforge agent list` to see all available agents
- Run `flowforge route "<task>"` to get agent suggestions
- Run `flowforge learn stats` to check learning progress
"#,
    );

    std::fs::write(claude_md_path, content)?;
    Ok(())
}
