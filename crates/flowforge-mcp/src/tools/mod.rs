mod agents;
mod conversations;
mod failure_patterns;
mod file_dependencies;
mod guidance;
mod learning;
mod mailbox;
mod memory;
mod plugins;
mod sessions;
mod team;
mod trajectories;
mod work;
mod error_recovery;
mod recovery_strategies;
mod tool_metrics;
mod work_stealing;

use serde_json::{json, Value};
use std::collections::HashMap;

use flowforge_agents::AgentRegistry;
use flowforge_core::FlowForgeConfig;
use flowforge_memory::{HnswCache, MemoryDb};

use crate::tool_builder::ToolBuilderExt;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct ToolRegistry {
    tools: HashMap<String, ToolDef>,
    config: FlowForgeConfig,
    db: Option<MemoryDb>,
    hnsw_cache: HnswCache,
    agent_registry: Option<AgentRegistry>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        Self::register_all(&mut tools);

        let config = FlowForgeConfig::load(&FlowForgeConfig::config_path())
            .unwrap_or_else(|_| FlowForgeConfig::default());
        let db_path = config.db_path();
        let db = MemoryDb::open(&db_path).ok();
        let hnsw_cache = flowforge_memory::new_hnsw_cache();
        let agent_registry = AgentRegistry::load(&config.agents).ok();

        Self {
            tools,
            config,
            db,
            hnsw_cache,
            agent_registry,
        }
    }

    pub fn list(&self) -> Vec<&ToolDef> {
        let mut tools: Vec<_> = self.tools.values().collect();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }

    pub fn get(&self, name: &str) -> Option<&ToolDef> {
        self.tools.get(name)
    }

    pub fn call(&self, name: &str, params: &Value) -> Value {
        match name {
            // Memory
            "memory_get" => self.use_db(|db, _, p| memory::get(db, p), params),
            "memory_set" => self.use_db(|db, _, p| memory::set(db, p), params),
            "memory_search" => self.use_db(|db, _, p| memory::search(db, p), params),
            "memory_delete" => self.use_db(|db, _, p| memory::delete(db, p), params),
            "memory_list" => self.use_db(|db, _, p| memory::list(db, p), params),
            "memory_import" => self.use_db(|db, _, p| memory::import(db, p), params),

            // Learning (uses persistent HNSW cache)
            "learning_store" => self.use_db(
                |db, c, p| learning::store_cached(db, c, p, &self.hnsw_cache),
                params,
            ),
            "learning_search" => self.use_db(
                |db, c, p| learning::search_cached(db, c, p, &self.hnsw_cache),
                params,
            ),
            "learning_feedback" => self.use_db(
                |db, c, p| learning::feedback_cached(db, c, p, &self.hnsw_cache),
                params,
            ),
            "learning_stats" => self.use_db(|db, _, _| learning::stats(db), params),
            "learning_clusters" => self.use_db(|db, _, _| learning::clusters(db), params),

            // Agents (uses cached registry)
            "agents_list" => agents::list_cached(self.agent_registry.as_ref(), params),
            "agents_route" => self.use_db(
                |db, c, p| agents::route_cached(db, c, p, self.agent_registry.as_ref()),
                params,
            ),
            "agents_info" => agents::info_cached(self.agent_registry.as_ref(), params),

            // Sessions
            "session_status" => self.use_db(|db, _, _| sessions::status(db), params),
            "session_metrics" => self.use_db(|db, _, p| sessions::metrics(db, p), params),
            "session_history" => self.use_db(|db, _, p| sessions::history(db, p), params),
            "session_agents" => self.use_db(|db, _, p| sessions::agents(db, p), params),

            // Team
            "team_status" => team::status(),
            "team_log" => team::log(params),

            // Work
            "work_create" => self.use_db(work::create, params),
            "work_list" => self.use_db(|db, _, p| work::list(db, p), params),
            "work_update" => self.use_db(work::update, params),
            "work_log" => self.use_db(|db, _, p| work::log(db, p), params),
            "work_close" => self.use_db(work::close, params),
            "work_comment" => self.use_db(work::comment, params),
            "work_sync" => self.use_db(|db, c, _| work::sync(db, c), params),
            "work_load" => self.use_db(|db, _, _| work::load(db), params),
            "work_status" => self.use_db(|db, _, _| work::status(db), params),

            // Work-stealing
            "work_claim" => self.use_db(|db, _, p| work_stealing::claim(db, p), params),
            "work_release" => self.use_db(|db, _, p| work_stealing::release(db, p), params),
            "work_steal" => self.use_db(|db, _, p| work_stealing::steal(db, p), params),
            "work_heartbeat" => self.use_db(|db, _, p| work_stealing::heartbeat(db, p), params),
            "work_stealable" => self.use_db(|db, _, p| work_stealing::stealable(db, p), params),

            // Conversations, checkpoints, forks
            "conversation_history" => self.use_db(|db, _, p| conversations::history(db, p), params),
            "conversation_search" => self.use_db(|db, _, p| conversations::search(db, p), params),
            "conversation_ingest" => self.use_db(|db, _, p| conversations::ingest(db, p), params),
            "checkpoint_create" => {
                self.use_db(|db, _, p| conversations::checkpoint_create(db, p), params)
            }
            "checkpoint_list" => {
                self.use_db(|db, _, p| conversations::checkpoint_list(db, p), params)
            }
            "checkpoint_get" => {
                self.use_db(|db, _, p| conversations::checkpoint_get(db, p), params)
            }
            "session_fork" => self.use_db(|db, _, p| conversations::session_fork(db, p), params),
            "session_forks" => self.use_db(|db, _, p| conversations::session_forks(db, p), params),
            "session_lineage" => {
                self.use_db(|db, _, p| conversations::session_lineage(db, p), params)
            }

            // Mailbox
            "mailbox_send" => self.use_db(|db, _, p| mailbox::send(db, p), params),
            "mailbox_read" => self.use_db(|db, _, p| mailbox::read(db, p), params),
            "mailbox_history" => self.use_db(|db, _, p| mailbox::history(db, p), params),
            "mailbox_agents" => self.use_db(|db, _, p| mailbox::agents(db, p), params),

            // Guidance
            "guidance_rules" => self.use_config(|c, _| guidance::rules(c), params),
            "guidance_trust" => self.use_db(|db, _, p| guidance::trust(db, p), params),
            "guidance_audit" => self.use_db(|db, _, p| guidance::audit(db, p), params),
            "guidance_verify" => self.use_db(|db, _, p| guidance::verify(db, p), params),

            // Plugins
            "plugin_list" => self.use_config(|c, _| plugins::list(c), params),
            "plugin_info" => self.use_config(plugins::info, params),

            // Trajectories
            "trajectory_list" => self.use_db(|db, _, p| trajectories::list(db, p), params),
            "trajectory_get" => self.use_db(|db, _, p| trajectories::get(db, p), params),
            "trajectory_judge" => self.use_db(trajectories::judge, params),

            // Failure patterns
            "failure_patterns" => {
                self.use_db(|db, _, p| failure_patterns::list(db, p), params)
            }

            // File dependencies
            "file_dependencies" => {
                self.use_db(|db, _, p| file_dependencies::list(db, p), params)
            }

            // Error recovery
            "error_list" => self.use_db(|db, _, p| error_recovery::list(db, p), params),
            "error_find" => self.use_db(|db, _, p| error_recovery::find(db, p), params),
            "error_stats" => self.use_db(|db, _, _| error_recovery::stats(db), params),

            // Recovery strategies
            "recovery_strategies" => {
                self.use_db(|db, _, p| recovery_strategies::list(db, p), params)
            }

            // Tool metrics
            "tool_metrics" => self.use_db(|db, _, p| tool_metrics::list(db, p), params),
            "tool_best_agents" => self.use_db(|db, _, p| tool_metrics::best(db, p), params),
            "session_cost" => self.use_db(|db, _, p| tool_metrics::session_cost(db, p), params),

            // Intelligence
            "task_decomposition" => {
                self.use_db(|db, _, p| task_decomposition(db, p), params)
            }
            "similar_trajectories" => {
                self.use_db(|db, _, p| similar_trajectories(db, p), params)
            }
            "batching_insights" => self.use_db(|db, _, _| batching_insights(db), params),

            _ => json!({ "error": format!("unknown tool: {}", name) }),
        }
    }

    // ── Registration ───────────────────────────────────────────────

    fn register_all(tools: &mut HashMap<String, ToolDef>) {
        // Memory tools
        tools
            .tool("memory_get", "Get a memory entry by key")
            .required_str("key", "The memory key to retrieve")
            .build();
        tools
            .tool("memory_set", "Store a memory entry with a key and value")
            .required_str("key", "The memory key")
            .required_str("value", "The value to store")
            .optional_str("category", "Optional category for the memory")
            .build();
        tools
            .tool("memory_search", "Search memory entries by query string")
            .required_str("query", "Search query")
            .optional_int_default("limit", "Max results to return", 10)
            .build();
        tools
            .tool("memory_delete", "Delete a memory entry by key")
            .required_str("key", "The memory key to delete")
            .build();
        tools
            .tool(
                "memory_list",
                "List all memory entries, optionally filtered by category",
            )
            .optional_str("category", "Filter by category")
            .optional_int_default("limit", "Max results", 50)
            .build();
        tools
            .tool("memory_import", "Import memory entries from a JSON array")
            .required_array(
                "entries",
                "Array of memory entries to import",
                json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string" },
                        "value": { "type": "string" },
                        "category": { "type": "string" }
                    },
                    "required": ["key", "value"]
                }),
            )
            .build();

        // Learning tools
        tools
            .tool(
                "learning_store",
                "Store a learned pattern from an observation",
            )
            .required_str("content", "The pattern content")
            .required_str("category", "Pattern category (e.g., code_style, error_fix)")
            .optional_num_default("confidence", "Initial confidence 0.0-1.0", 0.5)
            .build();
        tools
            .tool("learning_search", "Search learned patterns by query")
            .required_str("query", "Search query")
            .optional_str("category", "Filter by category")
            .optional_int_default("limit", "Max results", 10)
            .build();
        tools
            .tool(
                "learning_feedback",
                "Provide feedback on a learned pattern (positive or negative)",
            )
            .required_str("pattern_id", "The pattern ID")
            .required_bool("positive", "Whether the feedback is positive")
            .build();
        tools
            .tool("learning_stats", "Get statistics about learned patterns")
            .build();
        tools
            .tool(
                "learning_clusters",
                "Get topic cluster information for pattern vectors",
            )
            .build();

        // Agent tools
        tools
            .tool(
                "agents_list",
                "List all available agents with their capabilities",
            )
            .optional_str("source", "Filter by source: builtin, global, project")
            .build();
        tools
            .tool(
                "agents_route",
                "Route a task description to the best matching agent",
            )
            .required_str("task", "Task description to route")
            .optional_int_default("top_k", "Number of top candidates", 3)
            .build();
        tools
            .tool("agents_info", "Get detailed info about a specific agent")
            .required_str("name", "Agent name")
            .build();

        // Session tools
        tools
            .tool(
                "session_status",
                "Get current session status including active tasks and edits",
            )
            .build();
        tools
            .tool(
                "session_metrics",
                "Get session metrics: edits, commands, routing decisions",
            )
            .optional_str("session_id", "Session ID (defaults to current)")
            .build();
        tools
            .tool("session_history", "Get session history with summaries")
            .optional_int_default("limit", "Max sessions to return", 10)
            .build();
        tools
            .tool(
                "session_agents",
                "List agent sessions for a given session or the current session",
            )
            .optional_str("session_id", "Parent session ID (defaults to current)")
            .build();

        // Team tools
        tools
            .tool(
                "team_status",
                "Get current team status including all member states",
            )
            .build();
        tools
            .tool("team_log", "Get recent team activity log")
            .optional_int_default("limit", "Max log entries", 20)
            .build();

        // Work tracking tools
        tools
            .tool(
                "work_create",
                "Create a new work item (task, epic, bug, story)",
            )
            .required_str("title", "Title of the work item")
            .optional_str_default(
                "type",
                "Item type: task, epic, bug, story, sub-task",
                "task",
            )
            .optional_str("description", "Optional description")
            .optional_str("parent_id", "Parent work item ID for hierarchy")
            .optional_int_default("priority", "Priority 0-3 (0=critical)", 2)
            .build();
        tools
            .tool("work_list", "List work items with optional filters")
            .optional_str(
                "status",
                "Filter by status: pending, in_progress, blocked, completed",
            )
            .optional_str("type", "Filter by item type")
            .optional_int_default("limit", "Max results", 20)
            .build();
        tools
            .tool("work_update", "Update a work item's status")
            .required_str("id", "Work item ID")
            .required_str(
                "status",
                "New status: pending, in_progress, blocked, completed",
            )
            .build();
        tools
            .tool("work_log", "Query the work tracking audit trail")
            .optional_str("work_item_id", "Filter by work item ID (optional)")
            .optional_int_default("limit", "Max events", 20)
            .build();
        tools
            .tool(
                "conversation_history",
                "Get conversation messages for a session (paginated)",
            )
            .required_str("session_id", "Session ID")
            .optional_int_default("limit", "Max messages", 20)
            .optional_int_default("offset", "Offset for pagination", 0)
            .build();
        tools
            .tool(
                "conversation_search",
                "Search conversation messages by content (LIKE search)",
            )
            .required_str("session_id", "Session ID")
            .required_str("query", "Search query")
            .optional_int_default("limit", "Max results", 10)
            .build();
        tools
            .tool(
                "conversation_ingest",
                "Trigger transcript ingestion for a session",
            )
            .required_str("session_id", "Session ID")
            .required_str("transcript_path", "Path to JSONL transcript")
            .build();
        tools
            .tool(
                "checkpoint_create",
                "Create a named checkpoint at the current conversation position",
            )
            .required_str("session_id", "Session ID")
            .required_str("name", "Checkpoint name")
            .optional_str("description", "Optional description")
            .build();
        tools
            .tool("checkpoint_list", "List checkpoints for a session")
            .required_str("session_id", "Session ID")
            .build();
        tools
            .tool(
                "checkpoint_get",
                "Get a checkpoint by ID or by name+session",
            )
            .optional_str("id", "Checkpoint ID")
            .optional_str("session_id", "Session ID (for name lookup)")
            .optional_str("name", "Checkpoint name (requires session_id)")
            .build();
        tools
            .tool(
                "session_fork",
                "Fork a session's conversation at a checkpoint or message index",
            )
            .required_str("session_id", "Source session ID")
            .optional_str("checkpoint_name", "Fork at this checkpoint")
            .optional_int("at_index", "Fork at this message index")
            .optional_str("reason", "Reason for the fork")
            .build();
        tools
            .tool("session_forks", "List forks for a session")
            .required_str("session_id", "Session ID")
            .build();
        tools
            .tool(
                "session_lineage",
                "Trace the fork lineage of a session back to root",
            )
            .required_str("session_id", "Session ID")
            .build();
        tools
            .tool("mailbox_send", "Send a message to co-agents on a work item")
            .required_str("work_item_id", "Work item ID (coordination hub)")
            .required_str("from_session_id", "Sender session ID")
            .required_str("from_agent_name", "Sender agent name")
            .optional_str("to_session_id", "Target session ID (omit for broadcast)")
            .optional_str("to_agent_name", "Target agent name (omit for broadcast)")
            .required_str("content", "Message content")
            .optional_str_default(
                "message_type",
                "Message type: text, status_update, request, result",
                "text",
            )
            .optional_int_default("priority", "Priority 0-3 (0=highest)", 2)
            .build();
        tools
            .tool("mailbox_read", "Read unread mailbox messages for a session")
            .required_str("session_id", "Session ID")
            .build();
        tools
            .tool(
                "mailbox_history",
                "Get mailbox message history for a work item",
            )
            .required_str("work_item_id", "Work item ID")
            .optional_int_default("limit", "Max messages", 20)
            .build();
        tools
            .tool("mailbox_agents", "List agents assigned to a work item")
            .required_str("work_item_id", "Work item ID")
            .build();
        tools
            .tool(
                "guidance_rules",
                "List guidance rules and gate configuration",
            )
            .build();
        tools
            .tool("guidance_trust", "Get trust score for a session")
            .optional_str("session_id", "Session ID (optional, defaults to current)")
            .build();
        tools
            .tool("guidance_audit", "Get gate decision audit trail")
            .optional_str("session_id", "Session ID")
            .optional_int("limit", "Max results (default 20)")
            .build();
        tools
            .tool("work_claim", "Claim a work item for the current session")
            .required_str("id", "Work item ID")
            .build();
        tools
            .tool("work_release", "Release a claimed work item")
            .required_str("id", "Work item ID")
            .build();
        tools
            .tool("work_steal", "Steal a stealable work item")
            .optional_str(
                "id",
                "Work item ID (optional, steals highest priority if omitted)",
            )
            .build();
        tools
            .tool("work_heartbeat", "Update heartbeat for claimed work items")
            .optional_int("progress", "Progress percentage (0-100)")
            .optional_str("id", "Work item ID for progress update")
            .build();
        tools.tool("plugin_list", "List installed plugins").build();
        tools
            .tool("plugin_info", "Get detailed plugin information")
            .required_str("name", "Plugin name")
            .build();
        tools
            .tool("trajectory_list", "List recorded trajectories")
            .optional_str("session_id", "Session ID")
            .optional_str(
                "status",
                "Filter by status: recording, completed, failed, judged",
            )
            .optional_int("limit", "Max results (default 20)")
            .build();
        tools
            .tool("trajectory_get", "Get trajectory details with steps")
            .required_str("id", "Trajectory ID")
            .build();
        tools
            .tool("trajectory_judge", "Judge a completed trajectory")
            .required_str("id", "Trajectory ID")
            .build();
        tools
            .tool("work_close", "Close a work item (set status to completed)")
            .required_str("id", "Work item ID to close")
            .build();
        tools
            .tool(
                "work_comment",
                "Add a comment to a work item (syncs to kanbus/beads backend)",
            )
            .required_str("id", "Work item ID")
            .required_str("text", "Comment text")
            .build();
        tools
            .tool(
                "work_sync",
                "Sync work items with external backend (kanbus/beads/claude_tasks)",
            )
            .build();
        tools
            .tool("work_load", "Show work distribution across agents")
            .build();
        tools
            .tool(
                "guidance_verify",
                "Verify SHA-256 audit hash chain integrity",
            )
            .optional_str("session_id", "Session ID (optional, defaults to current)")
            .build();
        tools
            .tool(
                "work_stealable",
                "List work items that are available for stealing",
            )
            .optional_int("limit", "Max items to return (default 10)")
            .build();
        tools
            .tool("work_status", "Get work item counts by status")
            .build();

        // Failure patterns
        tools
            .tool(
                "failure_patterns",
                "List known failure patterns and optionally mine new ones from failed trajectories",
            )
            .optional_bool("mine", "Also mine new patterns from failed trajectories")
            .optional_int_default("min_occurrences", "Minimum occurrences to report when mining", 2)
            .build();

        // File dependencies
        tools
            .tool(
                "file_dependencies",
                "Show file co-edit dependency graph. When a file is specified, shows files commonly edited together with it. Without a file, shows the full dependency graph.",
            )
            .optional_str("file", "File path to show dependencies for (omit for full graph)")
            .optional_int_default("limit", "Max results to return", 20)
            .build();

        // Error recovery
        tools
            .tool("error_list", "List known error fingerprints with occurrence counts")
            .optional_int_default("limit", "Max results", 20)
            .build();
        tools
            .tool("error_find", "Find resolutions for an error by text or fingerprint ID")
            .optional_str("error_text", "Error text to search for")
            .optional_str("fingerprint_id", "Error fingerprint ID")
            .optional_int_default("limit", "Max resolutions", 5)
            .build();
        tools
            .tool("error_stats", "Get error recovery statistics")
            .build();

        // Recovery strategies
        tools
            .tool("recovery_strategies", "Get recovery suggestions for guidance gate denials")
            .optional_str("gate_name", "Filter by gate name")
            .optional_str("trigger", "Filter by trigger pattern")
            .build();

        // Tool metrics
        tools
            .tool("tool_metrics", "List tool success/failure metrics")
            .optional_str("agent_name", "Filter by agent name")
            .build();
        tools
            .tool("tool_best_agents", "Get best agents for a specific tool")
            .required_str("tool_name", "Tool name to find best agents for")
            .optional_int_default("limit", "Max results", 5)
            .build();
        tools
            .tool("session_cost", "Get session cost metrics (tool calls, bytes, errors)")
            .optional_str("session_id", "Session ID (defaults to current)")
            .build();

        // Intelligence
        tools
            .tool("task_decomposition", "Predict subtasks and phases for a task based on historical trajectories")
            .required_str("task", "Task description to decompose")
            .build();
        tools
            .tool("similar_trajectories", "Find similar past trajectories for cross-session knowledge transfer")
            .required_str("task", "Task description to find similar past work for")
            .optional_int_default("limit", "Max results", 5)
            .build();
        tools
            .tool("batching_insights", "Detect sequential tool calls that could be parallelized for efficiency")
            .build();
    }
}

