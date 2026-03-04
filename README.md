# FlowForge

[![CI](https://github.com/belchman/flow-forge/actions/workflows/ci.yml/badge.svg)](https://github.com/belchman/flow-forge/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Agent orchestration for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). FlowForge adds intelligent routing, pattern learning, memory, safety guardrails, and team coordination to your Claude Code sessions through hooks, an MCP server, and 60 built-in agents.

## Features

- **Agent routing** &mdash; routes tasks to the best agent using pattern matching, capability scoring, and learned weights
- **Pattern learning** &mdash; records what works and improves routing over time via short-term/long-term pattern promotion
- **Trajectory learning** &mdash; records complete tool-use sequences per session, judges outcomes, distills successful paths into reusable strategies
- **Semantic memory** &mdash; SQLite + HNSW vector search with real semantic embeddings (AllMiniLM-L6-v2 via fastembed), DBSCAN topic clustering, per-cluster P95 dedup thresholds, and cluster-aware decay
- **Guidance control plane** &mdash; configurable safety gates on all tool uses with trust scoring, SHA-256 audit chain, and automatic rule enforcement
- **Work tracking** &mdash; tracks tasks, epics, and bugs with a full audit trail; native Kanbus crate integration (no CLI shelling), Beads JSONL reads, and Claude Tasks dual-write; work-stealing redistributes stale/abandoned tasks automatically
- **Conversation storage** &mdash; ingests Claude Code JSONL transcripts into SQLite for querying past sessions
- **Checkpoints &amp; forks** &mdash; named snapshots at any point in a conversation; fork a session to branch reasoning
- **Co-agent mailbox** &mdash; peer agents on the same work item exchange messages, auto-injected into context
- **Plugin SDK** &mdash; extend FlowForge with custom tools, hooks, and agents via TOML manifests (no recompilation)
- **13 Claude Code hooks** &mdash; session lifecycle, guidance gates, trajectory recording, prompt routing, edit tracking, work-stealing heartbeats, team monitoring
- **53 MCP tools** &mdash; memory, learning, agents, sessions, conversations, checkpoints, forks, mailbox, team, work tracking, guidance, plugins, trajectories, clustering
- **60 built-in agents** &mdash; specialized agents for coding, architecture, security, testing, DevOps, documentation, consensus, and more
- **tmux team monitor** &mdash; real-time dashboard for multi-agent coordination

## Quick Start

```bash
git clone https://github.com/belchman/flow-forge.git
cd flow-forge
./setup.sh
```

This builds FlowForge and installs the `flowforge` binary to `~/.cargo/bin/`. Then initialize it in your project:

```bash
cd /path/to/your/project
flowforge init --project
```

Start a new Claude Code session to activate hooks.

### Manual Install

```bash
cargo install --path crates/flowforge-cli
cd /path/to/your/project
flowforge init --project
```

### Prerequisites

- Rust toolchain (1.88+)

## Usage

```bash
# See all available agents
flowforge agent list

# Route a task to the best agent
flowforge route "fix the authentication bug"

# Store and recall knowledge
flowforge memory set auth_pattern "JWT with refresh tokens"
flowforge memory search "auth"

# Track work items
flowforge work create --type task --title "Fix login flow"
flowforge work status
flowforge work claim <id>
flowforge work stealable

# Guidance control plane
flowforge guidance rules
flowforge guidance trust
flowforge guidance audit
flowforge guidance verify

# Conversation history and checkpoints
flowforge session history
flowforge session checkpoint "before-refactor"
flowforge session fork --checkpoint "before-refactor"

# Co-agent mailbox
flowforge mailbox send --work-item <id> --from agent-a "found the bug"
flowforge mailbox read

# Trajectory learning
flowforge learn stats
flowforge learn trajectories
flowforge learn trajectory <id>

# Semantic embeddings
flowforge learn download-model   # Pre-download the embedding model
flowforge learn clusters          # View topic clusters

# Plugin management
flowforge plugin list
flowforge plugin info <name>

# Start the tmux team monitor
flowforge tmux start

# Test hooks with realistic Claude Code payloads
flowforge test-hooks

# Show overall status
flowforge status
```

## Architecture

```
flowforge (single binary, ~7.5 MB)
├── flowforge-cli      CLI commands + 13 hook handlers
├── flowforge-core     Config, types, hook I/O, guidance engine, plugin loader, work tracking
├── flowforge-memory   SQLite DB, HNSW vectors, semantic embeddings, DBSCAN clustering, pattern learning, trajectory judge
├── flowforge-agents   60 built-in agents, registry, router (+ plugin agents)
├── flowforge-mcp      MCP server (53 tools over JSON-RPC 2.0)
└── flowforge-tmux     tmux team monitor
```

### Hooks

FlowForge wires into all 13 Claude Code hook events:

| Event | What FlowForge Does |
|-------|---------------------|
| SessionStart | Creates session record, initializes trust score, starts trajectory recording, syncs work items |
| SessionEnd | Closes trajectory, runs judgment + distillation, ingests transcript, consolidates patterns |
| UserPromptSubmit | Routes to best agent, sets trajectory task description, injects context + mailbox messages |
| PreToolUse | Runs 5 guidance gates on ALL tools, executes plugin hooks, updates work heartbeats, blocks dangerous commands |
| PostToolUse | Records trajectory step (success), tracks file edits |
| PostToolUseFailure | Records trajectory step (failure), records error patterns for learning |
| PreCompact | Injects guidance before context compaction |
| SubagentStart | Updates monitor, stores transcript path, assigns work to agent |
| SubagentStop | Ingests agent transcript, extracts patterns from output |
| TeammateIdle | Detects stale work items, marks them stealable, updates monitor |
| TaskCompleted | Releases work claims, links trajectory to work item, updates routing weights |
| Stop | Ends active session |
| Notification | Logs to audit trail |

### MCP Tools

When Claude connects to the FlowForge MCP server, 53 tools become available:

| Category | Tools |
|----------|-------|
| Memory | `memory_get`, `memory_set`, `memory_delete`, `memory_list`, `memory_search`, `memory_import` |
| Learning | `learning_store`, `learning_search`, `learning_feedback`, `learning_stats`, `learning_clusters` |
| Agents | `agents_list`, `agents_info`, `agents_route` |
| Sessions | `session_status`, `session_history`, `session_metrics`, `session_agents` |
| Conversations | `conversation_history`, `conversation_search`, `conversation_ingest` |
| Checkpoints | `checkpoint_create`, `checkpoint_list`, `checkpoint_get` |
| Forks | `session_fork`, `session_forks`, `session_lineage` |
| Mailbox | `mailbox_send`, `mailbox_read`, `mailbox_history`, `mailbox_agents` |
| Team | `team_status`, `team_log` |
| Work | `work_create`, `work_list`, `work_update`, `work_log`, `work_close`, `work_sync`, `work_load`, `work_claim`, `work_release`, `work_steal`, `work_heartbeat` |
| Guidance | `guidance_rules`, `guidance_trust`, `guidance_audit`, `guidance_verify` |
| Plugins | `plugin_list`, `plugin_info` |
| Trajectories | `trajectory_list`, `trajectory_get`, `trajectory_judge` |

### Guidance Control Plane

The guidance engine evaluates every tool use against 5 configurable gates:

1. **Destructive ops** &mdash; blocks `rm -rf /`, `DROP TABLE`, `git reset --hard`, fork bombs, etc.
2. **Secrets detection** &mdash; denies tool inputs containing AWS keys, bearer tokens, private keys, API secrets
3. **File scope** &mdash; blocks writes to `.env`, `*.key`, `*.pem`, `.ssh/*`, and custom protected paths
4. **Custom rules** &mdash; user-defined regex rules in `config.toml` scoped to tool, command, or file
5. **Diff size** &mdash; asks for confirmation on edits exceeding `max_diff_lines`

Trust scoring adjusts per session: denials lower trust, clean passes raise it. Above the threshold, `ask` auto-promotes to `allow`. All decisions are logged with SHA-256 hash chains for tamper-evident auditing.

### Work-Stealing

When agents stall or die, their work items are automatically redistributed:

- Every tool use sends a heartbeat for claimed work items
- `TeammateIdle` hook detects items with stale heartbeats (configurable threshold)
- Stale items are marked stealable; abandoned items are auto-released back to pending
- Other agents can steal stealable items via `flowforge work steal`

### Plugin SDK

Extend FlowForge without recompiling:

```
.flowforge/plugins/my-plugin/
├── plugin.toml          # Manifest: tools, hooks, agents
├── scripts/
│   └── my_tool.py       # Tool: reads JSON from stdin, writes JSON to stdout
└── agents/
    └── specialist.md    # Agent definition (markdown)
```

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"

[[tools]]
name = "my_custom_tool"
description = "Does a thing"
command = "python3 scripts/my_tool.py"
timeout = 5000

[[hooks]]
event = "PreToolUse"
command = "bash scripts/check.sh"
priority = 10

[[agents]]
path = "agents/specialist.md"
```

### Trajectory Learning

FlowForge records the complete tool-use sequence for every session:

1. **Recording** &mdash; each tool use is logged as a step with SHA-256 hashed input
2. **Judgment** &mdash; at session end, trajectories are scored: `success_ratio * 0.6 + work_item_factor * 0.3 + pattern_match * 0.1`
3. **Distillation** &mdash; successful trajectories are converted to reusable strategy patterns stored in HNSW
4. **Consolidation** &mdash; old failures are pruned, similar successes are merged

### Semantic Memory

FlowForge uses real semantic embeddings instead of simple hash-based n-grams:

- **Embedder abstraction** &mdash; `Embedder` trait with two implementations: `HashEmbedder` (fast, zero-dependency fallback) and `SemanticEmbedder` (384-dim AllMiniLM-L6-v2 quantized via [fastembed](https://github.com/Anush008/fastembed-rs))
- **DBSCAN clustering** &mdash; patterns are automatically grouped into topic clusters using DBSCAN (via [linfa](https://github.com/rust-ml/linfa)); clusters are recomputed during consolidation when the outlier count exceeds a threshold
- **Per-cluster P95 thresholds** &mdash; deduplication uses each cluster's 95th-percentile distance instead of a single global threshold, so tightly-related patterns are deduped more aggressively
- **Cluster-aware search** &mdash; results in the same cluster as the query get a 10% similarity boost
- **Adaptive decay** &mdash; large active clusters (>10 members) decay at 0.5x rate; isolated outliers decay at 2.0x rate
- **Feature-gated** &mdash; semantic embeddings are on by default (`semantic` Cargo feature). Compile with `--no-default-features` for hash-only mode

The embedding model (~30 MB) is downloaded automatically on first use from Hugging Face. Pre-download with `flowforge learn download-model`.

## CLI Reference

| Command | Description |
|---------|-------------|
| `flowforge init --project` | Initialize FlowForge in the current project |
| `flowforge init --global` | Set up global config |
| `flowforge status` | Show overall FlowForge status |
| `flowforge agent list\|info\|search` | Manage agents |
| `flowforge route "<task>"` | Route a task to the best agent |
| `flowforge memory get\|set\|delete\|list\|search` | Memory operations |
| `flowforge session current\|list\|metrics\|agents\|history\|ingest\|checkpoint\|checkpoints\|fork\|forks` | Session management |
| `flowforge learn store\|search\|stats\|trajectories\|trajectory\|judge\|clusters\|download-model` | Pattern learning, trajectories, clustering |
| `flowforge work create\|list\|update\|close\|sync\|status\|log\|claim\|release\|stealable\|steal\|load` | Work tracking + work-stealing |
| `flowforge mailbox send\|read\|history\|agents` | Co-agent mailbox |
| `flowforge guidance rules\|trust\|audit\|verify` | Guidance control plane |
| `flowforge plugin list\|info\|enable\|disable` | Plugin management |
| `flowforge tmux start\|update\|stop\|status` | tmux team monitor |
| `flowforge mcp serve` | Start the MCP server |
| `flowforge statusline` | Output status line for Claude Code |
| `flowforge test-hooks [--event NAME] [--verbose]` | Test all hooks with realistic Claude Code payloads |

## Testing

```bash
# Run all tests
cargo test --workspace

# Test hooks with realistic Claude Code payloads
flowforge test-hooks
flowforge test-hooks --verbose            # Show stdin/stdout/stderr/timing
flowforge test-hooks --event pre-tool-use # Test a single hook

# Run with clippy lints
cargo clippy --workspace -- -D warnings

# Check formatting
cargo fmt --all --check
```

**120+ tests** across 5 crates:

| Crate | Tests | Coverage |
|-------|-------|----------|
| flowforge-cli | 42 | CLI commands, hooks, realistic Claude Code payloads, sessions, mailbox, MCP (integration) |
| flowforge-memory | 44 | DB operations, semantic + hash embedding, HNSW search, pattern learning, DBSCAN clustering |
| flowforge-agents | 15 | Agent loading, registry, routing |
| flowforge-mcp | 11 | JSON-RPC server, tool dispatch |
| flowforge-tmux | 11 | State management, display rendering |

## Project Structure

```
.
├── agents/                  60 built-in agent definitions (markdown)
│   ├── core/                coder, reviewer, researcher, tester, planner
│   ├── specialized/         architect, security, frontend, backend, database, devops, docs
│   ├── coordination/        team-lead, integrator
│   ├── swarm/               hierarchical, mesh, adaptive coordinators
│   ├── hive-mind/           queen, scout, worker, collective intelligence
│   ├── consensus/           byzantine, crdt, gossip, quorum, raft, security
│   ├── github/              PR, issues, releases, code review, workflow
│   ├── sparc/               specification, pseudocode, architecture, refinement, completion
│   ├── goal/                goal planner, code goal planner
│   ├── testing/             production validator, TDD
│   ├── analysis/            code analyzer, code quality
│   ├── data/                ML model
│   ├── documentation/       OpenAPI docs
│   ├── devops/              CI/CD GitHub Actions
│   └── custom/              database, project, python, rust specialists
├── crates/
│   ├── flowforge-cli/       Binary entry point, commands, hooks
│   ├── flowforge-core/      Config, types, guidance engine, plugin loader, work tracking
│   ├── flowforge-memory/    SQLite, semantic vectors, DBSCAN clustering, patterns, trajectory judge
│   ├── flowforge-agents/    Agent loader, registry, router
│   ├── flowforge-mcp/       MCP server (53 tools)
│   └── flowforge-tmux/      tmux monitor
├── .claude/settings.json    Claude Code hook wiring
├── .mcp.json                MCP server registration
├── CLAUDE.md                Agent orchestration instructions
├── CONTRIBUTING.md          Development guide: what to update when changing FlowForge
├── SETUP.md                 Detailed setup guide
└── setup.sh                 One-command setup script
```

## License

[MIT](LICENSE)
