# Research Document: terraphim-grep & terraphim-agent Audit and Fix Plan

**Status**: Draft
**Author**: Alex (via disciplined-research skill)
**Date**: 2026-08-30
**Reviewers**: terraphim-clients maintainers
**Scope**: `terraphim_grep 1.21.12` + `terraphim_agent 1.21.13` binaries and their public docs

## Executive Summary

A systematic audit of the two CLI binaries shipped from the `terraphim-clients`
workspace surfaced **1 critical safety bug**, **4 behavioural inconsistencies**,
**17 documentation gaps**, and **5 missing blog posts**. The critical bug is in
the `terraphim-agent hook --hook-type pre-tool-use` pipeline: a destructive
command like `rm -rf /tmp/foo` is silently rewritten to `rm -Readiness Feedback
/tmp/foo` via substring matching against the thesaurus, instead of being blocked
or passed through unchanged. This is a release-blocker. The remaining findings
are documentation and example-coverage work that can land after the safety fix.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energising? | Yes | One critical safety bug + four behavioural inconsistencies found in current release candidates. Audit also revealed that ≈60% of top-level commands lack README examples. |
| Leverages strengths? | Yes | All changes are local to `terraphim-clients`; no new external dependencies; Rust idiomatic; existing test infrastructure (`cargo test`, `insta`, `assert_cmd`) sufficient. |
| Meets real need? | Yes | Public binaries are how AI agents (Claude Code, OpenCode, pi) integrate with Terraphim. Correct command rewriting is a precondition for trust. |

**Proceed**: Yes (3/3)

## Problem Statement

### Description

A focused audit of `terraphim-grep` and `terraphim-agent` produced a 30-row
matrix (commands × docs × examples × blog × correctness) and six high-severity
findings. The most serious finding is the PreToolUse hook rewrite bug, which
means a guard system documented as "block destructive git/filesystem commands
before execution" can silently mutate user commands into unknown strings.

### Impact

- **Safety**: AI agents and humans relying on the `terraphim-agent hook`
  pipeline may have their destructive commands silently rewritten. The user
  sees a different command being attempted, but no warning that the hook
  modified it.
- **Trust**: The `terraphim-grep` binary in the user's PATH (`1.21.11`) is
  behind the workspace source (`1.21.12`). Users who read the CHANGELOG and
  try to use `--search-only` see "unexpected argument" with no explanation.
- **Discoverability**: 17 of the 21 top-level `terraphim-agent` subcommands
  have no README example. Users discover them only via `terraphim-agent
  <subcommand> --help`.
- **Onboarding**: 5 major aspects (sessions, setup wizard, robot mode, shared
  learning, R2 self-update) have no blog post and no first-class doc.

### Success Criteria

- PreToolUse hook either passes `rm -rf /tmp/foo` through unchanged, or
  blocks it with a `permissionDecision: deny` response — never rewrites it.
- `terraphim-grep --search-only` works in any binary claiming to be ≥1.21.12.
- Every top-level `terraphim-agent` subcommand has at least one example in
  the README or a linked reference doc.
- Guard priority order is documented in the README and pinned by a test.
- All audit findings have an open Gitea issue with severity, file paths, and
  acceptance criteria.

## Current State Analysis

### Existing Implementation

Two source trees under `terraphim-clients/crates/`:

- `terraphim_grep/` — ~3,500 lines of Rust across 9 modules (lib.rs,
  hybrid_searcher.rs, kg_curation.rs, main.rs, etc.)
- `terraphim_agent/` — ~51,000 lines of Rust across 47 files in 11 module
  trees (commands, forgiving, learnings, repl, robot, shared_learning,
  onboarding, plus top-level service.rs, main.rs, listener.rs)

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| Hook pipeline | `crates/terraphim_agent/src/main.rs:2730-2810` | PreToolUse hook handler |
| KG substitution | `crates/terraphim_agent/src/kg_validation.rs` | Substitutes matched KG terms |
| Replacement service | `crates/terraphim_hooks::ReplacementService` (external dep) | Calls `replace_fail_open(command)` |
| Guard | `crates/terraphim_agent/src/guard_patterns.rs:140-200` | Three-valued decision |
| Allowlist thesaurus | `crates/terraphim_agent/data/guard_allowlist.json` | Contains `rm -rf /tmp/` pattern |
| README | `crates/terraphim_agent/README.md` | 139 lines; 8 commands in key-commands table |
| Robot schemas | `crates/terraphim_agent/src/robot/docs.rs` | Self-doc of REPL commands |

### Data Flow (PreToolUse hook)

