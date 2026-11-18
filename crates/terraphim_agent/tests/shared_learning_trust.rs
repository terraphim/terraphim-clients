//! Trust and status rules for shared learnings (#388).
//!
//! Two properties matter:
//!
//! 1. a **rejected** learning is never injected, suggested or synced, whatever
//!    it claims to be worth, and
//! 2. a shared learning **cannot promote itself**: the trust level written in
//!    the file is advisory; L2 requires recorded evidence and L3 requires an
//!    approval.
//!
//! The property test walks the whole (trust level, status, evidence) space so
//! the rule is checked for every combination rather than for a few examples.

use proptest::prelude::*;
use terraphim_agent::shared_learning::{
    SuggestionStatus, TrustLevel, effective_trust, meets_l2_criteria, status_is_usable,
};
use terraphim_types::shared_learning::QualityMetrics;

/// Evidence that satisfies the L2 criteria.
fn peer_validated_evidence() -> QualityMetrics {
    QualityMetrics {
        applied_count: 3,
        effective_count: 3,
        agent_count: 2,
        agent_names: vec!["alpha".to_string(), "beta".to_string()],
        success_rate: Some(1.0),
        ..Default::default()
    }
}

/// Any evidence shape: applied/effective counts, agent count and success rate.
fn any_quality() -> impl Strategy<Value = QualityMetrics> {
    (0u32..8, 0u32..8, 0u32..4, proptest::option::of(0.0f64..1.0)).prop_map(
        |(applied, effective, agents, success_rate)| QualityMetrics {
            applied_count: applied,
            effective_count: effective,
            agent_count: agents,
            success_rate,
            ..Default::default()
        },
    )
}

proptest! {
    /// A rejected learning is never usable, for any declared trust level and
    /// any evidence.
    #[test]
    fn rejected_learnings_are_never_usable(
        declared in proptest::sample::select(vec![
            TrustLevel::L0,
            TrustLevel::L1,
            TrustLevel::L2,
            TrustLevel::L3,
        ]),
        quality in proptest::option::of(any_quality()),
    ) {
        prop_assert!(!status_is_usable(SuggestionStatus::Rejected));
        prop_assert_eq!(
            effective_trust(SuggestionStatus::Rejected, quality.as_ref()),
            TrustLevel::L0,
            "a rejected learning must never carry trust, declared {:?}",
            declared,
        );
    }

    /// A pending learning is peer-validated only when the evidence supports
    /// it; a file that merely claims L2 stays L1.
    #[test]
    fn pending_learnings_need_evidence_for_l2(
        quality in proptest::option::of(any_quality()),
    ) {
        let trust = effective_trust(SuggestionStatus::Pending, quality.as_ref());
        match quality.as_ref() {
            Some(quality) if meets_l2_criteria(quality) => {
                prop_assert_eq!(trust, TrustLevel::L2);
            }
            _ => {
                prop_assert_eq!(
                    trust,
                    TrustLevel::L1,
                    "without evidence a learning must not be peer-validated",
                );
            }
        }
    }

    /// An approved learning is human-approved (L3) whatever its evidence says.
    #[test]
    fn approved_learnings_are_l3(
        quality in proptest::option::of(any_quality()),
    ) {
        prop_assert_eq!(
            effective_trust(SuggestionStatus::Approved, quality.as_ref()),
            TrustLevel::L3,
        );
    }
}

/// The L2 criteria need a success-rate floor: three failed applications across
/// two agents are not peer validation.
#[test]
fn l2_needs_a_success_rate_floor() {
    let failed = QualityMetrics {
        applied_count: 3,
        effective_count: 0,
        agent_count: 2,
        success_rate: Some(0.0),
        ..Default::default()
    };
    assert!(
        !meets_l2_criteria(&failed),
        "three failed applications must not be peer validation"
    );

    let mixed = QualityMetrics {
        applied_count: 3,
        effective_count: 2,
        agent_count: 2,
        success_rate: Some(0.66),
        ..Default::default()
    };
    assert!(
        meets_l2_criteria(&mixed),
        "two effective applications out of three is peer validation"
    );

    let single_agent = QualityMetrics {
        agent_count: 1,
        ..peer_validated_evidence()
    };
    assert!(
        !meets_l2_criteria(&single_agent),
        "one agent cannot peer-validate a learning"
    );
}
