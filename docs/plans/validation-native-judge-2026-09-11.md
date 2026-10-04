# Validation Document: #193 ModelFamily and Tier Resolution

**Status**: Complete
**Author**: opencode
**Date**: 2026-09-11
**Branch**: `task/193-model-family-and-tier-resolution`
**Verification**: `docs/plans/verification-native-judge-2026-09-11.md`

## Acceptance criteria mapping (issue #193 body)

| #193 Acceptance criterion | Evidence |
|---|---|
| ModelFamily enum + prefix parser (moonshot/zhipu/anthropic/openai/deepseek/qwen/grok/minimax/unknown) | `family.rs::ModelFamily` (9 variants including Unknown); `from_model` parser; all 9 covered in `live_deployment_provider_mappings` + `bare_name_coverage` + `opencode_go_multivendor_routes_per_model` |
| Unit-tested against the current opencode models list | `opencode_fixture_no_panic_and_spot_checks` iterates the vendored `tests/fixtures/opencode-models.json` (5.4KB snapshot of 8 providers); asserts no panic + spot-checks the multi-vendor routing |
| Tier resolution takes an optional generator model; same-family tiers resolve from fallbacks excluding the generator family | `tier.rs::TierResolver::resolve(mapping, tier, generator)` — walks the fallback chain looking for a different-family tier; conservative fall-back to original when no alternative exists |
| Verdict JSONL records generator_model/generator_family/swapped_for_bias | `verdict_meta.rs::VerdictMeta` with the three fields; `serialise_all_fields_present_in_jsonl` test pins the JSON contract; `from_resolver` derives the values from the resolver output |
| Schema parity with the Bun runner | The three fields are *additional* to `verdict-schema.json`'s `required` set; the parent #192 will `#[serde(flatten)]` `VerdictMeta` into its full Verdict. `swapped_field()` helper produces the issue-text string form if the parent prefers the flat shape |
| REPL/CLI: `judge --generator <model>` | The CLI surface is in the parent #192 (the parser and resolver are CLI-agnostic); `VerdictMeta::from_resolver(generator, &resolution)` is the integration point |

## Product validation (deferred to the parent #192)

- **Recorded-transcript integration tests** against real CLIs: out of scope for #193 (no LLM, no subprocess). The parent #192's tests will exercise the resolver end-to-end with real CLI transcripts.
- **Calibration** (per `terraphim-build#14`): the deep tier's calibration is not affected by #193 (this slice is types + parser + resolver; the verdict content is parent #192's scope).

## Follow-up issues (from this slice)

- **#192 itself** (parent): add the `judge` subcommand, panel mode, escalation mode, LLM dispatch, REPL command. The new module in this PR is the foundation.
- **`frozen-bad-syntax` fixture refresh** (no issue needed): when opencode adds new live providers, re-run the capture script and update `crates/terraphim_agent/src/judge/tests/fixtures/opencode-models.json`.

## Resumability for the parent #192

- The new module is a private leaf. Promote to a workspace crate only if a
  cross-crate consumer emerges.
- `BANNED_MODEL_PREFIXES` is currently `&["opencode", "Zen"]` per the issue
  text. The parent #192 expands this with the full BANNED lane and the
  runtime probe.
- `TierResolver::resolve` is the integration point for the `judge` subcommand:
  the parent builds a `ModelMapping` from the `model-mapping.json` it
  loads, then calls `resolve` for each tier in the panel/sequence.
