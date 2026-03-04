use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main FlowForge configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowForgeConfig {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub memory: MemoryConfig,
    #[serde(default)]
    pub agents: AgentsConfig,
    #[serde(default)]
    pub routing: RoutingConfig,
    #[serde(default)]
    pub patterns: PatternsConfig,
    #[serde(default)]
    pub tmux: TmuxConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub work_tracking: WorkTrackingConfig,
    #[serde(default)]
    pub guidance: GuidanceConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_true")]
    pub telemetry: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            telemetry: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_db_name")]
    pub db_name: String,
    #[serde(default = "default_hnsw_m")]
    pub hnsw_m: usize,
    #[serde(default = "default_hnsw_ef_construction")]
    pub hnsw_ef_construction: usize,
    #[serde(default = "default_hnsw_ef_search")]
    pub hnsw_ef_search: usize,
    #[serde(default = "default_embedding_dim")]
    pub embedding_dim: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_name: default_db_name(),
            hnsw_m: default_hnsw_m(),
            hnsw_ef_construction: default_hnsw_ef_construction(),
            hnsw_ef_search: default_hnsw_ef_search(),
            embedding_dim: default_embedding_dim(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentsConfig {
    #[serde(default = "default_true")]
    pub load_builtin: bool,
    #[serde(default = "default_true")]
    pub load_global: bool,
    #[serde(default = "default_true")]
    pub load_project: bool,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            load_builtin: true,
            load_global: true,
            load_project: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(default = "default_pattern_weight")]
    pub pattern_weight: f64,
    #[serde(default = "default_capability_weight")]
    pub capability_weight: f64,
    #[serde(default = "default_learned_weight")]
    pub learned_weight: f64,
    #[serde(default = "default_priority_weight")]
    pub priority_weight: f64,
    #[serde(default = "default_context_weight")]
    pub context_weight: f64,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            pattern_weight: default_pattern_weight(),
            capability_weight: default_capability_weight(),
            learned_weight: default_learned_weight(),
            priority_weight: default_priority_weight(),
            context_weight: default_context_weight(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternsConfig {
    #[serde(default = "default_short_term_max")]
    pub short_term_max: usize,
    #[serde(default = "default_short_term_ttl_hours")]
    pub short_term_ttl_hours: u64,
    #[serde(default = "default_long_term_max")]
    pub long_term_max: usize,
    #[serde(default = "default_promotion_usage")]
    pub promotion_min_usage: u32,
    #[serde(default = "default_promotion_confidence")]
    pub promotion_min_confidence: f64,
    #[serde(default = "default_decay_rate")]
    pub decay_rate_per_hour: f64,
    #[serde(default = "default_dedup_threshold")]
    pub dedup_similarity_threshold: f64,
    #[serde(default = "default_trajectory_max")]
    pub trajectory_max: usize,
    #[serde(default = "default_trajectory_prune_days")]
    pub trajectory_prune_days: u64,
    #[serde(default = "default_trajectory_merge_threshold")]
    pub trajectory_merge_threshold: f64,
    #[serde(default = "default_true")]
    pub semantic_embeddings: bool,
    #[serde(default = "default_clustering_min_points")]
    pub clustering_min_points: usize,
    #[serde(default = "default_clustering_epsilon")]
    pub clustering_epsilon: f64,
    #[serde(default = "default_outlier_recluster_threshold")]
    pub outlier_recluster_threshold: usize,
    #[serde(default = "default_cluster_decay_active_factor")]
    pub cluster_decay_active_factor: f64,
    #[serde(default = "default_cluster_decay_isolated_factor")]
    pub cluster_decay_isolated_factor: f64,
}

impl Default for PatternsConfig {
    fn default() -> Self {
        Self {
            short_term_max: default_short_term_max(),
            short_term_ttl_hours: default_short_term_ttl_hours(),
            long_term_max: default_long_term_max(),
            promotion_min_usage: default_promotion_usage(),
            promotion_min_confidence: default_promotion_confidence(),
            decay_rate_per_hour: default_decay_rate(),
            dedup_similarity_threshold: default_dedup_threshold(),
            trajectory_max: default_trajectory_max(),
            trajectory_prune_days: default_trajectory_prune_days(),
            trajectory_merge_threshold: default_trajectory_merge_threshold(),
            semantic_embeddings: true,
            clustering_min_points: default_clustering_min_points(),
            clustering_epsilon: default_clustering_epsilon(),
            outlier_recluster_threshold: default_outlier_recluster_threshold(),
            cluster_decay_active_factor: default_cluster_decay_active_factor(),
            cluster_decay_isolated_factor: default_cluster_decay_isolated_factor(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxConfig {
    #[serde(default = "default_tmux_session")]
    pub session_name: String,
    #[serde(default = "default_true")]
    pub auto_start: bool,
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_ms: u64,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            session_name: default_tmux_session(),
            auto_start: true,
            refresh_interval_ms: default_refresh_interval(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksConfig {
    #[serde(default = "default_true")]
    pub bash_validation: bool,
    #[serde(default = "default_true")]
    pub edit_tracking: bool,
    #[serde(default = "default_true")]
    pub routing: bool,
    #[serde(default = "default_true")]
    pub learning: bool,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            bash_validation: true,
            edit_tracking: true,
            routing: true,
            learning: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkTrackingConfig {
    #[serde(default = "default_backend_auto")]
    pub backend: String,
    #[serde(default = "default_true")]
    pub log_all: bool,
    #[serde(default)]
    pub require_task: bool,
    #[serde(default)]
    pub kanbus: KanbusSyncConfig,
    #[serde(default)]
    pub beads: BeadsSyncConfig,
    #[serde(default)]
    pub claude_tasks: ClaudeTasksSyncConfig,
    #[serde(default)]
    pub work_stealing: WorkStealingConfig,
}

impl Default for WorkTrackingConfig {
    fn default() -> Self {
        Self {
            backend: default_backend_auto(),
            log_all: true,
            require_task: false,
            kanbus: KanbusSyncConfig::default(),
            beads: BeadsSyncConfig::default(),
            claude_tasks: ClaudeTasksSyncConfig::default(),
            work_stealing: WorkStealingConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkStealingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_stale_threshold")]
    pub stale_threshold_mins: u64,
    #[serde(default = "default_abandon_threshold")]
    pub abandon_threshold_mins: u64,
    #[serde(default = "default_stale_min_progress")]
    pub stale_min_progress: i32,
}

impl Default for WorkStealingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            stale_threshold_mins: default_stale_threshold(),
            abandon_threshold_mins: default_abandon_threshold(),
            stale_min_progress: default_stale_min_progress(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KanbusSyncConfig {
    #[serde(default)]
    pub project_key: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BeadsSyncConfig {}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeTasksSyncConfig {
    #[serde(default)]
    pub list_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuidanceConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub destructive_ops_gate: bool,
    #[serde(default = "default_true")]
    pub file_scope_gate: bool,
    #[serde(default = "default_true")]
    pub diff_size_gate: bool,
    #[serde(default = "default_true")]
    pub secrets_gate: bool,
    #[serde(default = "default_max_diff_lines")]
    pub max_diff_lines: usize,
    #[serde(default = "default_trust_initial")]
    pub trust_initial_score: f64,
    #[serde(default = "default_trust_ask_threshold")]
    pub trust_ask_threshold: f64,
    #[serde(default = "default_trust_decay")]
    pub trust_decay_per_hour: f64,
    #[serde(default)]
    pub protected_paths: Vec<String>,
    #[serde(default)]
    pub custom_rules: Vec<crate::types::GuidanceRule>,
}

impl Default for GuidanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            destructive_ops_gate: true,
            file_scope_gate: true,
            diff_size_gate: true,
            secrets_gate: true,
            max_diff_lines: default_max_diff_lines(),
            trust_initial_score: default_trust_initial(),
            trust_ask_threshold: default_trust_ask_threshold(),
            trust_decay_per_hour: default_trust_decay(),
            protected_paths: vec![],
            custom_rules: vec![],
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginsConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
}

fn default_backend_auto() -> String {
    "auto".to_string()
}
fn default_max_diff_lines() -> usize {
    500
}
fn default_trust_initial() -> f64 {
    0.5
}
fn default_trust_ask_threshold() -> f64 {
    0.8
}
fn default_trust_decay() -> f64 {
    0.02
}
fn default_stale_threshold() -> u64 {
    30
}
fn default_abandon_threshold() -> u64 {
    60
}
fn default_stale_min_progress() -> i32 {
    25
}
fn default_trajectory_max() -> usize {
    5000
}
fn default_trajectory_prune_days() -> u64 {
    7
}
fn default_trajectory_merge_threshold() -> f64 {
    0.9
}

// Default value functions
fn default_log_level() -> String {
    "info".to_string()
}
fn default_true() -> bool {
    true
}
fn default_db_name() -> String {
    "flowforge.db".to_string()
}
fn default_hnsw_m() -> usize {
    16
}
fn default_hnsw_ef_construction() -> usize {
    100
}
fn default_hnsw_ef_search() -> usize {
    50
}
fn default_embedding_dim() -> usize {
    128
}
fn default_pattern_weight() -> f64 {
    0.35
}
fn default_capability_weight() -> f64 {
    0.25
}
fn default_learned_weight() -> f64 {
    0.20
}
fn default_priority_weight() -> f64 {
    0.05
}
fn default_context_weight() -> f64 {
    0.15
}
fn default_short_term_max() -> usize {
    500
}
fn default_short_term_ttl_hours() -> u64 {
    24
}
fn default_long_term_max() -> usize {
    2000
}
fn default_promotion_usage() -> u32 {
    3
}
fn default_promotion_confidence() -> f64 {
    0.6
}
fn default_decay_rate() -> f64 {
    0.005
}
fn default_dedup_threshold() -> f64 {
    0.88
}
fn default_clustering_min_points() -> usize {
    3
}
fn default_clustering_epsilon() -> f64 {
    0.3
}
fn default_outlier_recluster_threshold() -> usize {
    50
}
fn default_cluster_decay_active_factor() -> f64 {
    0.5
}
fn default_cluster_decay_isolated_factor() -> f64 {
    2.0
}
fn default_tmux_session() -> String {
    "flowforge".to_string()
}
fn default_refresh_interval() -> u64 {
    1000
}

impl FlowForgeConfig {
    /// Load config from a TOML file, falling back to defaults
    pub fn load(path: &Path) -> crate::Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let config: Self = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Save config to a TOML file
    pub fn save(&self, path: &Path) -> crate::Result<()> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get project .flowforge directory
    pub fn project_dir() -> PathBuf {
        PathBuf::from(".flowforge")
    }

    /// Get global ~/.flowforge directory
    pub fn global_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".flowforge")
    }

    /// Get the database path for a project
    pub fn db_path(&self) -> PathBuf {
        Self::project_dir().join(&self.memory.db_name)
    }

    /// Get the config file path for a project
    pub fn config_path() -> PathBuf {
        Self::project_dir().join("config.toml")
    }

    /// Get the tmux state file path
    pub fn tmux_state_path() -> PathBuf {
        Self::project_dir().join("tmux-state.json")
    }

    /// Get the plugins directory
    pub fn plugins_dir() -> PathBuf {
        Self::project_dir().join("plugins")
    }
}
