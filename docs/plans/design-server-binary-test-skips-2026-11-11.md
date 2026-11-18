# Design Document: Server-dependent tests skip when the server binary is absent

**Status**: Approved (Phase 2 gate passed)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-11
**Research input**: `docs/plans/research-server-binary-test-skips-2026-11-11.md`
**Mode**: `full` (cross-module test infrastructure; the change touches five suites and the shared support module)

## Objective

Make `cargo test -p terraphim_agent --all-targets` report **zero failures**
on a machine that has no `terraphim_server` binary, while leaving CI behaviour
unchanged: with the binary present, every affected test still runs and still
fails when the server misbehaves.

Acceptance source: terraphim-clients#466, item 3.

## Constraints Carried From Research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | Rust has no runtime skip state | "Skip" = print a notice and return early; the suite must not fail |
| C2 | `terraphim_server` is not a workspace member | The helper resolves a *path*; it never builds one |
| C3 | Skip must trigger on absence only | The helper distinguishes "absent" from "present but broken" |
| C4 | CI installs the binary and exports `TERRAPHIM_SERVER_BIN` | The present-binary path must remain exactly as it is |

## API Design

New file: `crates/terraphim_agent/tests/support/server_binary.rs`
(exported through `crates/terraphim_agent/tests/support/mod.rs`).

```rust
/// Resolve a `terraphim_server` binary from an explicit environment value and
/// a fallback path.
///
/// Returns `None` when neither candidate points at a real file. Pure: it takes
/// its inputs as arguments, so the resolution rules are testable without
/// touching the process environment.
pub fn resolve_server_binary(env_value: Option<&str>, fallback: &Path) -> Option<PathBuf>;

/// The `terraphim_server` binary this machine can use, if any.
///
/// Resolution order: `TERRAPHIM_SERVER_BIN`, then `<workspace>/target/debug/terraphim_server`.
pub fn server_binary_path() -> Option<PathBuf>;

/// Announce that a test is skipped because no server binary is available.
///
/// Prints the repository's standard notice and returns `true`; returns `false`
/// when a binary exists and the test must run.
pub fn skip_without_server(test: &str) -> bool;
```

Notice wording (single source, matches the existing precedent in
`extract_functionality_validation.rs`):

```
skipping: <test>: no terraphim_server binary (<env var> unset and <fallback> absent)
```

### New fields / values introduced

| Item | Type | Where it is set | Where it is read | Traces to |
|------|------|-----------------|-----------------|-----------|
| `env_value` (`TERRAPHIM_SERVER_BIN`) | `Option<&str>` | `server_binary_path()` | `resolve_server_binary()` | C2, C4 |
| `fallback` (`target/debug/terraphim_server`) | `&Path` | `server_binary_path()` | `resolve_server_binary()` | C2 |
| `test` (test name in the notice) | `&str` | each call site | `skip_without_server()` | C1 |

Every new value has a producer, a consumer, and an acceptance criterion; none
is declared without a reference.

## Call Sites

| File | Change | Tests affected |
|---|---|---|
| `tests/support/server_binary.rs` (new) | the helper above | — |
| `tests/support/mod.rs` | declare the module | — |
| `tests/support/binary.rs` | keep `server_binary()` as the path *string* resolver, but implement it on top of the shared helper so there is one resolution order | — |
| `tests/server_mode_tests.rs` | `start_test_server()` returns `Ok(None)` when no binary; each test returns early | 11 |
| `tests/cross_mode_consistency_test.rs` | delete `ensure_server_binary()`; use the helper | 4 |
| `tests/kg_ranking_integration_test.rs` | delete `ensure_server_binary()`; use the helper | 3 |
| `tests/integration_tests.rs` | replace `server_binary_path()` with the helper | 1 |
| `tests/server_process_guard_test.rs` | delete the local `server_binary()`; use the helper | 2 |
| `tests/extract_functionality_validation.rs` | route its notice through the helper (one wording) | 0 (already skipping) |

## Implementation Steps

1. Add `tests/support/server_binary.rs` with the three functions and unit
   tests for the pure `resolve_server_binary`.
2. Export it from `tests/support/mod.rs`.
3. Make `tests/support/binary.rs::server_binary()` delegate to the new
   resolver, so the resolution order exists once.
4. Convert the five suites, one file at a time, to the shared helper. Each
   conversion keeps the existing test bodies unchanged apart from an early
   return when the helper says "skip".
5. Route the existing `extract_functionality_validation.rs` notice through the
   helper.
6. Add `tests/server_binary_resolution.rs` with the pure-resolution tests
   (environment value wins, missing environment value falls back, a
   non-existent environment value is treated as absent, a present file is
   accepted).

## Test Strategy

