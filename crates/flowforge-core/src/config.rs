use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main FlowForge configuration, loaded from `.flowforge/config.toml`.
///
/// All sections use `#[serde(default)]` so partial TOML files work — any
/// missing section or field falls back to its `Default` implementation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowForgeConfig {
    /// General settings (log level, telemetry).
    #[serde(default)]
    pub general: GeneralConfig,
    /// SQLite / HNSW memory backend configuration.
    #[serde(default)]
    pub memory: MemoryConfig,
    /// Which agent sources to load (built-in, global, project).
    #[serde(default)]
    pub agents: AgentsConfig,
    /// Weights for the multi-signal agent router.
    #[serde(default)]
    pub routing: RoutingConfig,
    /// Pattern storage, promotion, decay, and clustering knobs.
    #[serde(default)]
    pub patterns: PatternsConfig,
    /// tmux monitor panel settings.
    #[serde(default)]
    pub tmux: TmuxConfig,
    /// Which hook behaviours are active.
    #[serde(default)]
    pub hooks: HooksConfig,
    /// Work-tracking backend selection and work-stealing parameters.
    #[serde(default)]
    pub work_tracking: WorkTrackingConfig,
    /// Guidance control plane (safety gates, trust scoring).
    #[serde(default)]
    pub guidance: GuidanceConfig,
    /// Plugin enable/disable lists.
    #[serde(default)]
    pub plugins: PluginsConfig,
}

/// General FlowForge settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    /// Logging verbosity filter. Valid values: "trace", "debug", "info", "warn", "error".
    /// Default: "info".
    pub log_level: String,
    /// Whether to collect anonymous usage telemetry. Default: true.
    pub telemetry: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            telemetry: true,
        }
    }
}

/// SQLite + HNSW vector-search backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    /// SQLite database filename inside `.flowforge/`. Default: "flowforge.db".
    pub db_name: String,
    /// HNSW `M` parameter — max edges per node. Higher = better recall, more RAM.
    /// Valid range: 4..128. Default: 16.
    pub hnsw_m: usize,
    /// HNSW index-build quality. Higher = slower builds, better recall.
    /// Valid range: 16..500. Default: 100.
    pub hnsw_ef_construction: usize,
    /// HNSW search quality. Higher = slower queries, better recall.
    /// Valid range: 10..500. Default: 50.
    pub hnsw_ef_search: usize,
    /// Embedding vector dimensionality. Must match the model (AllMiniLM-L6-v2 = 128 after PCA).
    /// Default: 128. Changing this requires rebuilding the index.
    pub embedding_dim: usize,
    /// Days to retain append-only data (gate decisions, work events, edits, etc.).
    /// Set to 0 to disable pruning. Default: 90.
    pub retention_days: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            db_name: "flowforge.db".to_string(),
            hnsw_m: 16,
            hnsw_ef_construction: 100,
            hnsw_ef_search: 50,
            embedding_dim: 128,
            retention_days: 90,
        }
    }
}

/// Controls which agent sources are loaded into the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentsConfig {
    /// Load built-in agents shipped with the FlowForge binary. Default: true.
    pub load_builtin: bool,
    /// Load agents from `~/.flowforge/agents/`. Default: true.
    pub load_global: bool,
    /// Load agents from `<project>/.flowforge/agents/`. Default: true.
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

/// Weights for the multi-signal agent router. All weights should sum to 1.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingConfig {
    /// Weight for pattern-match signal. Range: 0.0..1.0. Default: 0.30.
    pub pattern_weight: f64,
    /// Weight for capability-match signal. Range: 0.0..1.0. Default: 0.20.
    pub capability_weight: f64,
    /// Weight for learned (historical success) signal. Range: 0.0..1.0. Default: 0.20.
    pub learned_weight: f64,
    /// Weight for agent priority ordering. Range: 0.0..1.0. Default: 0.05.
    pub priority_weight: f64,
    /// Weight for contextual cues (file types, recent edits). Range: 0.0..1.0. Default: 0.10.
    pub context_weight: f64,
    /// Weight for semantic (embedding) similarity. Range: 0.0..1.0. Default: 0.15.
    pub semantic_weight: f64,
    /// Sigmoid sharpening factor for confidence scores. Higher = sharper separation.
    /// 0.0 disables sharpening. Default: 8.0.
    pub confidence_sharpening: f64,
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            pattern_weight: 0.30,
            capability_weight: 0.20,
            learned_weight: 0.20,
            priority_weight: 0.05,
            context_weight: 0.10,
            semantic_weight: 0.15,
            confidence_sharpening: 8.0,
        }
    }
}