```
input_json
    │
    ▼
extract tool_name="Bash" → tool_input.command
    │
    ├─ if --with-guard: CommandGuard.check(command)
    │      └─ Block? → emit { permissionDecision: "deny" }
    │
    ├─ kg_validation::validate_command_against_kg(command)
    │      └─ Returns findings (no early-exit)
    │
    ├─ terraphim_hooks::ReplacementService::replace_fail_open(command)
    │      └─ Substitutes KG synonyms (substring match, no word boundary)
    │
    └─ emit rewritten input_json if any change, else pass through
```

The `kg_validation` and `ReplacementService` calls always run. There is no
guard rail between them and the destructive patterns: any command can be
rewritten if its substrings match a thesaurus term.

## Constraints

### Technical Constraints

- **Public API stability**: `terraphim-agent hook --hook-type pre-tool-use`
  is part of the public contract consumed by Claude Code and OpenCode. Adding
  a default flag flip is safe; removing a flag is not.
- **Feature flags**: `terraphim_agent` is built with default features
  `["repl-interactive", "llm", "repl-sessions"]`. Adding new flags must
  compile under this default.
- **Workspace version pin**: `terraphim-clients` workspace is at 1.21.13;
  `terraphim_grep` source carries 1.21.12 fixes. New fixes go in a single
  minor bump.
- **Cross-crate consistency**: changes to `kg_validation` or `guard_patterns`
  affect other consumers in `terraphim-ai` (the upstream polyrepo) and must
  not break the published `terraphim_orchestrator` 1.21.0 family.

### Business Constraints

- **Release pipeline**: the `release-comprehensive.yml` workflow builds
  signed binaries for 7 targets. New tests must pass `native-ci` on bigbox.
- **Backwards compatibility**: agents deployed with the old hook behaviour
  must not break after the fix. The fix must be opt-in (or default-on with
  a documented migration).

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Hook decision latency | < 5 ms p95 | ~2 ms |
| Guard false-positive rate | < 5% | depends on thesaurus; not measured |
| Test coverage on hook/guard | > 90% lines | unknown (no `cargo tarpaulin` baseline) |
| Doc build time | < 30 s | unknown |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| PreToolUse hook must not silently rewrite destructive commands | Safety contract violated; agents trust the hook to be either deny or pass-through | Finding 1 in audit; `rm -rf /tmp/foo` → `rm -Readiness Feedback /tmp/foo` confirmed |
| `terraphim-grep --search-only` must work in any binary claiming to be ≥1.21.12 | CHANGELOG advertises the flag; users hitting it see "unexpected argument" | Finding 3 in audit; `terraphim-grep 1.21.11` rejects the flag |
| Guard priority order must be documented and pinned by a test | Silent allowlist-override of destructive patterns surprises users | Finding 4 in audit; `rm -rf /tmp/foo` allowed because `rm -rf /tmp/` is in allowlist |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Comprehensive test coverage report (cargo-tarpaulin baseline) | Out of vital few; can be added as a follow-up issue |
| Migration of all 21 subcommands into a generated reference doc site | Documentation framework choice is a separate decision |
| Self-update R2 backend redesign (replacing GitHub fallback entirely) | R2 manifest is the default per CHANGELOG; design is settled |
| Refactor `kg_validation` to use word boundaries by default globally | Scope creep; the fix is scoped to the hook pipeline |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `terraphim_hooks::ReplacementService` | The rewrite we want to gate behind `--rewrite` | Low — already feature-gated in the hook flag set |
| `kg_validation` module | Provides the substitution patterns | Low — pure function over the KG |
| `guard_patterns::CommandGuard` | The deny check we want to default-on | Low — fail-open on load error |
| `serde_json::Value` | Used in hook output | None |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `terraphim_hooks` | 1.21.0 | Low — published, versioned | Pass-through to literal |
| `terraphim_automata` | 1.21.0 | None — already pinned in `[patch.crates-io]` | n/a |
| `clap` | 4 | None — already a workspace dep | n/a |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Agents deployed today expect the silent-rewrite behaviour; flipping default breaks them | Med | High | Default `--with-guard` to `true` and `--rewrite` to `false`; emit a `WARN` log line on first run if rewrite is requested but not opted-in |
| The guard priority order may be deliberate in some user setups (e.g., users want `rm -rf /tmp/foo` allowed) | Low | Med | Document priority in README; expose `--no-allowlist` flag for strict mode |
| New `--rewrite` flag may confuse existing hook users | Low | Low | CHANGELOG entry + blog post |
| Substring match behaviour is depended on by some users (low-quality but documented) | Low | Med | Default to off; preserve opt-in |

### Open Questions

