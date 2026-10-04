//! Unit tests for `VerdictMeta` serialisation (Refs #193).
//!
//! The parent #192 will `#[serde(flatten)]` `VerdictMeta` into its full
//! `Verdict` struct. This test pins the JSONL contract for the three
//! additional fields so any future parent-version rebuild doesn't
//! accidentally rename them.

use super::super::{ModelFamily, SwapInfo, TierResolution, VerdictMeta};

#[test]
fn serialise_all_fields_present_in_jsonl() {
    let meta = VerdictMeta {
        generator_model: Some("kimi-for-coding/k3".to_string()),
        generator_family: Some(ModelFamily::Moonshot),
        swapped_for_bias: Some(SwapInfo {
            from_tier: "deep".to_string(),
            to_tier: "deep_alt".to_string(),
        }),
    };
    let v = serde_json::to_value(&meta).unwrap();
    assert_eq!(v["generator_model"], "kimi-for-coding/k3");
    assert_eq!(v["generator_family"], "moonshot");
    assert_eq!(v["swapped_for_bias"]["from_tier"], "deep");
    assert_eq!(v["swapped_for_bias"]["to_tier"], "deep_alt");
}

#[test]
fn serialise_with_no_swap_and_no_generator() {
    let meta = VerdictMeta::default();
    let v = serde_json::to_value(&meta).unwrap();
    assert!(v["generator_model"].is_null());
    assert!(v["generator_family"].is_null());
    assert!(v["swapped_for_bias"].is_null());
}

/// `from_resolver` derives the family and swap from the resolution
/// output, so the parent #192's verdict emit never has to thread them
/// separately.
#[test]
fn from_resolver_populates_fields() {
    let resolution = TierResolution {
        tier: "deep_alt".to_string(),
        model: "opencode-go/kimi-k2.6".to_string(),
        family: ModelFamily::Moonshot,
        swapped_for_bias: Some(SwapInfo {
            from_tier: "deep".to_string(),
            to_tier: "deep_alt".to_string(),
        }),
    };
    let meta = VerdictMeta::from_resolver(Some("kimi-for-coding/k3"), &resolution);
    assert_eq!(meta.generator_model.as_deref(), Some("kimi-for-coding/k3"));
    assert_eq!(meta.generator_family, Some(ModelFamily::Moonshot));
    assert_eq!(
        meta.swapped_for_bias
            .as_ref()
            .map(|s| (s.from_tier.as_str(), s.to_tier.as_str())),
        Some(("deep", "deep_alt")),
    );
}
