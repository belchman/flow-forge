## [FlowForge] Agent Orchestration

This project uses FlowForge for intelligent agent orchestration.

### Agent Teams
- For multi-file changes, use agent teams (TeamCreate + Task)
- FlowForge will route tasks to specialized agents and provide context
- Maximum 6-8 agents per team for optimal coordination
- Use the anti-drift swarm pattern (hierarchical topology, raft consensus)

### Dual Memory System
FlowForge uses BOTH a fast Rust-based memory system AND Claude's native auto-memory:

**FlowForge Memory (fast, semantic, searchable):**
- SQLite + HNSW vector search with semantic embeddings (AllMiniLM-L6-v2) and DBSCAN topic clustering
- Stores learned patterns, routing weights, session history, edit records
- Use `flowforge memory set <key> <value>` for project-specific knowledge
- Use `flowforge memory search <query>` to recall stored knowledge
- Use `flowforge learn store "<pattern>" --category <cat>` for reusable patterns
- Automatically learns from agent outcomes (routing weights, success rates)
- MCP tools available: `memory_get`, `memory_set`, `memory_search`, `learning_store`, `learning_clusters`

**Claude's Auto-Memory (semantic, cross-session, natural language):**
- Claude's built-in MEMORY.md and topic files for high-level insights
- Best for architectural decisions, user preferences, project conventions
- Persists across all sessions automatically
- Natural language — good for nuanced context

**When to use which:**
- FlowForge memory: routing weights, patterns, metrics, structured data, fast lookup
- Claude memory: design decisions, workflow preferences, project philosophy
- Use BOTH for critical knowledge — redundancy improves recall

### Work Tracking & Work-Stealing
- FlowForge tracks all work items (epics, tasks, bugs) automatically via hooks
- Every task completion, agent assignment, and status change is logged
- **Work-stealing**: agents auto-detect stale/abandoned tasks and redistribute them
- Use `flowforge work status` to see active work
- Use `flowforge work create` to create tracked items
- Use `flowforge work claim <id>` / `flowforge work release <id>` for claim lifecycle
- Use `flowforge work stealable` / `flowforge work steal` for redistribution
- Use `flowforge work load` to see work distribution across agents
- Supported backends: Claude Tasks, Beads, Kanbus (auto-detected)
- MCP tools: `work_create`, `work_list`, `work_update`, `work_log`, `work_close`, `work_sync`, `work_load`, `work_claim`, `work_release`, `work_steal`, `work_heartbeat`

### Guidance Control Plane
- Enforces configurable safety rules on ALL tool uses via `pre_tool_use` hook
- 5 built-in gates: destructive ops, secrets detection, file scope, custom rules, diff size
- Trust scoring with decay and auto-promotion thresholds
- SHA-256 auditable hash chain for all gate decisions
- Use `flowforge guidance rules` to see active gates
- Use `flowforge guidance trust` to check session trust score
- Use `flowforge guidance audit` / `flowforge guidance verify` for audit trail
- MCP tools: `guidance_rules`, `guidance_trust`, `guidance_audit`, `guidance_verify`

### Plugin SDK
- Extend FlowForge with custom tools, hooks, and agents without recompilation
- Plugins live in `.flowforge/plugins/<name>/` with a `plugin.toml` manifest
- Plugin tools execute shell commands with JSON stdin/stdout
- Plugin hooks run in priority order during `pre_tool_use` and other events
- Plugin agents load as markdown files with `AgentSource::Plugin`
- Use `flowforge plugin list` / `flowforge plugin info <name>` to manage
- MCP tools: `plugin_list`, `plugin_info`

### Trajectory Learning
- Records complete execution paths (tool sequences) per session
- Automatically judges outcomes: success ratio + work item completion
- Successful trajectories are distilled into reusable strategy patterns
- Use `flowforge learn trajectories` to list recorded trajectories
- Use `flowforge learn trajectory <id>` to see steps and verdict
- Use `flowforge learn judge <id>` to manually judge a trajectory
- MCP tools: `trajectory_list`, `trajectory_get`, `trajectory_judge`

### Building & Installing
- After any code change, rebuild and reinstall: `cargo build --release && rm -f ~/.cargo/bin/flowforge && cp target/release/flowforge ~/.cargo/bin/flowforge`
- **Important:** Always `rm` the old binary before copying. macOS caches in-place overwrites, causing the stale binary to hang indefinitely. All hooks will silently fail/timeout if this happens.
- Or use `./setup.sh` which handles this automatically.

### tmux Monitor
- Run `flowforge tmux start` for real-time team monitoring
- The monitor updates automatically via hooks

### Slash Commands
- `/status` — Unified project dashboard (work, learning, trust, agents)
- `/test-hooks` — Run hook tests with diagnostics
- `/setup` — Project initialization wizard (build, install, init, verify)

### Available Agents
- Run `flowforge agent list` to see all available agents
- Run `flowforge route "<task>"` to get agent suggestions
- Run `flowforge learn stats` to check learning progress
