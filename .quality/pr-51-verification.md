# Verification Report: PR #51 Fix #48 thesaurus NotFound ERROR suppress

**Status**: Verified
**Date**: 2026-08-30
**Branch**: `task/48-impl` @ `69faea4`
**Phase 2 Doc**: inline in `crates/terraphim_agent/src/logging.rs` module-level docstring
**Reference**: terraphim/terraphim-clients#48

## Summary

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| UBS scan | 0 critical | n/a (UBS module checksum mismatch; deferred) | DEGRADED |
| Rustfmt | clean | clean | PASS |
| Clippy | 0 warnings | 0 warnings | PASS |
| Unit tests (logging) | 7/7 pass | 7/7 pass | PASS |
| Unit tests (crate) | all pass | 444/444 pass | PASS |
| End-to-end probe | benign ERROR suppressed | suppressed; no ERROR in stderr | PASS |
| Hygiene cleanup | none required | none (already addressed by PR #44 follow-on) | PASS |

## Specialist Skill Results

### Static Analysis (`ubs-scanner` skill) — DEGRADED

Same UBS rust module checksum-mismatch infrastructure issue. Clippy
`-D warnings` substitutes; manual review confirms.

### Code Review (`code-review` skill) — PASS

Manual review of the new module:

- `is_benign_thesaurus_not_found`: narrow predicate. Filters by
  - `level == Error`
  - `target` starts with `terraphim_service`
  - message contains `Failed to load thesaurus`
  - lowercased message contains `not found` or `notfound`

  This covers both `Display` (`"Not found: thesaurus_default.json"`)
  and `Debug` (`NotFound("thesaurus_default.json")`) renderings of the
  underlying `terraphim_persistence::Error::NotFound`. Genuine failures
  such as `"Failed to build thesaurus from local KG"` do not match the
  `"Failed to load"` substring and are preserved.

- `FilteredLogger<L: Log>`: thin wrapper implementing the `Log` trait
  by delegating to the inner logger after the predicate check. No
  buffering, no state — just a `L::enabled`, `L::log`, `L::flush`.

- `build_inner_logger`: mirrors `terraphim_service::logging::detect_logging_config`.
  Honours explicit `LOG_LEVEL` env var; defaults to `INFO` in debug
  builds and `WARN` in release. Format unchanged (`format_timestamp_secs`,
  `format_module_path(false)` in release).

- `init_logging`: `Once::call_once` + `set_boxed_logger`. Documented
  to be a no-op when another logger is already installed, which
  matters for test harnesses. This is the standard pattern.

- Service call site change (`crates/terraphim_agent/src/service.rs:45-50`):
  replaces the old `terraphim_service::logging::init_logging(...)` call
  with `crate::logging::init_logging()`. Old call site is removed in
  full — no dangling references.

- Tests use `CapturingLogger`, a real `Log` impl with a `Vec` of
  records behind a `Mutex`. Not a mock (per project policy), and the
  test that exercises the predicate pins the exact reproduction string
  derived from `terraphim_persistence::Error::NotFound("thesaurus_default.json")`
  formatted with `{:?}`.

### Requirements Traceability (`requirements-traceability` skill)

| Requirement (Source) | Implementation | Test | Status |
|----------------------|----------------|------|--------|
| #48: benign thesaurus NotFound ERROR must not appear in stderr | `FilteredLogger` + `is_benign_thesaurus_not_found` | 7 unit tests + end-to-end probe | PASS |
| #48: genuine thesaurus errors must still surface | `is_benign_thesaurus_not_found` requires `"Failed to load thesaurus"` substring | `predicate_preserves_genuine_thesaurus_failures` | PASS |
| #48: must not regress non-error log lines | wrapper delegates everything non-matching to inner logger | `filtered_logger_drops_benign_and_keeps_the_rest` | PASS |

## Defect Register

| ID | Description | Origin Phase | Severity | Resolution | Status |
|----|-------------|--------------|----------|------------|--------|
| (none found) | - | - | - | - | - |

The PR was clean: no rustfmt issues, no clippy warnings, no hygiene
leaks (the `.cachebro/` files were already removed by the PR #44
follow-on; the `git merge main` here did not reintroduce them).

## Gate Checklist

- [x] UBS scan — DEGRADED (infrastructure); clippy substitutes
- [x] All new public functions have unit tests (7/7)
- [x] Edge cases from predicate covered (Display vs Debug, level, target)
- [x] Traceability matrix complete
- [x] Code review checklist passed
- [x] Rustfmt clean
- [x] Clippy clean
- [x] End-to-end probe (stderr clean of benign ERROR) PASS

## Approval

| Approver | Role | Decision | Date |
|----------|------|----------|------|
| Disciplined Verification Specialist | Phase 4 gate | Approved | 2026-08-30 |
