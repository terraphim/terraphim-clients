# Implementation Plan: Unblock PR #84 (CI test-gate broadening)

**Status**: Review
**Research Doc**: `.docs/research-pr84-workflow-fix.md`
**Author**: Claude Code
**Date**: 2026-08-31
**Estimated Effort**: ~45 min (patch: 5 min; local gate run: ~10 min; PR: 5 min)

## Overview

### Summary

Patch `.gitea/workflows/native-ci.yml` so the CI gate exercises every `#[test]` in the workspace, with `TERRAPHIM_SERVER_BIN` set workspace-wide so the three server-binary-dependent integration test files pass, and using `--tests --bins --examples --lib` (not `--all-targets`) to skip the 5-minute criterion bench.

### Approach

Take PR #84's simplification (replace steps 4-8 with one workspace-wide gate) and add the missing `TERRAPHIM_SERVER_BIN=…` prefix. Keep the install step (it's the precondition for the env var). Drop the three redundant `TERRAPHIM_SERVER_BIN=… cargo test --test {name}` invocations because they are now subsumed by the broadened gate. Keep all focused single-target steps (9-12) for fast failure attribution.

### Scope

**In Scope:**
- `.gitea/workflows/native-ci.yml` (single file, ~6 lines net change)
- A new branch `task/84-fix-workflow` from `gitea/main` @ `75930d8`
- A new PR that supersedes PR #84
- Closing PR #84 with a "superseded by #XXX" comment

**Out of Scope:**
- Source code in any `crates/`
- `.github/workflows/ci.yml` (different runner, different policy)
- `crates/terraphim_grep/Cargo.toml` `[[bench]]` block
- `.terraphim/adf.toml` or any ADF config
- Adding a nightly/scheduled workflow job (deferred — not in scope of audit remediation)

**Avoid At All Cost** (from 5/25 analysis):

| Danger | Why |
|---|---|
| Adding shell wrappers or `&&` chains | Runner allowlist is cargo-only |
| Adding a new file or split into multiple workflows | Audit rule: 1 PR per repo |
| Running `cargo test --all-targets` instead of `--tests --bins --examples --lib` | Triggers 5+ min bench timeouts |
| Touching `terraphim_server` install flags | Inline comment block documents each flag's purpose; any change risks the [patch.crates-io] / registry / locked breakage |
| Adding `--locked` to the workspace-wide gate | `cargo test --workspace` reads the workspace's existing `Cargo.lock`; no `--locked` needed |
| Hard-coding `TERRAPHIM_SERVER_BIN` to anything other than `/tmp/terraphim_server_install/bin/terraphim_server` | That's where step 5 installs the binary; mismatch would silently make step 5 useless |
| Adding a new env var like `TERRAPHIM_AGENT_KG` or similar | Out of scope; we only need `TERRAPHIM_SERVER_BIN` |

## Architecture

### Component Diagram

The CI workflow is a single sequential list of `cargo` invocations. The change is purely at the step level — no new components, no new files.

```
┌─────────────────────────────────────────────────────────────────────┐
│ Gitea Actions runner: terraphim-native                              │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │ Step 4 (PATCHED):                                              │  │
│  │   TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/...    │  │
│  │     cargo test --workspace --tests --bins --examples --lib       │  │
│  │     --no-fail-fast                                             │  │
│  └───────────────────────────────────────────────────────────────┘  │
│           ↑                                                          │
│           │ env var inherited by all cargo test subprocesses         │
│           │                                                          │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │ Step 5 (UNCHANGED): installs terraphim_server from terraphim-ai │  │
│  │   v1.21.3 tag to /tmp/terraphim_server_install/bin/             │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                     │
│  Steps 9-12 (UNCHANGED): focused single-target gates for fast       │
│                          failure attribution                        │
└─────────────────────────────────────────────────────────────────────┘
```

### Data Flow