| Level | What it proves |
|-------|----------------|
| Unit (`resolve_server_binary` cases) | The resolution rules, without env mutation |
| Suite (`cargo test -p terraphim_agent --all-targets` on a machine without the binary) | The acceptance criterion: 0 failures instead of 21, with skip notices |
| CI (native-ci, binary installed) | The present-binary path still runs; the affected tests still fail on a real server regression |

Author gate: `cargo fmt --all -- --check`,
`cargo clippy -p terraphim_agent --all-targets -- -D warnings`,
`cargo test -p terraphim_agent --all-targets --no-fail-fast`.

## Avoid At All Cost

| Item | Marker |
|------|--------|
| Making `terraphim_server` a workspace member or vendoring it | Out-of-scope: it lives in terraphim-ai; the change would not be a local repair |
| Marking the suites `#[ignore]` and enabling them in CI separately | Out-of-scope: the CI lanes already export `TERRAPHIM_SERVER_BIN`, and `#[ignore]` would remove the tests from the default CI run |
| Skipping on *any* server start error (not only absence) | General: skipping on a broken server would hide real regressions (C3) |
| Replacing `set_env_var` in these suites | Out-of-scope: the unsafe-audit epic (#438) owns the env-mutation pattern |
| Changing the CI workflow to install the binary differently | Out-of-scope: CI already installs it; the fix is local to the test suites |

## Risks

| Risk | Mitigation |
|------|------------|
| A test silently stops running in CI | The helper only skips when the binary is absent; CI installs it, so the skip cannot trigger there. The author gate re-runs the suite locally *and* CI proves it. |
| The notice is swallowed by nextest | The wording starts with `skipping:` and the test name, matching the existing precedent |
| One suite is missed | The acceptance criterion is measured on the whole package, not per file |

## Rollback

The change is confined to test code; reverting the branch restores the current
behaviour with no production impact.

## Next Steps

Phase 3 (implementation) per the steps above, then verification and validation
against the acceptance criterion.

## Gate Record: Phase 2 (Design) Quality Evaluation

**Document type**: Implementation Plan / Design Document
**Phase transition**: Phase 2 (Design) -> Phase 3 (Implementation)
**Evaluator**: opencode-cli (self-evaluation against the KLS rubric)
**Date**: 2026-11-11

| Dimension | Score | Justification |
|-----------|-------|---------------|
| Physical | 4/5 | Tables for constraints, API, call sites, risks; code blocks for the signatures. |
| Empirical | 4/5 | Every call site names a file and a test count; the notice wording is quoted verbatim from the precedent. |
| Syntactic | 4/5 | Constraints C1-C4 flow into the API, the steps and the risks; no orphan section. |
| Semantic | 4/5 | The API matches the domain (resolution order, absence vs broken) and the existing support-module conventions. |
| Pragmatic | 5/5 | Six ordered steps, a per-level test strategy, a measurable acceptance command and a rollback statement. |
| Social | 3/5 | The requester has been asked to confirm the interpretation of "skip" on #466; no reply yet. |

**Average**: 4.0/5 | **Minimum**: 3/5 (Social)

### Essentialism

| Check | Status | Evidence |
|-------|--------|----------|
| Vital Few (<=5 items) | Pass | One helper, five call-site conversions, one wording change, one unit-test file. |
| Eliminated Noise | Pass | The "Avoid At All Cost" table names five rejected alternatives with markers. |
| Effortless Path | Pass | Builds on the existing `tests/support/` module and an existing in-repo precedent; no new dependency. |
| 90% Rule | Pass | Every step maps to acceptance item 3 of #466 or to a constraint that protects it. |

### Traceability

**Check 1 - Avoid At All Cost items carry markers**: all five carry
`Out-of-scope:` or `General:` markers. **Pass.**

**Check 2 - new fields have references**:

| New value | In API Design | In Implementation Steps | Traces to |
|-----------|---------------|-------------------------|-----------|
| `env_value` | Yes | Steps 1, 3, 6 | C2, C4 |
| `fallback` | Yes | Steps 1, 3, 6 | C2 |
| `test` (notice) | Yes | Steps 1, 4, 5 | C1 |

**Pass.** No value is declared without a producer, a consumer and a criterion.

### Decision

**CONDITIONAL PASS** - all dimensions >= 3, average 4.0 >= 3.5, both
traceability checks pass, essentialism satisfied.

**Required action before closing the phase**: the interpretation of
"reports skips" was published on #466 for the requester's confirmation
(comment on 2026-11-11). If the requester requires runner-level "ignored"
marking, the design changes (out of Rust's reach at runtime) and the issue
stays open; the local-skip behaviour would still be delivered.

**Recommended actions**: keep the notice wording identical in the helper and
in the README of the test support module; when the unsafe-audit epic lands,
replace the remaining `set_env_var` calls in these suites with `Command::env`.
