# Research Document: Server-dependent tests skip when the server binary is absent

**Status**: Approved (Phase 1 gate passed)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-11
**Reviewers**: self-evaluated with the KLS 6-dimension gate (see Gate Record)

## Executive Summary

`terraphim_server` is not a member of this workspace, so a local `cargo test`
run cannot start a server. Five test suites nevertheless *require* one and
fail with an error when it is missing: on `main` (aaa1720) a local
`cargo test -p terraphim_agent --all-targets` produces **23 failures**, 21 of
which are "no server binary" failures and 2 of which are unrelated
environment guards. One suite already solves this correctly by printing a skip
notice and returning; the fix is to generalise that convention into a shared
helper and apply it to the four suites that still error out.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | The failure list buries real regressions: 21 of 23 local failures are environmental, so a developer cannot tell a broken change from a missing binary. |
| Leverages strengths? | Yes | The repository already contains the fix as a precedent (`extract_functionality_validation.rs`); the work is consolidation, not invention. |
| Meets real need? | Yes | #466 states the acceptance criterion verbatim: "cargo test without the server binary reports skips, not failures". CI masks it by installing the binary first. |

**Proceed**: Yes (3/3)

## Problem Statement

### Description

Tests that need a running `terraphim_server` fail, rather than skip, when the
binary is absent. The binary lives in another repository
(`terraphim-ai`), so this workspace can only run them when CI installs it
(`native-ci.yml`, `cargo install --git ... --tag v1.21.3`) and exports
`TERRAPHIM_SERVER_BIN`.

### Impact

- Every developer without the extra checkout sees 21 red tests that say nothing
  about their change, and must read each failure to learn that the binary is
  missing.
