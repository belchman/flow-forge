use serde::{Deserialize, Serialize};

/// Agent definition loaded from markdown with YAML frontmatter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default = "default_priority")]
    pub priority: Priority,
    #[serde(default)]
    pub color: Option<String>,
    /// Full markdown body (after frontmatter)
    #[serde(skip)]
    pub body: String,
    /// Where this agent was loaded from
    #[serde(skip)]
    pub source: AgentSource,
}

fn default_priority() -> Priority {
    Priority::Normal
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Critical,
    High,
    Normal,
    Low,
}

impl Priority {
    pub fn boost(&self) -> f64 {
        match self {
            Priority::Critical => 1.0,
            Priority::High => 0.75,
            Priority::Normal => 0.5,
            Priority::Low => 0.25,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentSource {
    #[default]
    BuiltIn,
    Global,
    Project,
    Plugin(String),
}

/// Session context for context-aware routing
#[derive(Debug, Clone, Default)]
pub struct RoutingContext {
    /// File extensions from recent edits (e.g. ["rs", "toml"])
    pub active_file_extensions: Vec<String>,
    /// Last N tool names from trajectory steps
    pub recent_tools: Vec<String>,
    /// Currently running agent type
    pub active_agent: Option<String>,
    /// Active work item type (e.g. "bug", "task", "feature")
    pub active_work_type: Option<String>,
    /// Number of edits in the current session
    pub session_edit_count: u64,
}

/// Routing result: which agent should handle a task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingResult {
    pub agent_name: String,
    pub confidence: f64,
    pub breakdown: RoutingBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingBreakdown {
    pub pattern_score: f64,
    pub capability_score: f64,
    pub learned_score: f64,
    pub priority_score: f64,
    #[serde(default)]
    pub context_score: f64,
}

/// A capability discovered through observing agent success/failure patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredCapability {
    pub agent_name: String,
    pub capability: String,
    pub task_pattern: String,
    pub success_count: u64,
    pub failure_count: u64,
    pub confidence: f64,
}