```
push → terraphim-native runner
     → step 1 (fmt check)
     → step 2 (clippy workspace)
     → step 3 (build workspace)
     → step 5 (install terraphim_server)         [UNCHANGED, runs before new step 4]
     → step 4 (NEW: TERRAPHIM_SERVER_BIN=... cargo test --workspace --tests --bins --examples --lib --no-fail-fast)
     → step 9 (clippy --features enrichment)   [UNCHANGED]
     → step 10 (test --features enrichment)    [UNCHANGED]
     → step 11 (test packaged_install_graph)   [UNCHANGED]
     → step 12 (test ci_guards)                 [UNCHANGED]
```

(Gitea runs steps in declaration order; the new step 4 follows step 3 and precedes step 5 — but the install step must run BEFORE the test step. See "Step ordering" below.)

### Step Ordering

The patch needs the install (step 5) to run BEFORE the test (step 4). Currently the workflow lists them as 4 (lib test), 5 (install), 6-8 (server tests). After the patch:

- Option A: Move the install to step 4; new test becomes step 5. Steps 9-12 shift down.
- Option B: Keep order; explicitly make the new step depend on the install (in Gitea Actions, this is implicit if all steps are in the same job — they run sequentially in declaration order).

Gitea Actions jobs with no `needs:` clause run steps sequentially in declaration order. Option B is simpler (no step renumbering), but means the install runs AFTER the new workspace-wide test. **This breaks the test step.** Must use Option A: move the install to step 4, then the new workspace-wide test as step 5.

Wait — re-reading the current workflow: the lib test is at step 4, install at step 5, server tests at 6-8. After the patch, we want: install first, then the new workspace-wide test. So:

```
Step 4 (PATCHED): cargo install ... terraphim_server (was step 5)
Step 5 (NEW): TERRAPHIM_SERVER_BIN=... cargo test --workspace --tests --bins --examples --lib --no-fail-fast (replaces step 4)
```

Or equivalently: keep the install at step 5, and move the new test to be step 6 (right after install). Then drop the old steps 6-8.

Cleaner approach: rewrite the steps 4-8 block as a contiguous block of:

```yaml
- run: cargo install --locked --git ... terraphim_server
- run: TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test --workspace --tests --bins --examples --lib --no-fail-fast
```

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|----------|-----------|----------------------|
| Use `--tests --bins --examples --lib` instead of `--all-targets` | Skips 5+ min criterion bench; still covers every `#[test]` | `--all-targets` (timeout risk); `--lib` only (current bug) |
| Set `TERRAPHIM_SERVER_BIN` on the workspace-wide line | All 8 server-binary tests pass; preserves #113 invariants | Set env var at job level (Gitea Actions `env:` block at job level — works but verbose; doesn't compose with the existing focused steps 11-12 which don't need it) |
| Drop the three single-target `TERRAPHIM_SERVER_BIN=... cargo test --test {name}` lines | They are now subsumed by the workspace-wide gate; keeping them doubles runtime | Keep them (audit rule violation; doubles ~3 min) |
| Move the install step to run before the new test | The new test depends on `TERRAPHIM_SERVER_BIN` pointing at the installed binary | Use Gitea Actions `needs:` (more complex; not idiomatic for single-job workflows) |
| Keep focused steps 9-12 (`--features enrichment`, `--test packaged_install_graph_regression`, `--test ci_guards`) | Fast failure attribution; these aren't covered by the broadened workspace gate (different features/targets) | Drop them (loses fast attribution; doubles risk of regressions in these specific paths) |
| Update inline comments to reflect the new gate | The current comments document the lib-test + 3 single-target invocations pattern; stale after the patch | Leave stale comments (confusing; future maintainers will not understand the change) |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|-----------------|--------------|-------------------|
| Run benches in CI | Unbounded runtime; belongs in a nightly job, not per-PR gate | CI wall-clock balloons; per-PR feedback loop broken |
| Set `TERRAPHIM_SERVER_BIN` via a job-level `env:` block | Works but requires restructuring the existing step-level env pattern; minimal benefit | More YAML, same outcome |
| Replace `cargo install --git` with a pre-built binary download | Requires new release pipeline integration; out of scope | Scope creep; "1 PR per repo" violation |
| Drop `cargo install` entirely and let tests build `terraphim_server` from source | terraphim_server is a separate repo; not a workspace member; would require adding it to the workspace | Workspace restructure; major scope |
| Add a `cargo test --doc` step | Doc tests already run via `cargo test --lib` | Duplicate work |
| Add `cargo bench` as a separate step | Not in scope; benches belong in a scheduled job | CI runtime increases; per-PR feedback broken |
| Move the workflow file to a different directory | No benefit; convention is `.gitea/workflows/*.yml` | Convention violation |

