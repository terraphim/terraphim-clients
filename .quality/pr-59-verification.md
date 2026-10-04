# Verification Report: PR #59 Fix #4325 grep default-feature smoke test

**Status**: Verified
**Date**: 2026-08-30
**Branch**: `task/4325-grep-default-smoke-echo` @ `b0b53ef` (rebased onto current main)
**Phase 2 Doc**: n/a (test addition; inline rationale in test file)
**Reference**: terraphim/terraphim-ai#4325

## Summary

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| UBS scan | 0 critical | n/a (UBS rust module cache broken) | DEGRADED |
| Rustfmt | clean | clean | PASS |
| Clippy | 0 warnings | 0 warnings | PASS |
| New smoke test | passes | passes | PASS |
| Full grep suite | all pass | 72/72 (48 lib + 15 + 1 new + 3 + 1 + 3 + 1 ignored doctest) | PASS |
| Pre-existing adf.toml commit | obsolete on main | dropped during rebase (see Defect Register) | RESOLVED |

## Specialist Skill Results

### Code Review (`code-review` skill) — PASS

Manual review of `crates/terraphim_grep/tests/default_feature_smoke.rs`:

- Uses `env!("CARGO_BIN_EXE_terraphim-grep")` to obtain the freshly-built
  binary path, eliminating any chance of testing a stale `cargo run`ed
  version. This is the same built-binary pattern as
  `tests/router_capability_routing.rs` and `tests/no_thesaurus_cli.rs`.
- Creates a one-file corpus with a known matching token
  (`smoke_target_match`).
- Invokes the binary with `--json` so the assertion can parse the
  chunks array directly from stdout.
- Failure message explicitly cites the regression ("DEFAULT-FEATURE
  REGRESSION (terraphim/terraphim-ai#3025): ... Is `code-search` still
  in the `default` feature set?") so the failure mode is actionable
  without code archaeology.
- Distinct from `no_thesaurus_cli.rs` (which guards KG-absent fallback
  behaviour) — this test's single purpose is the default-feature
  contract, as documented in the module docstring.

CI workflow change (`.github/workflows/ci.yml`): adds the test
invocation between the enrichment-feature test (line 28) and the #95
install-graph regression test (line 31). Placed where related
per-feature regressions live.

### Requirements Traceability (`requirements-traceability` skill)

| Requirement (Source) | Implementation | Test | Status |
|----------------------|----------------|------|--------|
| #4325: default-feature `terraphim-grep` must return chunks | new CI invocation + integration test | `default_feature_build_returns_nonzero_chunks` | PASS |

## Defect Register

| ID | Description | Origin Phase | Severity | Resolution | Status |
|----|-------------|--------------|----------|------------|--------|
| D-PR59-01 | Second commit `fix(adf): remove invalid 'on_demand' schedule; mark disciplined-* agents Growth (on-deman)` was superseded by main | Phase 3 (pre-existing) | Low | `git rebase --skip` on the obsolete commit during rebase; main's correct `Core` layer choice preserved | Closed |
| D-PR59-02 | `.cachebro/` cleanup no longer needed (already in main from PR #44) | n/a | n/a | None required | Closed |

The PR was effectively a single-commit change by the time it landed on
current main (the second commit was made obsolete by a direct fix on
main that pre-dates this campaign). The remaining single-commit payload
is clean: one new test file and one new CI line.

## Gate Checklist

- [x] UBS — DEGRADED (infrastructure); clippy substitutes
- [x] New test green
- [x] Full grep suite green
- [x] Rustfmt clean
- [x] Clippy clean
- [x] Traceability complete
- [x] Defect register documented

## Approval

| Approver | Role | Decision | Date |
|----------|------|----------|------|
| Disciplined Verification Specialist | Phase 4 gate | Approved | 2026-08-30 |
