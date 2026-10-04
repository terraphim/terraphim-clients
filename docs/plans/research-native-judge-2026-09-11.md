# Research Document: Native Judge in terraphim-agent (#192 / #193)

**Status**: Draft
**Author**: opencode
**Date**: 2026-09-11
**Session**: `.agent/sessions/2026-09-11-native-judge-terraphim-agent.md`
**Reviewers**: (gate via `disciplined-quality-evaluation`)

## Executive Summary

The Bun and bash judge runners in `cto-executive-system` and `terraphim-skills`
work, but the rest of the org shells to them via `claude -p` / `opencode` with
ad-hoc scripts. The CTO direction (decision 2026-09-05) is to consolidate on a
native Rust `terraphim-agent judge` subcommand. This research supports the
first landed slice — **#193, the `ModelFamily` map and generator-aware tier
resolution** — and produces enough design for it to ship as one PR.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | The B1/B3 grade seam (judge is the only LLM-as-judge code path still in Bun); /evolve + disciplined skills depend on it |
| Leverages strengths? | Yes | Rust + the existing `terraphim_automata` / `terraphim_hooks` crates + terraphim-grep KG machinery are the org's strongest surface for a canonical judge |
| Meets real need? | Yes | /evolve, disciplined-skills phase gates, and `terraphim-build#11` JudgeValidator all consume this; #192 is blocking adoption |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

`terraphim-agent` (the workspace binary) does not have a `judge` subcommand.
Today, every consumer (`/evolve`, `disciplined-skills`, `terraphim-build#11`)
shells to a separate Bun (`cto-executive-system/automation/judge/run-judge.ts`)
or bash (`terraphim-skills/automation/judge/run-judge.sh`) runner that reads
`model-mapping.json`, dispatches to opencode/claude CLIs, and emits a JSON
verdict. The split runner surface means:

- the verdict contract is interpreted in two languages (Bun + bash) and two
  repos, so any change to the schema or model taxonomy has to ship to both;
