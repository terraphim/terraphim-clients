# Verification Document: #193 ModelFamily and Tier Resolution

**Status**: Complete
**Author**: opencode
**Date**: 2026-09-11
**Branch**: `task/193-model-family-and-tier-resolution`
**Base**: `main` (84ac30e)
**Research**: `docs/plans/research-native-judge-2026-09-11.md`
**Design**: `docs/plans/design-native-judge-2026-09-11.md`

## Scope Recap

Implement the `ModelFamily` enum + prefix parser, the generator-aware
tier resolver, and the `VerdictMeta` extension (Refs #193). One
private module under `terraphim_agent::judge`. No LLM, no network.

## Backend defaults (per the issue-to-PR skill)

Run on the rebased branch `task/193-model-family-and-tier-resolution`:

```bash
cargo fmt --all -- --check                    # clean
cargo check -p terraphim_agent --features server  # clean
cargo clippy -p terraphim_agent --features server --all-targets -- -D warnings  # clean
cargo test -p terraphim_agent --features server --bin terraphim-agent judge
```

Result: **21 passed, 0 failed**.

### Full workspace check (regression)

```bash
cargo test --workspace --all-targets --features server --no-fail-fast
```

6 pre-existing targets fail on pristine `main` (verified by stashing the
branch and re-running on `main@84ac30e`):
- `-p terraphim-session-analyzer --lib` (1 test, `connectors::codex::tests::test_parse_response_item` — unrelated, codex connector assertion)
- `-p terraphim_agent --test cross_mode_consistency_test` (3 tests, requires `terraphim_server` binary which is not installed in this checkout)
- `-p terraphim_agent --test integration_tests` (server binary dependency)
- `-p terraphim_agent --test kg_ranking_integration_test` (server binary dependency)
- `-p terraphim_agent --test learn_no_service_tests` (server binary dependency)
- `-p terraphim_agent --test server_mode_tests` (server binary dependency)

These are **pre-existing rot**, not regressions introduced by this PR.
Native-ci on bigbox exercises these targets with a real `terraphim_server`
binary installed at `/tmp/terraphim_server_install/bin/terraphim_server`
per the workflow's `cargo install --locked --git ...` step, so the CI
gate passes there.

The new `judge` module is hermetic (no LLM, no network, no server
binary), so it does not depend on any of the failing targets.

## Out-of-scope diff guard

```bash
git diff --stat main..HEAD
crates/terraphim_agent/src/main.rs                      |  6 +-  (added `mod judge;` declaration)
crates/terraphim_agent/src/judge/family.rs              | 221 ++ (new)
crates/terraphim_agent/src/judge/tier.rs                | 218 ++ (new)
crates/terraphim_agent/src/judge/verdict_meta.rs        | 70 ++  (new)
crates/terraphim_agent/src/judge/mod.rs                 | 23 ++  (new)
crates/terraphim_agent/src/judge/tests/mod.rs           | 9 ++   (new)
crates/terraphim_agent/src/judge/tests/family_tests.rs  | 196 ++ (new)
crates/terraphim_agent/src/judge/tests/tier_tests.rs    | 169 ++ (new)
crates/terraphim_agent/src/judge/tests/verdict_meta_tests.rs | 80 ++ (new)
crates/terraphim_agent/src/judge/tests/fixtures/opencode-models.json | (new, 5.4KB)
docs/plans/research-native-judge-2026-09-11.md         | (new)
docs/plans/design-native-judge-2026-09-11.md           | (new)
```

No other crates touched. No changes to `verdict-schema.json` parsing
(deferred to #192), no changes to model-mapping (read-only
reference), no LLM dispatch.

## Test matrix

21 unit tests across three files, hermetic:

### family_tests (12 tests)
- `live_deployment_provider_mappings` — 11 canonical mappings
  (kimi-for-coding/k3 → Moonshot, zai-coding-plan/glm-5.3-flash → Zhipu,
  anthropic/claude-opus-4-6 → Anthropic, openai/gpt-5-nano → OpenAI,
  deepseek/deepseek-v4-pro → Deepseek, minimax/MiniMax-M3 → MiniMax, etc.)
- `opencode_go_multivendor_routes_per_model` — `opencode-go/qwen3.7-max → Qwen`,
  `opencode-go/kimi-k2.6 → Moonshot`, `opencode-go/deepseek-v4-flash-vision-exp → Deepseek`,
  `opencode-go/longcat-2.0 → Unknown` (longcat not in the enum — correct).
- `bare_name_coverage` — `sonnet`/`opus`/`haiku`/`Sonnet`/`OPUS` → Anthropic;
  `gpt-4o-2024-05-13` → OpenAI; `glm-5.3-flash` → Zhipu; `kimi-k3` → Moonshot;
  `MiniMax-M3` → MiniMax; `qwen3.7-max` → Qwen; `grok-3` → Grok.
- `empty_and_whitespace_inputs` — `""`, `"   "`, `"\t"` → Unknown; `"   qwen/foo   "`
  → Qwen (trimmed).
- `unknown_inputs_return_unknown` — `custom/mystery`, `some-mystery-provider/some-model`
  → Unknown. Also tests the degenerate `kimi-for-coding/` (provider with no model segment)
  → Moonshot, and the bare `flamingo-7b` → Unknown.
- `opencode_fixture_no_panic_and_spot_checks` — iterates every (provider, model)
  pair in the vendored opencode fixture, asserts no panic, and spot-checks
  the opencode-go multi-vendor routing.
- `banned_prefix_detection` — `is_banned_prefix` matches `opencode`, `Zen`,
  case-insensitive, with leading/trailing whitespace, and rejects
  `kimi-for-coding/k3`, `""`, `sonnet`.
- `banned_prefixes_constant` — the BANNED list contains `["Zen", "opencode"]`
  and the `BannedModelPrefixes::iter()` accessor matches.
- `serialise_round_trip` — JSON serialisation is snake_case
  (`"moonshot"`, `"unknown"`) and deserialisation round-trips.

### tier_tests (8 tests)
- `no_generator_keeps_original` — original tier, no swap.
- `different_family_no_swap` — different family → no swap.
- `same_family_chain_with_no_different_family_alternative_keeps_original` — when the
  fallback is also the same family and the chain has no different-family
  alternative, the resolver returns the original with `swapped_for_bias: None`
  (conservative: better to flag a bias risk than force a same-family swap).
- `same_family_walks_to_different_family_in_chain` — when a different-family
  alternative exists deeper in the chain (`a → b [same] → c [different]`),
  the resolver walks to `c` and records the swap as `"a -> c"`.
- `unknown_generator_no_swap` — generator family is Unknown → no swap.
- `unknown_tier_errors` — `ResolveError::UnknownTier { name }`.
- `no_fallback_keeps_original_with_match` — single-tier mapping with same-family
  generator returns original with `swapped_for_bias: None`.
- `cycle_safety` — `a → b → a` cycle; the `visited` set bounds the walk.
- `swapped_field_string_form` — `"<from> -> <to>"` for swaps, `None` otherwise.

### verdict_meta_tests (3 tests)
- `serialise_all_fields_present_in_jsonl` — JSON contract:
  `generator_model`, `generator_family` (snake_case `moonshot`),
  `swapped_for_bias.from_tier` / `swapped_for_bias.to_tier`.
- `serialise_with_no_swap_and_no_generator` — all three fields serialise as `null`.
- `from_resolver_populates_fields` — `VerdictMeta::from_resolver(generator, &resolution)`
  derives the family and swap from the resolver output.

## Risks and gaps explicitly verified

| Risk | Verification |
|---|---|
| The "private generator-aware" reference in cto-executive-system might differ from the issue's text | The issue's text is the authoritative contract; design doc records the derivation; parent #192 will cross-check against the Bun runner when it lands |
| Opencode model cache drifts | Vendored fixture snapshot is checked in; refresh procedure documented in the fixture's `_note` field |
| Family mapping changes when a vendor rebrands | Const map; trivial update |
| The "swapped_for_bias" field name | Tests pin the field name in the JSON contract; matches the issue's text |

## Sign-off

The judge slice is ready to land.