// ── Helpers ────────────────────────────────────────────────────

impl ToolRegistry {
    fn use_db<F>(&self, f: F, params: &Value) -> Value
    where
        F: FnOnce(&MemoryDb, &FlowForgeConfig, &Value) -> flowforge_core::Result<Value>,
    {
        match &self.db {
            Some(db) => match f(db, &self.config, params) {
                Ok(v) => v,
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            None => {
                json!({"status": "error", "message": "Database not available"})
            }
        }
    }

    fn use_config<F>(&self, f: F, params: &Value) -> Value
    where
        F: FnOnce(&FlowForgeConfig, &Value) -> flowforge_core::Result<Value>,
    {
        match f(&self.config, params) {
            Ok(v) => v,
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }
}

// ── Intelligence tool handlers ─────────────────────────────────

fn task_decomposition(db: &MemoryDb, params: &Value) -> flowforge_core::Result<Value> {
    use crate::params::ParamExt;
    let task = params.require_str("task")?;
    let decomp = db.predict_decomposition(task)?;
    Ok(json!({
        "status": "ok",
        "task": decomp.task,
        "confidence": decomp.confidence,
        "sample_count": decomp.sample_count,
        "phases": decomp.phases.iter().map(|p| json!({
            "name": p.name,
            "tools": p.tools,
            "suggested_agent": p.suggested_agent,
            "estimated_steps": p.estimated_steps,
            "confidence": p.confidence,
        })).collect::<Vec<_>>(),
    }))
}

fn similar_trajectories(db: &MemoryDb, params: &Value) -> flowforge_core::Result<Value> {
    use crate::params::ParamExt;
    let task = params.require_str("task")?;
    let limit = params.u64_or("limit", 5) as usize;
    let keywords: Vec<&str> = task.split_whitespace().filter(|w| w.len() > 3).collect();
    let insights = db.find_similar_trajectories(&keywords, limit)?;
    Ok(json!({
        "status": "ok",
        "count": insights.len(),
        "insights": insights.iter().map(|i| json!({
            "task_description": i.task_description,
            "agent_name": i.agent_name,
            "verdict": i.verdict,
            "confidence": i.confidence,
            "total_steps": i.total_steps,
            "success_rate": i.success_rate,
        })).collect::<Vec<_>>(),
    }))
}

fn batching_insights(db: &MemoryDb) -> flowforge_core::Result<Value> {
    let stats = db.get_global_batching_stats(2, 20)?;
    Ok(json!({
        "status": "ok",
        "count": stats.len(),
        "opportunities": stats.iter().map(|o| json!({
            "tool_name": o.tool_name,
            "max_consecutive": o.consecutive_count,
            "occurrences": o.occurrence_count,
        })).collect::<Vec<_>>(),
    }))
}

pub fn current_session_id(db: &MemoryDb) -> String {
    db.get_current_session()
        .ok()
        .flatten()
        .map(|s| s.id)
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ParamExt;

    #[test]
    fn test_registry_has_68_tools() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.list().len(), 68);
    }

