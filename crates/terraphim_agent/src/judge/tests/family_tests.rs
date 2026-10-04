//! Unit tests for `ModelFamily` parsing (Refs #193).
//!
//! Hermetic: no network. The opencode fixture is checked in.

use super::super::{BANNED_MODEL_PREFIXES, BannedModelPrefixes, ModelFamily};

/// Live-deployment provider mappings (Refs `terraphim-ai/AGENTS.md` and
/// `model-mapping.json`). These are the live routes as of 2026-09-11.
#[test]
fn live_deployment_provider_mappings() {
    assert_eq!(
        ModelFamily::from_model("kimi-for-coding/k3"),
        ModelFamily::Moonshot,
    );
    assert_eq!(
        ModelFamily::from_model("kimi-for-coding/kimi-for-coding-highspeed"),
        ModelFamily::Moonshot,
    );
    assert_eq!(
        ModelFamily::from_model("zai-coding-plan/glm-5.3-flash"),
        ModelFamily::Zhipu,
    );
    assert_eq!(
        ModelFamily::from_model("zai-coding-plan/glm-4.7"),
        ModelFamily::Zhipu,
    );
    assert_eq!(
        ModelFamily::from_model("anthropic/claude-opus-4-6"),
        ModelFamily::Anthropic,
    );
    assert_eq!(
        ModelFamily::from_model("anthropic/claude-sonnet-4-6"),
        ModelFamily::Anthropic,
    );
    assert_eq!(
        ModelFamily::from_model("openai/gpt-5-nano"),
        ModelFamily::OpenAI,
    );
    assert_eq!(
        ModelFamily::from_model("openai/gpt-4.1-nano"),
        ModelFamily::OpenAI,
    );
    assert_eq!(
        ModelFamily::from_model("deepseek/deepseek-v4-pro"),
        ModelFamily::Deepseek,
    );
    assert_eq!(
        ModelFamily::from_model("minimax/MiniMax-M3"),
        ModelFamily::MiniMax,
    );
    assert_eq!(
        ModelFamily::from_model("minimax/MiniMax-M2.7"),
        ModelFamily::MiniMax,
    );
}

/// `opencode-go` is multi-vendor; the parser recurses on the model
/// segment to resolve the vendor family.
#[test]
fn opencode_go_multivendor_routes_per_model() {
    assert_eq!(
        ModelFamily::from_model("opencode-go/qwen3.7-max"),
        ModelFamily::Qwen,
    );
    assert_eq!(
        ModelFamily::from_model("opencode-go/kimi-k2.6"),
        ModelFamily::Moonshot,
    );
    assert_eq!(
        ModelFamily::from_model("opencode-go/deepseek-v4-flash-vision-exp"),
        ModelFamily::Deepseek,
    );
    assert_eq!(
        ModelFamily::from_model("opencode-go/glm-5.2"),
        ModelFamily::Zhipu,
    );
    // `opencode-go/longcat-2.0` -- `longcat` is not in the issue's
    // enum, so it is correctly Unknown. New families can be added
    // without breaking callers.
    assert_eq!(
        ModelFamily::from_model("opencode-go/longcat-2.0"),
        ModelFamily::Unknown,
    );
}

/// The issue's bare-name coverage. The Claude CLI uses bare
/// `sonnet` / `opus` / `haiku`; openai CLI uses bare `gpt-*`; the
/// opencode cache carries similar aliases.
#[test]
fn bare_name_coverage() {
    assert_eq!(ModelFamily::from_model("sonnet"), ModelFamily::Anthropic);
    assert_eq!(ModelFamily::from_model("opus"), ModelFamily::Anthropic);
    assert_eq!(ModelFamily::from_model("haiku"), ModelFamily::Anthropic);
    assert_eq!(ModelFamily::from_model("Sonnet"), ModelFamily::Anthropic);
    assert_eq!(ModelFamily::from_model("OPUS"), ModelFamily::Anthropic);

    assert_eq!(
        ModelFamily::from_model("gpt-4o-2024-05-13"),
        ModelFamily::OpenAI
    );
    assert_eq!(ModelFamily::from_model("gpt-5-nano"), ModelFamily::OpenAI);

    assert_eq!(ModelFamily::from_model("glm-5.3-flash"), ModelFamily::Zhipu);
    assert_eq!(ModelFamily::from_model("kimi-k3"), ModelFamily::Moonshot);
    assert_eq!(
        ModelFamily::from_model("deepseek-v4-pro"),
        ModelFamily::Deepseek
    );
    assert_eq!(ModelFamily::from_model("qwen3.7-max"), ModelFamily::Qwen);
    assert_eq!(ModelFamily::from_model("grok-3"), ModelFamily::Grok);
    assert_eq!(ModelFamily::from_model("MiniMax-M3"), ModelFamily::MiniMax);
}

