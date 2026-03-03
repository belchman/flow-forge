use chrono::{DateTime, Utc};
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
}

/// Session info tracked across hooks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub cwd: String,
    pub edits: u64,
    pub commands: u64,
    pub summary: Option<String>,
}

/// Edit record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditRecord {
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub file_path: String,
    pub operation: String,
    pub file_extension: Option<String>,
}

/// Short-term pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortTermPattern {
    pub id: String,
    pub content: String,
    pub category: String,
    pub confidence: f64,
    pub usage_count: u32,
    pub created_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub embedding_id: Option<i64>,
}

/// Long-term pattern (promoted from short-term)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LongTermPattern {
    pub id: String,
    pub content: String,
    pub category: String,
    pub confidence: f64,
    pub usage_count: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub created_at: DateTime<Utc>,
    pub promoted_at: DateTime<Utc>,
    pub last_used: DateTime<Utc>,
    pub embedding_id: Option<i64>,
}

/// Routing weight for learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingWeight {
    pub task_pattern: String,
    pub agent_name: String,
    pub weight: f64,
    pub successes: u32,
    pub failures: u32,
    pub updated_at: DateTime<Utc>,
}

/// HNSW entry stored in SQLite
#[derive(Debug, Clone)]
pub struct HnswEntry {
    pub id: i64,
    pub source_type: String,
    pub source_id: String,
    pub vector: Vec<f32>,
    pub created_at: DateTime<Utc>,
}

/// Team member state (for tmux display)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMemberState {
    pub agent_id: String,
    pub agent_type: String,
    pub status: TeamMemberStatus,
    pub current_task: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamMemberStatus {
    Active,
    Idle,
    Completed,
    Error,
}

/// Tmux display state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxState {
    pub session_name: String,
    pub team_name: Option<String>,
    pub members: Vec<TeamMemberState>,
    pub recent_events: Vec<String>,
    pub memory_count: u64,
    pub pattern_count: u64,
    pub updated_at: DateTime<Utc>,
}