    #[test]
    fn test_tool_lookup() {
        let registry = ToolRegistry::new();
        assert!(registry.get("memory_get").is_some());
        assert!(registry.get("team_log").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_tool_call() {
        let registry = ToolRegistry::new();
        let result = registry.call("memory_get", &json!({ "key": "test" }));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_unknown_tool_call() {
        let registry = ToolRegistry::new();
        let result = registry.call("bogus", &json!({}));
        assert!(result["error"].as_str().unwrap().contains("unknown tool"));
    }

    #[test]
    fn test_all_tools_have_schemas() {
        let registry = ToolRegistry::new();
        for tool in registry.list() {
            assert_eq!(
                tool.input_schema["type"], "object",
                "tool {} missing schema",
                tool.name
            );
        }
    }

    #[test]
    fn test_new_tools_registered() {
        let registry = ToolRegistry::new();
        assert!(registry.get("work_close").is_some());
        assert!(registry.get("work_sync").is_some());
        assert!(registry.get("work_load").is_some());
        assert!(registry.get("guidance_verify").is_some());
        assert!(registry.get("work_stealable").is_some());
        assert!(registry.get("work_status").is_some());
    }

    #[test]
    fn test_work_close_requires_id() {
        let registry = ToolRegistry::new();
        let schema = &registry.get("work_close").unwrap().input_schema;
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("id")));
    }

    #[test]
    fn test_work_sync_no_required_params() {
        let registry = ToolRegistry::new();
        let schema = &registry.get("work_sync").unwrap().input_schema;
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn test_guidance_verify_optional_session_id() {
        let registry = ToolRegistry::new();
        let schema = &registry.get("guidance_verify").unwrap().input_schema;
        assert!(schema.get("required").is_none());
        assert!(schema["properties"]["session_id"].is_object());
    }

    #[test]
    fn test_work_close_call_returns_status() {
        let registry = ToolRegistry::new();
        let result = registry.call("work_close", &json!({"id": "test-id"}));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_work_sync_call_returns_status() {
        let registry = ToolRegistry::new();
        let result = registry.call("work_sync", &json!({}));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_work_load_call_returns_status() {
        let registry = ToolRegistry::new();
        let result = registry.call("work_load", &json!({}));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_guidance_verify_call_returns_status() {
        let registry = ToolRegistry::new();
        let result = registry.call("guidance_verify", &json!({}));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_work_stealable_call_returns_status() {
        let registry = ToolRegistry::new();
        let result = registry.call("work_stealable", &json!({}));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_work_status_call_returns_status() {
        let registry = ToolRegistry::new();
        let result = registry.call("work_status", &json!({}));
        assert!(result.get("status").is_some());
    }

    #[test]
    fn test_work_stealable_no_required_params() {
        let registry = ToolRegistry::new();
        let schema = &registry.get("work_stealable").unwrap().input_schema;
        assert!(schema.get("required").is_none());
    }

    #[test]
    fn test_work_status_no_required_params() {
        let registry = ToolRegistry::new();
        let schema = &registry.get("work_status").unwrap().input_schema;
        assert!(schema.get("required").is_none());
    }

    // ── ToolBuilder tests ──

    #[test]
    fn test_tool_builder_required_str() {
        let mut tools = HashMap::new();
        tools
            .tool("test_tool", "A test tool")
            .required_str("name", "The name")
            .build();
        let tool = tools.get("test_tool").unwrap();
        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description, "A test tool");
        assert_eq!(tool.input_schema["type"], "object");
        let required = tool.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("name")));
        assert_eq!(tool.input_schema["properties"]["name"]["type"], "string");
    }

    #[test]
    fn test_tool_builder_optional_fields() {
        let mut tools = HashMap::new();
        tools
            .tool("opt_tool", "Optional fields")
            .optional_str("filter", "Filter query")
            .optional_int("limit", "Max results")
            .optional_num("threshold", "Min threshold")
            .build();
        let tool = tools.get("opt_tool").unwrap();
        assert!(tool.input_schema.get("required").is_none());
        assert_eq!(tool.input_schema["properties"]["filter"]["type"], "string");
        assert_eq!(tool.input_schema["properties"]["limit"]["type"], "integer");
        assert_eq!(
            tool.input_schema["properties"]["threshold"]["type"],
            "number"
        );
    }

    #[test]
    fn test_tool_builder_defaults() {
        let mut tools = HashMap::new();
        tools
            .tool("def_tool", "Defaults")
            .optional_int_default("limit", "Max results", 50)
            .optional_str_default("format", "Output format", "json")
            .optional_num_default("threshold", "Min threshold", 0.5)
            .build();
        let tool = tools.get("def_tool").unwrap();
        assert_eq!(tool.input_schema["properties"]["limit"]["default"], 50);
        assert_eq!(tool.input_schema["properties"]["format"]["default"], "json");
        assert_eq!(tool.input_schema["properties"]["threshold"]["default"], 0.5);
    }

    #[test]
    fn test_tool_builder_mixed_required_optional() {
        let mut tools = HashMap::new();
        tools
            .tool("mixed", "Mixed params")
            .required_str("id", "Item ID")
            .optional_str("description", "Optional desc")
            .required_bool("confirm", "Must confirm")
            .build();
        let tool = tools.get("mixed").unwrap();
        let required = tool.input_schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.iter().any(|v| v.as_str() == Some("id")));
        assert!(required.iter().any(|v| v.as_str() == Some("confirm")));
        assert!(!required.iter().any(|v| v.as_str() == Some("description")));
    }

    // ── ParamExt tests ──

    // ── v4 Phase 6: require_str validation tests ──

    #[test]
    fn test_require_str_missing_param_returns_error() {
        let params = json!({});
        let result = params.require_str("key");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("missing required parameter: key"),
            "got: {err}"
        );
    }

    #[test]
    fn test_require_str_empty_param_returns_error() {
        let params = json!({"key": ""});
        let result = params.require_str("key");
        assert!(result.is_err(), "empty string should fail require_str");
    }

    #[test]
    fn test_require_str_wrong_type_returns_error() {
        let params = json!({"key": 123});
        let result = params.require_str("key");
        assert!(result.is_err(), "non-string should fail require_str");
    }

    #[test]
    fn test_require_str_valid_param_succeeds() {
        let params = json!({"key": "hello"});
        let result = params.require_str("key");
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn test_mcp_work_create_missing_title_returns_error() {
        let registry = ToolRegistry::new();
        let result = registry.call("work_create", &json!({}));
        assert_eq!(result["status"], "error");
        assert!(
            result["message"]
                .as_str()
                .unwrap()
                .contains("missing required"),
            "got: {}",
            result["message"]
        );
    }

    #[test]
    fn test_mcp_memory_set_missing_key_returns_error() {
        let registry = ToolRegistry::new();
        let result = registry.call("memory_set", &json!({"value": "test"}));
        assert_eq!(result["status"], "error");
    }

    #[test]
    fn test_param_ext_str_or() {
        let params = json!({"name": "test", "empty": ""});
        assert_eq!(params.str_or("name", "default"), "test");
        assert_eq!(params.str_or("missing", "default"), "default");
        assert_eq!(params.str_or("empty", "default"), "");
    }

    #[test]
    fn test_param_ext_opt_str() {
        let params = json!({"name": "test"});
        assert_eq!(params.opt_str("name"), Some("test"));
        assert_eq!(params.opt_str("missing"), None);
    }

    #[test]
    fn test_param_ext_u64_or() {
        let params = json!({"count": 42});
        assert_eq!(params.u64_or("count", 0), 42);
        assert_eq!(params.u64_or("missing", 10), 10);
    }

    #[test]
    fn test_param_ext_i64_or() {
        let params = json!({"offset": -5});
        assert_eq!(params.i64_or("offset", 0), -5);
        assert_eq!(params.i64_or("missing", 100), 100);
    }

    #[test]
    fn test_param_ext_bool_or() {
        let params = json!({"flag": true});
        assert!(params.bool_or("flag", false));
        assert!(!params.bool_or("missing", false));
    }

    #[test]
    fn test_param_ext_opt_i64() {
        let params = json!({"val": 99});
        assert_eq!(params.opt_i64("val"), Some(99));
        assert_eq!(params.opt_i64("missing"), None);
    }

    #[test]
    fn test_param_ext_opt_u32() {
        let params = json!({"val": 42});
        assert_eq!(params.opt_u32("val"), Some(42));
        assert_eq!(params.opt_u32("missing"), None);
    }

    // ── Tool registration completeness ──

    #[test]
    fn test_all_work_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "work_create",
            "work_list",
            "work_update",
            "work_log",
            "work_claim",
            "work_release",
            "work_steal",
            "work_heartbeat",
            "work_close",
            "work_comment",
            "work_sync",
            "work_load",
            "work_stealable",
            "work_status",
        ] {
            assert!(registry.get(name).is_some(), "Missing work tool: {name}");
        }
    }

    #[test]
    fn test_all_guidance_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "guidance_rules",
            "guidance_trust",
            "guidance_audit",
            "guidance_verify",
        ] {
            assert!(
                registry.get(name).is_some(),
                "Missing guidance tool: {name}"
            );
        }
    }

    #[test]
    fn test_all_trajectory_tools_registered() {
        let registry = ToolRegistry::new();
        for name in ["trajectory_list", "trajectory_get", "trajectory_judge"] {
            assert!(
                registry.get(name).is_some(),
                "Missing trajectory tool: {name}"
            );
        }
    }

    #[test]
    fn test_all_memory_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "memory_get",
            "memory_set",
            "memory_search",
            "memory_delete",
            "memory_list",
            "memory_import",
        ] {
            assert!(registry.get(name).is_some(), "Missing memory tool: {name}");
        }
    }

    #[test]
    fn test_all_conversation_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "conversation_history",
            "conversation_search",
            "conversation_ingest",
            "checkpoint_create",
            "checkpoint_list",
            "checkpoint_get",
            "session_fork",
            "session_forks",
            "session_lineage",
        ] {
            assert!(
                registry.get(name).is_some(),
                "Missing conversation tool: {name}"
            );
        }
    }

    #[test]
    fn test_all_mailbox_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "mailbox_send",
            "mailbox_read",
            "mailbox_history",
            "mailbox_agents",
        ] {
            assert!(registry.get(name).is_some(), "Missing mailbox tool: {name}");
        }
    }

    #[test]
    fn test_all_session_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "session_status",
            "session_metrics",
            "session_history",
            "session_agents",
        ] {
            assert!(registry.get(name).is_some(), "Missing session tool: {name}");
        }
    }

    #[test]
    fn test_all_learning_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "learning_store",
            "learning_search",
            "learning_feedback",
            "learning_stats",
            "learning_clusters",
        ] {
            assert!(
                registry.get(name).is_some(),
                "Missing learning tool: {name}"
            );
        }
    }

    #[test]
    fn test_all_agent_tools_registered() {
        let registry = ToolRegistry::new();
        for name in ["agents_list", "agents_route", "agents_info"] {
            assert!(registry.get(name).is_some(), "Missing agent tool: {name}");
        }
    }

    #[test]
    fn test_all_team_tools_registered() {
        let registry = ToolRegistry::new();
        for name in ["team_status", "team_log"] {
            assert!(registry.get(name).is_some(), "Missing team tool: {name}");
        }
    }

    #[test]
    fn test_all_plugin_tools_registered() {
        let registry = ToolRegistry::new();
        for name in ["plugin_list", "plugin_info"] {
            assert!(registry.get(name).is_some(), "Missing plugin tool: {name}");
        }
    }

    #[test]
    fn test_all_work_stealing_tools_registered() {
        let registry = ToolRegistry::new();
        for name in [
            "work_claim",
            "work_release",
            "work_steal",
            "work_heartbeat",
            "work_stealable",
        ] {
            assert!(
                registry.get(name).is_some(),
                "Missing work-stealing tool: {name}"
            );
        }
    }

    // ── ToolRegistry persistent state tests ──

    #[test]
    fn test_registry_has_config() {
        let registry = ToolRegistry::new();
        // Config should be loaded (defaults if no config file)
        assert!(!registry.config.general.log_level.is_empty());
    }

    #[test]
    fn test_registry_use_config_returns_value() {
        let registry = ToolRegistry::new();
        let result = registry.use_config(
            |config, _| Ok(json!({"log_level": config.general.log_level})),
            &json!({}),
        );
        assert!(result.get("log_level").is_some());
    }

    #[test]
    fn test_registry_use_db_returns_error_when_no_db() {
        let mut registry = ToolRegistry::new();
        registry.db = None;
        let result = registry.use_db(|_, _, _| Ok(json!({"ok": true})), &json!({}));
        assert_eq!(result["status"], "error");
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("not available"));
    }

    #[test]
    fn test_registry_has_hnsw_cache() {
        let registry = ToolRegistry::new();
        // Cache should start empty
        assert!(registry.hnsw_cache.borrow().is_none());
    }

    #[test]
    fn test_registry_has_agent_registry() {
        let registry = ToolRegistry::new();
        // Agent registry should be loaded (built-in agents at minimum)
        assert!(registry.agent_registry.is_some());
    }

    #[test]
    fn test_agents_list_uses_cached_registry() {
        let registry = ToolRegistry::new();
        let result = registry.call("agents_list", &json!({}));
        assert_eq!(result["status"], "ok");
        assert!(result["agents"].as_array().unwrap().len() > 0);
    }

    #[test]
    fn test_agents_info_uses_cached_registry() {
        let registry = ToolRegistry::new();
        // Get first agent name from the cached list
        let list_result = registry.call("agents_list", &json!({}));
        let first_name = list_result["agents"][0]["name"].as_str().unwrap();
        let result = registry.call("agents_info", &json!({"name": first_name}));
        assert_eq!(result["status"], "ok");
    }
}
