use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Short-term pattern
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Which tier a pattern match came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternTier {
    Short,
    Long,
}

/// A unified pattern match result from either tier with similarity score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternMatch {
    pub id: String,
    pub content: String,
    pub category: String,
    pub confidence: f64,
    pub usage_count: u32,
    pub tier: PatternTier,
    pub similarity: f32,
}

/// Topic cluster for pattern vectors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCluster {
    pub id: i64,
    pub centroid: Vec<f32>,
    pub member_count: i64,
    pub p95_distance: f64,
    pub avg_confidence: f64,
    pub created_at: DateTime<Utc>,
    pub last_recomputed: DateTime<Utc>,
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

/// Context injection record for impact tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextInjection {
    pub id: i64,
    pub session_id: String,
    pub trajectory_id: Option<String>,
    pub injection_type: String,
    pub reference_id: String,
    pub similarity: Option<f64>,
    pub timestamp: String,
    /// Optional JSON metadata (e.g. serialized RoutingBreakdown for routing injections)
    #[serde(default)]
    pub metadata: Option<String>,
}