### Simplicity Check

> "Minimum code that solves the problem. Nothing speculative." -- Karpathy

**What if this could be easy?**

- It IS easy: 4 lines removed (the old `--lib` line + 3 redundant single-target invocations), 1 line modified (the new workspace-wide gate with env-var prefix), 1 line moved (install), comments refreshed. Net: ~6 lines.
- No new abstractions, no new env vars, no new files, no new workflows, no new dependencies.

**Senior Engineer Test**: Would a senior engineer call this overcomplicated? **No.** It's the canonical minimal fix for "broaden the gate without losing server-binary tests, without triggering bench timeouts."

**Nothing Speculative Checklist**:

- [x] No features the user didn't request (just broadening what CI runs)
- [x] No abstractions "in case we need them later" (no helper functions, no shared step definitions)
- [x] No flexibility "just in case" (no env-var overrides, no per-target flags)
- [x] No error handling for scenarios that cannot occur (if `terraphim_server` install fails, the workflow step fails — no try/catch needed)
- [x] No premature optimization (no parallelisation, no incremental test runner)

## File Changes

### New Files

None.

### Modified Files

| File | Changes |
|------|---------|
| `.gitea/workflows/native-ci.yml` | Step 4 (lib test) replaced by a 2-step block: install + workspace-wide test with env-var prefix. Steps 6-8 (3 redundant single-target invocations) deleted. Inline comments refreshed. |

### Deleted Files

None.

## API Design

N/A — workflow YAML, no public API.

## Test Strategy

### Verification (not unit tests — workflow yml is YAML, not Rust)

| Verification | Command | Expected Outcome |
|---|---|---|
| YAML is valid | `python3 -c "import yaml; yaml.safe_load(open('.gitea/workflows/native-ci.yml'))"` | Exit 0, no output |
| New `cargo test` invocation flags are correct | `grep -E 'cargo test --workspace --tests --bins --examples --lib' .gitea/workflows/native-ci.yml` | One match |
| Old `--lib` line is gone | `! grep -E 'cargo test --workspace --lib' .gitea/workflows/native-ci.yml` | Exit 1 (no match) |
| Old single-target server-binary lines are gone | `! grep -E 'TERRAPHIM_SERVER_BIN=.*cargo test -p terraphim_agent --test' .gitea/workflows/native-ci.yml` | Exit 1 (no match) |
| Local workspace gate passes | `TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test --workspace --tests --bins --examples --lib --no-fail-fast` | Exit 0, "test result: ok" for every suite |
| Local gate matches the CI invocation | (same as above) | Same outcome |
| Server-binary tests now pass | (included in the local gate) | cross_mode_consistency, integration_tests server-mode, kg_ranking_integration all green |
| Focused steps 9-12 still work | `cargo clippy -p terraphim_sessions --features enrichment -- -D warnings` etc. | All green (no change to those steps) |

### Local Pre-Merge Verification

Before pushing the branch:

```bash
# 1. YAML valid
python3 -c "import yaml; yaml.safe_load(open('.gitea/workflows/native-ci.yml'))"

# 2. Install terraphim_server (same command as the workflow)
cargo install --locked --git https://git.terraphim.cloud/terraphim/terraphim-ai \
  --tag v1.21.3 \
  --root /tmp/terraphim_server_install \
  --config 'registries.terraphim.index="sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/"' \
  --config 'registry.global-credential-providers=["cargo:token"]' \
  --bin terraphim_server terraphim_server

# 3. Run the broadened workspace gate (this is what CI will run)
TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server \
  cargo test --workspace --tests --bins --examples --lib --no-fail-fast 2>&1 | tee /tmp/gate_pr84.log

# 4. Confirm 0 failures
grep -E "^test result.*[1-9]+ failed" /tmp/gate_pr84.log && echo "FAIL" || echo "PASS"
```

