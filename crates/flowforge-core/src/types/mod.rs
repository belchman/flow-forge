mod agents;
mod collaboration;
pub mod error_recovery;
mod guidance;
mod patterns;
mod sessions;
mod work;

pub use agents::*;
pub use collaboration::*;
pub use error_recovery::{
    classify_error, fingerprint_error, normalize_error, ErrorCategory, ErrorFingerprint,
    ErrorResolution, PreviousSessionContext,
};
pub use guidance::*;
pub use patterns::*;
pub use sessions::*;
pub use work::*;

#[cfg(test)]
mod tests {
    #[test]
    fn test_all_types_accessible_from_crate_root() {
        let _: Option<crate::AgentDef> = None;
        let _: Option<crate::WorkItem> = None;
        let _: Option<crate::GuidanceRule> = None;
        let _: Option<crate::SessionInfo> = None;
        let _: Option<crate::ShortTermPattern> = None;
        let _: Option<crate::ConversationMessage> = None;
        let _: Option<crate::TmuxState> = None;
        let _: Option<crate::RoutingResult> = None;
        let _: Option<crate::PatternCluster> = None;
        let _: Option<crate::WorkFilter> = None;
        let _: Option<crate::MailboxMessage> = None;
        let _: Option<crate::TrustScore> = None;
        let _: Option<crate::DiscoveredCapability> = None;
        let _: Option<crate::ErrorFingerprint> = None;
        let _: Option<crate::PreviousSessionContext> = None;
    }
}