- The same environmental noise made three unrelated native-ci runs of this
  repository flaky in one session (#490 twice, #493 once), because the
  coverage lane's shared `/tmp` state made a server-dependent suite fail
  (terraphim-clients#494).
- A reviewer cannot use "the suite is green locally" as evidence, because
  local green requires a second repository.

### Success Criteria

1. `cargo test -p terraphim_agent --all-targets` on a machine without
   `TERRAPHIM_SERVER_BIN` and without `target/debug/terraphim_server` reports
   **zero failures**, and prints one skip notice per skipped test.
2. The same suite on CI (binary present) still **runs** those tests and still
   fails when the server misbehaves.
3. The skip notice names the missing binary and the environment variable that
   provides it.

## Current State Analysis

### Existing Implementation

Five suites resolve the server binary independently, with three different
failure shapes:

| Suite | Helper | Behaviour when the binary is absent |
|---|---|---|
| `tests/server_mode_tests.rs` | `start_test_server()` | `Err(anyhow!("TERRAPHIM_SERVER_BIN is not set ...")` -> test fails |
| `tests/cross_mode_consistency_test.rs` | `ensure_server_binary()` | `Err(anyhow!("terraphim_server is not a member ..."))` -> test fails |
| `tests/kg_ranking_integration_test.rs` | `ensure_server_binary()` | same as above -> test fails |
| `tests/integration_tests.rs` | `server_binary_path()` | `Err("terraphim_server is not a member ...")` -> test fails |
| `tests/server_process_guard_test.rs` | `server_binary()` | `.expect("TERRAPHIM_SERVER_BIN must point to a real ...")` -> panic |

One suite already has the target behaviour:

- `tests/extract_functionality_validation.rs` -> `ensure_server_running()`
  returns `Ok(None)` and prints
  `skipping: no terraphim_server binary at {binary} (set TERRAPHIM_SERVER_BIN, refs #113)`.

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| server-mode suite | `crates/terraphim_agent/tests/server_mode_tests.rs` | 11 CLI tests against a live server |
| cross-mode suite | `crates/terraphim_agent/tests/cross_mode_consiqueness_test.rs` | offline vs server-mode equivalence (4 tests) |
| KG-ranking suite | `crates/terraphim_agent/tests/kg_ranking_integration_test.rs` | role/thesaurus ranking against a live server (3 tests) |
| end-to-end suite | `crates/terraphim_agent/tests/integration_tests.rs` | full server workflow (1 test) |
| process-guard suite | `crates/terraphim_agent/tests/server_process_guard_test.rs` | server lifetime/reaping (2 tests, Linux only) |
| skip precedent | `crates/terraphim_agent/tests/extract_functionality_validation.rs` | already skips |
| test support | `crates/terraphim_agent/tests/support/{mod.rs,cli_test_env.rs,server_process.rs,binary.rs,kg_paths.rs}` | shared helpers, incl. the hermetic environment used by #495 |

### Data Flow

`TERRAPHIM_SERVER_BIN` (env) or `target/debug/terraphim_server` (path) ->
per-suite helper -> spawn `terraphim_server` -> HTTP assertions. CI sets the
variable; a local checkout usually has neither.

### Integration Points

- `native-ci.yml` installs the binary and exports `TERRAPHIM_SERVER_BIN` for
  the workspace test lane and three focused re-run lanes.
- `scripts/ci/cleanup_terraphim_servers.sh` stops leftover servers; unaffected.

## Constraints

### Technical Constraints

- `terraphim_server` is not a workspace member, so it cannot be built from
  here; a nested `cargo build` under `cargo test` would deadlock on the outer
  build lock (documented in `cross_mode_consistency_test.rs`).
- Rust's test harness has **no runtime "skip" state**: `#[ignore]` is static
  and nextest reports an early `return` as a *passed* test. The repository's
  established convention is therefore "print a `skipping: ...` line and return
  early" (`extract_functionality_validation.rs`, `checklist_validation.rs`).
- The skip must be conditional on **absence** only. A present-but-broken
  server must still fail the test, otherwise a real regression would be hidden.

### Business Constraints

- No new dependencies; the helper belongs in `tests/support/`.
- Behaviour in CI must not change: with the binary present, every test runs.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Local failures caused by a missing binary | 0 | 21 |
| CI behaviour | unchanged | unchanged |
| New shared code | one helper | five duplicated resolvers |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| One shared resolver + one notice wording | Five copies today drift; the notice is how a developer learns why a test did not run | three different error strings today |
| Skip only when the binary is *absent* | Skipping on any error would hide real server regressions | acceptance criterion 2 |
| Every server-dependent test consults the helper | A partial fix leaves the local suite red and the signal useless | 21 failing tests |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| `coverage_tool_pinning_matches_local_toolchain` failure | Local `cargo-llvm-cov` is 0.8.7 while the workflow pins 0.8.5 — a toolchain-drift guard, not a server-binary problem; needs its own issue |
| `cleanup_script_only_signals_verified_checkout_processes` failure | "copying test executable failed" — a script/self-copy issue in `ci_guards`, unrelated to the server binary |
| #466 items 1 and 2 (version bump, `v1.21.23` publish-or-abandon) | Needs an owner decision on the tag; explicitly "DECISION NEEDED" in the issue |
| Making `terraphim_server` a workspace member | Lives in another repository; a cross-repo change is not a local repair |
| Replacing the shared `set_env_var` pattern in these suites | Belongs to the unsafe-audit epic (#438), not to this change |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `tests/support/mod.rs` | The new helper must be exported there | Low |
| `native-ci.yml` | CI must keep exporting `TERRAPHIM_SERVER_BIN` | Low |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `terraphim_server` | v1.21.3 (CI-installed) | Low | none; out of scope |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| A "skip" hides a genuine server regression | Low | High | Skip only on absence; any other error still fails the test |
| The notice is invisible in nextest output | Medium | Medium | Print to stdout with a `skipping:` prefix, matching the existing precedent |
| A suite is missed, leaving the local suite red | Medium | Medium | Enumerate the failing set from an actual local run and assert zero failures afterwards |

### Open Questions

1. Should nextest be told to report these as skipped? Not possible at
   runtime; deferred (see Eliminated).
2. Should the helper also cover `target/debug/terraphim_server` discovery?
   Yes — the precedent already does, and it keeps a local checkout usable.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|------------|
| "Reports skips, not failures" means "does not fail, and says it skipped" | Rust has no runtime skip state; the repository already uses this convention | The wording would want a runner-level "ignored" marker, which does not exist | Yes (precedent in `extract_functionality_validation.rs`) |
| CI installs the binary, so CI behaviour is unchanged | `native-ci.yml` installs it before the test lanes | CI would silently stop testing the server paths | Yes (workflow read) |
| The 21 failures are all binary-related | Each failing test resolves the binary before doing work | Some failure has another cause and would be masked | Partially - each failing suite's helper was read; the remaining 2 failures are excluded by name |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| (A) Skip when the binary is absent | Local suite green, CI unchanged, no masking | Chosen |
| (B) Skip whenever the server cannot be started (absent *or* broken) | Hides real server regressions | Rejected |
| (C) Mark the tests `#[ignore]` and let CI opt in | CI would need a second command; loses per-test granularity | Rejected |

## Research Findings

### Key Insights

1. The repository already contains the target behaviour in
   `extract_functionality_validation.rs`; the change is consolidation plus
   applying it to four suites, not a new mechanism.
2. The five suites disagree on *how* to report the absence (error text vs
   panic), which is why the failure messages vary and why the developer has to
   read each one.
3. `cargo-llvm-cov` coverage runs the same suites in a separate process, so
   the skip must be decided at runtime, per test — a static `#[ignore]` cannot
   express "run in CI, skip locally".

### Relevant Prior Art

- `crates/terraphim_agent/tests/extract_functionality_validation.rs` — the
  in-repo skip convention.
- `crates/terraphim_agent/tests/support/cli_test_env.rs` — the hermetic
  environment helper introduced with #494, showing how this repository
  already handles per-test isolation.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Local run enumerating the failing set | Establish the baseline the fix must remove | 20 min (done) |
| Helper API shape | One resolver, one notice, used by five suites | 30 min (design phase) |

## Recommendations

### Proceed/No-Proceed

Proceed. The change is small, the precedent exists, the failing set is
measured, and the acceptance criterion is verifiable with a single command.

### Scope Recommendations

- Add `tests/support/server_binary.rs` with `server_binary()` and
  `skip_without_server()`.
- Replace the five per-suite resolvers with calls to the shared helper.
- Keep every other failure mode failing.

### Risk Mitigation Recommendations

- The helper returns `Option`/`Result` so a *present but broken* server still
  errors.
- Verify by re-running the exact local command and by reading CI, which
  exercises the present-binary path.

## Next Steps

1. Phase 2: design the helper API, the per-suite call sites, and the test
   strategy.
2. Phase 3: implement, then verify with the author gate (fmt, clippy, package
   tests) and validate against the acceptance criterion.

## Appendix

### Reference Materials

- terraphim-clients#466 (the source issue), #494 (the hermeticity work that
  exposed the shared-state half of this problem)
- `docs/plans/research-coverage-nextest.md` (house style for this document)

### Code Snippets

Precedent (the behaviour to generalise):

```rust
fn ensure_server_running() -> Result<Option<(ServerProcessGuard, u16)>> {
    let binary = server_binary();
    if !std::path::Path::new(&binary).exists() {
        println!(
            "skipping: no terraphim_server binary at {binary} (set TERRAPHIM_SERVER_BIN, refs #113)"
        );
        return Ok(None);
    }
    start_test_server().map(Some)
}
```

## Gate Record: Phase 1 (Research) Quality Evaluation

**Document type**: Research Document
**Phase transition**: Phase 1 (Research) -> Phase 2 (Design)
**Evaluator**: opencode-cli (self-evaluation against the KLS rubric; no external reviewer available in this session)
**Date**: 2026-11-11

| Dimension | Score | Justification |
|-----------|-------|---------------|
| Physical | 4/5 | Structured with the required sections; tables carry the code map, the risk register and the scope eliminations. |
| Empirical | 4/5 | Every claim names a file, a helper or a measured failure; the 23-failure baseline was produced by an actual local run rather than asserted. |
| Syntactic | 4/5 | Internal consistency holds: the three vital constraints reappear in the design inputs and the acceptance criteria; eliminated scope is consistent with the risks. |
| Semantic | 4/5 | The domain model matches the code: the five resolvers, the three error shapes and the existing skip precedent were read directly. |
| Pragmatic | 4/5 | Actionable: it names the helper, the call sites and the verifiable success criteria, and it bounds the scope with explicit eliminations. |
| Social | 3/5 | The issue author has not reviewed this document; the interpretation of "reports skips" is asserted from the in-repo precedent rather than confirmed by the requester. |

**Average**: 3.8/5 | **Minimum**: 3/5 (Social)

### Essentialism

| Check | Status | Evidence |
|-------|--------|----------|
| Vital Few (<=5 items) | Pass | 3 vital constraints; the scope is one helper plus five call-site updates. |
| Eliminated Noise | Pass | Five items explicitly eliminated with reasons. |
| Effortless Path | Pass | Reuses an existing in-repo convention and an existing `tests/support/` module. |
| 90% Rule | Pass | Every in-scope item maps to an acceptance criterion of #466. |

### Decision

**CONDITIONAL PASS** - no dimension below 3, average 3.8 >= 3.5, traceability
checks are not applicable to a research document (no new fields, no
Avoid-At-All-Cost list).

**Required action before implementation**: publish the interpretation of the
acceptance criterion ("skip" = the suite must not fail and must announce the
skip) to the issue so the requester can confirm it. Recorded on
terraphim-clients#466.