### Rollback Plan

If the workflow breaks CI after merge:

1. Revert the merge commit on `main` (`git revert <merge-sha>`).
2. Or: hot-fix the workflow file with the previous content.

The change is purely additive (one workflow file) and reversible in one command.

## Implementation Steps

### Step 1: Branch and patch

**Files**: `.gitea/workflows/native-ci.yml`
**Description**: Create branch `task/84-fix-workflow` off `gitea/main` @ `75930d8`. Apply the workflow patch.
**Verification**: YAML valid; greps confirm old lines gone, new line present.
**Estimated**: 5 min

The patch (approximate diff shape):

```diff
@@ -7,11 +7,11 @@ jobs:
       - run: cargo fmt --all -- --check
       - run: cargo clippy --workspace --all-targets -- -D warnings
       - run: cargo build --workspace
-      - run: cargo test --workspace --lib --no-fail-fast
-      # #113: build terraphim_server from terraphim-ai so the
+      # #113 + #84: install terraphim_server from terraphim-ai so the
       # server-binary-dependent integration tests have a real binary.
       # ... (existing comment block preserved) ...
       - run: cargo install --locked --git ... --bin terraphim_server terraphim_server
-      # #113: run the integration tests that require a real
-      # terraphim_server binary. ... ensure_server_binary() ...
-      - run: TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test cross_mode_consistency_test -- --nocapture
-      - run: TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test integration_tests -- --nocapture
-      - run: TERRAPHIM_SERVER_BIN=... cargo test -p terraphim_agent --test kg_ranking_integration_test -- --nocapture
+      # #84: workspace-wide test gate. --tests --bins --examples --lib
+      # exercises every #[test] in the workspace without running benches
+      # (bench/hybrid_search.rs runs 5+ min unbounded). The
+      # TERRAPHIM_SERVER_BIN prefix is inherited by every test process,
+      # so the 3 server-binary-dependent test files run green.
+      - run: TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test --workspace --tests --bins --examples --lib --no-fail-fast
```

**Commit**: `ci(native-ci): broaden workspace test gate (Refs #84)`.

### Step 2: Local verification

**Files**: (no file changes)
**Description**: Run the verification commands listed above. Confirm 0 failures.
**Verification**: All 7 verification rows pass.
**Estimated**: 10 min

**No commit** — verification step only.

### Step 3: Push, open PR, close #84

**Files**: (no file changes)
**Description**: Push the branch, open a new PR that supersedes PR #84, close PR #84 with a "superseded by #XXX" comment linking to the new PR.
**Verification**: PR created, comment posted, #84 status = closed.
**Estimated**: 5 min

**No commit** — workflow-only.

## Rollback Plan

If CI is red after merge:

1. `git revert <merge-sha>` on `main`.
2. Push the revert.
3. Investigate offline.

## Migration

None — no schema, no API, no data.

## Dependencies

### New Dependencies

None.

### Dependency Updates

None.

## Software Release Definition (SRD)

Not applicable — no formal SRD requirement for workflow changes.

## Performance Considerations

### Expected Performance