/// Pattern storage, promotion, decay, and DBSCAN clustering configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PatternsConfig {
    /// Maximum number of short-term patterns in memory. Oldest evicted first.
    /// Default: 500.
    pub short_term_max: usize,
    /// Hours before an unused short-term pattern expires. Default: 24.
    pub short_term_ttl_hours: u64,
    /// Maximum number of long-term (promoted) patterns. Default: 2000.
    pub long_term_max: usize,
    /// Minimum usage count before a short-term pattern is eligible for promotion.
    /// Default: 3. Interacts with `promotion_min_confidence`.
    pub promotion_min_usage: u32,
    /// Minimum confidence score for promotion. Range: 0.0..1.0. Default: 0.6.
    /// Both `promotion_min_usage` and this must be met for promotion.
    pub promotion_min_confidence: f64,
    /// Confidence decay rate per hour for unused patterns. Default: 0.005.
    pub decay_rate_per_hour: f64,
    /// Cosine similarity threshold for deduplication. Two patterns above this
    /// threshold are considered duplicates and merged. Range: 0.0..1.0. Default: 0.88.
    pub dedup_similarity_threshold: f64,
    /// Maximum number of trajectory records to retain. Default: 5000.
    pub trajectory_max: usize,
    /// Days after which old trajectory records are pruned. Default: 7.
    pub trajectory_prune_days: u64,
    /// Similarity threshold for merging similar trajectories. Range: 0.0..1.0. Default: 0.9.
    pub trajectory_merge_threshold: f64,
    /// Whether to compute semantic embeddings (AllMiniLM-L6-v2) for patterns.
    /// Disabling saves CPU but removes vector-search and clustering. Default: true.
    pub semantic_embeddings: bool,
    /// DBSCAN `min_points` — minimum cluster size. Default: 2.
    pub clustering_min_points: usize,
    /// DBSCAN `epsilon` — maximum distance between cluster neighbours.
    /// Smaller = tighter clusters. Default: 0.5. Use `flowforge learn tune-clusters` to auto-tune.
    pub clustering_epsilon: f64,
    /// Number of outlier patterns that triggers an automatic re-clustering pass. Default: 50.
    pub outlier_recluster_threshold: usize,
    /// Decay multiplier applied to patterns inside active (growing) clusters.
    /// Lower = slower decay for clustered patterns. Default: 0.5.
    pub cluster_decay_active_factor: f64,
    /// Decay multiplier applied to isolated (unclustered) patterns.
    /// Higher = faster decay for orphan patterns. Default: 2.0.
    pub cluster_decay_isolated_factor: f64,
    /// Minimum similarity score for pattern injection into context.
    /// Patterns below this threshold are filtered out as noise. Range: 0.0..1.0. Default: 0.55.
    pub min_injection_similarity: f64,
    /// Fraction of patterns to withhold from injection for A/B effectiveness testing.
    /// Range: 0.0..1.0. Default: 0.0 (no holdout).
    #[serde(default)]
    pub ab_test_holdout_rate: f64,
    /// Minimum feedback samples required before demotion logic activates. Default: 5.
    pub demotion_min_feedback: u32,
    /// Failure ratio threshold above which a long-term pattern is demoted back to
    /// short-term. Range: 0.0..1.0. Default: 0.6.
    pub demotion_failure_ratio: f64,
    /// Maximum failure-correlation score allowed for promotion. Patterns whose
    /// effectiveness score exceeds this are blocked from promotion.
    /// Range: 0.0..1.0. Default: 0.3.
    pub promotion_failure_correlation_max: f64,
    /// Maximum number of short-term patterns to consider during deduplication.
    /// Limits the O(n^2) comparison to the newest patterns. Default: 1000.
    pub max_dedup_patterns: usize,
}

impl Default for PatternsConfig {
    fn default() -> Self {
        Self {
            short_term_max: 500,
            short_term_ttl_hours: 24,
            long_term_max: 2000,
            promotion_min_usage: 3,
            promotion_min_confidence: 0.6,
            decay_rate_per_hour: 0.005,
            dedup_similarity_threshold: 0.88,
            trajectory_max: 5000,
            trajectory_prune_days: 7,
            trajectory_merge_threshold: 0.9,
            semantic_embeddings: true,
            clustering_min_points: 2,
            clustering_epsilon: 0.5,
            outlier_recluster_threshold: 50,
            cluster_decay_active_factor: 0.5,
            cluster_decay_isolated_factor: 2.0,
            min_injection_similarity: 0.55,
            ab_test_holdout_rate: 0.0,
            demotion_min_feedback: 5,
            demotion_failure_ratio: 0.6,
            promotion_failure_correlation_max: 0.3,
            max_dedup_patterns: 1000,
        }
    }
}

