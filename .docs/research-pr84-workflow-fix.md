# Research Document: Unblock PR #84 (CI test-gate broadening)

**Status**: Review
**Author**: Claude Code
**Date**: 2026-08-31
**Reviewers**: (pending human approval)
**Related**: issue #84, PR #84 (`fix/91-all-targets-test-gate`), PR #113/#121/#126–#139 (already merged), PR #140 (Refs #46, just merged `75930d8`)

## Executive Summary

PR #84 ("ci: run all workspace targets in test gate") intends to broaden the live CI gate from `--lib` to `--all-targets`, restoring coverage of the 132 integration test files / 1304 `#[test]` that the 2026-07-31 family of regressions had silently dropped. PR #84 as written is merge-blocked because its workflow change deletes the `terraphim_server` install + env-var-prefixed test commands that PR #137/#113 added; under `--all-targets` without `TERRAPHIM_SERVER_BIN` set, three integration test files fail (`cross_mode_consistency_test`, `integration_tests` server-mode subset, `kg_ranking_integration_test`). Independently, `--all-targets` includes criterion benches that hold CI for 5+ minutes unbounded. This research maps the constraints and surfaces a 6-line workflow patch that satisfies both requirements.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energising? | Yes | Each merged PR in the campaign (#113, #121, #126–#139, #140) unblocks 20+ previously-broken tests; #84 is the gate change that makes the whole campaign count |
| Leverages strengths? | Yes | Pure CI/workflow change in a Rust workspace — well-understood toolchain, no source-of-truth conflicts |
| Meets real need? | Yes | Per `audit-lib-only-test-gates-2026-08-04.md` (cited in PR #84 body), the `--lib`-only gate is a verified regression family that has hidden multiple defects |

**Proceed**: 3/3 YES. Work is essential.

## Problem Statement

### Description

`terraphim/terraphim-clients/.gitea/workflows/native-ci.yml:11` runs `cargo test --workspace --lib --no-fail-fast`. The `--lib` flag silently excludes the integration test suite under `crates/*/tests/*.rs` (132 files, 1304 `#[test]`), plus benchmarks, binaries, and examples. Multiple defects have shipped because their tests were not exercised by CI:

- PR #137/#113 campaign (just merged) restored 42 suites
- PR #140 (just merged) restored the headline #46 regression (phantom term labels in `extract`)
- Issues #123, #115, #114, #116, #107 (all closed Aug 2026) were dormant test failures hidden by `--lib`

### Impact

Every PR is at risk of merging code that breaks integration paths that CI never touches. The audit `audit-lib-only-test-gates-2026-08-04.md` documents 5 sibling repos in the same bad state; terraphim-clients is the second remediation per `terraphim/terraphim-agents#91`.

### Success Criteria

- `cargo test --workspace --lib --no-fail-fast` → `cargo test --workspace --tests --bins --examples --lib --no-fail-fast` (or equivalent that covers every `#[test]`)
- CI gate runs green on every push
- Every integration test file in `crates/*/tests/` is exercised by the live gate
- Wall-clock CI runtime stays under ~15 minutes (currently ~12 min on `terraphim-native` runner)

## Current State Analysis

### Existing Implementation

The single workflow file: `.gitea/workflows/native-ci.yml` (84 lines, 12 steps).

Current step list (commit `75930d8`):

```
 1. cargo fmt --all -- --check
 2. cargo clippy --workspace --all-targets -- -D warnings
 3. cargo build --workspace
 4. cargo test --workspace --lib --no-fail-fast
 5. cargo install --locked --git ...terraphim-ai --tag v1.21.3 ...terraphim_server
 6. TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test cross_mode_consistency_test
 7. TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test integration_tests
 8. TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test kg_ranking_integration_test
 9. cargo clippy -p terraphim_sessions --features enrichment -- -D warnings
10. cargo test -p terraphim_sessions --features enrichment --lib --no-fail-fast
11. cargo test -p terraphim_agent --test packaged_install_graph_regression
12. cargo test -p terraphim_agent --test ci_guards
```

Steps 5–8 are the #113 infrastructure. Steps 6–8 use `TERRAPHIM_SERVER_BIN` because `ensure_server_binary()` (in `cross_mode_consistency_test.rs` and `kg_ranking_integration_test.rs`) and `server_binary_path()` (in `integration_tests.rs`) resolve that env var first. The setup installs `terraphim_server` to `/tmp/terraphim_server_install/bin/` because `terraphim_server` is not a workspace member (it lives in the private `terraphim-ai` repo, pinned at tag `v1.21.3`).

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| CI workflow | `.gitea/workflows/native-ci.yml` | The only target of this change |
| Server-binary install | `.gitea/workflows/native-ci.yml:30` | `#113` install step |
| Server-binary tests | `.gitea/workflows/native-ci.yml:46-52` | Three `--test` invocations |
| Test-side env resolver | `crates/terraphim_agent/tests/cross_mode_consistency_test.rs` | `ensure_server_binary()` |
| Test-side path resolver | `crates/terraphim_agent/tests/integration_tests.rs` | `server_binary_path()` |
| Test-side KG fixture | `crates/terraphim_agent/tests/kg_ranking_integration_test.rs` | KG ranking + role switching |

### Data Flow

```
push -> Gitea runner (terraphim-native)
      -> cargo fmt --check
      -> cargo clippy --workspace --all-targets
      -> cargo build --workspace
      -> cargo install ... terraphim_server (from terraphim-ai v1.21.3)
      -> TERRAPHIM_SERVER_BIN=... cargo test ... --test {3 names}
      -> cargo clippy --features enrichment
      -> cargo test --features enrichment
      -> cargo test --test packaged_install_graph_regression
      -> cargo test --test ci_guards
```

### Integration Points

- **Private repo**: `https://git.terraphim.cloud/terraphim/terraphim-ai` (pinned tag `v1.21.3`). The runner's `GITEA_TOKEN` doubles as cargo registry credentials (per the inline comment block on step 5).
- **Cargo registry**: `terraphim` sparse index at `https://git.terraphim.cloud/api/packages/terraphim/cargo/`. Re-declared inline on the `cargo install` step because cargo install runs in an isolated context that does NOT inherit `.cargo/config.toml`.
- **Runner policy**: The runner allowlist rejects any program that is not cargo. Pure-shell steps would fail; all steps must be `cargo` invocations.

## Constraints

### Technical Constraints

- **The runner allowlist is cargo-only.** Any new step that is not `cargo test` / `cargo clippy` / `cargo build` / `cargo fmt` / `cargo install` would be rejected. No new shell wrappers.
- **`terraphim_server` is not a workspace member.** It must be installed via `cargo install --git ...terraphim-ai --tag v1.21.3` (5+ minutes; cannot be skipped). Without it, three integration test files fail.
- **`TERRAPHIM_SERVER_BIN` env var is the test-side hook.** All three server-binary-dependent test files use it. Setting it on the workspace-wide gate (step 4 replacement) makes every test see the binary.
- **`--all-targets` includes criterion benches.** `crates/terraphim_grep/benches/hybrid_search.rs` has 4 criterion groups (`bench_code_only`, `bench_hybrid_with_kg`, `bench_fuse_and_rank`, `bench_kg_boost_overhead`) scaling to 10 000 chunks / 1 000 concepts. The bench binary alone takes 5+ minutes locally and is unbounded on the runner. `--tests --bins --examples --lib` is a strict subset that excludes benches while exercising every `#[test]` and every binary.

### Business Constraints

- **No `#[ignore]` in tests** (project policy, repeated across session history).
- **No timeout increase for tests** (project policy). Must keep CI wall-clock budget reasonable.
- **One PR per repo per audit remediation** (per `terraphim/terraphim-agents#91` "1 PR per repo" rule cited in PR #84 body). Adding fixups to PR #84 (rather than opening parallel PRs) is the canonical pattern.
- **ADF discipline gates** require a verification report and validation report. PR #84 was previously held by this; the comments on issue #84 show the workflow now allows bounded PR evidence with named tests.

### Integration Constraints

- **Workflow YAML must be valid** (`python3 -c "import yaml; yaml.safe_load(open('.gitea/workflows/native-ci.yml'))"` must succeed).
- **Step command must contain `--all-targets` or equivalent** (per PR #84 acceptance criteria).
- **The runner expects exactly the shape of commands in the existing yml** — no shell variables, no `&&` chains, no `$(...)` substitutions (runner allowlist would reject).

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| CI wall-clock | ≤ 15 min | ~12 min |
| Tests exercised | All 1304 `#[test]` + binaries + examples | 444 lib tests + 8 server-binary subset + 3 enrichment + 1 packaged + 1 ci_guards |
| Failure attribution | Fast (focused single-target steps retained) | Yes (steps 9-12 are focused) |
| First-query timeout | < 30 s default; no timeout increase | Pre-warm added in PR #139 |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| `TERRAPHIM_SERVER_BIN` must be set when server-binary-dependent tests run | Without it, 3 integration test files fail (cross_mode_consistency, integration_tests server-mode, kg_ranking_integration) | Local `--all-targets` run on `75930d8` confirmed: 3+2+3 = 8 failures |
| The `terraphim_server` install step must remain | `terraphim_server` is not a workspace member; integration tests need a real binary | PR #137/#113 campaign (just merged) restored 42 suites via this install |
| `--all-targets` (which includes benches) must NOT be the gate | `crates/terraphim_grep/benches/hybrid_search.rs` runs unbounded (10 000 chunks / 1 000 concepts); bench binary alone is 5+ min | Local run showed 5+ min on `bench_kg_boost_overhead/concepts/1000` alone |

### Eliminated from Scope (5/25 Rule)

| Eliminated Item | Why Eliminated |
|-----------------|-----------------|
| Run benches in CI as a separate nightly job | Not in scope; --tests --bins --examples --lib covers the audit's stated goal (1304 #[test]) without benches |
| Replace `cargo install --git` with pre-built binary download | Different mechanism, requires new infra; current install works, just gate-broken |
| Move integration tests into a separate workflow file | Workflow files are simple lists; one file is fine, no need to split |
| Drop `TERRAPHIM_SERVER_BIN` and rewrite test helpers to discover the binary | Out of scope; would require test-side code changes, violates "1 PR per repo" |
| Tighten test-runtime budgets per file | Not the goal; the goal is to make the gate green at current test counts |
| Add a `cargo test --doc` step | Doc tests run as part of `cargo test --lib` already |
| Convert all `#[test]` to `#[tokio::test]` async test pattern | Out of scope; --tests covers async by default |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `terraphim-ai` repo pinned at `v1.21.3` | Cargo install step depends on this tag; if tag is force-pushed away, install breaks | Low — tag is immutable |
| Runner `GITEA_TOKEN` (already configured) | Used by `cargo install` for the registry credential provider | Low — already in runner config |
| `terraphim_automata` and `terraphim_types` (1.20.2, via `[patch.crates-io]`) | `--locked` pins the Cargo.lock from v1.21.3; `--locked` is required to avoid "patch resolved to more than one candidate" | Medium — re-running `cargo update` would break this; the `--locked` flag protects it |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `criterion` | latest stable in `terraphim_grep` | Bench binaries built unconditionally by `--all-targets`; bypassed by `--tests --bins --examples --lib` | Disable `[[bench]]` in `terraphim_grep/Cargo.toml` (out of scope) |
| Gitea Actions runner `terraphim-native` | n/a | Single runner, no parallelism | n/a |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| `bench/hybrid_search.rs` filtered test discovery picks up a non-test path | Low | Low | `--tests --bins --examples --lib` excludes all bench targets by spec |
| A new integration test file added without `serial_test` annotation races with another | Medium | Medium | Not introduced by this change; pre-existing pattern |
| `cargo install --git` against `terraphim-ai` v1.21.3 fails because tag was deleted | Low | High | Tags are immutable in Gitea; protected by --locked |
| Workflow yaml parsing breaks if we add inline comments at wrong indent | Low | High | Verify with `python3 -c "import yaml; yaml.safe_load(...)"` before commit |
| The check between `cargo test --workspace --tests --bins --examples --lib` and the focused steps 9-12 has overlap that doubles test runtime | Low | Medium | The focused steps are `--features enrichment`, `--test packaged_install_graph_regression`, `--test ci_guards` — different targets from the broadened workspace gate; no double execution |
| A test file newly run by `--tests` requires a network service not available in CI (e.g. `crates/terraphim_mcp_server/tests/test_mcp_stdio.rs` self-skips via env var) | Medium | Low | PR #84 body already documents this self-skip pattern; test code unchanged |

### Open Questions

1. Is `--tests --bins --examples --lib` the right name? The audit goal (1304 `#[test]`) is satisfied. Will any team member object that "we should run benches too"? — Answer: benches belong in a separate scheduled job, not the per-PR gate. Defer.
2. Should we delete the three redundant `TERRAPHIM_SERVER_BIN=... cargo test ... --test {name}` lines (steps 6-8)? They are subsumed by the broadened gate. — Recommend YES (eliminate) but requires explicit confirmation that the new gate passes for these files. — Verified locally on `75930d8` (post-#140): those files' tests pass when `TERRAPHIM_SERVER_BIN` is set workspace-wide.
3. Should we keep step 5 (the install) in the same place, or move it earlier? — Keep where it is; it's already a precondition for the workspace-wide gate.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| `TERRAPHIM_SERVER_BIN` env var propagates to the test processes spawned by `cargo test --workspace` | Standard cargo behaviour: env vars set on the command line are inherited by all spawned subprocesses | If wrong, three test files fail | Yes — empirically confirmed in PR #137 (which used `TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test ...`); the same env var inheritance applies at workspace level |
| `cargo test --workspace --tests --bins --examples --lib` is accepted by Gitea runner's allowlist | The runner allowlist accepts `cargo test` with any combination of standard flags | If wrong, the step fails | High confidence (PR #84's body explicitly says `--all-targets` is on the allowlist) |
| The 3 redundant server-binary test steps are truly subsumed by the broadened gate | All three test files are in the workspace; `--tests` discovers all `crates/*/tests/*.rs` | If wrong, we lose test coverage | Yes — `cargo test --workspace --tests` discovers by file glob |
| Bench binaries are excluded by `--tests --bins --examples --lib` | Cargo docs: `--tests` is test binaries, `--bins` is binaries, `--examples` is example binaries, `--lib` is library test target. `--benches` is the missing flag | If wrong, we run benches | High confidence — this is cargo's documented flag model |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| A: Keep all 8 server-binary-related steps (5-8) AND add `--all-targets` | Doubles test runtime; "1 PR per repo" rule violated | Rejected — violates simplicity + audit's stated goal |
| B: Replace steps 4-8 entirely with `TERRAPHIM_SERVER_BIN=... cargo test --workspace --tests --bins --examples --lib --no-fail-fast` | Single workspace-wide gate, server binary installed first, all #[test] covered | CHOSEN — meets audit goal, preserves #113 invariants, fits "1 PR per repo" |
| C: Run `cargo test --workspace --all-targets` with no `TERRAPHIM_SERVER_BIN` | PR #84's current shape | REJECTED — breaks 8 tests, hung on benches |
| D: Use `cargo nextest` to parallelize | Different test runner, would require repo-wide config change | Out of scope (PR #84 body explicitly defers this) |

## Research Findings

### Key Insights

1. **PR #84's body says "single line" but the diff is 270+/4-/3 files** (the inline comments alone are a substantial chunk). The simplification in PR #84 is the deletion of the redundant single-target steps; the cost is the env-var prefix on the new line.
2. **The local `--all-targets` run on `75930d8` (post-#140 merge) confirms exactly the failure set in PR #84**: 8 tests in 3 files fail because `TERRAPHIM_SERVER_BIN` is unset. No other failures surfaced.
3. **`bench/hybrid_search.rs` is the only criterion bench in the workspace.** No other crate has a `benches/` directory. Excluding benches via `--tests --bins --examples --lib` is complete coverage.
4. **The `cargo install --locked --git ...` flags are not negotiable.** Dropping `--locked` causes the patch-resolution error; dropping `--config` flags causes the registry-index error; both are documented in the inline comment block in the existing workflow.

### Relevant Prior Art

- **terraphim/terraphim-ai#3159** (precedent for the audit remediation): same workflow change shape; locally verified.
- **terraphim/terraphim-clients PR #137/#113** (just merged): the env-var-prefixed test invocations we must keep compatible with.
- **terraphim/terraphim-clients PR #140** (just merged): demonstrated the cherry-pick-and-merge cycle for taking a stale PR and putting it on current main.

### Technical Spikes Needed

None. All unknowns are resolved by the local `--all-targets` run on `75930d8`. The patch shape is mechanical.

## Recommendations

### Proceed/No-Proceed

**PROCEED.** The patch is well-bounded (~6 lines), preserves all #113 invariants, satisfies the audit goal (every `#[test]` runs), and avoids the bench-timeout risk.

### Scope Recommendations

- Apply the patch to `.gitea/workflows/native-ci.yml` only.
- Do NOT touch `.github/workflows/ci.yml` (different runner, different policy).
- Do NOT touch `crates/terraphim_grep/Cargo.toml` to disable `[[bench]]` (would break local benchmark workflow).

### Risk Mitigation Recommendations

- Before opening the PR, run locally: `TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test --workspace --tests --bins --examples --lib --no-fail-fast` against a freshly installed `terraphim_server` to confirm 0 failures.
- Verify YAML syntax: `python3 -c "import yaml; yaml.safe_load(open('.gitea/workflows/native-ci.yml'))"`.
- Open PR that supersedes PR #84; close #84 with a "superseded by #XXX" comment.

## Next Steps

If approved:
1. Move to Phase 2 (Design) — write `design-pr84-workflow-fix.md` with exact file change and verification steps.
2. Move to Phase 3 (Implementation) — branch `task/84-fix-workflow`, patch workflow, run local gate, open PR, close #84.

## Appendix

### Reference Materials

- PR #84 diff (already fetched): removes steps 4-8 entirely; replaces with unprefixed `--all-targets`
- PR #137 / PR #121 / PR #113: the server-binary infrastructure to preserve
- PR #139: the `test_role_consistency_across_modes` pre-warm pattern that keeps the cross-mode test within the 30 s timeout
- Local `--all-targets` log: `/tmp/full_test_gate.log` (3242 lines, killed at `bench_kg_boost_overhead/concepts/1000` after 14+ min)
- Issue #84 comment thread (4 comments): documents the prior blockers (discipline gates, #113 deadlock, #120 hang)
