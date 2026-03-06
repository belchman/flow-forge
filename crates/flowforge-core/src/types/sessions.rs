use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Session info tracked across hooks
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
