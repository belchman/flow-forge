//! Trajectory types for recording execution paths.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrajectoryStatus {
    Recording,
    Completed,
    Failed,
    Judged,
}

impl std::fmt::Display for TrajectoryStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recording => write!(f, "recording"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Judged => write!(f, "judged"),
        }
    }
}

impl std::str::FromStr for TrajectoryStatus {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "recording" => Ok(Self::Recording),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "judged" => Ok(Self::Judged),
            other => Err(format!("unknown trajectory status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrajectoryVerdict {
    Success,
    Partial,
    Failure,
}

impl std::fmt::Display for TrajectoryVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::Partial => write!(f, "partial"),
            Self::Failure => write!(f, "failure"),
        }
    }
}

impl std::str::FromStr for TrajectoryVerdict {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "success" => Ok(Self::Success),
            "partial" => Ok(Self::Partial),
            "failure" => Ok(Self::Failure),
            other => Err(format!("unknown trajectory verdict: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StepOutcome {
    Success,
    Failure,
    Skipped,
}

impl std::fmt::Display for StepOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Success => write!(f, "success"),
            Self::Failure => write!(f, "failure"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

impl std::str::FromStr for StepOutcome {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "success" => Ok(Self::Success),
            "failure" => Ok(Self::Failure),
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("unknown step outcome: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    pub id: String,
    pub session_id: String,
    pub work_item_id: Option<String>,
    pub agent_name: Option<String>,
    pub task_description: Option<String>,
    pub status: TrajectoryStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub verdict: Option<TrajectoryVerdict>,
    pub confidence: Option<f64>,
    pub metadata: Option<String>,
    pub embedding_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStep {
    pub id: i64,
    pub trajectory_id: String,
    pub step_index: i32,
    pub tool_name: String,
    pub tool_input_hash: Option<String>,
    pub outcome: StepOutcome,
    pub duration_ms: Option<i64>,
    pub timestamp: DateTime<Utc>,
}
