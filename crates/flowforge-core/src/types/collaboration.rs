use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::sessions::TeamMemberState;

/// Conversation message persisted from transcript
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
