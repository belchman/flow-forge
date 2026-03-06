pub mod config;
pub mod error;
pub mod guidance;
pub mod hook;
pub mod plugin;
pub mod plugin_exec;
pub mod trajectory;
pub mod transcript;
pub mod types;
pub mod work_tracking;

pub use config::FlowForgeConfig;
pub use error::{Error, Result};
pub use types::{
    // Agents
    AgentDef,
    DiscoveredCapability,
    // Sessions
    AgentSession,
    AgentSessionStatus,
    AgentSource,
    // Collaboration
    Checkpoint,
    // Patterns
    ContextInjection,
    ConversationMessage,
    EditRecord,
    // Error Recovery
    ErrorCategory,
    ErrorFingerprint,
    ErrorResolution,
    PreviousSessionContext,
    // Guidance
    GateAction,
    GateDecision,
    GuidanceRule,
    HnswEntry,
    LongTermPattern,
    MailboxMessage,
    PatternCluster,
    PatternMatch,
    PatternTier,
    Priority,
    RiskLevel,
    RoutingBreakdown,
    RoutingCategory,
    RoutingContext,
    RoutingResult,
    RoutingWeight,
    RuleScope,
    SessionFork,
    SessionInfo,
    ShortTermPattern,
    TeamMemberState,
    TeamMemberStatus,
    TmuxState,
    TrustScore,
    // Work
    WorkEvent,
    WorkFilter,
    WorkItem,
    WorkStatus,
};