/// tmux monitor panel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TmuxConfig {
    /// tmux session name for the FlowForge monitor. Default: "flowforge".
    pub session_name: String,
    /// Automatically start the tmux monitor on session start. Default: true.
    pub auto_start: bool,
    /// Refresh interval in milliseconds for the tmux display. Default: 1000.
    pub refresh_interval_ms: u64,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            session_name: "flowforge".to_string(),
            auto_start: true,
            refresh_interval_ms: 1000,
        }
    }
}

/// Controls which Claude Code hook behaviours are active.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksConfig {
    /// Enable bash command safety validation in pre_tool_use. Default: true.
    pub bash_validation: bool,
    /// Track file edits (path, diff) in post_tool_use. Default: true.
    pub edit_tracking: bool,
    /// Enable agent routing suggestions in pre_tool_use. Default: true.
    pub routing: bool,
    /// Enable pattern learning from session outcomes. Default: true.
    pub learning: bool,
    /// If true, inject the full agent markdown body into context instead of a
    /// compact 1-line summary (~460 tokens saved when false). Default: false.
    pub inject_agent_body: bool,
}

impl Default for HooksConfig {
    fn default() -> Self {
        Self {
            bash_validation: true,
            edit_tracking: true,
            routing: true,
            learning: true,
            inject_agent_body: false,
        }
    }
}

/// Work-tracking backend and enforcement configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkTrackingConfig {
    /// Which work backend to use. "auto" probes kanbus -> beads -> claude_tasks.
    /// Valid: "auto", "kanbus", "beads", "claude_tasks". Default: "auto".
    pub backend: String,
    /// Log all work events to the audit trail. Default: true.
    pub log_all: bool,
    /// Require an active work item before allowing code changes. Default: false.
    pub require_task: bool,
    /// Block mutating tool calls when no active work item exists.
    /// Toggle off with `enforce_gate = false` or `FLOWFORGE_NO_WORK_GATE=1`. Default: true.
    pub enforce_gate: bool,
    /// Kanbus backend configuration.
    pub kanbus: KanbusSyncConfig,
    /// Beads backend configuration.
    pub beads: BeadsSyncConfig,
    /// Claude Tasks backend configuration.
    pub claude_tasks: ClaudeTasksSyncConfig,
    /// Work-stealing parameters (claim lifecycle, heartbeat, redistribution).
    pub work_stealing: WorkStealingConfig,
}

impl Default for WorkTrackingConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_string(),
            log_all: true,
            require_task: false,
            enforce_gate: true,
            kanbus: KanbusSyncConfig::default(),
            beads: BeadsSyncConfig::default(),
            claude_tasks: ClaudeTasksSyncConfig::default(),
            work_stealing: WorkStealingConfig::default(),
        }
    }
}

/// Work-stealing parameters controlling claim lifecycle and redistribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkStealingConfig {
    /// Enable the work-stealing subsystem. Default: true.
    pub enabled: bool,
    /// Minutes without a heartbeat before a claim is considered stale. Default: 30.
    /// Must be strictly less than `abandon_threshold_mins`.
    pub stale_threshold_mins: u64,
    /// Minutes without a heartbeat before a claim is abandoned and becomes stealable.
    /// Default: 60. Must be strictly greater than `stale_threshold_mins`.
    pub abandon_threshold_mins: u64,
    /// Minimum progress percentage below which a stale item is auto-released.
    /// Range: 0..100. Default: 25.
    pub stale_min_progress: i32,
    /// Maximum number of times a single work item can be stolen. Default: 3.
    pub max_steal_count: u32,
    /// Cooldown in minutes after a steal before the same item can be stolen again.
    /// Default: 10.
    pub steal_cooldown_mins: u64,
    /// Maximum number of work items a single agent can claim concurrently.
    /// Must be >= 1. Default: 5.
    pub max_concurrent_claims: u64,
    /// Interval in minutes between automatic stale-work scans. Default: 5.
    pub scan_interval_mins: u64,
}

