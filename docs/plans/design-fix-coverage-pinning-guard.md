# Implementation Plan: Re-point the coverage tool-pinning guard at the native lane

**Status**: Draft
**Canonical Path**: `docs/plans/design-fix-coverage-pinning-guard.md`
**Change Slug**: `fix-coverage-pinning-guard`
**Research**: `docs/plans/research-fix-coverage-pinning-guard.md` (approved 2026-10-02)
**Author**: Meiko (agent) for Alexander Mikhalev
**Date**: 2026-10-02
**Estimated Effort**: ~1 hour

## Overview

### Summary

Repair `coverage_tool_pinning_matches_local_toolchain` so main goes green:
replace the deleted GitHub `tool:`-block contract with the surviving
canonical pin site — the `cargo install --version X --locked` lines in
`.gitea/workflows/native-ci.yml` — and update that file's stale
comments/step names to declare the pins canonical. Versions unchanged
(cargo-llvm-cov 0.8.5, cargo-nextest 0.9.144). No lanes added or removed.

### Approach

Per research option A: the guard's job (from #313) is keeping coverage-tool
install pins equal to runner-local installs so lcov ABI cannot drift silently
(run 522 class). Post-#342 there is exactly one lane and exactly one pin
site; point the guard at it. The #313 invariant survives intact.

### Scope

**In Scope:**
- `crates/terraphim_agent/tests/ci_guards.rs` — re-point the guard, update
  its doc comment, add parser unit tests.
- `.gitea/workflows/native-ci.yml` — rewrite the stale pin-provenance
  comment block and the two "(pinned to GH ci.yml version)" step names.

**Out of Scope:**
- Tool version bumps; GH workflow changes; historical plan documents;
  process/pre-merge-gating changes; the other four ci_guards (passing).

**Avoid At All Cost** (5/25 elimination):
- Restoring a GH coverage lane to "give the guard something to check" —
  reverses #342 and recreates the two-pin-site drift class.
- Deleting or weakening the guard (fail-open on missing pin) — the current
  red main *is* the fail-closed property working; never trade it away.
- A YAML-parsing dependency for two literal lines — std-only line parsing
  matches the file's existing conventions.
- Refactoring the shared local-version-resolution code beyond the minimal
  message/comment updates — churn without value.

## Architecture

No component changes. Text contract moves from one checked-in file to
another:

```
before:  native-ci.yml (installs, pins)  ──mirror──▶  ci.yml tool: block (SoT)
                              guard compares local installs against ci.yml

after:   native-ci.yml (installs, pins, SoT)
                              guard compares local installs against native-ci.yml
```

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|----------|-----------|----------------------|
| Parse `run: cargo install <tool> --version <v> --locked` lines from `native-ci.yml` | It is the file that consumes the pins; single SoT by construction | Dedicated version file (indirection); YAML lib (dependency for 2 lines) |
| Keep fail-closed: missing/unparseable pin line ⇒ panic naming the file and expected shape | Silent unpinning is the failure mode this guard exists to prevent (run 522) | Warn-and-continue; skip when absent |
| Keep versions at 0.8.5 / 0.9.144 | Runners already install exactly these; changing them invalidates the lcov baseline | Opportunistic bump |
| Unit-test the parser with inline fixtures | Parser is the only new logic; fixtures are free, no runner needed | End-to-end only via CI |
| Stale comments/step names updated in the same commit | The false "we mirror the GH block" claim is part of the bug | Separate cosmetic commit (delays clarity) |

No durable decision record needed beyond the guard's doc comment and commit
message: this restores a #313 invariant, it establishes no new architecture.
No ADR. No contract/spec artefacts (no executable interface changes).
Expected downstream artefacts: Phase 4 verification report +
traceability matrix (`docs/verification/`), validation report N/A
(verification-only change, no user-visible behaviour).

### Simplicity Check

**What if this could be easy?** One test swaps which file it reads and how
it extracts two version strings; one workflow file gets truthful comments.
That *is* this design. A senior engineer would not call this complicated.

**Nothing Speculative Checklist**:
- [x] No features the user didn't request
- [x] No abstractions "in case we need them later"
- [x] No flexibility "just in case"
- [x] No error handling for scenarios that cannot occur
- [x] No premature optimization

## File Changes

