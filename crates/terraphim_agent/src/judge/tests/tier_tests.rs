//! Unit tests for `TierResolver` (Refs #193).
//!
//! Hermetic: builds an in-test `ModelMapping` (no I/O, no on-disk
//! fixture) so the resolver's same-family-swap behaviour is
//! deterministic and independent of the opencode snapshot.

use std::collections::BTreeMap;

use super::super::{CliKind, ModelFamily, ModelMapping, ResolveError, TierConfig, TierResolver};

fn cfg(cli: CliKind, model: &str, fallback: Option<&str>) -> TierConfig {
    TierConfig {
        cli,
        model: model.to_string(),
        fallback: fallback.map(str::to_string),
    }
}

fn mapping() -> ModelMapping {
    // Mirrors the live model-mapping.json tiers used by the
    // /evolve + task-review profiles.
    let mut tiers = BTreeMap::new();
    tiers.insert(
        "quick".to_string(),
        cfg(
            CliKind::Opencode,
            "kimi-for-coding/kimi-for-coding-highspeed",
            Some("quick_alt"),
        ),
    );
    tiers.insert(
        "quick_alt".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/kimi-for-coding", None),
    );
    tiers.insert(
        "deep".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/k3", Some("deep_alt")),
    );
    tiers.insert(
        "deep_alt".to_string(),
        cfg(CliKind::Opencode, "opencode-go/kimi-k2.6", None),
    );
    tiers.insert(
        "tiebreaker".to_string(),
        cfg(CliKind::Claude, "sonnet", None),
    );
    ModelMapping { tiers }
}

/// No generator: the original tier is returned, no swap.
#[test]
fn no_generator_keeps_original() {
    let m = mapping();
    let r = TierResolver::new().resolve(&m, "quick", None).unwrap();
    assert_eq!(r.tier, "quick");
    assert_eq!(r.model, "kimi-for-coding/kimi-for-coding-highspeed");
    assert_eq!(r.family, ModelFamily::Moonshot);
    assert!(r.swapped_for_bias.is_none());
}

/// Generator with a different family from the tier: no swap needed.
#[test]
fn different_family_no_swap() {
    let m = mapping();
    let r = TierResolver::new()
        .resolve(&m, "deep", Some("openai/gpt-5-nano"))
        .unwrap();
    assert_eq!(r.tier, "deep");
    assert_eq!(r.model, "kimi-for-coding/k3");
    assert_eq!(r.family, ModelFamily::Moonshot);
    assert!(r.swapped_for_bias.is_none());
}

/// Same family with a fallback that is also the same family: the
/// resolver walks the chain and, finding no different-family
/// alternative, keeps the original tier with `swapped_for_bias: None`.
/// (Conservative: rather than force a swap that might still be biased,
/// keep the original so the verdict's `swapped_for_bias` is `null` and
/// the human-in-the-loop can see the bias risk.)
#[test]
fn same_family_chain_with_no_different_family_alternative_keeps_original() {
    let m = mapping();
    let r = TierResolver::new()
        .resolve(&m, "quick", Some("kimi-for-coding/kimi-for-coding"))
        .unwrap();
    assert_eq!(r.tier, "quick");
    assert_eq!(r.model, "kimi-for-coding/kimi-for-coding-highspeed");
    assert_eq!(r.family, ModelFamily::Moonshot);
    assert!(r.swapped_for_bias.is_none());
}

/// The chain DOES contain a different-family tier further down: the
/// resolver walks to it and records the swap.
#[test]
fn same_family_walks_to_different_family_in_chain() {
    // Synthetic mapping: a -> b (same family) -> c (different family).
    let mut tiers = BTreeMap::new();
    tiers.insert(
        "a".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/k3", Some("b")),
    );
    tiers.insert(
        "b".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/kimi-k2.6", Some("c")),
    );
    tiers.insert(
        "c".to_string(),
        cfg(CliKind::Opencode, "openai/gpt-5-nano", None),
    );
    let m = ModelMapping { tiers };
    let r = TierResolver::new()
        .resolve(&m, "a", Some("kimi-for-coding/kimi-for-coding"))
        .unwrap();
    assert_eq!(r.tier, "c");
    assert_eq!(r.model, "openai/gpt-5-nano");
    assert_eq!(r.family, ModelFamily::OpenAI);
    assert_eq!(
        r.swapped_for_bias
            .as_ref()
            .map(|s| (s.from_tier.as_str(), s.to_tier.as_str())),
        Some(("a", "c")),
    );
}

