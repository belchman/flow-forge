# FlowForge

[![CI](https://github.com/belchman/flow-forge/actions/workflows/ci.yml/badge.svg)](https://github.com/belchman/flow-forge/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Agent orchestration for [Claude Code](https://docs.anthropic.com/en/docs/claude-code). FlowForge adds intelligent routing, pattern learning, memory, and team coordination to your Claude Code sessions through hooks, an MCP server, and 60 built-in agents.

## Features

- **Agent routing** &mdash; routes tasks to the best agent using pattern matching, capability scoring, and learned weights
- **Pattern learning** &mdash; records what works and improves routing over time via short-term/long-term pattern promotion
- **Memory system** &mdash; SQLite + HNSW vector search for fast, structured, searchable knowledge
- **Work tracking** &mdash; tracks tasks, epics, and bugs with a full audit trail
- **Conversation storage** &mdash; ingests Claude Code JSONL transcripts into SQLite for querying past sessions
- **Checkpoints &amp; forks** &mdash; named snapshots at any point in a conversation; fork a session to branch reasoning
- **Co-agent mailbox** &mdash; peer agents on the same work item exchange messages, auto-injected into context
- **13 Claude Code hooks** &mdash; session lifecycle, prompt routing, dangerous command blocking, edit tracking, compaction guidance, team monitoring
- **36 MCP tools** &mdash; memory, learning, agents, sessions, conversations, checkpoints, forks, mailbox, team, and work tracking
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

- Rust toolchain (1.86+)

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

# Conversation history and checkpoints
flowforge session history
flowforge session checkpoint "before-refactor"
flowforge session fork --checkpoint "before-refactor"

# Co-agent mailbox
flowforge mailbox send --work-item <id> --from agent-a "found the bug"
flowforge mailbox read

# Check learning progress
flowforge learn stats

# Start the tmux team monitor
flowforge tmux start

# Show overall status
flowforge status
```

## Architecture

```
flowforge (single binary, ~7.5 MB)
├── flowforge-cli      CLI commands + 13 hook handlers
├── flowforge-core     Config, types, hook I/O, transcript parser, work tracking
├── flowforge-memory   SQLite DB, HNSW vectors, pattern learning, conversations
├── flowforge-agents   60 built-in agents, registry, router
├── flowforge-mcp      MCP server (36 tools over JSON-RPC 2.0)
└── flowforge-tmux     tmux team monitor
```

### Hooks

FlowForge wires into all 13 Claude Code hook events:

| Event | What FlowForge Does |
|-------|---------------------|
| SessionStart | Creates session record, stores transcript path, syncs work items |
| SessionEnd | Ingests transcript, ends session, consolidates patterns |
| UserPromptSubmit | Routes to best agent, injects context + mailbox messages |
| PreToolUse | Blocks dangerous Bash commands, tracks command count |
| PostToolUse | Tracks file edits (Write/Edit/MultiEdit) |
| PostToolUseFailure | Records error patterns for learning |
| PreCompact | Injects guidance before context compaction |
| SubagentStart | Updates monitor, stores transcript path, assigns work to agent |
| SubagentStop | Ingests agent transcript, extracts patterns from output |
| TeammateIdle | Updates monitor status |
| TaskCompleted | Updates routing weights (learning) |
| Stop | Ends active session |
| Notification | Logs to audit trail |

### MCP Tools

When Claude connects to the FlowForge MCP server, 36 tools become available:

| Category | Tools |
|----------|-------|
| Memory | `memory_get`, `memory_set`, `memory_delete`, `memory_list`, `memory_search`, `memory_import` |
| Learning | `learning_store`, `learning_search`, `learning_feedback`, `learning_stats` |
| Agents | `agents_list`, `agents_info`, `agents_route` |
| Sessions | `session_status`, `session_history`, `session_metrics` |
| Conversations | `conversation_history`, `conversation_search`, `conversation_ingest` |
| Checkpoints | `checkpoint_create`, `checkpoint_list`, `checkpoint_get` |
| Forks | `session_fork`, `session_forks`, `session_lineage` |
| Mailbox | `mailbox_send`, `mailbox_read`, `mailbox_history`, `mailbox_agents` |
| Team | `team_status`, `team_log` |
| Work | `work_create`, `work_list`, `work_update`, `work_log` |

## Testing

```bash
# Run all tests
cargo test --workspace

# Run with clippy lints
cargo clippy --workspace -- -D warnings

# Check formatting
cargo fmt --all --check
```

**88 tests** across 5 crates:

| Crate | Tests | Coverage |
|-------|-------|----------|
| flowforge-cli | 27 | CLI commands, hooks, sessions, mailbox, MCP (integration) |
| flowforge-memory | 24 | DB operations, embedding, HNSW search, pattern learning |
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
│   ├── flowforge-core/      Config, types, hook I/O
│   ├── flowforge-memory/    SQLite, vectors, patterns
│   ├── flowforge-agents/    Agent loader, registry, router
│   ├── flowforge-mcp/       MCP server
│   └── flowforge-tmux/      tmux monitor
├── .claude/settings.json    Claude Code hook wiring
├── .mcp.json                MCP server registration
├── CLAUDE.md                Agent orchestration instructions
├── SETUP.md                 Detailed setup guide
└── setup.sh                 One-command setup script
```

## License

[MIT](LICENSE)
