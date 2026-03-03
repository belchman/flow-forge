use serde_json::{json, Value};
use std::collections::HashMap;

use flowforge_agents::{AgentRegistry, AgentRouter};
use flowforge_core::FlowForgeConfig;
use flowforge_memory::{MemoryDb, PatternStore};
use flowforge_tmux::TmuxStateManager;

pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct ToolRegistry {
    tools: HashMap<String, ToolDef>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            tools: HashMap::new(),
        };
        registry.register_all();
        registry
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
            "memory_get" => self.memory_get(params),
            "memory_set" => self.memory_set(params),
            "memory_search" => self.memory_search(params),
            "memory_delete" => self.memory_delete(params),
            "memory_list" => self.memory_list(params),
            "memory_import" => self.memory_import(params),
            "learning_store" => self.learning_store(params),
            "learning_search" => self.learning_search(params),
            "learning_feedback" => self.learning_feedback(params),
            "learning_stats" => self.learning_stats(params),
            "agents_list" => self.agents_list(params),
            "agents_route" => self.agents_route(params),
            "agents_info" => self.agents_info(params),
            "session_status" => self.session_status(params),
            "session_metrics" => self.session_metrics(params),
            "session_history" => self.session_history(params),
            "session_agents" => self.session_agents(params),
            "team_status" => self.team_status(params),
            "team_log" => self.team_log(params),
            "work_create" => self.work_create(params),
            "work_list" => self.work_list(params),
            "work_update" => self.work_update(params),
            "work_log" => self.work_log(params),
            "conversation_history" => self.conversation_history(params),
            "conversation_search" => self.conversation_search(params),
            "conversation_ingest" => self.conversation_ingest(params),
            "checkpoint_create" => self.checkpoint_create(params),
            "checkpoint_list" => self.checkpoint_list(params),
            "checkpoint_get" => self.checkpoint_get(params),
            "session_fork" => self.session_fork(params),
            "session_forks" => self.session_forks(params),
            "session_lineage" => self.session_lineage(params),
            "mailbox_send" => self.mailbox_send(params),
            "mailbox_read" => self.mailbox_read(params),
            "mailbox_history" => self.mailbox_history(params),
            "mailbox_agents" => self.mailbox_agents(params),
            "guidance_rules" => self.guidance_rules(params),
            "guidance_trust" => self.guidance_trust(params),
            "guidance_audit" => self.guidance_audit(params),
            "work_claim" => self.work_claim(params),
            "work_release" => self.work_release(params),
            "work_steal" => self.work_steal(params),
            "work_heartbeat" => self.work_heartbeat(params),
            "plugin_list" => self.plugin_list(params),
            "plugin_info" => self.plugin_info(params),
            "trajectory_list" => self.trajectory_list(params),
            "trajectory_get" => self.trajectory_get(params),
            "trajectory_judge" => self.trajectory_judge(params),
            _ => json!({ "error": format!("unknown tool: {}", name) }),
        }
    }

    fn register_all(&mut self) {
        // Memory tools
        self.register(ToolDef {
            name: "memory_get".into(),
            description: "Get a memory entry by key".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The memory key to retrieve" }
                },
                "required": ["key"]
            }),
        });

        self.register(ToolDef {
            name: "memory_set".into(),
            description: "Store a memory entry with a key and value".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The memory key" },
                    "value": { "type": "string", "description": "The value to store" },
                    "category": { "type": "string", "description": "Optional category for the memory" }
                },
                "required": ["key", "value"]
            }),
        });

        self.register(ToolDef {
            name: "memory_search".into(),
            description: "Search memory entries by query string".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Max results to return", "default": 10 }
                },
                "required": ["query"]
            }),
        });

        self.register(ToolDef {
            name: "memory_delete".into(),
            description: "Delete a memory entry by key".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "key": { "type": "string", "description": "The memory key to delete" }
                },
                "required": ["key"]
            }),
        });

        self.register(ToolDef {
            name: "memory_list".into(),
            description: "List all memory entries, optionally filtered by category".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "category": { "type": "string", "description": "Filter by category" },
                    "limit": { "type": "integer", "description": "Max results", "default": 50 }
                }
            }),
        });

        self.register(ToolDef {
            name: "memory_import".into(),
            description: "Import memory entries from a JSON array".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "entries": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string" },
                                "value": { "type": "string" },
                                "category": { "type": "string" }
                            },
                            "required": ["key", "value"]
                        },
                        "description": "Array of memory entries to import"
                    }
                },
                "required": ["entries"]
            }),
        });

        // Learning tools
        self.register(ToolDef {
            name: "learning_store".into(),
            description: "Store a learned pattern from an observation".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "The pattern content" },
                    "category": { "type": "string", "description": "Pattern category (e.g., code_style, error_fix)" },
                    "confidence": { "type": "number", "description": "Initial confidence 0.0-1.0", "default": 0.5 }
                },
                "required": ["content", "category"]
            }),
        });

        self.register(ToolDef {
            name: "learning_search".into(),
            description: "Search learned patterns by query".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query" },
                    "category": { "type": "string", "description": "Filter by category" },
                    "limit": { "type": "integer", "description": "Max results", "default": 10 }
                },
                "required": ["query"]
            }),
        });

        self.register(ToolDef {
            name: "learning_feedback".into(),
            description: "Provide feedback on a learned pattern (positive or negative)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "pattern_id": { "type": "string", "description": "The pattern ID" },
                    "positive": { "type": "boolean", "description": "Whether the feedback is positive" }
                },
                "required": ["pattern_id", "positive"]
            }),
        });

        self.register(ToolDef {
            name: "learning_stats".into(),
            description: "Get statistics about learned patterns".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        });

        // Agent tools
        self.register(ToolDef {
            name: "agents_list".into(),
            description: "List all available agents with their capabilities".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Filter by source: builtin, global, project" }
                }
            }),
        });

        self.register(ToolDef {
            name: "agents_route".into(),
            description: "Route a task description to the best matching agent".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task": { "type": "string", "description": "Task description to route" },
                    "top_k": { "type": "integer", "description": "Number of top candidates", "default": 3 }
                },
                "required": ["task"]
            }),
        });

        self.register(ToolDef {
            name: "agents_info".into(),
            description: "Get detailed info about a specific agent".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Agent name" }
                },
                "required": ["name"]
            }),
        });

        // Session tools
        self.register(ToolDef {
            name: "session_status".into(),
            description: "Get current session status including active tasks and edits".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(ToolDef {
            name: "session_metrics".into(),
            description: "Get session metrics: edits, commands, routing decisions".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID (defaults to current)" }
                }
            }),
        });

        self.register(ToolDef {
            name: "session_history".into(),
            description: "Get session history with summaries".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max sessions to return", "default": 10 }
                }
            }),
        });

        self.register(ToolDef {
            name: "session_agents".into(),
            description: "List agent sessions for a given session or the current session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Parent session ID (defaults to current)" }
                }
            }),
        });

        // Team tools
        self.register(ToolDef {
            name: "team_status".into(),
            description: "Get current team status including all member states".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(ToolDef {
            name: "team_log".into(),
            description: "Get recent team activity log".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max log entries", "default": 20 }
                }
            }),
        });

        // Work tracking tools (C6)
        self.register(ToolDef {
            name: "work_create".into(),
            description: "Create a new work item (task, epic, bug, story)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "Title of the work item" },
                    "type": { "type": "string", "description": "Item type: task, epic, bug, story, sub-task", "default": "task" },
                    "description": { "type": "string", "description": "Optional description" },
                    "parent_id": { "type": "string", "description": "Parent work item ID for hierarchy" },
                    "priority": { "type": "integer", "description": "Priority 0-3 (0=critical)", "default": 2 }
                },
                "required": ["title"]
            }),
        });

        self.register(ToolDef {
            name: "work_list".into(),
            description: "List work items with optional filters".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "description": "Filter by status: pending, in_progress, blocked, completed" },
                    "type": { "type": "string", "description": "Filter by item type" },
                    "limit": { "type": "integer", "description": "Max results", "default": 20 }
                }
            }),
        });

        self.register(ToolDef {
            name: "work_update".into(),
            description: "Update a work item's status".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Work item ID" },
                    "status": { "type": "string", "description": "New status: pending, in_progress, blocked, completed" }
                },
                "required": ["id", "status"]
            }),
        });

        self.register(ToolDef {
            name: "work_log".into(),
            description: "Query the work tracking audit trail".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "work_item_id": { "type": "string", "description": "Filter by work item ID (optional)" },
                    "limit": { "type": "integer", "description": "Max events", "default": 20 }
                }
            }),
        });

        // Conversation tools
        self.register(ToolDef {
            name: "conversation_history".into(),
            description: "Get conversation messages for a session (paginated)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" },
                    "limit": { "type": "integer", "description": "Max messages", "default": 20 },
                    "offset": { "type": "integer", "description": "Offset for pagination", "default": 0 }
                },
                "required": ["session_id"]
            }),
        });

        self.register(ToolDef {
            name: "conversation_search".into(),
            description: "Search conversation messages by content (LIKE search)".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" },
                    "query": { "type": "string", "description": "Search query" },
                    "limit": { "type": "integer", "description": "Max results", "default": 10 }
                },
                "required": ["session_id", "query"]
            }),
        });

        self.register(ToolDef {
            name: "conversation_ingest".into(),
            description: "Trigger transcript ingestion for a session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" },
                    "transcript_path": { "type": "string", "description": "Path to JSONL transcript" }
                },
                "required": ["session_id", "transcript_path"]
            }),
        });

        // Checkpoint tools
        self.register(ToolDef {
            name: "checkpoint_create".into(),
            description: "Create a named checkpoint at the current conversation position".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" },
                    "name": { "type": "string", "description": "Checkpoint name" },
                    "description": { "type": "string", "description": "Optional description" }
                },
                "required": ["session_id", "name"]
            }),
        });

        self.register(ToolDef {
            name: "checkpoint_list".into(),
            description: "List checkpoints for a session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }),
        });

        self.register(ToolDef {
            name: "checkpoint_get".into(),
            description: "Get a checkpoint by ID or by name+session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Checkpoint ID" },
                    "session_id": { "type": "string", "description": "Session ID (for name lookup)" },
                    "name": { "type": "string", "description": "Checkpoint name (requires session_id)" }
                }
            }),
        });

        // Session fork tools
        self.register(ToolDef {
            name: "session_fork".into(),
            description: "Fork a session's conversation at a checkpoint or message index".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Source session ID" },
                    "checkpoint_name": { "type": "string", "description": "Fork at this checkpoint" },
                    "at_index": { "type": "integer", "description": "Fork at this message index" },
                    "reason": { "type": "string", "description": "Reason for the fork" }
                },
                "required": ["session_id"]
            }),
        });

        self.register(ToolDef {
            name: "session_forks".into(),
            description: "List forks for a session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }),
        });

        self.register(ToolDef {
            name: "session_lineage".into(),
            description: "Trace the fork lineage of a session back to root".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }),
        });

        // Mailbox tools
        self.register(ToolDef {
            name: "mailbox_send".into(),
            description: "Send a message to co-agents on a work item".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "work_item_id": { "type": "string", "description": "Work item ID (coordination hub)" },
                    "from_session_id": { "type": "string", "description": "Sender session ID" },
                    "from_agent_name": { "type": "string", "description": "Sender agent name" },
                    "to_session_id": { "type": "string", "description": "Target session ID (omit for broadcast)" },
                    "to_agent_name": { "type": "string", "description": "Target agent name (omit for broadcast)" },
                    "content": { "type": "string", "description": "Message content" },
                    "message_type": { "type": "string", "description": "Message type: text, status_update, request, result", "default": "text" },
                    "priority": { "type": "integer", "description": "Priority 0-3 (0=highest)", "default": 2 }
                },
                "required": ["work_item_id", "from_session_id", "from_agent_name", "content"]
            }),
        });

        self.register(ToolDef {
            name: "mailbox_read".into(),
            description: "Read unread mailbox messages for a session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }),
        });

        self.register(ToolDef {
            name: "mailbox_history".into(),
            description: "Get mailbox message history for a work item".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "work_item_id": { "type": "string", "description": "Work item ID" },
                    "limit": { "type": "integer", "description": "Max messages", "default": 20 }
                },
                "required": ["work_item_id"]
            }),
        });

        self.register(ToolDef {
            name: "mailbox_agents".into(),
            description: "List agents assigned to a work item".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "work_item_id": { "type": "string", "description": "Work item ID" }
                },
                "required": ["work_item_id"]
            }),
        });

        // Guidance tools
        self.register(ToolDef {
            name: "guidance_rules".into(),
            description: "List guidance rules and gate configuration".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(ToolDef {
            name: "guidance_trust".into(),
            description: "Get trust score for a session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID (optional, defaults to current)" }
                }
            }),
        });

        self.register(ToolDef {
            name: "guidance_audit".into(),
            description: "Get gate decision audit trail".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "limit": { "type": "integer", "description": "Max results (default 20)" }
                }
            }),
        });

        // Work-stealing tools
        self.register(ToolDef {
            name: "work_claim".into(),
            description: "Claim a work item for the current session".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Work item ID" }
                },
                "required": ["id"]
            }),
        });

        self.register(ToolDef {
            name: "work_release".into(),
            description: "Release a claimed work item".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Work item ID" }
                },
                "required": ["id"]
            }),
        });

        self.register(ToolDef {
            name: "work_steal".into(),
            description: "Steal a stealable work item".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Work item ID (optional, steals highest priority if omitted)" }
                }
            }),
        });

        self.register(ToolDef {
            name: "work_heartbeat".into(),
            description: "Update heartbeat for claimed work items".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "progress": { "type": "integer", "description": "Progress percentage (0-100)" },
                    "id": { "type": "string", "description": "Work item ID for progress update" }
                }
            }),
        });

        // Plugin tools
        self.register(ToolDef {
            name: "plugin_list".into(),
            description: "List installed plugins".into(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        });

        self.register(ToolDef {
            name: "plugin_info".into(),
            description: "Get detailed plugin information".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Plugin name" }
                },
                "required": ["name"]
            }),
        });

        // Trajectory tools
        self.register(ToolDef {
            name: "trajectory_list".into(),
            description: "List recorded trajectories".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "status": { "type": "string", "description": "Filter by status: recording, completed, failed, judged" },
                    "limit": { "type": "integer", "description": "Max results (default 20)" }
                }
            }),
        });

        self.register(ToolDef {
            name: "trajectory_get".into(),
            description: "Get trajectory details with steps".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Trajectory ID" }
                },
                "required": ["id"]
            }),
        });

        self.register(ToolDef {
            name: "trajectory_judge".into(),
            description: "Judge a completed trajectory".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Trajectory ID" }
                },
                "required": ["id"]
            }),
        });
    }

    fn register(&mut self, tool: ToolDef) {
        self.tools.insert(tool.name.clone(), tool);
    }

    // --- Helpers ---

    fn open_db() -> flowforge_core::Result<MemoryDb> {
        let config = Self::load_config()?;
        MemoryDb::open(&config.db_path())
    }

    fn load_config() -> flowforge_core::Result<FlowForgeConfig> {
        FlowForgeConfig::load(&FlowForgeConfig::config_path())
    }

    // --- Memory tool implementations ---

    fn memory_get(&self, params: &Value) -> Value {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        match Self::open_db() {
            Ok(db) => match db.kv_get(key, namespace) {
                Ok(value) => json!({"status": "ok", "key": key, "value": value}),
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn memory_set(&self, params: &Value) -> Value {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
        let namespace = params
            .get("namespace")
            .or_else(|| params.get("category"))
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        match Self::open_db() {
            Ok(db) => match db.kv_set(key, value, namespace) {
                Ok(()) => json!({"status": "ok", "key": key, "stored": true}),
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn memory_search(&self, params: &Value) -> Value {
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        match Self::open_db() {
            Ok(db) => match db.kv_search(query, limit) {
                Ok(results) => {
                    let entries: Vec<Value> = results
                        .iter()
                        .map(|(k, v, ns)| json!({"key": k, "value": v, "namespace": ns}))
                        .collect();
                    json!({"status": "ok", "query": query, "results": entries})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn memory_delete(&self, params: &Value) -> Value {
        let key = params.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let namespace = params
            .get("namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        match Self::open_db() {
            Ok(db) => match db.kv_delete(key, namespace) {
                Ok(()) => json!({"status": "ok", "key": key, "deleted": true}),
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn memory_list(&self, params: &Value) -> Value {
        let namespace = params
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        match Self::open_db() {
            Ok(db) => match db.kv_list(namespace) {
                Ok(entries) => {
                    let entries: Vec<Value> = entries
                        .iter()
                        .take(limit)
                        .map(|(k, v)| json!({"key": k, "value": v}))
                        .collect();
                    json!({"status": "ok", "entries": entries})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn memory_import(&self, params: &Value) -> Value {
        let entries = match params.get("entries").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return json!({"status": "error", "message": "missing entries array"}),
        };
        let total = entries.len();

        match Self::open_db() {
            Ok(db) => {
                let mut imported = 0usize;
                for entry in entries {
                    let key = entry.get("key").and_then(|v| v.as_str()).unwrap_or("");
                    let value = entry.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    let namespace = entry
                        .get("namespace")
                        .or_else(|| entry.get("category"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("default");
                    if db.kv_set(key, value, namespace).is_ok() {
                        imported += 1;
                    }
                }
                json!({"status": "ok", "imported": imported, "total": total})
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Learning tool implementations ---

    fn learning_store(&self, params: &Value) -> Value {
        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let category = params
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let store = PatternStore::new(&db, &config.patterns);
            let id = store.store_short_term(content, category)?;
            Ok(id)
        }) {
            Ok(id) => json!({"status": "ok", "pattern_id": id}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn learning_search(&self, params: &Value) -> Value {
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let store = PatternStore::new(&db, &config.patterns);
            store.search_patterns(query, limit)
        }) {
            Ok(results) => {
                let patterns: Vec<Value> = results
                    .iter()
                    .map(|(p, _score)| {
                        json!({
                            "id": p.id,
                            "content": p.content,
                            "category": p.category,
                            "confidence": p.confidence,
                            "usage_count": p.usage_count,
                        })
                    })
                    .collect();
                json!({"status": "ok", "patterns": patterns})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn learning_feedback(&self, params: &Value) -> Value {
        let pattern_id = params
            .get("pattern_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let positive = params
            .get("positive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let store = PatternStore::new(&db, &config.patterns);
            store.record_feedback(pattern_id, positive)
        }) {
            Ok(()) => json!({"status": "ok", "pattern_id": pattern_id, "updated": true}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn learning_stats(&self, _params: &Value) -> Value {
        match Self::open_db() {
            Ok(db) => {
                let short = db.count_patterns_short().unwrap_or(0);
                let long = db.count_patterns_long().unwrap_or(0);
                json!({
                    "status": "ok",
                    "short_term_count": short,
                    "long_term_count": long,
                    "total": short + long,
                })
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Agent tool implementations ---

    fn agents_list(&self, params: &Value) -> Value {
        let source_filter = params.get("source").and_then(|v| v.as_str());

        match Self::load_config().and_then(|config| AgentRegistry::load(&config.agents)) {
            Ok(registry) => {
                let agents: Vec<Value> = registry
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
                    .collect();
                json!({"status": "ok", "agents": agents})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn agents_route(&self, params: &Value) -> Value {
        let task = params.get("task").and_then(|v| v.as_str()).unwrap_or("");
        let top_k = params.get("top_k").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let registry = AgentRegistry::load(&config.agents)?;
            let router = AgentRouter::new(&config.routing);

            let weights_vec = db.get_all_routing_weights()?;
            let mut learned_weights: HashMap<(String, String), f64> = HashMap::new();
            for w in &weights_vec {
                learned_weights.insert((w.task_pattern.clone(), w.agent_name.clone()), w.weight);
            }

            let agent_refs: Vec<&_> = registry.list();
            let results = router.route(task, &agent_refs, &learned_weights, None);
            Ok(results)
        }) {
            Ok(results) => {
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
                json!({"status": "ok", "candidates": candidates})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn agents_info(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

        match Self::load_config().and_then(|config| AgentRegistry::load(&config.agents)) {
            Ok(registry) => match registry.get(name) {
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
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    // --- Session tool implementations ---

    fn session_status(&self, _params: &Value) -> Value {
        match Self::open_db() {
            Ok(db) => match db.get_current_session() {
                Ok(Some(session)) => {
                    let agents: Vec<Value> = db
                        .get_agent_sessions(&session.id)
                        .unwrap_or_default()
                        .iter()
                        .filter(|a| a.ended_at.is_none())
                        .map(|a| {
                            json!({
                                "agent_id": a.agent_id,
                                "agent_type": a.agent_type,
                                "status": a.status.to_string(),
                            })
                        })
                        .collect();
                    json!({
                        "status": "ok",
                        "session": {
                            "id": session.id,
                            "started_at": session.started_at.to_rfc3339(),
                            "cwd": session.cwd,
                            "edits": session.edits,
                            "commands": session.commands,
                            "summary": session.summary,
                        },
                        "agents": agents,
                    })
                }
                Ok(None) => json!({"status": "ok", "session": null}),
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn session_metrics(&self, params: &Value) -> Value {
        let session_id = params.get("session_id").and_then(|v| v.as_str());

        match Self::open_db() {
            Ok(db) => {
                let session = if let Some(id) = session_id {
                    db.list_sessions(1000)
                        .ok()
                        .and_then(|sessions| sessions.into_iter().find(|s| s.id == id))
                } else {
                    db.get_current_session().ok().flatten()
                };
                match session {
                    Some(s) => json!({
                        "status": "ok",
                        "session_id": s.id,
                        "edits": s.edits,
                        "commands": s.commands,
                    }),
                    None => {
                        json!({"status": "ok", "session_id": session_id, "edits": 0, "commands": 0})
                    }
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn session_history(&self, params: &Value) -> Value {
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        match Self::open_db() {
            Ok(db) => match db.list_sessions(limit) {
                Ok(sessions) => {
                    let entries: Vec<Value> = sessions
                        .iter()
                        .map(|s| {
                            json!({
                                "id": s.id,
                                "started_at": s.started_at.to_rfc3339(),
                                "ended_at": s.ended_at.map(|t| t.to_rfc3339()),
                                "cwd": s.cwd,
                                "edits": s.edits,
                                "commands": s.commands,
                                "summary": s.summary,
                            })
                        })
                        .collect();
                    json!({"status": "ok", "sessions": entries})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn session_agents(&self, params: &Value) -> Value {
        let session_id = params.get("session_id").and_then(|v| v.as_str());

        match Self::open_db() {
            Ok(db) => {
                let parent_id = if let Some(id) = session_id {
                    id.to_string()
                } else {
                    match db.get_current_session() {
                        Ok(Some(s)) => s.id,
                        Ok(None) => return json!({"status": "ok", "agents": [], "count": 0}),
                        Err(e) => return json!({"status": "error", "message": format!("{e}")}),
                    }
                };

                match db.get_agent_sessions(&parent_id) {
                    Ok(agents) => {
                        let entries: Vec<Value> = agents
                            .iter()
                            .map(|a| {
                                let duration_seconds =
                                    a.ended_at.map(|end| (end - a.started_at).num_seconds());
                                json!({
                                    "id": a.id,
                                    "agent_id": a.agent_id,
                                    "agent_type": a.agent_type,
                                    "status": a.status.to_string(),
                                    "started_at": a.started_at.to_rfc3339(),
                                    "ended_at": a.ended_at.map(|t| t.to_rfc3339()),
                                    "edits": a.edits,
                                    "commands": a.commands,
                                    "task_id": a.task_id,
                                    "duration_seconds": duration_seconds,
                                })
                            })
                            .collect();
                        json!({"status": "ok", "agents": entries, "count": entries.len()})
                    }
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Team tool implementations ---

    fn team_status(&self, _params: &Value) -> Value {
        let mgr = TmuxStateManager::new(FlowForgeConfig::tmux_state_path());
        match mgr.load() {
            Ok(state) => {
                let members: Vec<Value> = state
                    .members
                    .iter()
                    .map(|m| {
                        json!({
                            "agent_id": m.agent_id,
                            "agent_type": m.agent_type,
                            "status": format!("{:?}", m.status),
                            "current_task": m.current_task,
                            "updated_at": m.updated_at.to_rfc3339(),
                        })
                    })
                    .collect();

                // Enrich with DB-backed agent sessions
                let agent_sessions: Vec<Value> = if let Ok(db) = Self::open_db() {
                    db.get_active_agent_sessions()
                        .unwrap_or_default()
                        .iter()
                        .map(|a| {
                            json!({
                                "id": a.id,
                                "agent_id": a.agent_id,
                                "agent_type": a.agent_type,
                                "status": a.status.to_string(),
                                "started_at": a.started_at.to_rfc3339(),
                                "edits": a.edits,
                                "commands": a.commands,
                            })
                        })
                        .collect()
                } else {
                    vec![]
                };

                json!({
                    "status": "ok",
                    "team": state.team_name,
                    "members": members,
                    "agent_sessions": agent_sessions,
                    "memory_count": state.memory_count,
                    "pattern_count": state.pattern_count,
                    "updated_at": state.updated_at.to_rfc3339(),
                })
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn team_log(&self, params: &Value) -> Value {
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        let mgr = TmuxStateManager::new(FlowForgeConfig::tmux_state_path());
        match mgr.load() {
            Ok(state) => {
                let events: Vec<&String> = state.recent_events.iter().take(limit).collect();
                json!({"status": "ok", "events": events})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    // --- Work tracking tool implementations (C6) ---

    fn work_create(&self, params: &Value) -> Value {
        let title = params.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let item_type = params
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("task");
        let description = params.get("description").and_then(|v| v.as_str());
        let parent_id = params.get("parent_id").and_then(|v| v.as_str());
        let priority = params.get("priority").and_then(|v| v.as_i64()).unwrap_or(2) as i32;

        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let now = chrono::Utc::now();
            let backend =
                flowforge_core::work_tracking::detect_backend(&config.work_tracking).to_string();

            let item = flowforge_core::WorkItem {
                id: uuid::Uuid::new_v4().to_string(),
                external_id: None,
                backend,
                item_type: item_type.to_string(),
                title: title.to_string(),
                description: description.map(|s| s.to_string()),
                status: "pending".to_string(),
                assignee: None,
                parent_id: parent_id.map(|s| s.to_string()),
                priority,
                labels: vec![],
                created_at: now,
                updated_at: now,
                completed_at: None,
                session_id: None,
                metadata: None,
                claimed_by: None,
                claimed_at: None,
                last_heartbeat: None,
                progress: 0,
                stealable: false,
            };

            flowforge_core::work_tracking::create_item(&db, &config.work_tracking, &item)?;
            Ok(item.id)
        }) {
            Ok(id) => json!({"status": "ok", "id": id, "title": title}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn work_list(&self, params: &Value) -> Value {
        let status = params.get("status").and_then(|v| v.as_str());
        let item_type = params.get("type").and_then(|v| v.as_str());
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        match Self::open_db() {
            Ok(db) => {
                let filter = flowforge_core::WorkFilter {
                    status: status.map(|s| s.to_string()),
                    item_type: item_type.map(|s| s.to_string()),
                    limit: Some(limit),
                    ..Default::default()
                };
                match db.list_work_items(&filter) {
                    Ok(items) => {
                        let entries: Vec<Value> = items
                            .iter()
                            .map(|i| {
                                json!({
                                    "id": i.id,
                                    "title": i.title,
                                    "type": i.item_type,
                                    "status": i.status,
                                    "assignee": i.assignee,
                                    "priority": i.priority,
                                    "backend": i.backend,
                                    "created_at": i.created_at.to_rfc3339(),
                                })
                            })
                            .collect();
                        json!({"status": "ok", "items": entries, "count": entries.len()})
                    }
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn work_update(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let new_status = params.get("status").and_then(|v| v.as_str()).unwrap_or("");

        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            flowforge_core::work_tracking::update_status(
                &db,
                &config.work_tracking,
                id,
                new_status,
                "mcp",
            )?;
            Ok(())
        }) {
            Ok(()) => json!({"status": "ok", "id": id, "new_status": new_status}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    // --- Conversation tool implementations ---

    fn conversation_history(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        let offset = params.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

        match Self::open_db() {
            Ok(db) => {
                let total = db.get_conversation_message_count(session_id).unwrap_or(0);
                match db.get_conversation_messages(session_id, limit, offset) {
                    Ok(msgs) => {
                        let entries: Vec<Value> = msgs
                            .iter()
                            .map(|m| {
                                json!({
                                    "message_index": m.message_index,
                                    "role": m.role,
                                    "message_type": m.message_type,
                                    "content": m.content,
                                    "model": m.model,
                                    "timestamp": m.timestamp.to_rfc3339(),
                                    "source": m.source,
                                })
                            })
                            .collect();
                        json!({"status": "ok", "messages": entries, "total": total})
                    }
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn conversation_search(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        match Self::open_db() {
            Ok(db) => match db.search_conversation_messages(session_id, query, limit) {
                Ok(msgs) => {
                    let entries: Vec<Value> = msgs
                        .iter()
                        .map(|m| {
                            json!({
                                "message_index": m.message_index,
                                "role": m.role,
                                "content": m.content,
                                "timestamp": m.timestamp.to_rfc3339(),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "results": entries, "count": entries.len()})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn conversation_ingest(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let path = params
            .get("transcript_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::open_db() {
            Ok(db) => match db.ingest_transcript(session_id, path) {
                Ok(count) => json!({"status": "ok", "ingested": count, "session_id": session_id}),
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Checkpoint tool implementations ---

    fn checkpoint_create(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let description = params.get("description").and_then(|v| v.as_str());

        match Self::open_db() {
            Ok(db) => {
                let message_index = db.get_latest_message_index(session_id).unwrap_or(0);
                let cp = flowforge_core::Checkpoint {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    name: name.to_string(),
                    message_index,
                    description: description.map(|s| s.to_string()),
                    git_ref: None,
                    created_at: chrono::Utc::now(),
                    metadata: None,
                };
                match db.create_checkpoint(&cp) {
                    Ok(()) => {
                        json!({"status": "ok", "id": cp.id, "name": name, "message_index": message_index})
                    }
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn checkpoint_list(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::open_db() {
            Ok(db) => match db.list_checkpoints(session_id) {
                Ok(cps) => {
                    let entries: Vec<Value> = cps
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.id,
                                "name": c.name,
                                "message_index": c.message_index,
                                "description": c.description,
                                "git_ref": c.git_ref,
                                "created_at": c.created_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "checkpoints": entries})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn checkpoint_get(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str());
        let session_id = params.get("session_id").and_then(|v| v.as_str());
        let name = params.get("name").and_then(|v| v.as_str());

        match Self::open_db() {
            Ok(db) => {
                let cp = if let Some(id) = id {
                    db.get_checkpoint(id)
                } else if let (Some(sid), Some(n)) = (session_id, name) {
                    db.get_checkpoint_by_name(sid, n)
                } else {
                    return json!({"status": "error", "message": "Provide either id or session_id+name"});
                };
                match cp {
                    Ok(Some(c)) => json!({
                        "status": "ok",
                        "checkpoint": {
                            "id": c.id,
                            "session_id": c.session_id,
                            "name": c.name,
                            "message_index": c.message_index,
                            "description": c.description,
                            "git_ref": c.git_ref,
                            "created_at": c.created_at.to_rfc3339(),
                        }
                    }),
                    Ok(None) => json!({"status": "error", "message": "Checkpoint not found"}),
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Session fork tool implementations ---

    fn session_fork(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let checkpoint_name = params.get("checkpoint_name").and_then(|v| v.as_str());
        let at_index = params
            .get("at_index")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);
        let reason = params.get("reason").and_then(|v| v.as_str());

        match Self::open_db() {
            Ok(db) => {
                // Determine fork point
                let (fork_index, checkpoint_id) = if let Some(cp_name) = checkpoint_name {
                    match db.get_checkpoint_by_name(session_id, cp_name) {
                        Ok(Some(cp)) => (cp.message_index, Some(cp.id)),
                        Ok(None) => {
                            return json!({"status": "error", "message": format!("Checkpoint '{}' not found", cp_name)})
                        }
                        Err(e) => return json!({"status": "error", "message": format!("{e}")}),
                    }
                } else if let Some(idx) = at_index {
                    (idx, None)
                } else {
                    let latest = db.get_latest_message_index(session_id).unwrap_or(0);
                    (latest.saturating_sub(1), None)
                };

                let new_session_id = uuid::Uuid::new_v4().to_string();
                let now = chrono::Utc::now();

                // Create new session
                let new_session = flowforge_core::SessionInfo {
                    id: new_session_id.clone(),
                    started_at: now,
                    ended_at: None,
                    cwd: ".".to_string(),
                    edits: 0,
                    commands: 0,
                    summary: Some(format!("Forked from {}", session_id)),
                    transcript_path: None,
                };
                if let Err(e) = db.create_session(&new_session) {
                    return json!({"status": "error", "message": format!("{e}")});
                }

                // Copy conversation
                let copied = match db.fork_conversation(session_id, &new_session_id, fork_index) {
                    Ok(c) => c,
                    Err(e) => return json!({"status": "error", "message": format!("{e}")}),
                };

                // Record fork
                let fork = flowforge_core::SessionFork {
                    id: uuid::Uuid::new_v4().to_string(),
                    source_session_id: session_id.to_string(),
                    target_session_id: new_session_id.clone(),
                    fork_message_index: fork_index,
                    checkpoint_id,
                    reason: reason.map(|s| s.to_string()),
                    created_at: now,
                };
                if let Err(e) = db.create_session_fork(&fork) {
                    return json!({"status": "error", "message": format!("{e}")});
                }

                json!({
                    "status": "ok",
                    "fork_id": fork.id,
                    "new_session_id": new_session_id,
                    "fork_message_index": fork_index,
                    "messages_copied": copied,
                })
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn session_forks(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::open_db() {
            Ok(db) => match db.get_session_forks(session_id) {
                Ok(forks) => {
                    let entries: Vec<Value> = forks
                        .iter()
                        .map(|f| {
                            json!({
                                "id": f.id,
                                "source_session_id": f.source_session_id,
                                "target_session_id": f.target_session_id,
                                "fork_message_index": f.fork_message_index,
                                "checkpoint_id": f.checkpoint_id,
                                "reason": f.reason,
                                "created_at": f.created_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "forks": entries})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn session_lineage(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::open_db() {
            Ok(db) => match db.get_session_lineage(session_id) {
                Ok(lineage) => {
                    let entries: Vec<Value> = lineage
                        .iter()
                        .map(|f| {
                            json!({
                                "source_session_id": f.source_session_id,
                                "target_session_id": f.target_session_id,
                                "fork_message_index": f.fork_message_index,
                                "created_at": f.created_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "lineage": entries, "depth": entries.len()})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Mailbox tool implementations ---

    fn mailbox_send(&self, params: &Value) -> Value {
        let work_item_id = params
            .get("work_item_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let from_session_id = params
            .get("from_session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let from_agent_name = params
            .get("from_agent_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let to_session_id = params.get("to_session_id").and_then(|v| v.as_str());
        let to_agent_name = params.get("to_agent_name").and_then(|v| v.as_str());
        let content = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let message_type = params
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text");
        let priority = params.get("priority").and_then(|v| v.as_i64()).unwrap_or(2) as i32;

        match Self::open_db() {
            Ok(db) => {
                let msg = flowforge_core::MailboxMessage {
                    id: 0,
                    work_item_id: work_item_id.to_string(),
                    from_session_id: from_session_id.to_string(),
                    from_agent_name: from_agent_name.to_string(),
                    to_session_id: to_session_id.map(|s| s.to_string()),
                    to_agent_name: to_agent_name.map(|s| s.to_string()),
                    message_type: message_type.to_string(),
                    content: content.to_string(),
                    priority,
                    read_at: None,
                    created_at: chrono::Utc::now(),
                    metadata: None,
                };
                match db.send_mailbox_message(&msg) {
                    Ok(id) => json!({"status": "ok", "message_id": id}),
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn mailbox_read(&self, params: &Value) -> Value {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::open_db() {
            Ok(db) => match db.get_unread_messages(session_id) {
                Ok(msgs) => {
                    let entries: Vec<Value> = msgs
                        .iter()
                        .map(|m| {
                            json!({
                                "id": m.id,
                                "from_agent_name": m.from_agent_name,
                                "to_agent_name": m.to_agent_name,
                                "message_type": m.message_type,
                                "content": m.content,
                                "priority": m.priority,
                                "created_at": m.created_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    let count = entries.len();
                    // Mark as read
                    let _ = db.mark_messages_read(session_id);
                    json!({"status": "ok", "messages": entries, "count": count})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn mailbox_history(&self, params: &Value) -> Value {
        let work_item_id = params
            .get("work_item_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        match Self::open_db() {
            Ok(db) => match db.get_mailbox_history(work_item_id, limit) {
                Ok(msgs) => {
                    let entries: Vec<Value> = msgs
                        .iter()
                        .map(|m| {
                            json!({
                                "id": m.id,
                                "from_agent_name": m.from_agent_name,
                                "to_agent_name": m.to_agent_name,
                                "message_type": m.message_type,
                                "content": m.content,
                                "priority": m.priority,
                                "read_at": m.read_at.map(|t| t.to_rfc3339()),
                                "created_at": m.created_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "messages": entries})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn mailbox_agents(&self, params: &Value) -> Value {
        let work_item_id = params
            .get("work_item_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match Self::open_db() {
            Ok(db) => match db.get_agents_on_work_item(work_item_id) {
                Ok(agents) => {
                    let entries: Vec<Value> = agents
                        .iter()
                        .map(|a| {
                            json!({
                                "agent_id": a.agent_id,
                                "agent_type": a.agent_type,
                                "status": a.status.to_string(),
                                "started_at": a.started_at.to_rfc3339(),
                            })
                        })
                        .collect();
                    json!({"status": "ok", "agents": entries, "count": entries.len()})
                }
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    fn work_log(&self, params: &Value) -> Value {
        let work_item_id = params.get("work_item_id").and_then(|v| v.as_str());
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        match Self::open_db() {
            Ok(db) => {
                let events = if let Some(id) = work_item_id {
                    db.get_work_events(id, limit)
                } else {
                    db.get_recent_work_events(limit)
                };

                match events {
                    Ok(events) => {
                        let entries: Vec<Value> = events
                            .iter()
                            .map(|e| {
                                json!({
                                    "work_item_id": e.work_item_id,
                                    "event_type": e.event_type,
                                    "old_value": e.old_value,
                                    "new_value": e.new_value,
                                    "actor": e.actor,
                                    "timestamp": e.timestamp.to_rfc3339(),
                                })
                            })
                            .collect();
                        json!({"status": "ok", "events": entries})
                    }
                    Err(e) => json!({"status": "error", "message": format!("{e}")}),
                }
            }
            Err(e) => {
                json!({"status": "error", "message": format!("Failed to open database: {e}")})
            }
        }
    }

    // --- Guidance tool implementations ---

    fn guidance_rules(&self, _params: &Value) -> Value {
        match Self::load_config() {
            Ok(config) => {
                let g = &config.guidance;
                let mut rules = vec![];
                if g.destructive_ops_gate {
                    rules.push(json!({"name": "destructive_ops", "enabled": true, "description": "Block dangerous commands"}));
                }
                if g.file_scope_gate {
                    rules.push(json!({"name": "file_scope", "enabled": true, "description": "Block writes to protected paths"}));
                }
                if g.diff_size_gate {
                    rules.push(json!({"name": "diff_size", "enabled": true, "max_lines": g.max_diff_lines, "description": "Ask for large diffs"}));
                }
                if g.secrets_gate {
                    rules.push(json!({"name": "secrets", "enabled": true, "description": "Detect API keys and secrets"}));
                }
                for rule in &g.custom_rules {
                    rules.push(json!({
                        "name": rule.id,
                        "enabled": rule.enabled,
                        "pattern": rule.pattern,
                        "action": format!("{}", rule.action),
                        "scope": format!("{}", rule.scope),
                        "description": rule.description
                    }));
                }
                json!({
                    "status": "ok",
                    "gates": rules,
                    "trust_config": {
                        "initial": g.trust_initial_score,
                        "ask_threshold": g.trust_ask_threshold,
                        "decay_per_hour": g.trust_decay_per_hour
                    }
                })
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn guidance_trust(&self, params: &Value) -> Value {
        let session_id = params.get("session_id").and_then(|v| v.as_str());
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let sid = match session_id {
                Some(s) => s.to_string(),
                None => db
                    .get_current_session()?
                    .map(|s| s.id)
                    .unwrap_or_else(|| "unknown".to_string()),
            };
            Ok((db, sid))
        }) {
            Ok((db, sid)) => match db.get_trust_score(&sid) {
                Ok(Some(t)) => json!({
                    "status": "ok",
                    "session_id": sid,
                    "score": t.score,
                    "total_checks": t.total_checks,
                    "denials": t.denials,
                    "asks": t.asks,
                    "allows": t.allows
                }),
                Ok(None) => json!({
                    "status": "ok",
                    "session_id": sid,
                    "score": null,
                    "message": "no trust score found"
                }),
                Err(e) => json!({"status": "error", "message": format!("{e}")}),
            },
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn guidance_audit(&self, params: &Value) -> Value {
        let session_id = params.get("session_id").and_then(|v| v.as_str());
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let sid = match session_id {
                Some(s) => s.to_string(),
                None => db
                    .get_current_session()?
                    .map(|s| s.id)
                    .unwrap_or_else(|| "unknown".to_string()),
            };
            let decisions = db.get_gate_decisions(&sid, limit)?;
            Ok(decisions)
        }) {
            Ok(decisions) => {
                let entries: Vec<Value> = decisions
                    .iter()
                    .map(|d| {
                        json!({
                            "gate_name": d.gate_name,
                            "tool_name": d.tool_name,
                            "action": format!("{}", d.action),
                            "reason": d.reason,
                            "risk_level": format!("{}", d.risk_level),
                            "trust_before": d.trust_before,
                            "trust_after": d.trust_after,
                            "timestamp": d.timestamp.to_rfc3339()
                        })
                    })
                    .collect();
                json!({"status": "ok", "count": entries.len(), "entries": entries})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    // --- Work-stealing tool implementations ---

    fn work_claim(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let session_id = db
                .get_current_session()?
                .map(|s| s.id)
                .unwrap_or_else(|| "unknown".to_string());
            let claimed = db.claim_work_item(id, &session_id)?;
            Ok(claimed)
        }) {
            Ok(claimed) => json!({"status": "ok", "claimed": claimed, "id": id}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn work_release(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            db.release_work_item(id)?;
            Ok(())
        }) {
            Ok(()) => json!({"status": "ok", "id": id}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn work_steal(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str());
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let session_id = db
                .get_current_session()?
                .map(|s| s.id)
                .unwrap_or_else(|| "unknown".to_string());
            let target = match id {
                Some(id) => id.to_string(),
                None => {
                    let items = db.get_stealable_items(1)?;
                    items.first().map(|i| i.id.clone()).unwrap_or_default()
                }
            };
            if target.is_empty() {
                return Ok((false, String::new()));
            }
            let stolen = db.steal_work_item(&target, &session_id)?;
            Ok((stolen, target))
        }) {
            Ok((stolen, id)) => json!({"status": "ok", "stolen": stolen, "id": id}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn work_heartbeat(&self, params: &Value) -> Value {
        let progress = params
            .get("progress")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32);
        let id = params.get("id").and_then(|v| v.as_str());
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let session_id = db
                .get_current_session()?
                .map(|s| s.id)
                .unwrap_or_else(|| "unknown".to_string());
            let updated = db.update_heartbeat(&session_id)?;
            if let (Some(id), Some(progress)) = (id, progress) {
                db.update_progress(id, progress)?;
            }
            Ok(updated)
        }) {
            Ok(updated) => json!({"status": "ok", "items_updated": updated}),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    // --- Plugin tool implementations ---

    fn plugin_list(&self, _params: &Value) -> Value {
        match Self::load_config().and_then(|config| {
            let plugins = flowforge_core::plugin::load_all_plugins(&config.plugins)?;
            Ok((plugins, config))
        }) {
            Ok((plugins, config)) => {
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
                json!({"status": "ok", "count": entries.len(), "plugins": entries})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn plugin_info(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        match Self::load_config().and_then(|config| {
            let plugins = flowforge_core::plugin::load_all_plugins(&config.plugins)?;
            Ok((plugins, config))
        }) {
            Ok((plugins, config)) => {
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
                        json!({
                            "status": "ok",
                            "name": name,
                            "version": p.manifest.plugin.version,
                            "description": p.manifest.plugin.description,
                            "enabled": !disabled,
                            "tools": tools,
                            "hooks": hooks
                        })
                    }
                    None => {
                        json!({"status": "error", "message": format!("plugin '{name}' not found")})
                    }
                }
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    // --- Trajectory tool implementations ---

    fn trajectory_list(&self, params: &Value) -> Value {
        let session_id = params.get("session_id").and_then(|v| v.as_str());
        let status = params.get("status").and_then(|v| v.as_str());
        let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            db.list_trajectories(session_id, status, limit)
        }) {
            Ok(trajectories) => {
                let entries: Vec<Value> = trajectories
                    .iter()
                    .map(|t| {
                        json!({
                            "id": t.id,
                            "session_id": t.session_id,
                            "status": format!("{}", t.status),
                            "verdict": t.verdict.as_ref().map(|v| format!("{v}")),
                            "confidence": t.confidence,
                            "task_description": t.task_description,
                            "started_at": t.started_at.to_rfc3339()
                        })
                    })
                    .collect();
                json!({"status": "ok", "count": entries.len(), "trajectories": entries})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn trajectory_get(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let trajectory = db.get_trajectory(id)?;
            let steps = db.get_trajectory_steps(id)?;
            let ratio = db.trajectory_success_ratio(id)?;
            Ok((trajectory, steps, ratio))
        }) {
            Ok((Some(t), steps, ratio)) => {
                let step_entries: Vec<Value> = steps
                    .iter()
                    .map(|s| {
                        json!({
                            "step_index": s.step_index,
                            "tool_name": s.tool_name,
                            "outcome": format!("{}", s.outcome),
                            "duration_ms": s.duration_ms,
                            "timestamp": s.timestamp.to_rfc3339()
                        })
                    })
                    .collect();
                json!({
                    "status": "ok",
                    "id": t.id,
                    "session_id": t.session_id,
                    "status_field": format!("{}", t.status),
                    "verdict": t.verdict.as_ref().map(|v| format!("{v}")),
                    "confidence": t.confidence,
                    "task_description": t.task_description,
                    "success_ratio": ratio,
                    "steps": step_entries
                })
            }
            Ok((None, _, _)) => {
                json!({"status": "error", "message": "trajectory not found"})
            }
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }

    fn trajectory_judge(&self, params: &Value) -> Value {
        let id = params.get("id").and_then(|v| v.as_str()).unwrap_or("");
        match Self::load_config().and_then(|config| {
            let db = MemoryDb::open(&config.db_path())?;
            let judge = flowforge_memory::trajectory::TrajectoryJudge::new(&db, &config.patterns);
            let result = judge.judge(id)?;
            Ok(result)
        }) {
            Ok(result) => json!({
                "status": "ok",
                "verdict": format!("{}", result.verdict),
                "confidence": result.confidence,
                "reason": result.reason
            }),
            Err(e) => json!({"status": "error", "message": format!("{e}")}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_has_48_tools() {
        let registry = ToolRegistry::new();
        assert_eq!(registry.list().len(), 48);
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
        // With real backend, this may return error (no DB) or ok
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
}
