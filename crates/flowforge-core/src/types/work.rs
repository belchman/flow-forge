use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Work item status with compiler-enforced valid values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl std::fmt::Display for WorkStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

impl std::str::FromStr for WorkStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "pending" | "open" | "backlog" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" | "closed" | "done" => Ok(Self::Completed),
            "blocked" => Ok(Self::Blocked),
            other => Err(format!("unknown work status: {other}")),
        }
    }
}

/// Work item for tracking tasks/epics/bugs across backends
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    #[serde(default)]
    pub status: WorkStatus,
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
fn default_work_priority() -> i32 {
    2
}

/// Work event for audit trail
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub status: Option<WorkStatus>,
    pub item_type: Option<String>,
    pub backend: Option<String>,
    pub assignee: Option<String>,
    pub parent_id: Option<String>,
    pub limit: Option<usize>,
    pub stealable: Option<bool>,
    pub claimed_by: Option<String>,
}
