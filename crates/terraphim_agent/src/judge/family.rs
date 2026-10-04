//! `ModelFamily` enum and model-name parser (Refs #193).
//!
//! The enum is the **vendor-level** family: the underlying provider
//! behind an opencode subscription or CLI model identifier. The parser
//! accepts both `provider/model` strings (e.g. `kimi-for-coding/k3`,
//! `opencode-go/qwen3.7-max`) and bare model names (e.g. `sonnet`,
//! `gpt-5-nano`) and maps them to the enum. The mapping is grounded in
//! the live opencode model cache and `terraphim-ai/AGENTS.md`; see
//! `docs/plans/research-native-judge-2026-09-11.md` for the derivation.
//!
//! This is the first Rust `ModelFamily` concept in the org. The parent
//! #192 (native judge subcommand) will use it to enforce the model lane
//! hierarchy (`PRIMARY` / `FALLBACK` / `BANNED`) and to record the
//! generator-aware swap in the verdict JSONL.

use serde::{Deserialize, Serialize};

/// Vendor-level model family.
///
/// `Unknown` is the catch-all for any model that does not parse against
/// the provider or bare-name tables. New families can be added without
/// breaking callers (the enum is non-exhaustive in spirit — callers
/// should treat `Unknown` as the safe default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ModelFamily {
    /// Moonshot AI (kimi-for-coding subscriptions, kimi-* models).
    Moonshot,
    /// Zhipu AI (zai-coding-plan, glm-*).
    Zhipu,
    /// Anthropic (claude, sonnet, opus, haiku).
    Anthropic,
    /// OpenAI (gpt-*).
    OpenAI,
    /// Deepseek.
    Deepseek,
    /// Alibaba Qwen.
    Qwen,
    /// xAI Grok.
    Grok,
    /// MiniMax.
    MiniMax,
    /// Unrecognised model; no known vendor mapping.
    Unknown,
}

/// Model-prefix strings that are **banned** in the judge workflow (Refs
/// #192, "BANNED = bare opencode/ Zen prefix (refuse fatally)"). The
/// parent #192 expands this with the full BANNED lane and the runtime
/// probe. The `ModelFamily::is_banned_prefix` static check is the
/// cheap pre-flight the parent calls before each dispatch.
pub(crate) const BANNED_MODEL_PREFIXES: &[&str] = &["opencode", "Zen"];

/// Read-only view of the banned prefix list, exposed for callers that
/// need to enumerate it (e.g. the parent #192's startup probe).
pub(crate) struct BannedModelPrefixes;

impl BannedModelPrefixes {
    /// Iterate the banned model prefixes.
    pub(crate) fn iter() -> impl Iterator<Item = &'static str> {
        BANNED_MODEL_PREFIXES.iter().copied()
    }
}

impl ModelFamily {
    /// Parse a model identifier into its vendor family.
    ///
    /// Accepts:
    /// - `provider/model` strings: split on `/`; the provider goes through
    ///   the provider-table; `opencode-go/<x>` recurses on the model
    ///   segment because that provider is multi-vendor.
    /// - Bare model names: prefix-matched against the bare-name table.
    /// - Empty / whitespace: `Unknown`.
    ///
    /// Never panics. Unrecognised inputs return `ModelFamily::Unknown`.
    pub(crate) fn from_model(model: &str) -> Self {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            return Self::Unknown;
        }
        if let Some((provider, rest)) = trimmed.split_once('/') {
            let provider_family = provider_table(provider);
            if provider_family == Self::Unknown && provider == "opencode-go" {
                return Self::from_model(rest);
            }
            return provider_family;
        }
        bare_name_table(trimmed)
    }

    /// Cheap static check: does this model's prefix match a banned
    /// entry? The parent #192 calls this before each dispatch to fail
    /// fast on a misconfigured generator or tier. Case-insensitive.
    pub(crate) fn is_banned_prefix(model: &str) -> bool {
        let trimmed = model.trim().to_ascii_lowercase();
        BANNED_MODEL_PREFIXES
            .iter()
            .any(|b| trimmed.starts_with(&b.to_ascii_lowercase()))
    }
}

/// Provider-to-family table. `opencode-go` is a multi-vendor provider
/// and is handled specially in `from_model` (recurses on the model
/// segment). All other providers are looked up here.
fn provider_table(provider: &str) -> ModelFamily {
    // Lowercase once, then match. Empty after split_once already handled.
    let p = provider.to_ascii_lowercase();
    match p.as_str() {
        // Moonshot subscriptions
        "kimi-for-coding" | "moonshot" => ModelFamily::Moonshot,
        // Zhipu subscriptions
        "zai-coding-plan" | "zhipu" => ModelFamily::Zhipu,
        // Anthropic
        "anthropic" | "claude" | "claude-code" => ModelFamily::Anthropic,
        // OpenAI
        "openai" => ModelFamily::OpenAI,
        // Deepseek
        "deepseek" => ModelFamily::Deepseek,
        // Alibaba Qwen (qwen-coder is the same vendor's coding variant)
        "qwen" | "qwen-coder" => ModelFamily::Qwen,
        // xAI Grok
        "grok" | "x-ai" => ModelFamily::Grok,
        // MiniMax
        "minimax" | "minimax-coding-plan" => ModelFamily::MiniMax,
        // Unknown provider -- caller (from_model) decides whether to
        // recurse on the model segment (e.g. opencode-go case).
        _ => ModelFamily::Unknown,
    }
}

/// Bare-model-name table. Prefix match, case-insensitive, longest-prefix
/// wins. The check is a single pass through a small array; cost is
/// negligible (these are the only ones the parent #192 will hit, given
/// `verdict-schema.json` and the live model-mapping.json).
fn bare_name_table(name: &str) -> ModelFamily {
    let lower = name.to_ascii_lowercase();
    // Order matters only when one model name is a prefix of another
    // (e.g. `claude-opus-4-6` must match the `claude-*` Anthropic rule
    // before the generic `gpt-*` etc.). The patterns below are
    // disjoint, so the order is for clarity.
    if lower.starts_with("claude") || matches_anthropic_bare(&lower) {
        ModelFamily::Anthropic
    } else if lower.starts_with("gpt") {
        ModelFamily::OpenAI
    } else if lower.starts_with("glm") {
        ModelFamily::Zhipu
    } else if lower.starts_with("kimi") {
        ModelFamily::Moonshot
    } else if lower.starts_with("deepseek") {
        ModelFamily::Deepseek
    } else if lower.starts_with("qwen") {
        ModelFamily::Qwen
    } else if lower.starts_with("grok") {
        ModelFamily::Grok
    } else if lower.starts_with("minimax") {
        ModelFamily::MiniMax
    } else {
        ModelFamily::Unknown
    }
}

fn matches_anthropic_bare(lower: &str) -> bool {
    // The opencode cache carries bare aliases `sonnet`, `opus`, `haiku`
    // (e.g. `anthropic/claude-sonnet-4-6` AND `claude/sonnet` both exist
    // in some provider lists). The Claude CLI also accepts bare
    // `sonnet` / `opus` / `haiku`. Recognise them.
    lower == "sonnet" || lower == "opus" || lower == "haiku"
}