/// Whitespace and empty inputs never panic; they return Unknown.
#[test]
fn empty_and_whitespace_inputs() {
    assert_eq!(ModelFamily::from_model(""), ModelFamily::Unknown);
    assert_eq!(ModelFamily::from_model("   "), ModelFamily::Unknown);
    assert_eq!(ModelFamily::from_model("\t"), ModelFamily::Unknown);
    assert_eq!(ModelFamily::from_model("   qwen/foo   "), ModelFamily::Qwen); // trimmed
}

/// Unknown providers and models return Unknown (not an error).
#[test]
fn unknown_inputs_return_unknown() {
    assert_eq!(
        ModelFamily::from_model("custom/mystery"),
        ModelFamily::Unknown
    );
    assert_eq!(
        ModelFamily::from_model("some-mystery-provider/some-model"),
        ModelFamily::Unknown,
    );
    // Provider key only (no model) -- degenerate, but the parser
    // returns the provider's family (or Unknown).
    assert_eq!(
        ModelFamily::from_model("kimi-for-coding/"),
        ModelFamily::Moonshot
    );
    // Bare name with no known family.
    assert_eq!(ModelFamily::from_model("flamingo-7b"), ModelFamily::Unknown);
}

/// Drive the parser across every (provider, model) pair in the
/// checked-in opencode fixture and assert no panic, plus spot-check
/// the known mappings. The full family classification is asserted
/// separately per-provider.
#[test]
fn opencode_fixture_no_panic_and_spot_checks() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/judge/tests/fixtures/opencode-models.json"
    );
    let raw =
        std::fs::read_to_string(path).expect("opencode fixture readable; vendored 2026-09-11");
    let fixture: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    let providers = fixture
        .get("providers")
        .and_then(|v| v.as_object())
        .expect("fixture has 'providers' object");

    let mut checked = 0usize;
    for (provider, body) in providers {
        let families = body
            .get("families")
            .and_then(|v| v.as_object())
            .expect("provider has 'families' object");
        for (_opencode_family, ids) in families {
            let ids = ids.as_array().expect("family ids is array");
            for id in ids {
                let id = id.as_str().unwrap();
                let full = format!("{provider}/{id}");
                let _ = ModelFamily::from_model(&full);
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "fixture should have entries");

    // Spot-check the opencode-go multi-vendor routing.
    assert_eq!(
        ModelFamily::from_model("opencode-go/qwen3.7-max"),
        ModelFamily::Qwen,
    );
}

/// Banned prefix detection is case-insensitive and matches both
/// `opencode` (bare provider) and `Zen` (any model starting with
/// `zen` or `Zen`).
#[test]
fn banned_prefix_detection() {
    assert!(ModelFamily::is_banned_prefix("opencode"));
    assert!(ModelFamily::is_banned_prefix("opencode/whatever"));
    assert!(ModelFamily::is_banned_prefix("Zen"));
    assert!(ModelFamily::is_banned_prefix("zen-3"));
    assert!(ModelFamily::is_banned_prefix("  ZEN  "));

    assert!(!ModelFamily::is_banned_prefix("kimi-for-coding/k3"));
    assert!(!ModelFamily::is_banned_prefix(
        "anthropic/claude-sonnet-4-6"
    ));
    assert!(!ModelFamily::is_banned_prefix(""));
    assert!(!ModelFamily::is_banned_prefix("sonnet"));
}

/// BANNED_MODEL_PREFIXES is the canonical list and BannedModelPrefixes
/// iterates it. The parent #192 will add entries; this slice ships the
/// two the issue names (bare `opencode/` and `Zen`).
#[test]
fn banned_prefixes_constant() {
    let mut v: Vec<&str> = BannedModelPrefixes::iter().collect();
    v.sort();
    assert_eq!(v, vec!["Zen", "opencode"]);
    // The same list is exposed as a slice for callers that need the
    // raw `&[&str]`.
    assert_eq!(BANNED_MODEL_PREFIXES.len(), 2);
}

/// Serialisation shape -- the JSON contract that consumers (and the
/// parent #192) read.
#[test]
fn serialise_round_trip() {
    let s = serde_json::to_string(&ModelFamily::Moonshot).unwrap();
    assert_eq!(s, "\"moonshot\"");
    let s = serde_json::to_string(&ModelFamily::Unknown).unwrap();
    assert_eq!(s, "\"unknown\"");
    let round = serde_json::from_str::<ModelFamily>("\"anthropic\"").unwrap();
    assert_eq!(round, ModelFamily::Anthropic);
}