### Modified Files
| File | Changes |
|------|---------|
| `crates/terraphim_agent/tests/ci_guards.rs` | Replace GH `tool:`-block parsing with native-ci.yml install-pin parsing; update doc comment (Refs #313, #342); add `#[cfg(test)]` parser unit tests; drift messages name `native-ci.yml` |
| `.gitea/workflows/native-ci.yml` | Rewrite comment block at lines ~50–58 (pins are canonical here since #342; guard enforces them; keep run 518/522/524 history); rename two install steps to drop the "pinned to GH ci.yml version" claim |

### Deleted Files
None.

## API Design

Test-file internal items only (no public API).

```rust
/// Extract the pinned `--version` of `tool` from a `cargo install` line in
/// `.gitea/workflows/native-ci.yml`.
///
/// Contract (enforced fail-closed): exactly one distinct pin per tool, on a
/// line of the shape:
///     run: cargo install <tool> --version <semver> --locked
///
/// # Panics
/// - no `cargo install <tool>` line exists
/// - the line has no `--version <value>` or `--locked`
/// - two install lines for `<tool>` carry different versions
fn native_ci_install_pin<'t>(text: &'t str, tool: &str) -> &'t str;
```

Behaviour:
1. Iterate `text.lines()`, `trim_start` each.
2. Keep lines starting with `cargo install <tool> ` (note: `cargo install cargo-llvm-cov` must not match a search for `cargo-nextest` — token-boundary match on whitespace-split tokens).
3. For each kept line: require token sequence contains `--version` followed by a value starting with an ASCII digit; require a `--locked` token. Panic with the offending line otherwise.
4. Collect distinct versions; if > 1 distinct → panic (inconsistent pins); if 0 lines → panic ("native-ci.yml has no pinned `cargo install <tool>` line; the coverage toolchain is unpinned — expected `run: cargo install <tool> --version <semver> --locked`").
5. Return the single pinned version.

The existing local-version-resolution block (`cargo llvm-cov --version` /
`cargo-nextest --version` / `cargo nextest --version` fallback) is unchanged
except drift-message wording: "native-ci.yml pins {x}, local toolchain has
{y}. Align them so coverage ABI cannot drift. Refs #313, #342."

## Test Strategy

### Unit Tests (new, in `ci_guards.rs` `#[cfg(test)] mod parser_tests`)
| Test | Fixture | Purpose |
|------|---------|---------|
| `install_pin_happy_path` | `run: cargo install cargo-llvm-cov --version 0.8.5 --locked` (+ indented) | Extracts `0.8.5` |
| `install_pin_requires_locked` | install line without `--locked` | Panics (pin contract) |
| `install_pin_missing_tool` | fixture without the tool | Panics "coverage toolchain is unpinned" |
| `install_pin_rejects_divergent_duplicates` | two install lines, different versions | Panics (inconsistent pins) |
| `install_pin_allows_identical_duplicates` | two install lines, same version | Returns it |
| `install_pin_token_boundary` | line for `cargo-llvm-cov` only | Searching `cargo-nextest` panics (no substring false-match) |

### Integration / Gate Verification
| Command | Where | Purpose |
|---------|-------|---------|
| `cargo test -p terraphim_agent --test ci_guards -- --nocapture` | Local (needs cargo-llvm-cov + cargo-nextest installed to reach the assert) and native lane (authoritative) | Guard passes against native-ci.yml |
| `cargo fmt --all -- --check` | Local | Formatting |
| `cargo clippy --workspace --all-targets -- -D warnings` | Local | Lints |
| Next native-lane run on main | Gitea | Main goes green (post-merge check) |

## Implementation Steps

### Step 1: Guard re-point
**Files:** `crates/terraphim_agent/tests/ci_guards.rs`
**Description:** Add `native_ci_install_pin`, swap the parse source from
`.github/workflows/ci.yml` to `.gitea/workflows/native-ci.yml`, update doc
comment and drift messages.
**Tests:** Parser unit tests from the table above (written with the helper).
**Estimated:** 25 min

### Step 2: native-ci.yml truthfulness pass
**Files:** `.gitea/workflows/native-ci.yml`
**Description:** Rewrite the ~9-line comment above the install steps: pins
are canonical in this file (post-#342 the GH coverage lane and its `tool:`
block no longer exist); `coverage_tool_pinning_matches_local_toolchain`
compares runner-local installs against these lines and fails closed if they
are removed or reshaped; retain the run 518/522/524 rationale
(--version/--locked, no `--root`, three separate steps). Rename the two
steps: "Install cargo-llvm-cov (pinned; guarded by
coverage_tool_pinning_matches_local_toolchain)" and the nextest equivalent.
**Tests:** Step 1 unit tests keep passing (file still matches the parse
contract); visual diff of the workflow.
**Dependencies:** Step 1 (defines the contract the comments describe)
**Estimated:** 15 min

### Step 3: Local verification
**Files:** none (commands only)
**Description:** `cargo fmt --all -- --check`; `cargo clippy --workspace
--all-targets -- -D warnings`; `cargo test -p terraphim_agent --test
ci_guards -- --nocapture` (skips-version-resolution note if local tools
absent — parser unit tests still run and must pass).
**Dependencies:** Steps 1–2
**Estimated:** 15 min (clippy-dominated)

### Step 4: Commit, push, PR
**Files:** git only
**Description:** Single commit: `fix(ci): re-point
coverage_tool_pinning_matches_local_toolchain at native-ci.yml pins
(#313, #342)`. Push branch `task/fix-coverage-pinning-guard` to Gitea, open
PR to main, verify the PR's native run is green before merge.
**Dependencies:** Step 3
**Estimated:** 5 min + CI wait

## Rollback Plan

Revert the single commit. No state, no migrations, no flags. Main stays red
until re-fixed — the failure is loud by design.

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Gitea issue number for branch rename to `task/<n>-...` convention | Deferred (non-blocking) | Alexander |

## Approval

- [x] Technical review complete
- [x] Test strategy approved
- [x] Human approval received (research gate, 2026-10-02) — design gate pending