| Metric | Target | Measurement |
|--------|--------|-------------|
| CI wall-clock (post-#84 merge) | ≤ 15 min | Gitea Actions runner timer |
| Step 5 install time | ~5 min (unchanged) | Same as today |
| Step 4 (NEW) workspace-wide test | ~6-8 min (vs current ~5 min for lib + 3 server-binary invocations) | First run will be slower due to compilation of newly-exercised tests; subsequent runs benefit from cargo cache |
| Step 9-12 (focused) | unchanged | unchanged |

### Benchmarks to Add

None. The gate does not run criterion benches.

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Placeholder for any friction point discovered during implementation | Open | Implementer (Claude Code) |
| Decide whether to add a scheduled nightly `cargo bench` workflow | Deferred (out of scope of #84) | Future PR |

## Local Verification Findings (2026-08-31)

Local run of the new gate `TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server cargo test --workspace --tests --bins --examples --lib --no-fail-fast` against `75930d8 + 49fe059` (my patch) completed in 642.58 s with the following results:

- **82 test targets PASSED**
- **9 test targets FAILED** — all pre-existing, unrelated to the workflow change
- Exit code of `cargo test`: non-zero (the trailing `EXIT=0` in my run was from `tee`, not cargo)

### The 9 pre-existing failures

| # | Target | Root cause | Notes |
|---|--------|-----------|-------|
| 1 | `terraphim_agent::replace_feature_tests` (5 of 14) | `docs/src/kg/` missing | Has `is_ci_environment()` skip → will pass in CI |
| 2 | `terraphim_agent::user_prompt_submit_tests` (3 of 4) | KG path missing | No CI skip; will fail in CI |
| 3 | `terraphim_mcp_server::integration_test` (2 of 7) | "Default role should return at least one document" | No CI skip |
| 4 | `terraphim_mcp_server::mcp_autocomplete_e2e_test` (5 of 6) | `docs/src/kg/` missing | No CI skip |
| 5 | `terraphim_mcp_server::mcp_rolegraph_validation_test` (4 of 4) | KG path missing (line 36) | No CI skip |
| 6 | `terraphim_mcp_server::test_all_mcp_tools` (1 of 1) | KG path missing (line 53) | No CI skip |
| 7 | `terraphim_mcp_server::test_find_files` (1 of 5) | KG path missing (line 111) | No CI skip |
| 8 | `terraphim_mcp_server::test_tools_list` (1 of 1) | KG path missing (line 53) | No CI skip |
| 9 | `terraphim_update::manifest` (1 of 4) | `NoAssetForTarget { target: "aarch64-macos" }` — test fixture manifest doesn't list a current-platform asset | Not KG-related; fixture gap |

**Single root cause for 8 of 9**: the workspace never provisions `docs/src/kg/`. The path was never tracked in git (`git log -- "docs/src/kg"` returns empty), and the original `native-ci.yml` had no provisioning step. These tests have likely been broken since they were added, hidden by the lib-only gate which never executed them.

### Why the original research doc missed this

The research doc explicitly enumerated "8 confirmed failures (3 in cross_mode_consistency_test, 2 in integration_tests server-mode, 3 in kg_ranking_integration_test) when running `--all-targets` without `TERRAPHIM_SERVER_BIN`". All 8 are server-binary-dependent and are fixed by setting `TERRAPHIM_SERVER_BIN`. The research did NOT enumerate the additional failures that surface when the gate is broadened to `--tests --bins --examples --lib` (the new behaviour). This is a research-doc gap, surfaced by disciplined implementation.

### Decision required (cannot auto-proceed)

Per the project policy ("Don't commit - you haven't made any progress - only commit on success, which means all tests are fully functional"), a PR that makes the gate red is not committable. Three viable paths:

| Path | Effort | Trade-off |
|------|--------|-----------|
| **A. Provision `docs/src/kg/` in CI** — add a step that generates / checks in minimal KG fixtures | ~1-2 h | Cleanest. Requires creating fixture files + new workflow step. Scope-creep vs #84 but eliminates 8 of 9 failures. |
| **B. Fix the 9 failing tests to skip when KG path missing** (add `is_ci_environment()` style guards) | ~1 h | Smaller diff but changes test semantics. All 8 KG failures get the same fix. |
| **C. Push the PR as-is and file follow-up issues for each failure** | ~15 min | Honest but leaves main with a red gate after merge. Violates the commit-on-success rule. |

**Recommendation**: Path A (provision KG fixture) because (a) it fixes the root cause, (b) the existing `replace_feature_tests.rs` already shows the design pattern (`is_ci_environment()` for KG-skipped branches), and (c) the `terraphim_update::manifest` failure is unrelated and needs its own fix (one-line manifest edit).

Pending human decision before proceeding to Step 3.

## Outcome (2026-08-31)

After Path A execution and re-verification, **5 of 9 failures were fixed** by adding the docs/src/kg fixture (covers 7 failing tests across 4 targets: replace_feature_tests, mcp_autocomplete_e2e_test, mcp_rolegraph_validation_test, integration_test) and extending the terraphim_update manifest fixture (1 test).

**4 of 9 failures remain**, all environment-specific and out of scope for #84:

| # | Target | Root cause | Status |
|---|--------|-----------|--------|
| 1 | `terraphim_agent::user_prompt_submit_tests` (3 of 4) | `learn hook` doesn't write correction files; pre-existing, unrelated to KG | needs separate issue |
| 2 | `terraphim_mcp_server::test_find_files::find_files_with_kg_scorer_boosts_matching_paths` | `find_files` doesn't return `automata`-pathed results; pre-existing logic issue | needs separate issue |
| 3 | `terraphim_mcp_server::test_tools_list` | Test expects `../terraphim_settings/default/settings_local_dev.toml` (a directory in parent workspace, not in this repo) | needs separate issue |
| 4 | `terraphim_mcp_server::test_all_mcp_tools` | Same settings path issue | needs separate issue |

Because shipping the broadened gate (`49fe059`) would make CI red on main until all 4 issues close, that commit was reverted in favour of:

- **Commit A** (`docs/src/kg/{terraphim-graph,bun}.md`): minimal KG fixtures. Pure test infra, no risk.
- **Commit B** (`crates/terraphim_update/tests/manifest.rs`): extend manifest fixture to cover macOS+Windows targets. Pure test infra, no risk.

PR #84 remains open with a comment explaining that the broadened gate is blocked on the 4 follow-up issues. The workflow change itself (`49fe059`) is preserved as a draft commit on the branch in case a future session wants to re-base once follow-ups land.

Follow-up issues to be filed (next step): 4 issues, one per remaining failure.

## Approval

- [x] Research approved (signed off 2026-08-31)
- [ ] Design approved (pending — see Refresh below)
- [ ] Verification plan approved (pending)
- [ ] Performance targets agreed: ≤ 15 min CI wall-clock (pending)
- [ ] Human approval received (pending)

## Refresh — 2026-08-31 (Session Restart after PR #140 Merge)

### What changed since the original design

1. **PR #140 merged into main** at `75930d8`. `main` HEAD is now `75930d8`. Local branch `task/84-fix-workflow` was reset back to `75930d8` (the previous working-tree commit `49fe059` was soft-reset out).
2. **PR #84 was substantially rewritten** by its owner between the prior session and this one. The PR body still describes a single-line change, but the actual diff is **46 files changed, 74 insertions, 18,098 deletions** — including:
   - Deletion of `terraphim_server/default/terraphim_engineer_config.json`
   - Deletion of all 41 files under `terraphim_server/fixtures/` (haystack + thesauri)
   - Deletion of `crates/terraphim_agent/tests/{guard_priority,hook_safety,robot_schemas}.rs`
   - Deletion of `crates/terraphim_grep/tests/search_only_flag.rs`
   - Deletion of `adr/ADR-002-guard-priority-order.md` and `adr/ADR-003-pretool-hook-rewrite.md`
   - Deletion of 5 blog posts, 2 plan docs, 1 agent-reference doc
   - Substantial deletions in `crates/terraphim_agent/src/{main,service,guard_patterns,robot/docs}.rs`
3. **Root comment on PR #84 (2026-08-31 18:11)** by `root` identified exactly two minimal fixes that would make PR #84 mergeable:
   - **(a)** Restore the `terraphim_server` install step inside the workflow and prefix the new `--all-targets` line with `TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server`.
   - **(b)** Replace `--all-targets` with `--tests --bins --examples --lib` to skip the 5+ minute criterion bench.
4. **My local branch (`task/84-fix-workflow`) is not PR #84's branch.** PR #84's head ref is `fix/91-all-targets-test-gate` (head SHA `e049ac46aa`). My local branch was created separately and has been working toward a parallel-but-different plan: ship the test-infra prep that PR #84 (or any successor gate-broadening PR) will need.

### Refreshed local verification (2026-08-31, post-#140)

All five test-infra prep targets now pass locally against `75930d8` (main) without modifying the workflow file:

| Target | Result | Driver |
|---|---|---|
| `terraphim_update::manifest` | 4/4 pass | extended `sample_manifest_json()` covers 8 targets |
| `terraphim_agent::replace_feature_tests` | 14/14 pass | `docs/src/kg/bun.md` (yarn→bun, pnpm→bun, npm→bun) |
| `terraphim_mcp_server::mcp_autocomplete_e2e_test` | 6/6 pass + 2 ignored | `docs/src/kg/terraphim-graph.md` |
| `terraphim_mcp_server::mcp_rolegraph_validation_test` | 4/4 pass | `docs/src/kg/terraphim-graph.md` |
| `terraphim_agent::integration_tests` (server-mode subset) | 5/5 pass when `TERRAPHIM_SERVER_BIN` is set | environment variable, not fixture |

Workspace `--lib` baseline: **937 passed / 0 failed / 1 ignored** across 10 crates.

### Refreshed plan (3 options, decision required)

| Option | What ships | Trade-off |
|---|---|---|
| **A. Test-infra prep only** (prior plan, still valid) | Commits A + B (KG fixtures + manifest fix) as a new PR from `task/84-fix-workflow`. Comment on PR #84 explaining the broadened gate is blocked on the 4 remaining environment-specific failures + root's recommended fix (a)+(b). **No force-push to PR #84.** | Safe, low-risk. PR #84 stays where its owner left it. The 4 follow-up issues still need to be filed. |
| **B. Test-infra prep + force-push PR #84** with root's minimal fix | Same as A, plus a force-push onto `gitea/fix/91-all-targets-test-gate` that strips the 18k lines of unrelated deletions and applies only root's recommended (a)+(b) gate change. | Higher scope; assumes PR #84 is mine to rewrite. May conflict with the owner's intent for the deletions. |
| **C. Close PR #84, open minimal successor** | Same as A, plus a `gtr comment --close` on PR #84 ("superseded by #XXX — see minimal gate-only PR") and a new PR with only root's (a)+(b) change. | Cleanest end-state. Forces decision about whether the 18k deletions should be preserved. |

### Recommendation

**Option A.** Rationale:

1. The user's prior instruction (Pick Path A in the prior session) was exactly this: provision the test-infra fixture without expanding scope.
2. The 4 follow-up issues are out-of-scope for any gate-broadening PR and should be filed as separate issues regardless of which option is chosen.
3. The 18k deletions in PR #84 are a substantial change that I do not have authority to rewrite unilaterally; the PR's owner (or a follow-up reviewer) should make that decision.
4. PR #84 already has a root comment (2026-08-31 18:11) explaining the merge-block; my follow-up comment should be additive, not duplicative.

### File paths in the refreshed plan

| Path | Change | Commit |
|---|---|---|
| `docs/src/kg/terraphim-graph.md` | NEW (Logseq KG concept) | Commit A |
| `docs/src/kg/bun.md` | NEW (Logseq KG concept) | Commit A |
| `crates/terraphim_update/tests/manifest.rs` | MODIFY (extend `sample_manifest_json()` to 8 targets; update `assets.len()` assertion) | Commit B |
| `.docs/design-pr84-workflow-fix.md` (this file) | UNTRACKED session artefact, do NOT commit | — |
| `.docs/research-pr84-workflow-fix.md` | UNTRACKED session artefact, do NOT commit | — |

### Open follow-up issues (to file via `gtr create-issue` after merge)

1. **`terraphim_agent::user_prompt_submit_tests`** — 3 of 4 fail because the learn hook does not write correction files at the expected path. Pre-existing logic issue, not KG-related.
2. **`terraphim_mcp_server::test_find_files::find_files_with_kg_scorer_boosts_matching_paths`** — `find_files` does not return `automata`-pathed results. Pre-existing logic issue.
3. **`terraphim_mcp_server::test_tools_list`** — expects `../terraphim_settings/default/settings_local_dev.toml` (a directory in the parent workspace, not in this repo).
4. **`terraphim_mcp_server::test_all_mcp_tools`** — same parent-workspace settings path issue.

### Pending human decision (2026-08-31)

- [ ] **Approve Option A** (test-infra prep only — recommended).
- [ ] OR Approve Option B (force-push PR #84 with minimal fix).
- [ ] OR Approve Option C (close PR #84, open minimal successor).
- [ ] OR defer / reject with new direction.
