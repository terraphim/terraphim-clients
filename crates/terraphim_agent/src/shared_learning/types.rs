//! Types for the shared learning system
//!
//! Re-exports from terraphim_types for backward compatibility, plus the trust
//! decision that every consumer of a shared learning has to agree on.

pub use terraphim_types::shared_learning::*;

/// Minimum number of applications for peer validation (L2).
pub const L2_MIN_APPLICATIONS: u32 = 3;

/// Minimum number of distinct agents for peer validation (L2).
pub const L2_MIN_AGENTS: u32 = 2;

/// Minimum share of applications that must have had a positive outcome.
///
/// Without a floor, three failed applications across two agents would still
/// count as "peer validated" (terraphim-clients#388).
pub const L2_MIN_SUCCESS_RATE: f64 = 0.5;

/// Whether the recorded evidence promotes a learning to peer-validated (L2).
///
/// The evidence is what makes the claim checkable: a learning that says it was
/// applied three times by two agents must also show that most of those
/// applications actually worked.
pub fn meets_l2_criteria(quality: &QualityMetrics) -> bool {
    quality.applied_count >= L2_MIN_APPLICATIONS
        && quality.agent_count >= L2_MIN_AGENTS
        && quality.success_rate.unwrap_or(0.0) >= L2_MIN_SUCCESS_RATE
}

/// Whether a learning in this status may be used at all.
///
/// A rejected learning is never injected, suggested or synced
/// (terraphim-clients#388).
pub fn status_is_usable(status: SuggestionStatus) -> bool {
    status != SuggestionStatus::Rejected
}

/// The trust a learning may be used at, derived from its status and evidence.
///
/// The trust level written into a shared file is **advisory**: the file was
/// written by another agent, so it cannot promote itself. L2 requires recorded
/// evidence, L3 requires an approval, and a rejected learning is never usable.
///
/// The declared level is deliberately not consulted - a file that claims L2
/// without the evidence behind the claim stays L1.
pub fn effective_trust(status: SuggestionStatus, quality: Option<&QualityMetrics>) -> TrustLevel {
    match status {
        SuggestionStatus::Rejected => TrustLevel::L0,
        SuggestionStatus::Approved => TrustLevel::L3,
        SuggestionStatus::Pending => {
            if quality.is_some_and(meets_l2_criteria) {
                TrustLevel::L2
            } else {
                TrustLevel::L1
            }
        }
    }
}
