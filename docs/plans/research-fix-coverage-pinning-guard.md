# Research: Re-point the coverage tool-pinning guard at the native lane

**Status**: Draft
**Canonical Path**: `docs/plans/research-fix-coverage-pinning-guard.md`
**Change Slug**: `fix-coverage-pinning-guard`
**Author**: Meiko (agent) for Alexander Mikhalev
**Date**: 2026-10-02
**Reviewers**: Alexander Mikhalev

## Executive Summary

Main has been red since 2026-10-01 (Gitea run 36153, the #342 merge). The
`coverage_tool_pinning_matches_local_toolchain` ci_guard in
`crates/terraphim_agent/tests/ci_guards.rs` still parses
`.github/workflows/ci.yml` for a `tool:` line, but #342 removed the GitHub
coverage lane — that file no longer contains one. The fix is to re-point the
guard at the sole remaining source of truth for coverage tool pins,
`.gitea/workflows/native-ci.yml`, preserving the #313 drift-detection
invariant rather than deleting it. The pinned versions themselves
(cargo-llvm-cov 0.8.5, cargo-nextest 0.9.144) are unchanged.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | A silently-unpinned coverage toolchain corrupts lcov comparability — exactly the class of drift CI guards exist to catch |
| Leverages strengths? | Yes | Guard/contract verification is the repo's established ci_guards pattern (#313, #328, #335) |
| Meets real need? | Yes | Main is red; every push to main fails the native gate until this lands |

**Proceed**: Yes — 3/3.

## Problem Statement

### Description

`cargo test -p terraphim_agent --test ci_guards` fails on every native-lane
run with:

```
ci.yml has no `tool:` line; the GH coverage toolchain is unpinned
  (crates/terraphim_agent/tests/ci_guards.rs:98)
```

### Impact

- Main red since 2026-10-01 (Gitea runs 36153 on main, 36127 on the #342 PR
  branch — the failure was visible pre-merge).
- Every subsequent push to main fails the same gate, masking any new
  breakage and eroding trust in the gate.

### Success Criteria

- `coverage_tool_pinning_matches_local_toolchain` passes on the native
  runner while the pins in `native-ci.yml` match runner-local installs.
- The guard fails closed if the pins disappear from `native-ci.yml`
  (no silent "unpinned" state).
- No stale references to the deleted GH `tool:` block remain in
  `native-ci.yml` comments or step names.
- The #313 invariant (pinned coverage tools; pins match local installs) is
  preserved, not removed.

## Current State Analysis

### Existing Implementation

History (git log on `crates/terraphim_agent/tests/ci_guards.rs`):

| Commit | Change |
|--------|--------|
| `95f0777` | Add `coverage_tool_pinning_matches_local_toolchain` (Refs #313) |
| `c6e95a2` | Fix #313: switch both coverage lanes to `cargo llvm-cov nextest` |
| `053db95` | Fix #328: rustfmt the guard |
| `8a245e5`, `090b02c` | Fix #335: token-alias guards (unrelated, still passing) |

Pre-#342 contract (#313 design, `docs/plans/design-coverage-nextest.md`):

- GH lane: `taiki-e/install-action@v2` with
  `tool: cargo-llvm-cov@v0.8.5,nextest@v0.9.144` in `.github/workflows/ci.yml`.
- Native lane: `cargo install cargo-llvm-cov --version 0.8.5 --locked` and
  `cargo install cargo-nextest --version 0.9.144 --locked` in
  `.gitea/workflows/native-ci.yml`, with comments stating the pins "match
  the GH lane's `with: tool:` block exactly".
- Guard: parse the GH `tool:` block, compare against runner-local
  `cargo llvm-cov --version` / `cargo nextest --version`.

Post-#342 reality (verified against `origin/main`, commit `03bcfcf`):

- `.github/workflows/ci.yml` has no coverage lane: no `taiki-e`, no `tool:`,
  no `llvm-cov`, no `nextest`. Its test step is
  `cargo test --workspace --lib --no-fail-fast` — **ci_guards is an
  integration test and does not run on GitHub at all anymore.** It runs
  only on the native lane (`.gitea/workflows/native-ci.yml:166`, plus the
  nextest `--tests` sweep).
- `.gitea/workflows/native-ci.yml` still pins 0.8.5 / 0.9.144 (lines 62–65)
  but its comments (lines 50–58) and step names
  ("Install cargo-llvm-cov (pinned to GH ci.yml version)") cite the GH
  `tool:` block as the authority. That block no longer exists.
- The guard still opens `.github/workflows/ci.yml` and hard-fails on the
  missing `tool:` line.

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| Drift guard (failing) | `crates/terraphim_agent/tests/ci_guards.rs:85-191` | Compare local tool versions vs pins |
| Canonical pins (new SoT) | `.gitea/workflows/native-ci.yml:62-65` | `cargo install --version X --locked` |
| Stale comments/names | `.gitea/workflows/native-ci.yml:50-65` | Reference deleted GH `tool:` block |
| Native lane guard step | `.gitea/workflows/native-ci.yml:166` | Runs `cargo test -p terraphim_agent --test ci_guards` |
| Deleted GH pin block | `.github/workflows/ci.yml` (pre-`176c490^`, line 35) | `tool: cargo-llvm-cov@v0.8.5,nextest@v0.9.144` |

### Data Flow

```
native-ci.yml install step (pin: --version 0.8.5 --locked)
        │  installs into runner CARGO_HOME (skips if same version cached:
        │  "Ignored package `cargo-llvm-cov v0.8.5` is already installed")
        ▼
coverage step: cargo llvm-cov nextest --workspace --all-targets --lcov
        │
        ▼
ci_guards test (same runner): asserts local install == pin
        ── reads the WRONG file since #342 (.github/workflows/ci.yml)
```

The guard's real value is catching the case where the runner image
pre-ships a different version in CARGO_HOME that `cargo install` refuses to
overwrite (run 522 precedent: image had 0.9.1, pin was 0.8.5 — only the
guard noticed).

## Constraints

### Technical Constraints

- **Runner command allowlist**: native-lane step first tokens must stay
  `cargo` / `test` (documented in ci_guards.rs; enforced by runner policy).
  Any workflow edit must not introduce a new step shape. Comment/name edits
  are unconstrained.
- **Guard runs on dev machines too**: it reads workflow files via
  `workspace_root()`, so the parsed file must exist in every checkout
  (`.gitea/workflows/native-ci.yml` does).
- **Fail-closed parsing**: the current failure *is* a fail-closed design
  working as intended (missing pin → hard error). The replacement must keep
  that property: no pin found → panic, never "skip check".
- **Line-based text parsing precedent**: the existing guard parses YAML as
  text lines; no YAML library dependency is available/wanted in this test
  crate. The replacement stays line-based and regex-free or minimal-regex
  (std-only).

### Business Constraints

- Main must go green quickly; the fix should be small and reviewable (this
  is a guard repair, not a CI redesign).
- #342's architectural decision (coverage is Gitea-native-only; the GH tree
  is a lean port) must be preserved, not reversed.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Guard latency | < 1 s (two `--version` subprocess calls) | ~0.25 s observed (run 36153 log) |
| Gate behaviour on missing pin | hard fail | hard fail (assert) — keep |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| Single source of truth for pins | Two pin sites is what created this drift class (#313 had to add a guard precisely because GH and native pins could diverge) | #313 design doc; run 522 |
| Fail-closed on missing pin | A silently-unpinned `cargo install` drifts on every upstream release, silently changing the coverage ABI | native-ci.yml comment, run 522 journal |
| Native pins remain the versions CI already runs (0.8.5 / 0.9.144) | Changing versions is out of scope and would invalidate the existing lcov baseline | run 36153 log: installs report 0.8.5 / 0.9.144 already present |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Restoring the GH coverage lane | Reverses #342's stated intent; adds hosted-runner cost for a port tree whose job is contracts, not coverage |
| Deleting the guard | Explicitly rejected by the user; would lose the run-522-class drift detection forever |
| Moving pins to a new TOML/JSON file | New indirection; pins already live in the file that consumes them |
| Version bumps (0.8.5 → newer) | Unrelated change; coverage ABI stability is the point of the pin |
| Updating historical plan docs (`design-coverage-nextest.md` etc.) | Repo treats plan artefacts as point-in-time records; the fix commit message and current comments carry the new contract |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `native-ci.yml` install steps | Guard parses them; their format (`cargo install <tool> --version <v> --locked`) becomes a de-facto parse contract | Low — format stable since #313, comments document it |
| Other ci_guards tests (token aliases, publish gate, duplicate crates) | Must keep passing | Low — orthogonal; verified passing in run 36153 |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| cargo-llvm-cov | 0.8.5 (pinned) | Low | — |
| cargo-nextest | 0.9.144 (pinned) | Low | — |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Line-based parse breaks if someone rewrites the install steps (multi-line `run:`, reordered flags) | Medium | Guard false-fails (loud, not silent) | Fail-closed panic message names the expected shape; design doc specifies the exact contract |
| Runner image changes CARGO_HOME shadowing behaviour | Low | Guard catches it — that is its purpose | n/a (this is the feature) |
| Future re-introduction of a GH coverage lane recreates two pin sites | Low | Drift class returns | Design note: if a GH lane returns, extend the guard to assert both files agree (one-line addition, documented in design) |

### Open Questions

1. Should a Gitea issue be filed for this fix (branch convention is
   `task/<issue>-<slug>`), or does it ride under #342? — Alexander to decide
   at the gate. Branch is currently `task/fix-coverage-pinning-guard`.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| Native pins 0.8.5 / 0.9.144 are the intended versions going forward | They match the deleted GH block exactly; runners already have them installed | Fix pins wrong versions; lcov baseline shifts | Yes — run 36153 log shows installs at exactly these versions |
| ci_guards running native-only is acceptable | GH ci.yml's `--lib`-only test step means the guard never ran on GH even before, by target selection; the native lane is its home | None identified — the guard protects the lane that produces lcov | Yes — grep of `.github/workflows/ci.yml` |
| No other consumer reads the GH `tool:` line | Repo-wide grep for `tool:` / `ci.yml` in tests, scripts, .quality | A second breakage surfaces after merge | Yes — only the guard itself references it |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| **A. Re-point guard at `native-ci.yml` pins** | Guard compares local installs vs the native install pins; comments/step names updated to make native canonical | **Chosen.** Preserves #313 invariant with one pin site; minimal diff; aligns SoT with the only lane that runs coverage |
| B. Delete the guard | No drift protection; silent unpinning becomes possible | Rejected — user said "fix, don't remove" |
| C. Restore GH coverage lane so the guard has a target | Reverses #342; duplicate pin site returns | Rejected — wrong direction, more cost, recreates the drift class |
| D. Move pins to a dedicated version file both sides read | Clean SoT but new file + indirection for a two-line pin | Rejected — YAGNI; native-ci.yml is already the file that consumes the pins |

## Research Findings

### Key Insights

1. The failure is the guard's fail-closed design working as designed — the
   contract it checks was deleted underneath it by #342. The response to a
   deleted contract is to re-point the check at the surviving contract, not
   to remove the check.
2. #313 always had a single-lane resolution available: the guard exists to
   keep *install pins* and *runner-local installs* in lockstep. With one
   lane, "the pins" and "the installs" both live on the native lane; the
   GH `tool:` block was only ever a mirror.
3. The failure was visible on the #342 PR branch (run 36127, failed) before
   merge — a pre-merge gate that would have caught it exists in principle
   but the merge proceeded. Worth noting for process; not in scope to fix
   here.

### Relevant Prior Art

- #313 / `docs/plans/design-coverage-nextest.md` — original dual-lane
  coverage design; the drift test rationale (Refs #313, run 522 journal)
  survives verbatim.
- #328 — `cargo install --version` pin split into three steps so a
  transient network failure retries independently; the pins' textual shape
  has been stable since.
- #342 — the canonical-tree sync that removed the GH coverage lane and
  triggered this breakage.

### Technical Spikes Needed

None. The parse target is two literal lines in a checked-in file; the
comparison logic already exists in the guard unchanged.

## Recommendations

### Proceed/No-Proceed

Proceed. Fix (don't remove): re-point
`coverage_tool_pinning_matches_local_toolchain` at
`.gitea/workflows/native-ci.yml`, update the stale comments/step names to
declare the native pins canonical, keep versions at 0.8.5 / 0.9.144, and
keep the fail-closed property.

### Scope Recommendations

- In scope: `ci_guards.rs` guard re-point (+ doc comment), `native-ci.yml`
  comment/step-name cleanup, commit message citing #313 and #342.
- Out of scope: version bumps, GH lane changes, historical plan docs,
  process changes to pre-merge gating.

### Risk Mitigation Recommendations

- Guard must panic with a message that names `native-ci.yml` and the
  expected install-line shape, so a future workflow rewrite gets a
  actionable failure.
- After merge, verify the next main run goes green before closing.

## Next Steps

If approved:
1. Phase 2 — `disciplined-design`: produce
   `docs/plans/design-fix-coverage-pinning-guard.md` with exact file
   changes, function signatures, test strategy, and step sequence.
2. After design approval — implement on
   `task/fix-coverage-pinning-guard`, run
   `cargo test -p terraphim_agent --test ci_guards` locally plus fmt/clippy
   per BUILD.md, push, PR to main.
3. Verify the post-merge native run is green.

## Appendix

### Reference Materials

- Gitea run 36153 (main, failed) and 36127 (#342 PR, failed) — job 71702 log
- `docs/plans/design-coverage-nextest.md` (historical, #313)
- `docs/plans/validation-coverage-nextest.md` — records the drift test
  passing as recently as the #313 validation
- `.gitea/workflows/native-ci.yml:40-65,166` (current, origin/main `03bcfcf`)
- `.github/workflows/ci.yml` pre-#342 at `176c490^` (deleted `tool:` block)

### Code Snippets

The two lines that become the parse contract (`.gitea/workflows/native-ci.yml:62-65`):

```yaml
      - name: Install cargo-llvm-cov (pinned to GH ci.yml version)
        run: cargo install cargo-llvm-cov --version 0.8.5 --locked
      - name: Install cargo-nextest (pinned to GH ci.yml version)
        run: cargo install cargo-nextest --version 0.9.144 --locked
```

The failing assertion (`crates/terraphim_agent/tests/ci_guards.rs:94-98`):

```rust
    let pinned_block = ci_text
        .lines()
        .find(|l| l.trim_start().starts_with("tool:"))
        .expect("ci.yml has no `tool:` line; the GH coverage toolchain is unpinned");
```