- the model-lane hierarchy (PRIMARY / FALLBACK / BANNED) and the
  generator-aware swap (avoid judging the generator's own output) are
  policy decisions buried in the Bun runner; there is no native enforcement.

### Impact

Three named consumers (`/evolve` automatic rule promotion, disciplined-skills
phase gates, `terraphim-build#11` JudgeValidator) plus any agent that wants
to grade an artefact. The CTO direction (2026-09-05) is to make
`terraphim-agent judge` the canonical surface.

### Success Criteria (for the parent #192)

- New subcommand `terraphim-agent judge <files...> [--profile <name>] [-d description]` + REPL command.
- Three-dimension rubric (Semantic/Pragmatic/Syntactic, 1-5); verdict JSONL
  compatible with `cto-executive-system/automation/judge/verdict-schema.json`
  (round, judge_tier, judge_model, timestamp, file, task_id, consensus, human_override).
- Panel + escalation modes.
- Model lane hierarchy + runtime guards (PRIMARY / FALLBACK / BANNED);
  BANNED `opencode/` / `Zen` model IDs refused fatally.
- embed judged content in the prompt; never use --file attachments.
- `cargo test green`; panel + escalation covered by recorded-transcript tests
  (no mocks); verdict JSONL round-trips against the schema.

### Success Criteria (for this slice, #193)

- `ModelFamily` enum + prefix parser.
- Tier resolution takes an optional generator model; same-family tiers
  resolve from fallbacks excluding the generator family.
- Verdict JSONL records `generator_model`, `generator_family`,
  `swapped_for_bias`.
- REPL/CLI: `judge --generator <model>`.
- Unit-tested against the current opencode models list (live cache).

## Current State Analysis

### Existing Implementations (read this session)

**`/Users/alex/cto-executive-system/automation/judge/`** (167 lines, Bun):
- `run-judge.ts`: thin CLI over `dispatch.ts`. Resolves tier(s) from the
  profile; panel mode = run every tier, GO iff unanimous, tiebreaker on split,
  UNDETERMINED escalates; sequence mode = first definitive or escalate.
- `dispatch.ts`: model-mapping types, prompt building (content embedded with
  truncation at `defaults.max_content_chars`), CLI dispatch (opencode / claude /
  curl).
- `model-mapping.json`: canonical mapping with `tiers.{quick,quick_alt,deep,
  deep_alt,tiebreaker,oracle,proxy}` and `profiles.{pre-push,task-review,
  calibration,legacy,evolution}`. Evolution is the only panel-mode profile.
- `verdict-schema.json`: the canonical contract (139 lines). `required`:
  `commit, timestamp, verdict, scores{semantic,pragmatic,syntactic},
  files_evaluated`. Verdict enum: `GO|NO-GO|UNDETERMINED`. `judge_model`,
  `judge_tier`, `judge_cli`, `judge_profile`, `round`, `latency_ms`,
  `reasoning_certificate`, `certificate_valid` are present but optional.

**`/Users/alex/projects/terraphim/terraphim-skills/automation/judge/run-judge.sh`** (717 lines, bash):
- Mostly the same contract expressed in bash + curl. Useful as a second
  reference but the Bun runner is the authoritative current shape.

**`terraphim-skills#84/#85`**: learnings behind "never use --file
attachments; embed content" (per #192 body). Confirmed by the Bun runner's
`buildPrompt` which inlines `content.slice(0, maxChars)` into the template.

### Notable: generator-aware logic is NOT in the Bun runner yet

The issue #193 body says "schema parity with the Bun runner — see the private
`cto-executive-system` generator-aware issue". That file is not on this
machine. So the generator-aware contract is mine to derive from the issue
text:

- `judge --generator <model>` records `generator_model`, `generator_family`,
  `swapped_for_bias` in the verdict.
- "same-family tiers resolve from fallbacks excluding the generator family"
  — if the resolved tier's model has the same family as the generator,
  swap to a different-family fallback (or walk the chain until different).

### Existing Code in `terraphim-clients` (this repo)

No existing `ModelFamily` enum, no LLM-provider parsing. `terraphim_agent`
is the binary crate; it already depends on `terraphim_automata`,
`terraphim_hooks`, etc. and dispatches the offline + server + TUI commands
(docs/src/llm.md is not present in this repo — the judge types are net-new).

The org DOES have provider/config code in `terraphim-ai`:
- `terraphim_spawner/src/config.rs::normalise_claude_model` — CLI/model-string
  handling, not family parsing.
- `terraphim_symphony` — workflow runner, not LLM taxonomy.
- `docs/plans/design-terraphim-proxy-routing-2026-08-25.md` has the live
  allow-list: `claude-code`, `opencode-go`, `kimi-for-coding`,
  `minimax-coding-plan`, `openai`, `zai-coding-plan`, `terraphim-proxy`.
- `docs/plans/design-adf-route-canary-2026-08-19.md` identifies
  `kimi-for-coding/k3` as the live deployment probe (BIGBOX_KIMI_ROUTE_OK);
  `kimi-for-coding/k2p5` is obsolete.

None of these are reusable Rust types — they are routing config strings.
#193 introduces the first Rust `ModelFamily` concept.

### Live Opencode Model Cache (grounding the unit tests)

`/Users/alex/.cache/opencode/models.json` — 213 providers, 172 opencode
`family` values. Each entry has `id, family, release_date, modalities, ...`.
The opencode `family` is a per-MODEL grouping (e.g. `kimi-k3`, `glm`,
`claude-opus`, `gpt-nano`, `minimax`); the **issue's `ModelFamily` is the
vendor/org** (Moonshot, Zhipu, Anthropic, OpenAI, Deepseek, Qwen, Grok,
MiniMax, Unknown). The parser must map opencode families → vendor families
via a small lookup table grounded in the cache.

### Code Locations (planned for this slice)

| Component | Location | Purpose |
|-----------|----------|---------|
| `ModelFamily` enum | `crates/terraphim_agent/src/judge/family.rs` (new module) | Vendor-level family enum + parser |
| `TierResolver` | `crates/terraphim_agent/src/judge/tier.rs` (new) | Resolves tier from mapping, generator-aware swap |
| `VerdictMeta` extension | `crates/terraphim_agent/src/judge/verdict.rs` (new) | The `generator_model / generator_family / swapped_for_bias` fields (schema parity — these are *additional* fields, not a breaking change) |
| Unit tests | same files, `#[cfg(test)]` mod | Opencode cache + model-mapping fixtures |

Placement: a private module under `terraphim_agent::judge`. The parent #192
will add the `judge` subcommand, the LLM dispatch, the panel mode, and the
REPL command. The module is a leaf today; if cross-crate use emerges the
parent can promote it to a workspace crate. Minimal blast radius.

## Constraints

### Technical
- Rust workspace with a 9-crate member list. `terraphim_agent` is the binary
  and the only sensible home for a `judge` subcommand per the issue.
- `terraphim_automata`, `terraphim_hooks`, `terraphim_sessions`,
  `terraphim_grep` are the existing crates to leverage for output parsing
  and KG-aware validate-style checks (per #192 scope). Out of scope for #193.
- No LLM SDK dependency. #193 only adds a type + parser; no network.

### Business
- Schema parity with `verdict-schema.json` is non-negotiable. The
  generator-aware fields are *additional* to the schema's required set,
  not breaking.
- Verdict JSONL must round-trip (consumers read it; field order/names matter).
- Calibration (per `terraphim-build#14`): the parent #192's deep tier must be
  calibrated before unattended gating. #193 only affects routing, not
  verdict content, so calibration is unaffected by this slice.

### Non-Functional
- Unit tests must be hermetic (no network). The opencode cache is read from
  `~/.cache/opencode/models.json` IF present at test time, otherwise from a
  in-tree fixture (`tests/fixtures/opencode-models.json`) snapshot. Both
  paths are covered.
- Parsing must be allocation-light: model name parsing happens on every
  verdict emit. The parser is `&str` → `ModelFamily` (Copy enum, no heap).
- `tier_resolver` is pure (no I/O); the only input is the parsed mapping,
  the tier name, and the optional generator. Pure function → trivial tests.

## Vital Few (Essentialism)

### Essential Constraints (Max 3)
1. **Schema parity with `verdict-schema.json`** — the existing Bun runner
   emits this schema; every field, every enum. Breaking parity breaks the
   /evolve and disciplined-skills consumers.
2. **Generator-aware swap must be explicit, not implicit** — the issue names
   three fields (`generator_model`, `generator_family`, `swapped_for_bias`)
   that must appear in the verdict. Hidden swaps that don't surface in the
   verdict break audit trails and calibration (`terraphim-build#14`).
3. **Hermetic unit tests against the opencode model list** — the issue says
   "unit-tested against the current opencode models list". The tests must
   pass without network, against a real model list, and the family
   mapping must be grounded in that list (not arbitrary).

### Eliminated from Scope (5/25)
- LLM dispatch (opencode/claude/curl subprocess + JSONL output) → #192.
- Panel/escalation mode logic → #192.
- Model lane hierarchy (PRIMARY/FALLBACK/BANNED) + runtime probing → #192.
- Verdict content (scores, findings, reasoning_certificate) → #192.
- The `judge` subcommand CLI surface and REPL command → #192.

These are explicitly carved out of #193 and the gate review of #193 is
allowed to fast-track them to "future slices" — see the design doc.

## Dependencies

### Internal
| Dependency | Impact | Risk |
|------------|--------|------|
| None (this slice) | New types only; no crate deps added | Low — no API surface yet |

### External
| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| serde (for VerdictMeta serialise) | already in workspace | Low | n/a |
| `~/.cache/opencode/models.json` (test input) | snapshot at test time | Low — read-only fixture, refreshed manually if it changes | n/a |

## Risks and Unknowns

### Known Risks
| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| The "private generator-aware" reference differs from what the issue text implies | Med | Med | Derive the contract from the issue text + the opencode family mapping; document the derivation in the design doc; the parent #192 can revise if the real spec surfaces |
| Opencode model cache drifts between machines | Med | Low | Use a checked-in fixture for the unit tests; treat the live cache as an opt-in |
| A future provider (`qwen-direct`, `grok-x`, `mistral`) needs a new family that isn't in the enum | High | Low | The enum has `Unknown` as the catch-all; new families can be added without breaking callers |
| The "swapped_for_bias" field name is the real schema | Med | Med | The issue text names it explicitly; treat as authoritative; the parent #192 cross-checks against the Bun runner's emit when it lands |
| The provider-prefix mapping (e.g. `kimi-for-coding` → Moonshot) changes when a vendor is re-acquired | Low | Low | The mapping is a small const table in the module; trivial to update |

### Open Questions
1. **Is `swapped_for_bias` always a string, or sometimes a structured object?** The issue text says "tier:deep -> tier:deep_alt" in prose, suggesting a string. The schema-permitting fields are flexible. **Decision: string, format `"<from_tier> -> <to_tier>"`. Document the format in the field doc-comment.**
2. **What happens when the resolved tier's fallback is also same-family?** The issue says "walk fallbacks excluding the generator family". The chain `deep -> deep_alt` is two long in the live mapping; deeper chains are unlikely but possible. **Decision: walk the fallback chain; if exhausted, return the original tier and mark `swapped_for_bias: null` (no compatible alternative).**
3. **What if the generator model is `Unknown` family (e.g. `custom/my-model`)?** The issue doesn't say. **Decision: a same-family swap requires the generator family to be a known vendor; `Unknown` means no swap. Safer (no false positives).**
4. **Bare model names like `sonnet` (claude CLI) — do they need a family?** Yes; the issue's family enum + the parent #192's `--generator` accept any string. **Decision: the parser has a small bare-name table (sonnet/opus/haiku → Anthropic; gpt-4* → OpenAI; etc.) before falling back to Unknown.**

### Assumptions Explicitly Stated
| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| The verdict JSONL's `generator_*` and `swapped_for_bias` fields are additional, not breaking | Issue text lists them as fields to record, not as schema replacements | If the real schema replaces existing fields, parent #192 must reconcile | No — derive-only |
| The `kimi-for-coding` provider is Moonshot | Opencode cache + `terraphim-ai/AGENTS.md` ("kimi-for-coding/k2p6 -- Moonshot subscription") | Wrong if Moonshot rebrands | Yes (AGENTS.md) |
| `zai-coding-plan` is Zhipu | `terraphim-ai/AGENTS.md` ("zai-coding-plan/...") + model-mapping.json | Same | Yes |
| The opencode `family` field is a stable, per-model string | Opencode cache shape (172 families, string values) | If opencode changes the schema, the tests' fixture needs refresh | No — runtime dep, fixture snapshots |

### Multiple Interpretations Considered
| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| `ModelFamily` is the opencode `family` field (e.g. `kimi-k3`, `glm`, `claude-opus`) | Grounded in the opencode cache; 172 values | Rejected: the issue names 9 VENDOR-level families explicitly; opencode families are per-model. The mapping is opencode-family → issue-family, and the issue enum is the org's surface. |
| `ModelFamily` is the opencode provider key (e.g. `kimi-for-coding`) | Mirrors the proxy allow-list; matches the model string's left side | Rejected for #193: provider keys are subscription/plan names, not vendor families. The provider `opencode-go` is multi-vendor. The vendor family is a strict refinement. |
| Tier fallback walks one level only | Simpler; matches the live mapping (max chain length 2) | Rejected: the issue says "same-family tiers resolve from fallbacks excluding the generator family" — a walk, not a one-level swap. Walk until different family OR null. |

## Research Findings

### Key Insights
1. The Bun runner is thin (167 lines) and the heavy work is in
   `dispatch.ts` (CLI shell-out, prompt building, JSON parsing) — all of
   which lives in the parent #192, not #193.
2. The "family" concept is genuinely new in the org's Rust. The
   opencode-provider key is the closest existing concept but maps to a
   subscription/plan, not a vendor family.
3. The opencode cache (`~/.cache/opencode/models.json`) is the authoritative
   test fixture for the parser — it's a snapshot of the live deployment
   surface, 172 opencode families across 213 providers.
4. Schema parity is the *only* consumer-facing contract for #193. The
   fields the issue lists (`generator_model/generator_family/swapped_for_bias`)
   are all NEW (not in the schema's `required`); they extend, they don't
   replace.
5. The CLI dispatch (`cli` field on the tier) is the dispatch router for
   the Bun runner. It does NOT consult the model family. So the
   generator-aware swap is a *new* concern; #193 introduces it.

### Relevant Prior Art
- `terraphim-ai/terraphim_spawner/src/config.rs::normalise_claude_model` —
  CLI/model-string handling, not family parsing. Useful as a style
  reference for a `From<&str>`-style parser.
- `terraphim-ai/docs/plans/design-adf-route-canary-2026-08-19.md` — the
  authoritative live-deployment probe; confirms `kimi-for-coding/k3` is
  the live route.
- The Bun runner's `dispatch.ts` — the closest existing implementation;
  thin (so the spec is implicit); the parent #192 will own it.

### Technical Spikes Needed
None for #193. The slice is types + a parser + a pure function; no I/O,
no LLM. Spikes belong in #192 (LLM CLI dispatch on macOS + Linux + RegionError handling).

## Recommendations

### Proceed

This slice is well-defined and unblocks #192. Recommend proceed to design.

### Scope Recommendations
- Land #193 as one PR (this slice) — types, parser, tier resolver, unit tests, an extended VerdictMeta with the new fields.
- Defer the `judge` subcommand, panel mode, REPL command, and LLM dispatch to follow-on PRs under #192.

### Risk Mitigation Recommendations
- The unit tests use a *checked-in fixture* of the opencode cache
  (`crates/terraphim_agent/src/judge/tests/fixtures/opencode-models.json`)
  so the suite is hermetic. A follow-up task can refresh the fixture from
  the live cache and PR it.
- Document the generator-aware swap behaviour in the type-level
  doc-comments and in the design doc. The parent #192's tests will
  exercise the swap end-to-end with real CLI transcripts.
- Add a "future slices" section in the design doc so the parent #192
  doesn't have to re-research.

## Next Steps

If approved by the quality gate:
1. Phase 2 (Design): write `docs/plans/design-native-judge-2026-09-11.md`
   specifying files, signatures, fixture snapshot, and the test matrix.
2. Phase 3 (Implementation): extract the opencode model fixture, add the
   judge module, write the types + parser + tier resolver + unit tests.
3. Phase 4 (Verification): backend defaults (`cargo fmt/clippy -D warnings/test`).
4. Phase 5 (Validation): map the issue acceptance criteria to evidence.
5. Phase 6 (Review): structural-pr-review.

## Appendix

### Reference Materials
- `/Users/alex/cto-executive-system/automation/judge/run-judge.ts` (167 lines)
- `/Users/alex/cto-executive-system/automation/judge/dispatch.ts`
- `/Users/alex/cto-executive-system/automation/judge/model-mapping.json`
- `/Users/alex/cto-executive-system/automation/judge/verdict-schema.json`
- `/Users/alex/projects/terraphim/terraphim-skills/automation/judge/run-judge.sh` (717 lines)
- `/Users/alex/.cache/opencode/models.json` (213 providers, 172 opencode families — unit-test fixture source)
- `/Users/alex/projects/terraphim/terraphim-ai/AGENTS.md` (model taxonomy, lines 370+)
- `/Users/alex/projects/terraphim/terraphim-ai/docs/plans/design-adf-route-canary-2026-08-19.md` (live deployment probe)
- `/Users/alex/projects/terraphim/terraphim-ai/docs/plans/design-terraphim-proxy-routing-2026-08-25.md` (proxy allow-list)
- terraphim-skills#81 (PR), #84, #85 (issues: "never --file attachments; embed")

### Issue Acceptance Mapping
| Acceptance criterion (parent #192) | Slice | Evidence |
|---|---|---|
| `terraphim-agent judge <files...>` subcommand | later | #192 PR |
| Verdict JSONL matches `verdict-schema.json` | partial (fields only) | This PR's VerdictMeta serialise test |
| Panel + escalation modes | later | #192 PR |
| Model lane hierarchy + runtime guards | later | #192 PR |
| Bare `opencode/` / `Zen` model IDs refused with fatal error | later | #192 PR (this PR's BannedFamily enum entry is the seed) |
| Embed content; never --file attachments | later | #192 PR |
| cargo test green | this PR | This PR's suite |
| Recorded-transcript integration tests, no mocks | later | #192 PR (this PR provides the resolver to record against) |
