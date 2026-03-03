use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main FlowForge configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            pattern_weight: default_pattern_weight(),
            capability_weight: default_capability_weight(),
            learned_weight: default_learned_weight(),
            priority_weight: default_priority_weight(),
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

// Default value functions
fn default_log_level() -> String { "info".to_string() }
fn default_true() -> bool { true }
fn default_db_name() -> String { "flowforge.db".to_string() }
fn default_hnsw_m() -> usize { 16 }
fn default_hnsw_ef_construction() -> usize { 100 }
fn default_hnsw_ef_search() -> usize { 50 }
fn default_embedding_dim() -> usize { 128 }
fn default_pattern_weight() -> f64 { 0.40 }
fn default_capability_weight() -> f64 { 0.30 }
fn default_learned_weight() -> f64 { 0.25 }
fn default_priority_weight() -> f64 { 0.05 }
fn default_short_term_max() -> usize { 500 }
fn default_short_term_ttl_hours() -> u64 { 24 }
fn default_long_term_max() -> usize { 2000 }
fn default_promotion_usage() -> u32 { 3 }
fn default_promotion_confidence() -> f64 { 0.6 }
fn default_decay_rate() -> f64 { 0.005 }
fn default_dedup_threshold() -> f64 { 0.95 }
fn default_tmux_session() -> String { "flowforge".to_string() }
fn default_refresh_interval() -> u64 { 1000 }

impl Default for FlowForgeConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            memory: MemoryConfig::default(),
            agents: AgentsConfig::default(),
            routing: RoutingConfig::default(),
            patterns: PatternsConfig::default(),
            tmux: TmuxConfig::default(),
            hooks: HooksConfig::default(),
        }
    }
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
}
