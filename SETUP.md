# FlowForge Setup Guide

## Quick Start

```bash
# From the flowforge repo root:
./setup.sh
```

This builds, installs, and initializes FlowForge for the current project.

## What `setup.sh` Does

1. **Builds** FlowForge in release mode (`cargo build --release`)
2. **Installs** the `flowforge` binary to `~/.cargo/bin/`
3. **Initializes** the current project with `flowforge init --project`, which creates:

| File | Purpose | Git tracked? |
|------|---------|-------------|
| `.flowforge/config.toml` | Project config (routing weights, pattern settings, guidance, plugins, etc.) | No (`.gitignore`) |
| `.flowforge/flowforge.db` | SQLite database (sessions, patterns, work items, trajectories, trust scores) | No (`.gitignore`) |
| `.flowforge/agents/` | Directory for project-specific custom agents | No (`.gitignore`) |
| `.flowforge/plugins/` | Directory for plugin extensions | No (`.gitignore`) |
| `.claude/settings.json` | Claude Code hooks (13 hook events wired to FlowForge) | **Yes** |
| `.mcp.json` | MCP server auto-registration | **Yes** |
| `CLAUDE.md` | Agent orchestration instructions for Claude | **Yes** |

## Prerequisites

- Rust toolchain (`rustup` + `cargo`)
- `~/.cargo/bin` on your `PATH`
- SQLite3 (comes with macOS, install via package manager on Linux)

## Manual Setup

If you prefer not to use the script:

```bash
# Build
cargo build --release

# Install
cargo install --path crates/flowforge-cli

# Initialize for your project
cd /path/to/your/project
flowforge init --project

# Optional: global config
flowforge init --global
```

## Setting Up a Different Project

FlowForge can be initialized in any project directory:

```bash
cd /path/to/other/project
flowforge init --project
```

This is safe to run multiple times — it merges into existing `.claude/settings.json` rather than overwriting.

## Verifying the Setup

```bash
# Check hooks work
echo '{"prompt":"test"}' | flowforge hook user-prompt-submit

# Check agents are loaded (should show 60)
flowforge agent list | tail -1

# Check session tracking
flowforge session current

# Check work tracking
flowforge work status

# Check routing
flowforge route "fix a bug"

# Check guidance gates
flowforge guidance rules

# Check plugins
flowforge plugin list
```

## Architecture

```
flowforge (CLI binary, ~7.5MB)
├── flowforge-cli      # CLI commands + 13 hook handlers
├── flowforge-core     # Config, types, hook I/O, guidance engine, plugin loader, work tracking
├── flowforge-memory   # SQLite DB, HNSW vectors, pattern learning, trajectory judge
├── flowforge-agents   # 60 built-in agents, registry, router (+ plugin agents)
├── flowforge-mcp      # MCP server (48 tools over JSON-RPC 2.0)
└── flowforge-tmux     # tmux team monitor
```

## Hook Events

All 13 Claude Code hook events are wired:

| Event | What FlowForge Does |
|-------|-------------------|
| `SessionStart` | Creates session record, initializes trust score, starts trajectory recording, syncs work items |
| `SessionEnd` | Closes trajectory, runs judgment + distillation, ingests transcript, consolidates patterns |
| `UserPromptSubmit` | Routes to best agent, sets trajectory task description, injects context |
| `PreToolUse` | Runs 5 guidance gates on ALL tools, executes plugin hooks, updates work heartbeats, blocks dangerous commands |
| `PostToolUse` | Records trajectory step (success), tracks file edits |
| `PostToolUseFailure` | Records trajectory step (failure), records error patterns |
| `PreCompact` | Injects guidance before context compaction |
| `SubagentStart` | Updates tmux monitor, assigns work item to agent |
| `SubagentStop` | Updates tmux monitor, extracts patterns from agent output |
| `TeammateIdle` | Detects stale work items, marks them stealable, updates monitor |
| `TaskCompleted` | Releases work claims, links trajectory to work item, updates routing weights |
| `Stop` | Ends active session |
| `Notification` | Logs notifications to audit trail |

## MCP Tools (48)

Available when Claude connects to the FlowForge MCP server:

**Memory:** `memory_get`, `memory_set`, `memory_delete`, `memory_list`, `memory_search`, `memory_import`
**Learning:** `learning_store`, `learning_search`, `learning_feedback`, `learning_stats`
**Agents:** `agents_list`, `agents_info`, `agents_route`
**Sessions:** `session_status`, `session_history`, `session_metrics`, `session_agents`
**Conversations:** `conversation_history`, `conversation_search`, `conversation_ingest`
**Checkpoints:** `checkpoint_create`, `checkpoint_list`, `checkpoint_get`
**Forks:** `session_fork`, `session_forks`, `session_lineage`
**Mailbox:** `mailbox_send`, `mailbox_read`, `mailbox_history`, `mailbox_agents`
**Team:** `team_status`, `team_log`
**Work:** `work_create`, `work_list`, `work_update`, `work_log`, `work_claim`, `work_release`, `work_steal`, `work_heartbeat`
**Guidance:** `guidance_rules`, `guidance_trust`, `guidance_audit`
**Plugins:** `plugin_list`, `plugin_info`
**Trajectories:** `trajectory_list`, `trajectory_get`, `trajectory_judge`

## Configuration

FlowForge is configured via `.flowforge/config.toml`. Key sections:

```toml
[routing]
# Agent routing weights and settings

[patterns]
# Pattern learning: short-term/long-term promotion, HNSW settings
trajectory_max = 5000
trajectory_prune_days = 7

[guidance]
enabled = true
destructive_ops_gate = true
file_scope_gate = true
diff_size_gate = true
secrets_gate = true
max_diff_lines = 500
trust_initial_score = 0.5
trust_ask_threshold = 0.8
trust_decay_per_hour = 0.02
protected_paths = []
custom_rules = []

[work_tracking]
# Backend auto-detection: kanbus, beads, claude_tasks

[work_tracking.work_stealing]
enabled = true
stale_threshold_mins = 30
abandon_threshold_mins = 60
stale_min_progress = 25

[plugins]
enabled = []    # empty = all enabled
disabled = []   # takes precedence over enabled
```

## Troubleshooting

### Hooks not firing
- Check `which flowforge` returns a path
- Verify `.claude/settings.json` has the hook entries
- Check `.flowforge/hook-errors.log` for errors
- Ensure PreToolUse has **no** `"matcher"` field (it must fire for ALL tools)

### "FlowForge not initialized" errors
- Run `flowforge init --project` in your project root

### MCP server not connecting
- Check `.mcp.json` exists with the flowforge entry
- Restart Claude Code session after adding `.mcp.json`

### Guidance gates not working
- Check `flowforge guidance rules` to see which gates are enabled
- Check `.flowforge/config.toml` has `[guidance] enabled = true`
- PreToolUse must fire for ALL tools (no matcher restriction in `.claude/settings.json`)

### Work tracking backend
- FlowForge auto-detects: `.kanbus.yml` -> Kanbus, `.beads/` -> Beads, else -> Claude Tasks
- Override in `.flowforge/config.toml`: `[work_tracking] backend = "kanbus"`

### Plugin not loading
- Check plugin directory exists: `.flowforge/plugins/<name>/plugin.toml`
- Run `flowforge plugin list` to see loaded plugins
- Check the plugin isn't in the `[plugins] disabled` list