1. Should the hook emit a warning when it silently drops a rewrite? (e.g.,
   "command `foo bar` had `bar` substituted to `baz`; this is now off by
   default — pass `--rewrite` to re-enable")
2. Does `terraphim_orchestrator` or any other consumer invoke the hook
   pipeline in a way that depends on the rewrite? — needs a code search.
3. Is there a CI gate that runs `terraphim-grep --search-only` against the
   installed binary? — if not, the version-lag bug recurs.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| The hook rewrite bug affects only `pre-tool-use`, not `post-tool-use`, `pre-commit`, `prepare-commit-msg` | Searching `src/main.rs` shows substitution only runs in the `PreToolUse` arm | Other hook types silently rewrite | Yes — verified by reading the dispatch |
| The `terraphim_hooks::ReplacementService::replace_fail_open` is fail-open by design | Method name | It might error instead | Yes — verified in terraphim-clients source |
| The README "Key Commands" table is meant to be a complete enumeration | It is titled "Key Commands" | Some commands intentionally omitted | No — treat as incomplete and document the rest |
| The `terraphim-ai` polyrepo consumes `terraphim_agent` from registry 1.21.3 | Workspace Cargo.toml in `terraphim-ai` | Wrong target | Yes — verified by reading `terraphim-ai/Cargo.toml` |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| Make the rewrite always-on but require a confirmation flag | Doesn't solve the safety problem; user still sees a rewritten command | Rejected |
| Default `--with-guard=true` and `--rewrite=false`, with a single `--rewrite` opt-in | Simple, safe, reversible | Chosen |
| Remove the rewrite entirely from the hook pipeline | Breaks any user who depended on it | Rejected for v1 |

## Research Findings

### Key Insights

1. The KG substitution runs **after** the destructive guard check, but the
   guard check is **off by default**. The result is that the most user-visible
   part of the hook pipeline (the substitution) runs without any safety net.
2. Three thesauri (`guard_destructive.json`, `guard_allowlist.json`,
   `guard_suspicious.json`) are compiled into the binary via `include_str!`.
   Changing priority means changing `guard_patterns.rs`, not the data.
3. The `terraphim-grep` binary lags the source by exactly one minor version
   (1.21.11 installed vs 1.21.12 source). The CHANGELOG faithfully reports
   the source state but the build pipeline does not rebuild on tag.
4. `terraphim-agent` has 21 top-level commands; 8 are in the README's key
   table; 5 are documented in this-audit-only-with-`--help`. There is no
   single-reference doc.

### Relevant Prior Art

- **OpenCode** (`opencode-bin`) hook: uses deny-or-passthrough semantics;
  no rewrite.
- **Claude Code** `UserPromptSubmit`/`PreToolUse`: deny-or-modify but
  requires the modify to be the same hook instance, not a downstream
  thesaurus.
- **Droids** by Factory: pure deny-or-passthrough; no rewrite.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Search `terraphim-ai` polyrepo for hook-pipeline callers | Confirm no other consumer depends on the rewrite | 1 hour |
| Rebuild `terraphim-grep` from source and verify `--search-only` | Confirm fix lands at binary level | 30 min |
| Test the allowlist priority with realistic dev-loop commands | Confirm the priority order is not breaking common workflows | 2 hours |

## Recommendations

### Proceed/No-Proceed

**Proceed**: the work is essential, leverages existing capability, and meets a
validated need (the safety bug is reproducible today).

### Scope Recommendations

- **Critical (Phase 3 must-do)**: fix the PreToolUse hook rewrite bug,
  rebuild `terraphim-grep` 1.21.12.
- **High (Phase 3 should-do)**: fix the README robot-mode example; document
  the guard priority order.
- **Medium (Phase 4 backlog)**: enumerate the remaining subcommands in a
  reference doc; mark REPL-only commands in robot schemas.
- **Low (Phase 4 backlog)**: blog posts for sessions, setup, robot mode,
  shared learning, R2 backend.

### Risk Mitigation Recommendations

1. The hook rewrite fix ships behind a default-flip with a CHANGELOG entry
   warning about the behaviour change.
2. A new CI job (suggested: `tests/hook_safety.rs`) pins the safety property
   so regressions are caught before merge.
3. A `tests/guard_priority.rs` integration test pins the priority order so
   the allowlist override of destructive patterns is intentional, not
   accidental.

## Next Steps

If approved:
1. Land the design document (`design-terraphim-grep-agent-fixes-2026-08-30.md`).
2. Open Gitea issues for each finding, with severity tag and acceptance
   criteria.
3. Implement the critical fix first (PreToolUse hook rewrite).
4. Implement the high-priority fixes.
5. Land the documentation PR.

## Appendix

### Reference Materials

- Audit report (this conversation, prior turn)
- `terraphim-clients/crates/terraphim_agent/src/main.rs:2730-2810`
- `terraphim-clients/crates/terraphim_agent/src/guard_patterns.rs`
- `terraphim-clients/crates/terraphim_agent/data/guard_allowlist.json`
- `terraphim-clients/crates/terraphim_grep/src/main.rs`
- `terraphim-ai/docs/terraphim-grep-offline-setup.md`

### Code Snippets

(see "Current State Analysis" above for the PreToolUse data flow)
