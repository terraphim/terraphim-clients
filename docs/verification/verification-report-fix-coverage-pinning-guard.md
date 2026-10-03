# Verification Report: Re-point the coverage tool-pinning guard at the native lane

**Status**: Verified (pending human sign-off + post-merge main-green check)
**Canonical Path**: `docs/verification/verification-report-fix-coverage-pinning-guard.md`
**Traceability Matrix**: `docs/verification/traceability-matrix-fix-coverage-pinning-guard.md`
**Change Slug**: `fix-coverage-pinning-guard`
**Date**: 2026-10-02
**Design**: `docs/plans/design-fix-coverage-pinning-guard.md`
**Specification**: N/A
**Decisions / ADRs**: N/A (design: no durable architectural decision)
**Contracts**: N/A
**Implementation**: PR #345 (`task/fix-coverage-pinning-guard`, commit `6c4b4e2`)

## Summary

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| ci_guards test binary | all pass | 14/14 | PASS |
| New parser unit tests | all pass | 9/9 | PASS |
| Pre-existing guards | no regression | 4/4 pass | PASS |
| `cargo fmt --all -- --check` | clean | clean | PASS |
| `cargo clippy -p terraphim_agent --test ci_guards -- -D warnings` | clean | clean | PASS |
| Fail-closed property | missing pin ⇒ panic | 4 unit tests + real incident evidence | PASS |
| Defects open | 0 critical/high | 0 | PASS |

## Specialist Skill Results

### Static Analysis (`ubs-scanner`)
Not available in this environment. Substituted by: full-branch unit coverage
of the new helper (9/9 branches, see matrix) plus manual diff review.
Critical findings: 0.

### Requirements Traceability (`requirements-traceability`)
Matrix: `docs/verification/traceability-matrix-fix-coverage-pinning-guard.md`.
All design decisions traced to code and tests. One gap (drift-failure path
not exercised locally) — closed with justification (proven by run 36153
history + panic symmetry).

### Code Review (`code-review`)
- fmt: clean. clippy (touched target, `-D warnings`): clean.
- Manual diff review caught one typo ("panks") pre-commit — fixed and
  re-verified (see Defect Register D004).
- No drive-by changes; diff confined to the coverage guard block, its new
  helper + test module, and the two `native-ci.yml` comment/name sites.

### Security Audit
Not applicable: test-only code, no auth/crypto/untrusted input. The parsed
workflow file is repo-controlled; the guard only reads it.

### Performance
Not applicable: guard latency ~1.2 s observed (two `--version`
subprocesses), unchanged from the pre-#313-era design.

### SRD Testability Check
Not applicable: no SRD for this change.

## Unit Test Results

`cargo test -p terraphim_agent --test ci_guards` — 14 passed, 0 failed:
9 parser unit tests, the re-pointed guard, and the 4 pre-existing guards
(`no_duplicate_terraphim_crates`, both native-token-alias guards,
`publish_gate_tests_pass`).

Branch coverage of `native_ci_install_pin`: 9/9 (each panic branch and
each continue path has a dedicated test; see matrix).

## Integration Test Results

The re-pointed guard ran end-to-end against the real
`.gitea/workflows/native-ci.yml` in the checkout: pins parsed (0.8.5 /
0.9.144), local tools resolved (cargo-llvm-cov 0.8.5, cargo-nextest
0.9.144), equality asserted. This is the exact code path that failed on
main in run 36153 (there, the parse target file lacked the contract).

Module boundary under test: test binary ↔ checked-in workflow text
contract. Verified.

Data flow: file read → per-tool pin parse → local `--version`
subprocesses → equality assert. Verified.

## Defect Register

| ID | Description | Origin Phase | Severity | Resolution | Status |
|----|-------------|--------------|----------|------------|--------|
| D001 | Original bug: guard parses deleted GH `tool:` block | Phase 2 of #342 (not this change) | High (main red) | This change: re-point at `native-ci.yml` | Closed by `6c4b4e2` |
| D002 | Implementation: parser rejected real lines carrying a YAML `run:` key prefix | Phase 3 (this change) | High (would fail CI) | Strip optional `run:` prefix; all fixtures + guard re-run green | Closed |
| D003 | No test covered the block-scalar / bare-command form that D002's fix supports | Phase 3 test gap | Medium | Added `install_pin_accepts_block_scalar_line` | Closed |
| D004 | Doc-comment typo ("panks") | Phase 3 (this change) | Low | Fixed; grep-verified; fmt+tests re-run | Closed |

Defect-loop discipline: D002/D003/D004 are Phase-3 defects → fixed in
Phase 3 and re-entered verification (did not bypass). D001 is the
change's raison d'être, originating in #342's design phase.

## Deviations from Approved Design

| Deviation | Reason | Impact |
|-----------|--------|--------|
| Added one unit test beyond the design's six (`install_pin_requires_version`, `install_pin_rejects_non_numeric_version`, `install_pin_accepts_block_scalar_line` — design named six categories, implementation has nine tests) | Design's panic contract has more branches than its test table enumerated; each branch deserves a test | More coverage, no behaviour change |
| `cargo clippy` scoped to the touched test target instead of `--workspace --all-targets` (BUILD.md canonical set) | Fresh worktree target dir; full-workspace clippy runs on the Gitea gate pre-merge | None for the gate; documented for reviewer |

## Verification Interview

Not run via formal interview (async review context). Questions folded into
the PR description for Alexander's review: (1) approve the deviations
above, (2) confirm no additional edge cases from production history beyond
runs 36153/36127/522/524, (3) confirm validation N/A (verification-only
change, no user-visible behaviour).

## Gate Checklist

- [x] All new logic branches have unit tests (ubs-scanner substituted: not available)
- [x] Edge cases covered (fail-closed panics, duplicates, boundary, block scalar)
- [x] Integration against the real workflow file verified
- [x] Pre-existing guards unaffected (4/4 pass)
- [x] fmt + clippy clean
- [x] Defect register complete; no open critical/high
- [x] Traceability matrix complete
- [ ] Human approval received (pending — PR #345 review)
- [ ] Post-merge: next native run on main is green (pending merge)

## Approval

| Approver | Role | Decision | Date |
|----------|------|----------|------|
| Alexander Mikhalev | Owner | Pending (PR #345) | — |
