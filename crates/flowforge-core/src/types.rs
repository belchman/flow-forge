use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Guidance Control Plane ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Critical,
    High,
    Medium,
    Low,
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "critical"),
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
        }
    }
}

impl std::str::FromStr for RiskLevel {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "critical" => Ok(Self::Critical),
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            other => Err(format!("unknown risk level: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateAction {
    Deny,
    Ask,
    Allow,
}

impl std::fmt::Display for GateAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Deny => write!(f, "deny"),
            Self::Ask => write!(f, "ask"),
            Self::Allow => write!(f, "allow"),
        }
    }
}

impl std::str::FromStr for GateAction {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "deny" => Ok(Self::Deny),
            "ask" => Ok(Self::Ask),
            "allow" => Ok(Self::Allow),
            other => Err(format!("unknown gate action: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleScope {
    Tool,
    Command,
    File,
}

impl std::fmt::Display for RuleScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tool => write!(f, "tool"),
            Self::Command => write!(f, "command"),
            Self::File => write!(f, "file"),
        }
    }
}

impl std::str::FromStr for RuleScope {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "tool" => Ok(Self::Tool),
            "command" => Ok(Self::Command),
            "file" => Ok(Self::File),
            other => Err(format!("unknown rule scope: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceRule {
    pub id: String,
    pub pattern: String,
    pub action: GateAction,
    pub scope: RuleScope,
    pub risk_level: RiskLevel,
    pub description: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateDecision {
    pub id: i64,
    pub session_id: String,
    pub rule_id: Option<String>,
    pub gate_name: String,
    pub tool_name: String,
    pub action: GateAction,
    pub reason: String,
    pub risk_level: RiskLevel,
    pub trust_before: f64,
    pub trust_after: f64,
    pub timestamp: DateTime<Utc>,
    pub hash: String,
    pub prev_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    pub session_id: String,
    pub score: f64,
    pub total_checks: u64,
    pub denials: u64,
    pub asks: u64,
    pub allows: u64,
    pub last_updated: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

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
    #[serde(default)]
    pub transcript_path: Option<String>,
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

/// Agent session tracked in the database (first-class sub-agent tracking)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub parent_session_id: String,
    pub agent_id: String,
    pub agent_type: String,
    pub status: AgentSessionStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub edits: u64,
    pub commands: u64,
    pub task_id: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentSessionStatus {
    Active,
    Idle,
    Completed,
    Error,
}

impl std::fmt::Display for AgentSessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Idle => write!(f, "idle"),
            Self::Completed => write!(f, "completed"),
            Self::Error => write!(f, "error"),
        }
    }
}

impl std::str::FromStr for AgentSessionStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "completed" => Ok(Self::Completed),
            "error" => Ok(Self::Error),
            other => Err(format!("unknown agent session status: {other}")),
        }
    }
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

/// Work item for tracking tasks/epics/bugs across backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: String,
    #[serde(default)]
    pub external_id: Option<String>,
    pub backend: String,
    #[serde(default = "default_item_type")]
    pub item_type: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_work_status")]
    pub status: String,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default = "default_work_priority")]
    pub priority: i32,
    #[serde(default)]
    pub labels: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub metadata: Option<String>,
    // Work-stealing fields
    #[serde(default)]
    pub claimed_by: Option<String>,
    #[serde(default)]
    pub claimed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_heartbeat: Option<DateTime<Utc>>,
    #[serde(default)]
    pub progress: i32,
    #[serde(default)]
    pub stealable: bool,
}

fn default_item_type() -> String {
    "task".to_string()
}
fn default_work_status() -> String {
    "pending".to_string()
}
fn default_work_priority() -> i32 {
    2
}

/// Work event for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkEvent {
    pub id: i64,
    pub work_item_id: String,
    pub event_type: String,
    #[serde(default)]
    pub old_value: Option<String>,
    #[serde(default)]
    pub new_value: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Filter for querying work items
#[derive(Debug, Clone, Default)]
pub struct WorkFilter {
    pub status: Option<String>,
    pub item_type: Option<String>,
    pub backend: Option<String>,
    pub assignee: Option<String>,
    pub parent_id: Option<String>,
    pub limit: Option<usize>,
    pub stealable: Option<bool>,
    pub claimed_by: Option<String>,
}

/// Conversation message persisted from transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub id: i64,
    pub session_id: String,
    pub message_index: u32,
    pub message_type: String,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub message_id: Option<String>,
    pub parent_uuid: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub metadata: Option<String>,
    pub source: String,
}

/// Named checkpoint at a message index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub message_index: u32,
    pub description: Option<String>,
    pub git_ref: Option<String>,
    pub created_at: DateTime<Utc>,
    pub metadata: Option<String>,
}

/// Record of a session fork
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFork {
    pub id: String,
    pub source_session_id: String,
    pub target_session_id: String,
    pub fork_message_index: u32,
    pub checkpoint_id: Option<String>,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Co-agent mailbox message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage {
    pub id: i64,
    pub work_item_id: String,
    pub from_session_id: String,
    pub from_agent_name: String,
    pub to_session_id: Option<String>,
    pub to_agent_name: Option<String>,
    pub message_type: String,
    pub content: String,
    pub priority: i32,
    pub read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub metadata: Option<String>,
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