impl Default for WorkStealingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            stale_threshold_mins: 30,
            abandon_threshold_mins: 60,
            stale_min_progress: 25,
            max_steal_count: 3,
            steal_cooldown_mins: 10,
            max_concurrent_claims: 5,
            scan_interval_mins: 5,
        }
    }
}

/// Kanbus work-tracking backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KanbusSyncConfig {
    /// Kanbus project key. Auto-detected if None.
    pub project_key: Option<String>,
    /// Kanbus CLI binary name. Default: "kbs".
    pub cli_command: String,
    /// Root directory for the kanbus board. Auto-detected if None.
    pub root: Option<std::path::PathBuf>,
}

impl Default for KanbusSyncConfig {
    fn default() -> Self {
        Self {
            project_key: None,
            cli_command: "kbs".to_string(),
            root: None,
        }
    }
}

/// Beads (JSONL) work-tracking backend configuration. Currently has no fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BeadsSyncConfig {}

/// Claude Tasks work-tracking backend configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeTasksSyncConfig {
    /// Claude task list ID. Auto-detected if None.
    pub list_id: Option<String>,
}

/// Guidance control plane — safety gates, trust scoring, and audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GuidanceConfig {
    /// Master switch for the guidance control plane. Default: true.
    pub enabled: bool,
    /// Gate that blocks destructive operations (rm -rf, DROP TABLE, etc.). Default: true.
    pub destructive_ops_gate: bool,
    /// Gate that restricts edits to files outside `protected_paths`. Default: true.
    pub file_scope_gate: bool,
    /// Gate that blocks diffs exceeding `max_diff_lines`. Default: true.
    pub diff_size_gate: bool,
    /// Gate that detects secrets (API keys, tokens) in tool arguments. Default: true.
    pub secrets_gate: bool,
    /// Maximum diff lines allowed before the diff-size gate triggers. Default: 500.
    pub max_diff_lines: usize,
    /// Initial trust score for a new session. Range: 0.0..1.0. Default: 0.5.
    /// Validated: must be between 0.0 and 1.0.
    pub trust_initial_score: f64,
    /// Trust score threshold above which gates auto-allow instead of asking.
    /// Range: 0.0..1.0. Default: 0.8.
    pub trust_ask_threshold: f64,
    /// Trust score threshold below which all tool uses require confirmation,
    /// even when all gates pass. Range: 0.0..1.0. Default: 0.1.
    pub trust_deny_threshold: f64,
    /// Trust score decay rate per hour of inactivity. Default: 0.02.
    pub trust_decay_per_hour: f64,
    /// File paths that the file-scope gate protects from modification. Default: [].
    pub protected_paths: Vec<String>,
    /// User-defined custom guidance rules. Default: [].
    pub custom_rules: Vec<crate::types::GuidanceRule>,
    /// Tools that bypass guidance gates entirely (read-only, safe tools).
    /// Default includes: Read, Glob, Grep, LSP, WebSearch, WebFetch, TaskList,
    /// TaskGet, TaskCreate, TaskUpdate, ToolSearch.
    pub safe_tools: Vec<String>,
}

impl Default for GuidanceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            destructive_ops_gate: true,
            file_scope_gate: true,
            diff_size_gate: true,
            secrets_gate: true,
            max_diff_lines: 500,
            trust_initial_score: 0.5,
            trust_ask_threshold: 0.8,
            trust_deny_threshold: 0.1,
            trust_decay_per_hour: 0.02,
            protected_paths: vec![],
            custom_rules: vec![],
            safe_tools: vec![
                "Read",
                "Glob",
                "Grep",
                "LSP",
                "WebSearch",
                "WebFetch",
                "TaskList",
                "TaskGet",
                "TaskCreate",
                "TaskUpdate",
                "ToolSearch",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        }
    }
}

