# Design Document: Native Judge in terraphim-agent — #193 ModelFamily and Tier Resolution

**Status**: Draft
**Author**: opencode
**Date**: 2026-09-11
**Research**: `docs/plans/research-native-judge-2026-09-11.md`
**Reviewers**: (gate via `disciplined-quality-evaluation`)

## Executive Summary

First landed slice of #192: the `ModelFamily` enum + prefix parser, the
generator-aware tier resolver, and the verdict-meta extension that
records `generator_model / generator_family / swapped_for_bias`. One
private module under `terraphim_agent::judge`. No LLM, no network.
Unit-tested against a hermetic opencode model fixture plus a
bare-name table. The parent #192 will add the dispatch + subcommand +
REPL on top of this foundation.

## Goals (this PR)

- `ModelFamily` enum covering the vendor-level families named in #193:
  `Moonshot, Zhipu, Anthropic, OpenAI, Deepseek, Qwen, Grok, MiniMax, Unknown`.
- Parser `ModelFamily::from_model(&str) -> Self` for:
  - `provider/model` strings (split on `/`; left → family via the provider-table)
  - bare model names (bare-name table: `sonnet|opus|haiku|claude-*` →
    Anthropic, `gpt-*` → OpenAI, `glm-*` → Zhipu, `kimi-*` → Moonshot, etc.)
  - fallback: `Unknown`