/// Unknown generator family: no swap (safer than guessing).
#[test]
fn unknown_generator_no_swap() {
    let m = mapping();
    let r = TierResolver::new()
        .resolve(&m, "deep", Some("custom/mystery"))
        .unwrap();
    assert_eq!(r.tier, "deep");
    assert!(r.swapped_for_bias.is_none());
}

/// Unknown tier: ResolveError::UnknownTier.
#[test]
fn unknown_tier_errors() {
    let m = mapping();
    let err = TierResolver::new().resolve(&m, "nope", None).unwrap_err();
    assert!(matches!(err, ResolveError::UnknownTier { ref name } if name == "nope"));
}

/// Cycle safety: a synthetic mapping with a cycle does not loop; the
/// resolver returns the original tier with `swapped_for_bias: None`.
#[test]
fn cycle_safety() {
    let mut tiers = BTreeMap::new();
    tiers.insert(
        "a".to_string(),
        cfg(CliKind::Opencode, "openai/gpt-5-nano", Some("b")),
    );
    tiers.insert(
        "b".to_string(),
        cfg(CliKind::Opencode, "openai/gpt-4.1-nano", Some("a")),
    );
    let m = ModelMapping { tiers };
    let r = TierResolver::new()
        .resolve(&m, "a", Some("openai/gpt-5-nano"))
        .unwrap();
    assert_eq!(r.tier, "a");
    assert!(r.swapped_for_bias.is_none());
}

/// No-fallback chain: when a tier has no `fallback` and the generator
/// matches, the resolver returns the original tier with
/// `swapped_for_bias: None` (the verdict will reflect that no swap
/// was performed).
#[test]
fn no_fallback_keeps_original_with_match() {
    let mut tiers = BTreeMap::new();
    tiers.insert(
        "only".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/k3", None),
    );
    let m = ModelMapping { tiers };
    let r = TierResolver::new()
        .resolve(&m, "only", Some("kimi-for-coding/k3"))
        .unwrap();
    assert_eq!(r.tier, "only");
    assert!(r.swapped_for_bias.is_none());
}

/// `swapped_field` is the string form for the JSONL emit. Verifies the
/// `"<from> -> <to>"` format the parent #192 will serialise.
#[test]
fn swapped_field_string_form() {
    // Same-family chain with no different-family alternative: the
    // resolver returns the original and `swapped_field` is `None`.
    let m = mapping();
    let r = TierResolver::new()
        .resolve(&m, "quick", Some("kimi-for-coding/kimi-for-coding"))
        .unwrap();
    assert_eq!(r.swapped_field(), None);

    // Different family: no swap, `swapped_field` is `None`.
    let r = TierResolver::new().resolve(&m, "deep", None).unwrap();
    assert_eq!(r.swapped_field(), None);

    // Walk to a different-family tier: `swapped_field` is the
    // "from -> to" string.
    let mut tiers = BTreeMap::new();
    tiers.insert(
        "a".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/k3", Some("b")),
    );
    tiers.insert(
        "b".to_string(),
        cfg(CliKind::Opencode, "kimi-for-coding/kimi-k2.6", Some("c")),
    );
    tiers.insert(
        "c".to_string(),
        cfg(CliKind::Opencode, "openai/gpt-5-nano", None),
    );
    let m2 = ModelMapping { tiers };
    let r = TierResolver::new()
        .resolve(&m2, "a", Some("kimi-for-coding/kimi-for-coding"))
        .unwrap();
    assert_eq!(r.swapped_field().as_deref(), Some("a -> c"));
}