/// Plugin enable/disable lists. Plugins live in `.flowforge/plugins/<name>/`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginsConfig {
    /// Plugins to explicitly enable (by name). Default: [].
    #[serde(default)]
    pub enabled: Vec<String>,
    /// Plugins to explicitly disable (by name). Overrides `enabled`. Default: [].
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl FlowForgeConfig {
    /// Load config from a TOML file, falling back to defaults
    pub fn load(path: &Path) -> crate::Result<Self> {
        let mut config = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            toml::from_str(&content)?
        } else {
            Self::default()
        };
        config.validate()?;
        Ok(config)
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

    /// Validate config for common misconfigurations.
    /// Auto-normalizes routing weights if they don't sum to ~1.0 (e.g. old configs
    /// without semantic_weight that get the new default added on top).
    pub fn validate(&mut self) -> crate::Result<()> {
        let ws = &self.work_tracking.work_stealing;
        if ws.abandon_threshold_mins <= ws.stale_threshold_mins {
            return Err(crate::Error::Config(
                "abandon_threshold_mins must be greater than stale_threshold_mins".to_string(),
            ));
        }
        if self.guidance.trust_initial_score > 1.0 || self.guidance.trust_initial_score < 0.0 {
            return Err(crate::Error::Config(
                "trust_initial_score must be between 0.0 and 1.0".to_string(),
            ));
        }
        if ws.max_concurrent_claims == 0 {
            return Err(crate::Error::Config(
                "max_concurrent_claims must be greater than 0".to_string(),
            ));
        }

        // HNSW parameter ranges
        let m = &self.memory;
        if !(4..=128).contains(&m.hnsw_m) {
            return Err(crate::Error::Config(format!(
                "hnsw_m must be in 4..=128, got {}",
                m.hnsw_m
            )));
        }
        if !(16..=500).contains(&m.hnsw_ef_construction) {
            return Err(crate::Error::Config(format!(
                "hnsw_ef_construction must be in 16..=500, got {}",
                m.hnsw_ef_construction
            )));
        }
        if !(10..=500).contains(&m.hnsw_ef_search) {
            return Err(crate::Error::Config(format!(
                "hnsw_ef_search must be in 10..=500, got {}",
                m.hnsw_ef_search
            )));
        }

        // Pattern config ranges
        let p = &self.patterns;
        if p.decay_rate_per_hour < 0.0 || p.decay_rate_per_hour > 1.0 {
            return Err(crate::Error::Config(
                "decay_rate_per_hour must be in 0.0..=1.0".to_string(),
            ));
        }
        if p.dedup_similarity_threshold < 0.0 || p.dedup_similarity_threshold > 1.0 {
            return Err(crate::Error::Config(
                "dedup_similarity_threshold must be in 0.0..=1.0".to_string(),
            ));
        }
        if p.clustering_epsilon <= 0.0 {
            return Err(crate::Error::Config(
                "clustering_epsilon must be > 0.0".to_string(),
            ));
        }
        if p.clustering_min_points < 1 {
            return Err(crate::Error::Config(
                "clustering_min_points must be >= 1".to_string(),
            ));
        }
        if p.min_injection_similarity < 0.0 || p.min_injection_similarity > 1.0 {
            return Err(crate::Error::Config(
                "min_injection_similarity must be in 0.0..=1.0".to_string(),
            ));
        }
        if p.promotion_min_confidence <= 0.0 || p.promotion_min_confidence > 1.0 {
            return Err(crate::Error::Config(
                "promotion_min_confidence must be in (0.0, 1.0]".to_string(),
            ));
        }
        if p.promotion_min_usage == 0 {
            return Err(crate::Error::Config(
                "promotion_min_usage must be > 0".to_string(),
            ));
        }
        if p.cluster_decay_active_factor < 0.0 {
            return Err(crate::Error::Config(
                "cluster_decay_active_factor must be >= 0.0".to_string(),
            ));
        }
        if p.cluster_decay_isolated_factor < 0.0 {
            return Err(crate::Error::Config(
                "cluster_decay_isolated_factor must be >= 0.0".to_string(),
            ));
        }
        if p.ab_test_holdout_rate < 0.0 || p.ab_test_holdout_rate > 1.0 {
            return Err(crate::Error::Config(
                "ab_test_holdout_rate must be in [0.0, 1.0]".to_string(),
            ));
        }

        // Work-tracking backend
        if !["auto", "kanbus", "beads", "claude_tasks", "flowforge"]
            .contains(&self.work_tracking.backend.as_str())
        {
            return Err(crate::Error::Config(format!(
                "work_tracking.backend must be one of: auto, kanbus, beads, claude_tasks, flowforge; got '{}'",
                self.work_tracking.backend
            )));
        }

        // Guidance ranges
        if self.guidance.trust_ask_threshold < 0.0 || self.guidance.trust_ask_threshold > 1.0 {
            return Err(crate::Error::Config(
                "trust_ask_threshold must be in 0.0..=1.0".to_string(),
            ));
        }
        if self.guidance.trust_deny_threshold < 0.0 || self.guidance.trust_deny_threshold > 1.0 {
            return Err(crate::Error::Config(
                "trust_deny_threshold must be in 0.0..=1.0".to_string(),
            ));
        }
        if self.guidance.trust_deny_threshold >= self.guidance.trust_ask_threshold {
            return Err(crate::Error::Config(
                "trust_deny_threshold must be < trust_ask_threshold".to_string(),
            ));
        }
        if self.guidance.max_diff_lines < 1 {
            return Err(crate::Error::Config(
                "max_diff_lines must be >= 1".to_string(),
            ));
        }

        // Work-stealing ranges
        if ws.stale_threshold_mins < 1 {
            return Err(crate::Error::Config(
                "stale_threshold_mins must be >= 1".to_string(),
            ));
        }
        if ws.steal_cooldown_mins < 1 {
            return Err(crate::Error::Config(
                "steal_cooldown_mins must be >= 1".to_string(),
            ));
        }
        if ws.stale_min_progress < 0 || ws.stale_min_progress > 100 {
            return Err(crate::Error::Config(
                "stale_min_progress must be 0..=100".to_string(),
            ));
        }

        // Cross-field: routing weights must sum to approximately 1.0
        // Auto-normalize if they don't (common when upgrading from old 5-weight config)
        {
            let r = &self.routing;
            let weight_sum = r.pattern_weight
                + r.capability_weight
                + r.learned_weight
                + r.priority_weight
                + r.context_weight
                + r.semantic_weight;
            if (weight_sum - 1.0).abs() > 0.01 {
                if weight_sum > 0.0 && weight_sum.is_finite() {
                    // Auto-normalize
                    self.routing.pattern_weight /= weight_sum;
                    self.routing.capability_weight /= weight_sum;
                    self.routing.learned_weight /= weight_sum;
                    self.routing.priority_weight /= weight_sum;
                    self.routing.context_weight /= weight_sum;
                    self.routing.semantic_weight /= weight_sum;
                } else {
                    return Err(crate::Error::Config(format!(
                        "routing weights must sum to ~1.0, got {weight_sum:.4}"
                    )));
                }
            }
        }
        if self.routing.confidence_sharpening < 0.0 {
            return Err(crate::Error::Config(
                "confidence_sharpening must be >= 0.0".to_string(),
            ));
        }

        // Cross-field: trust_ask_threshold must be >= trust_initial_score
        if self.guidance.trust_ask_threshold < self.guidance.trust_initial_score {
            return Err(crate::Error::Config(
                "trust_ask_threshold must be >= trust_initial_score".to_string(),
            ));
        }

        // Log level must be a valid filter
        if !["trace", "debug", "info", "warn", "error"].contains(&self.general.log_level.as_str()) {
            return Err(crate::Error::Config(
                "log_level must be trace|debug|info|warn|error".to_string(),
            ));
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let mut config = FlowForgeConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_bad_abandon_threshold() {
        let mut config = FlowForgeConfig::default();
        config.work_tracking.work_stealing.abandon_threshold_mins = 10;
        config.work_tracking.work_stealing.stale_threshold_mins = 30;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_bad_trust_score() {
        let mut config = FlowForgeConfig::default();
        config.guidance.trust_initial_score = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_zero_concurrent_claims() {
        let mut config = FlowForgeConfig::default();
        config.work_tracking.work_stealing.max_concurrent_claims = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_defaults_roundtrip() {
        let config = FlowForgeConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: FlowForgeConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.general.log_level, "info");
        assert_eq!(parsed.memory.hnsw_m, 16);
        assert!(parsed.guidance.enabled);
        assert_eq!(parsed.work_tracking.work_stealing.stale_threshold_mins, 30);
    }

    #[test]
    fn test_partial_toml_uses_defaults() {
        let toml_str = r#"
[general]
log_level = "debug"
"#;
        let config: FlowForgeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.log_level, "debug");
        assert!(config.general.telemetry); // default true
        assert_eq!(config.memory.hnsw_m, 16); // default
        assert!(config.guidance.enabled); // default true
    }

    #[test]
    fn test_validate_negative_trust_score() {
        let mut config = FlowForgeConfig::default();
        config.guidance.trust_initial_score = -0.1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_equal_thresholds() {
        let mut config = FlowForgeConfig::default();
        config.work_tracking.work_stealing.abandon_threshold_mins = 30;
        config.work_tracking.work_stealing.stale_threshold_mins = 30;
        // equal is also invalid (must be strictly greater)
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_save_load_roundtrip() {
        let config = FlowForgeConfig::default();
        let dir = std::env::temp_dir().join("flowforge-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        config.save(&path).unwrap();
        let loaded = FlowForgeConfig::load(&path).unwrap();
        assert_eq!(loaded.general.log_level, "info");
        assert_eq!(loaded.memory.hnsw_m, 16);
        assert!(loaded.guidance.enabled);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_config_load_nonexistent_returns_default() {
        let path = std::path::Path::new("/nonexistent/path/config.toml");
        let config = FlowForgeConfig::load(path).unwrap();
        assert_eq!(config.general.log_level, "info");
    }

    // ── Phase 3: Config validation hardening tests ──

    #[test]
    fn test_validate_hnsw_m_out_of_range() {
        let mut config = FlowForgeConfig::default();
        config.memory.hnsw_m = 0;
        assert!(config.validate().is_err());
        config.memory.hnsw_m = 200;
        assert!(config.validate().is_err());
        config.memory.hnsw_m = 16; // valid
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_hnsw_ef_construction_out_of_range() {
        let mut config = FlowForgeConfig::default();
        config.memory.hnsw_ef_construction = 5;
        assert!(config.validate().is_err());
        config.memory.hnsw_ef_construction = 1000;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_hnsw_ef_search_out_of_range() {
        let mut config = FlowForgeConfig::default();
        config.memory.hnsw_ef_search = 3;
        assert!(config.validate().is_err());
        config.memory.hnsw_ef_search = 600;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_pattern_ranges() {
        let mut config = FlowForgeConfig::default();
        config.patterns.decay_rate_per_hour = -0.1;
        assert!(config.validate().is_err());
        config.patterns.decay_rate_per_hour = 0.005; // reset
        config.patterns.dedup_similarity_threshold = 1.5;
        assert!(config.validate().is_err());
        config.patterns.dedup_similarity_threshold = 0.88; // reset
        config.patterns.clustering_epsilon = 0.0;
        assert!(config.validate().is_err());
        config.patterns.clustering_epsilon = 0.5; // reset
        config.patterns.min_injection_similarity = -1.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_guidance_ranges() {
        let mut config = FlowForgeConfig::default();
        config.guidance.trust_ask_threshold = 2.0;
        assert!(config.validate().is_err());
        config.guidance.trust_ask_threshold = 0.8; // reset
        config.guidance.max_diff_lines = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_work_stealing_ranges() {
        let mut config = FlowForgeConfig::default();
        config.work_tracking.work_stealing.stale_threshold_mins = 0;
        assert!(config.validate().is_err());
        config.work_tracking.work_stealing.stale_threshold_mins = 30; // reset
        config.work_tracking.work_stealing.steal_cooldown_mins = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_work_stealing_defaults() {
        let config = WorkStealingConfig::default();
        assert!(config.enabled);
        assert_eq!(config.stale_threshold_mins, 30);
        assert_eq!(config.abandon_threshold_mins, 60);
        assert_eq!(config.max_steal_count, 3);
        assert_eq!(config.max_concurrent_claims, 5);
    }

    #[test]
    fn test_guidance_config_defaults() {
        let config = GuidanceConfig::default();
        assert!(config.destructive_ops_gate);
        assert!(config.file_scope_gate);
        assert!(config.secrets_gate);
        assert!(config.diff_size_gate);
        assert_eq!(config.max_diff_lines, 500);
        assert_eq!(config.trust_initial_score, 0.5);
        assert!(config.safe_tools.contains(&"Read".to_string()));
        assert!(config.safe_tools.contains(&"Grep".to_string()));
    }

    #[test]
    fn test_patterns_config_defaults() {
        let config = PatternsConfig::default();
        assert_eq!(config.short_term_max, 500);
        assert_eq!(config.long_term_max, 2000);
        assert_eq!(config.promotion_min_usage, 3);
        assert!((config.promotion_min_confidence - 0.6).abs() < f64::EPSILON);
        assert!((config.min_injection_similarity - 0.55).abs() < f64::EPSILON);
    }

    #[test]
    fn test_routing_config_weights_sum_to_one() {
        let config = RoutingConfig::default();
        let sum = config.pattern_weight
            + config.capability_weight
            + config.learned_weight
            + config.priority_weight
            + config.context_weight
            + config.semantic_weight;
        assert!((sum - 1.0).abs() < 0.001);
    }

    // ── v4 Phase 5: Cross-field validation tests ──

    #[test]
    fn test_validate_routing_weights_must_sum_to_one() {
        let mut config = FlowForgeConfig::default();
        config.routing.pattern_weight = 0.0;
        config.routing.capability_weight = 0.0;
        config.routing.learned_weight = 0.0;
        config.routing.priority_weight = 0.0;
        config.routing.context_weight = 0.0;
        config.routing.semantic_weight = 0.0;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("routing weights"), "got: {err}");
    }

    #[test]
    fn test_validate_trust_ask_threshold_ge_initial() {
        let mut config = FlowForgeConfig::default();
        config.guidance.trust_initial_score = 0.9;
        config.guidance.trust_ask_threshold = 0.5;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("trust_ask_threshold"), "got: {err}");
    }

    #[test]
    fn test_validate_log_level_must_be_valid() {
        let mut config = FlowForgeConfig::default();
        config.general.log_level = "verbose".to_string();
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("log_level"), "got: {err}");
    }

    #[test]
    fn test_validate_stale_min_progress_range() {
        let mut config = FlowForgeConfig::default();
        config.work_tracking.work_stealing.stale_min_progress = 150;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("stale_min_progress"), "got: {err}");

        config.work_tracking.work_stealing.stale_min_progress = -1;
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("stale_min_progress"), "got: {err}");
    }

    // ── v5 Phase 4: New config validation tests ──

    #[test]
    fn test_validate_promotion_min_confidence_range() {
        let mut config = FlowForgeConfig::default();
        config.patterns.promotion_min_confidence = 0.0; // must be > 0
        assert!(config.validate().is_err());
        config.patterns.promotion_min_confidence = -0.5;
        assert!(config.validate().is_err());
        config.patterns.promotion_min_confidence = 1.1;
        assert!(config.validate().is_err());
        config.patterns.promotion_min_confidence = 1.0; // valid
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_promotion_min_usage_must_be_positive() {
        let mut config = FlowForgeConfig::default();
        config.patterns.promotion_min_usage = 0;
        assert!(config.validate().is_err());
        config.patterns.promotion_min_usage = 1; // valid
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_cluster_decay_factors() {
        let mut config = FlowForgeConfig::default();
        config.patterns.cluster_decay_active_factor = -0.1;
        assert!(config.validate().is_err());
        config.patterns.cluster_decay_active_factor = 0.0; // valid edge
        assert!(config.validate().is_ok());

        config.patterns.cluster_decay_isolated_factor = -1.0;
        assert!(config.validate().is_err());
        config.patterns.cluster_decay_isolated_factor = 0.0; // valid edge
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_ab_test_holdout_rate() {
        let mut config = FlowForgeConfig::default();
        config.patterns.ab_test_holdout_rate = -0.1;
        assert!(config.validate().is_err());
        config.patterns.ab_test_holdout_rate = 1.5;
        assert!(config.validate().is_err());
        config.patterns.ab_test_holdout_rate = 0.5; // valid
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_work_tracking_backend() {
        let mut config = FlowForgeConfig::default();
        config.work_tracking.backend = "invalid".to_string();
        let err = config.validate().unwrap_err().to_string();
        assert!(err.contains("backend"), "got: {err}");

        for valid in &["auto", "kanbus", "beads", "claude_tasks", "flowforge"] {
            config.work_tracking.backend = valid.to_string();
            assert!(
                config.validate().is_ok(),
                "Expected '{}' to be valid",
                valid
            );
        }
    }

    #[test]
    fn test_full_toml_parse() {
        let toml_str = r#"
[general]
log_level = "debug"
telemetry = false

[memory]
db_name = "custom.db"
hnsw_m = 32

[guidance]
enabled = false
max_diff_lines = 1000

[work_tracking]
backend = "kanbus"
enforce_gate = false

[work_tracking.work_stealing]
enabled = false
stale_threshold_mins = 60
abandon_threshold_mins = 120
"#;
        let config: FlowForgeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.log_level, "debug");
        assert!(!config.general.telemetry);
        assert_eq!(config.memory.db_name, "custom.db");
        assert_eq!(config.memory.hnsw_m, 32);
        assert!(!config.guidance.enabled);
        assert_eq!(config.guidance.max_diff_lines, 1000);
        assert_eq!(config.work_tracking.backend, "kanbus");
        assert!(!config.work_tracking.enforce_gate);
        assert!(!config.work_tracking.work_stealing.enabled);
        assert_eq!(config.work_tracking.work_stealing.stale_threshold_mins, 60);
    }
}