- Provider-to-family table grounded in the live opencode cache and
  `terraphim-ai/AGENTS.md`:
  - `kimi-for-coding → Moonshot`
  - `zai-coding-plan → Zhipu`
  - `anthropic → Anthropic`
  - `openai → OpenAI`
  - `deepseek → Deepseek`
  - `minimax → MiniMax`
  - `claude* (e.g. `claude-code`) → Anthropic`
  - `qwen* → Qwen`
  - `grok* → Grok`
  - `opencode-go → per-model lookup` (the provider is multi-vendor; route by the model's opencode family)
  - else: keep parsing the model segment against the provider-table and the bare-name table
- `TierResolver::resolve(mapping, tier_name, generator: Option<&str>) -> TierResolution`
  that returns the resolved tier + an `swapped_for_bias: Option<SwapInfo>`.
- `TierResolution` and `SwapInfo` serialize to the verdict JSONL as
  `generator_model`, `generator_family`, `swapped_for_bias`. The
  `swapped_for_bias` field is a string `"<from_tier> -> <to_tier>"` when a
  swap happened, `null` otherwise.
- `BannedFamily` enum (or a list of `ModelFamily` values flagged
  banned in this build) with at least `Bare opencode/* / Zen` entries
  (per #192 "BANNED = bare opencode/ Zen prefix") — implemented as a
  `BANNED` constant on `ModelFamily` and a `check_banned` helper. The
  full lane hierarchy + runtime probe is in #192; this slice only
  introduces the type.
- Unit tests:
  - Parser table-driven tests against the opencode fixture + bare-name
    cases + Unknown fallback.
  - Provider-table tests asserting the live-deployment mappings
    (`kimi-for-coding/k3 → Moonshot`, etc.) per
    `terraphim-ai/AGENTS.md`.
  - TierResolver tests:
    - No generator → original tier, no swap.
    - Generator same family as tier model → swap to fallback; if
      fallback also same family → walk chain; if chain exhausted → keep
      original, `swapped_for_bias: None`.
    - Generator `Unknown` family → no swap.
    - Generator model is `claude-code` and tier is `claude` →
      Anthropic match → swap to `claude` fallback (none in mapping → keep).
- `VerdictMeta` extension struct (the three new fields) + a
  `VerdictMeta::new(...)` constructor; it composes with the parent #192's
  future verdict struct. The parent owns the full verdict shape; #193
  only ships the three new fields as a separate, embeddable struct so
  the parent can `#[serde(flatten)] VerdictMeta` into its full
  Verdict when it lands.

## Non-Goals (this PR)

Explicitly out of scope (deferred to the parent #192 and to follow-on
slices):

- LLM dispatch (opencode/claude/curl subprocess + NDJSON output
  parsing) — parent #192.
- Panel mode (every tier in sequence runs, unanimous GO, tiebreaker on
  split) — parent #192.
- Escalation mode (sequence, stop at first definitive or escalate to
  next) — parent #192.
- Model lane hierarchy (PRIMARY / FALLBACK / BANNED) + runtime probing
  (per-host, per-region) — parent #192.
- The `judge` subcommand + REPL command — parent #192.
- Embedding content + never --file attachments (the Bun runner's
  `buildPrompt`) — parent #192.
- Banned-model detection at startup (the full probe that refuses
  `opencode/` / `Zen` with a fatal error) — parent #192. This slice
  introduces the type and the constant so #192 can use them.
- Recorded-transcript integration tests against real CLIs — parent
  #192.

## Architecture

### Module location

`crates/terraphim_agent/src/judge/` (private module under the existing
binary crate).

```
crates/terraphim_agent/src/judge/
├── mod.rs                -- re-exports + the public surface for the parent #192
├── family.rs             -- ModelFamily enum + parser + provider-table + bare-name-table + BannedFamily constant
├── tier.rs               -- TierResolver, TierResolution, SwapInfo
├── verdict_meta.rs       -- VerdictMeta (the three new fields)
└── tests/
    ├── family_tests.rs   -- table-driven parser tests
    ├── tier_tests.rs     -- TierResolver tests
    └── fixtures/
        └── opencode-models.json  -- snapshot of relevant providers
```

Why a private module and not a new crate: the parent #192 will own the
`judge` subcommand and the public API; the new types are net-new today
and have no consumer outside `terraphim_agent`. Promoting to a workspace
crate is a one-line Cargo.toml move when (and only when) cross-crate
consumers emerge.

### Public surface (re-exported from `terraphim_agent::judge`)

```rust
// family.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    Moonshot,
    Zhipu,
    Anthropic,
    OpenAI,
    Deepseek,
    Qwen,
    Grok,
    MiniMax,
    Unknown,
}

impl ModelFamily {
    /// Parse a model identifier (e.g. "kimi-for-coding/k3", "sonnet",
    /// "claude-opus-4-6") into its vendor family. Never panics; unknown
    /// returns ModelFamily::Unknown.
    pub fn from_model(model: &str) -> Self;

    /// True if this family is in the BANNED list (bare opencode/* / Zen).
    /// The full BANNED lane probe is in #192; this is the cheap static
    /// pre-check the parent will call before each dispatch.
    pub fn is_banned(self) -> bool;
}

pub const BANNED_FAMILIES: &[ModelFamily] = &[]; // populated in #192
// or: pub const BANNED: &[&str] = &["opencode", "Zen"]; // model-prefix banned

// tier.rs
#[derive(Debug, Clone, Serialize)]
pub struct SwapInfo {
    pub from_tier: String,
    pub to_tier: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierResolution {
    pub tier: String,        // final tier name (after swap)
    pub model: String,       // final model
    pub family: ModelFamily, // family of the final model
    pub swapped_for_bias: Option<SwapInfo>,
}

pub struct TierResolver;

impl TierResolver {
    pub fn new() -> Self;
    /// Resolve the tier to evaluate. If a generator model is provided
    /// and the resolved tier's family matches the generator's family,
    /// walk the tier's `fallback` chain to find a same-tier-but-different-family
    /// alternative. If no compatible alternative exists, keep the
    /// original tier and return `swapped_for_bias: None`.
    pub fn resolve(
        &self,
        mapping: &ModelMapping,
        tier: &str,
        generator: Option<&str>,
    ) -> Result<TierResolution, ResolveError>;
}

pub struct ModelMapping {
    pub tiers: BTreeMap<String, TierConfig>,
}

pub struct TierConfig {
    pub cli: CliKind,           // opencode | claude | curl  (mirrors dispatch.ts)
    pub model: String,
    pub fallback: Option<String>,
    // (timeout_seconds, max_budget_usd, endpoint, requires_env: optional,
    //  carried through but not used by #193 — #192 will)
}

pub enum CliKind { Opencode, Claude, Curl }

// verdict_meta.rs
#[derive(Debug, Clone, Serialize)]
pub struct VerdictMeta {
    /// The model that produced the artefact being judged (None for
    /// standalone judge use).
    pub generator_model: Option<String>,
    pub generator_family: Option<ModelFamily>,
    /// `"<from_tier> -> <to_tier>"` when the resolver swapped to avoid
    /// same-family bias; `None` when no swap happened.
    pub swapped_for_bias: Option<SwapInfo>,
}
```

### Parser rules (`family.rs`)

`from_model(s: &str) -> ModelFamily`:

1. Trim. If empty, `Unknown`.
2. If `s` contains `/`:
   a. `provider = s.split('/').next()`; `model = s.split('/').nth(1)..join('/')`.
   b. `family = provider_table(provider)`.
   c. If `family == Unknown` AND `provider == "opencode-go"`, recurse on the model segment (opencode-go is multi-vendor).
   d. Otherwise return `family`.
3. If `s` is bare (no `/`):
   a. `family = bare_name_table(s)`.
   b. Return `family`.

Tables (const BTreeMap or a `match`):

- `provider_table`:
  - `kimi-for-coding` → Moonshot
  - `zai-coding-plan` → Zhipu
  - `anthropic` → Anthropic
  - `claude` / `claude-code` → Anthropic
  - `openai` → OpenAI
  - `deepseek` → Deepseek
  - `qwen` / `qwen-coder` → Qwen
  - `grok` / `x-ai` → Grok
  - `minimax` / `minimax-coding-plan` → MiniMax
  - `moonshot` / `zhipu` (bare provider names that map to the vendor) → Moonshot / Zhipu
  - else → Unknown
- `bare_name_table` (prefix match, case-insensitive):
  - `sonnet` / `opus` / `haiku` / `claude` / `claude-*` → Anthropic
  - `gpt-*` → OpenAI
  - `glm*` → Zhipu
  - `kimi*` → Moonshot
  - `deepseek*` → Deepseek
  - `qwen*` → Qwen
  - `grok*` → Grok
  - `MiniMax-*` / `minimax*` → MiniMax
  - else → Unknown

### Tier resolution (`tier.rs`)

```
resolve(mapping, tier, generator):
  cfg = mapping.tiers[tier]? else error
  if generator is None:
    return TierResolution { tier, model: cfg.model, family: from_model(cfg.model), swapped: None }
  gen_family = from_model(generator)
  if gen_family == Unknown:
    return ... no swap ...
  current_tier = tier
  current_cfg = cfg
  visited = {tier}
  while from_model(current_cfg.model) == gen_family:
    fallback = current_cfg.fallback?
    if fallback is None or fallback in visited:
      // no compatible alternative; keep original
      return TierResolution { tier: original, model: original_model, family: original_family, swapped: None }
    visited.add(fallback)
    return TierResolution {
      tier: fallback,
      model: mapping.tiers[fallback].model,
      family: from_model(mapping.tiers[fallback].model),
      swapped: SwapInfo { from_tier: original_tier, to_tier: fallback },
    }
  // tier model family != gen_family; no swap needed
  return TierResolution { tier, model: cfg.model, family: from_model(cfg.model), swapped: None }
```

Notes:
- `visited` defends against cycles in the fallback chain (the live
  mapping has no cycles, but a misconfigured mapping should not loop).
- The chain walk is at most O(N) where N is the chain length; the live
  mapping's longest chain is 2 (`deep → deep_alt`).
- `swapped_for_bias` is a string `"<from> -> <to>"` on serialise, `None`
  when no swap.

### Verdict meta serialisation (`verdict_meta.rs`)

`VerdictMeta` serialises with `#[serde(rename_all = "snake_case")]` so the
JSONL output keys are `generator_model`, `generator_family`,
`swapped_for_bias` (string or null). The parent #192 will `#[serde(flatten)]`
this into its full `Verdict` struct when it lands.

## File-by-file plan

### `crates/terraphim_agent/src/judge/mod.rs` (new)

Re-exports the public surface. `pub mod family; pub mod tier; pub mod verdict_meta;` and `pub use` the key types.

### `crates/terraphim_agent/src/judge/family.rs` (new)

- `ModelFamily` enum (Copy, Eq, Hash, Serialize, snake_case).
- `ModelFamily::from_model(&str) -> Self` (the parser).
- Private `provider_table()` and `bare_name_table()` (const maps or
  match arms).
- `BannedFamily` constant (`pub const BANNED_FAMILIES: &[ModelFamily]` —
  empty in this PR; #192 will populate the real banned list; OR — see
  "Open question" below — `pub const BANNED_MODEL_PREFIXES: &[&str] = &["opencode", "Zen"]`).
- `is_banned(self) -> bool` checks the constant.

### `crates/terraphim_agent/src/judge/tier.rs` (new)

- `CliKind`, `TierConfig`, `ModelMapping` types.
- `TierResolver::resolve(...)`.
- `TierResolution`, `SwapInfo`.
- `ResolveError` enum (`UnknownTier { name: String }`).

### `crates/terraphim_agent/src/judge/verdict_meta.rs` (new)

- `VerdictMeta` struct + `VerdictMeta::new(...)` constructor.
- (No tests in this file; the serialise behaviour is tested via the
  family/tier modules' tests where it is composed.)

### `crates/terraphim_agent/src/judge/tests/fixtures/opencode-models.json` (new)

- 8-provider snapshot from `~/.cache/opencode/models.json` (5.4KB).
- Captured 2026-09-11 from the live cache; refresh procedure documented
  in the file's `_note` field.

### `crates/terraphim_agent/src/judge/tests/family_tests.rs` (new)

- `provider_table_live_deployment` — asserts the canonical mappings
  (`kimi-for-coding/k3 → Moonshot`, `zai-coding-plan/glm-5.3-flash → Zhipu`,
  `anthropic/claude-opus-4-6 → Anthropic`, `openai/gpt-5-nano → OpenAI`,
  `deepseek/deepseek-v4-pro → Deepseek`, `minimax/MiniMax-M3 → MiniMax`).
- `parser_from_model` — table-driven across the opencode fixture
  (per-provider, per-family) plus a hand-picked set of bare names
  (`sonnet`, `gpt-4.1-nano`, `kimi-k3`, `glm-5.3-flash`,
  `MiniMax-M3`, `unknown-model-xyz` → Unknown, `""` → Unknown, `   ` →
  Unknown).
- `opencode_go_multivendor` — `opencode-go/qwen3.7-max → Qwen`,
  `opencode-go/kimi-k2.6 → Moonshot`, `opencode-go/deepseek-v4-flash-vision-exp → Deepseek`,
  `opencode-go/longcat-2.0 → Unknown` (longcat not in the enum; `Unknown` is
  correct per the design).
- `banned_family_constant_is_stable` — asserts the BANNED constant is
  declared and currently empty (placeholder for #192).

### `crates/terraphim_agent/src/judge/tests/tier_tests.rs` (new)

Uses a small in-test `ModelMapping` fixture (not the on-disk opencode fixture) so the resolver tests are deterministic and don't depend on the opencode snapshot.

- `no_generator_keeps_original` — tier `quick` (kimi-for-coding/kimi-for-coding-highspeed, family Moonshot) with no generator → original tier, no swap.
- `same_family_swap_to_fallback` — tier `quick` with generator `kimi-for-coding/kimi-for-coding` (Moonshot) → swap to `quick_alt` (kimi-for-coding/kimi-for-coding-highspeed, family Moonshot). Expected: `swapped_for_bias: Some(quick -> quick_alt)` and the result tier is `quick_alt`. But `quick_alt` is also Moonshot — the chain should walk further; mapping has no `quick_alt.fallback`, so it returns the original with `swapped: None`. The test asserts that conservative behaviour.
- `same_family_swap_succeeds` — tier `deep` (kimi-for-coding/k3, Moonshot) with generator `kimi-for-coding/kimi-for-coding` (Moonshot) → swap to `deep_alt` (opencode-go/kimi-k2.6, Moonshot). Also same family. Walk to `None` fallback. Test asserts the conservative fall-back (keep original, `swapped: None`) and records this as a known limit of the live mapping (deeper chains would need fallback config in `model-mapping.json`).
- `different_family_no_swap` — tier `deep` (kimi-for-coding/k3, Moonshot) with generator `openai/gpt-5-nano` (OpenAI) → no swap.
- `unknown_generator_no_swap` — generator `custom/mystery` (Unknown) → no swap.
- `unknown_tier_error` — `mapping.tiers.get("nonexistent")` → `Err(UnknownTier { name: "nonexistent" })`.
- `cycle_safety` — a synthetic mapping with a cycle (`a -> b -> a`) does not loop; returns the original with `swapped: None`.

### Integration

- `crates/terraphim_agent/src/lib.rs` (or `main.rs`): add `pub mod judge;` near the other module decls. No new CLI surface in this PR.

## Test strategy

Per the backend defaults: `cargo fmt --check`, `cargo check`,
`cargo check --tests`, `cargo clippy --all-targets -D warnings`,
`cargo test --lib`, `cargo test --bin terraphim-agent`. The judge module is
private to `terraphim_agent`; its tests live in the same crate. No
integration tests with live CLIs (out of scope for this PR; that's
#192).

## Risks and open questions

### Risks
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| The "private generator-aware" reference (referenced from #193) uses a different `swapped_for_bias` field shape (object, not string) | Med | Med | The issue text gives a string example ("tier:deep -> tier:deep_alt"). Document the field shape and version it (`swapped_for_bias: string | null`); #192 can revise when the real ref surfaces. |
| The Bun runner adopts generator-aware before the parent #192 lands | Low | Low | Issue #192's `Closes the retrieval half of #202` pattern means the contract is set; this slice matches the issue's text exactly. |
| A vendor rebrands the opencode provider key (e.g. `kimi-for-coding` → `moonshot-kimi`) | Low | Low | The provider table is a const — trivial update. The bare-name table is the safety net. |

### Open Questions
1. **Banned representation** — the issue names "bare opencode/ Zen prefix" as banned. Is the banned set a list of `ModelFamily` values, or a list of *model-prefix strings*? **Tentative: BANNED_MODEL_PREFIXES as a `&[&str] = &["opencode", "Zen"]` constant** (string-based, matches the issue's wording). #192 will expand this. If the real schema/banned list is family-based, the constant is trivial to change.

### Assumptions (carried from research)
- The verdict JSONL's `generator_*` and `swapped_for_bias` fields are
  *additional* to `verdict-schema.json` (not replacements).
- The live opencode cache is a stable, acceptable test fixture source.
- `kimi-for-coding → Moonshot` and `zai-coding-plan → Zhipu` are
  authoritative per `terraphim-ai/AGENTS.md`.
- The "private generator-aware" reference in cto-executive-system does
  not contradict the issue's text (best we can do without seeing it).

## Artefact links

- Research: `docs/plans/research-native-judge-2026-09-11.md`
- Issue #192 body
- Issue #193 body
- `verdict-schema.json` (read-only reference)
- `model-mapping.json` (read-only reference)
- `~/.cache/opencode/models.json` (read-only reference; snapshot in
  `crates/terraphim_agent/src/judge/tests/fixtures/opencode-models.json`)
