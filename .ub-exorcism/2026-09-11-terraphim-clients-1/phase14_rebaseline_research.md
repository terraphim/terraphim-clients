# Research Document: Re-baseline the Phase 8 UB remediation plan against main@c7ae820

**Status**: Draft
**Author**: Claude Opus 5 (session 7432e3b0)
**Date**: 2026-10-07 (verified by `date '+%Y-%m-%d %H:%M %Z'` -> 2026-10-07 10:03 BST)
**Scope**: Phase 1 (Research) of a disciplined re-baseline. No design, no source edits.
**Tracking**: terraphim/terraphim-clients#252 (audit), #293, #296, #280, #290, #299, #302, #310

## Executive Summary

The UB audit (run `2026-09-11-terraphim-clients-1`) converged on 2026-09-13 and its
Phase 8 remediation plan remains the design of record. This document re-baselines that
plan's premises against `main` at `c7ae820`, four weeks later. Three premises have
changed, and all three changes make the situation worse rather than better: the plan's
CI-exposure table described an unmerged branch and never applied to `main`; two new
instances of the exact confirmed UB shape were committed in the window, one of them
three days ago; and four files the plan depends on (`deny.toml`, `rust-toolchain.toml`,
`.config/nextest.toml`, and the audit workspace itself) exist only on the unmergeable
branch. The plan's chosen remedies and their rubric winners are unaffected.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | Two CONFIRMED_UB findings on a production path (#293, #296) have been live in `main` for four weeks; one reproducibly SIGBUSes the shipped MCP server (EXP-302, exit 135). |
| Leverages strengths? | Yes | The audit already produced proofs, candidates, rubrics and a commit sequence. This is re-baselining, not re-deriving. |
| Meets real need? | Yes | `terraphim-grep` and the MCP `grep_files`/`multi_grep_files` tools are shipped and user-reachable; the UF-040 base path is attacker-chosen. |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

None of the audit's remediation has landed. The plan was written against tree `ae06cd2`
on branch `task/254-ci-pipeline`, which has since become unmergeable against `main`
(50 commits ahead on `main`, 264 on the branch, 19 files conflicting). Before any fix is
written, the plan's file:line citations and its stated preconditions must be re-verified
against `main`, because a plan whose premises have drifted will produce fixes that miss
sites or fail to compile.

### Impact

- `#293` (UF-039) and `#296` (UF-040), both `priority/urgent` and both CONFIRMED_UB on
  the production grep path, are reachable in `main` today.
- `#280`/`#290`/`#299` (env races in test binaries) are **more** exposed than the plan
  recorded, not less (see Finding 1).
- The shape is actively spreading: two new writers since the audit baseline (Finding 2).

### Success Criteria

A corrected plan (Delta 6) under which Phase 13 can be executed against `main` with
every file:line valid, every precondition either satisfied or explicitly listed as a
prerequisite commit, and no finding silently out of scope.

## Current State Analysis

### Verification commands run (all on `main` / `c7ae820`, 2026-10-07)

| Check | Command | Result |
|---|---|---|
| Branch/commit | `git rev-parse --abbrev-ref HEAD`; `git log -1` | detached `HEAD` at `c7ae820` |
| Audit workspace in main | `git ls-tree HEAD --name-only` | absent; only on `task/254-ci-pipeline` at `9b455e9` |
| UF-039 sites | `rg -n 'cache_budget' …` | 4 x `cache_budget: None` |
| UF-040 guard | `rg -n 'reject_hostile_tree' -g '*.rs'` | absent |
| fff-search pin | `rg -n 'fff-search' crates/*/Cargo.toml` | `version = "0.8.4"` (caret), unpinned, in 2 crates |
| Lockfile | `rg -A1 '^name = "(fff-search\|git2\|libgit2-sys)"' Cargo.lock` | fff-search 0.8.4, git2 0.20.4, libgit2-sys 0.18.5+1.9.4 |
| `unsafe` inventory | `rg -n 'unsafe \{\|unsafe impl\|unsafe fn' crates/` | **14 blocks** (audit recorded 6) |
| FilePicker sites | `rg -n 'FilePicker::new' crates/` | 4, matching the `cache_budget` count |
| Workspace lint lever | `rg -n 'lints\|unsafe_code' Cargo.toml` | absent |
| Edition | `rg -n '^edition' Cargo.toml` | `edition = "2024"` (root, line 19) |

### Code location map

| Component | Location (main@c7ae820) | Purpose / audit finding |
|---|---|---|
| fff-search mapping sites | `crates/terraphim_grep/src/hybrid_searcher.rs:358`; `crates/terraphim_mcp_server/src/lib.rs:1278`, `:1376`, `:1489` | UF-039 / #293 (`cache_budget: None`) |
| FilePicker constructors | same files, `:354`, `:1274`, `:1372`, `:1485` | UF-039 + UF-040 guard insertion points |
| UF-001 writer (original) | `crates/terraphim_grep/src/openrouter_client.rs:195,202,210,214` | moved from `:170,177,185,189` (file grew 87 lines) |
| **UF-001 writer (new)** | `crates/terraphim_grep/src/lib.rs:911,920,925,934,945,949` | new instance, same shape, same binary |
| UF-004 writer | `crates/terraphim_update/src/state.rs:131` | unchanged line |
| **New writer** | `crates/terraphim_update/tests/managed_mode.rs:326,343` | new instance, `#[serial(update_home_env)]` |
| UF-013 writer | `crates/terraphim_mcp_server/tests/test_mcp_stdio.rs:15` | unchanged line |
| `EnvVarGuard` / `set_env_var` users | `crates/terraphim_agent/tests/tui_service_tests.rs`; `crates/terraphim_mcp_server/tests/mcp_autocomplete_e2e_test.rs`, `mcp_rolegraph_validation_test.rs` | UF-002 / #290 |
| `terraphim_test_utils` dev-dep | `crates/terraphim_mcp_server/Cargo.toml:66`; `crates/terraphim_agent/Cargo.toml:114` | UF-003 upstream, plan commit 5 |
| Guard-test host | `crates/terraphim_agent/tests/ci_guards.rs` (656 lines, 5 `#[test]`) | exists on main; plan appends to it |

## Findings

### Finding 1 (correction to the plan AND to my previous report): the CI-exposure table described a branch that never merged; `main`'s exposure is wider

The plan's section 1 states that both CI test lanes run `cargo nextest`, giving per-test
process isolation, and that only the coverage lanes were exposed. That was true of tree
`ae06cd2` on `task/254-ci-pipeline`. It was never true of `main`.

Verified at the audit's own baseline commit `1120180` (**not** an ancestor of `main`,
confirmed by `git merge-base --is-ancestor 1120180 main` -> false):

| Lane | Command at `1120180` | Command at `main@c7ae820` | Process model | Exposed? |
|---|---|---|---|---|
| GitHub `build` job | `ci.yml:26` `cargo test --workspace --lib --no-fail-fast` | `ci.yml:45`, identical | plain libtest, in-process, ubuntu-latest (glibc) | **Yes**: terraphim_grep lib (UF-001 x2), terraphim_update lib (UF-004, UF-036) |
| GitHub updater lane | `ci.yml` (absent) | `ci.yml:50` `cargo test -p terraphim_update --test manifest --test r2_update --test managed_mode --test policy` | plain libtest | **Yes**: `managed_mode` (new writer, Finding 2) |
| native-ci `test` | `native-ci.yml:54` `cargo test --workspace --all-targets --no-fail-fast` | `native-ci.yml:130`, identical | plain libtest, bigbox glibc 2.39 | **Yes**: all of the above plus UF-002 (`--all-targets` reaches `--test` binaries) |
| native-ci `coverage` | absent | `native-ci.yml:199` `cargo llvm-cov nextest --workspace --all-targets` | **nextest** (#313 landed) | No |
| GitHub coverage | absent | absent | n/a | n/a |
| Developer default | `BUILD.md` `cargo test --workspace --no-fail-fast` | unchanged | plain libtest | Yes |

The exposure picture is therefore the **inverse** of the plan's table: the test lanes are
exposed and the coverage lane is isolated. Both exposed lanes run glibc, where the
`setenv`/`getenv` pair is a C11 data race rather than only a contract violation.

This also corrects my own previous statement in this session that "zero remediation
landed": **#313 landed** (closed; `native-ci.yml:169-199` is the nextest coverage lane,
with `BUILD.md` updated to match). One of 35 audit issues is resolved. It does not touch
the test lanes, which is why the exposure above stands.

### Finding 2 (new, not in the audit): the confirmed shape spread twice in the four-week window

The `unsafe` inventory grew from 6 blocks to 14. Both new clusters postdate the audit
baseline (`git merge-base --is-ancestor <sha> 1120180` -> false for both):

1. **`crates/terraphim_grep/src/lib.rs:905-953`**, test `rlm_max_tokens_default_and_override`,
   added by `e672f13` on **2026-10-04** ("fix(grep): raise RLM max_tokens to 8000", #349).
   Six `unsafe` blocks mutating `TERRAPHIM_GREP_MAX_TOKENS`, under the SAFETY comment
   *"test code, single-threaded, no concurrent env access"* -- the same sentence EXP-001b
   empirically disproved for this crate. It sits in the `terraphim_grep` **lib** binary
   alongside roughly eleven sibling tests that call `.search(...)` ->
   `HybridSearcher::search_code` -> fff-search -> `git2::init` -> libgit2 `getenv`
   (`lib.rs:651,678,711,744,788,819,868,1013,1059,1111`). This is UF-001's mechanism
   exactly, in the same binary, on the lane verified exposed in Finding 1.
2. **`crates/terraphim_update/tests/managed_mode.rs:320-347`**, writes `HOME` under
   `#[serial(update_home_env)]`. The file holds 9 tests and exactly one carries that
   attribute, which is EXP-014's proven defeat condition: `serial_test` excludes only
   other `#[serial]` tests with the same key, so the eight unannotated siblings run
   concurrently with the writer. This binary is run explicitly by `ci.yml:50` and by
   `native-ci.yml:130`.

Neither has an issue. Neither is in the plan's touch list. The second also makes the
SAFETY comment's claim ("serialized via `#[serial]` ... no other thread in this process
mutates HOME concurrently") true of writers but silent about readers, which is the error
EXP-014 was written to expose.

### Finding 3: four files the plan depends on do not exist on `main`

| File | On `main`? | Plan dependency | Consequence |
|---|---|---|---|
| `deny.toml` | **No** | Delta 5 item 4 (reachability note), commit 7 (ban `terraphim_test_utils`) | No `cargo deny` gate exists on `main`, and no `cargo deny` step in either workflow. The advisory-reachability correction I wrote in `e90b193` is branch-only. |
| `rust-toolchain.toml` | **No** | Delta 4 precondition (pins 1.97.1) | Not a blocker: `set_var` is `unsafe fn` because the root manifest sets `edition = "2024"` (`Cargo.toml:19`), which is an edition property, not a toolchain pin. The missing pin is a separate CI-reproducibility gap. |
| `.config/nextest.toml` | **No** | plan section 1, 4.2 candidate C | Consistent with Finding 1: `main` never used nextest for tests. Candidate C's co-location note is void on `main`. |
| `.ub-exorcism/` workspace | **No** | all phase artefacts, runbook, probe sources | The audit record, including the LD_PRELOAD instrument the regression tests need, is reachable only from `task/254-ci-pipeline`. |

### Finding 4: unchanged premises (re-verified, no action)

- All four `cache_budget: None` sites present; exactly four `FilePicker::new` sites; no
  `new_with_shared_state`. UF-039's touch list is still complete.
- No `reject_hostile_tree`; UF-040 wholly unaddressed.
- `fff-search = { version = "0.8.4" }` (caret) in both dependent crates; lockfile still
  0.8.4. Delta 5's `=0.8.4` pin requirement is unmet.
- git2 still 0.20.4 (RUSTSEC-2026-0183/0184 unpatched upstream, vulnerable APIs
  unreachable per Delta 5); `libgit2-sys` is **0.18.5+1.9.4** on `main`, i.e. the
  `46bbc43` bump to 0.18.8 was branch-only and is absent here.
- `terraphim_test_utils` dev-dep present in both manifests; three files use
  `set_env_var`/`EnvVarGuard`.
- No `[workspace.lints]`; the headline lever has not landed.
- `crates/terraphim_agent/tests/ci_guards.rs` exists with five tests, so the plan's
  "append a guard arm" instruction is directly executable.

## Constraints

### Essential constraints (the vital few)

| Constraint | Why it is vital | Evidence |
|---|---|---|
| The two production-path fixes must not be sequenced behind the test-only train | #293/#296 are urgent and user-reachable; the test train ends in the lint lever, which cannot land until six prior commits do | plan section 10 note; Delta 2 "Commit slot" |
| The headline lint (`unsafe_code = "forbid"`) must land last | It will not compile while any `unsafe` block remains, and there are now 14, not 6 | Finding 2; plan section 7 |
| The LD_PRELOAD regression instrument cannot be a CI gate | native-ci's runner allowlist rejects any program that is not `cargo` or a repo script | `native-ci.yml:164-167` comment |

### Eliminated from scope (5/25 rule)

| Eliminated | Why |
|---|---|
| Rescuing `task/254-ci-pipeline` / PR #266 as a whole | 19 conflicting files of CI work unrelated to the audit; a separate task (#254) |
| Re-running the audit or any experiment | Converged at round 11; #320 tracks residual instrument work |
| Upstream filings (#319) | Needs the user's GitHub account |
| fff-search 0.10.6 upgrade (#257) | Delta 5 decided: stay on 0.8.4; revisit only when upstream gates the fresh-mapping path |
| Operator items (#281 orphan processes, rogue runner, bigbox co-tenancy) | Not code changes |

## Risks and unknowns

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| A fix is written against the plan's stale line numbers and misses a site | Was high | High | This document's code-location map supersedes the plan's; Delta 6 carries it |
| The two new writers are left out and the lint lever fails to compile at commit 7 | High if unrecorded | Medium | Finding 2 adds them as new commits ahead of the lever; new Gitea issues needed |
| The shape spreads again during the remediation | Medium (it spread twice in four weeks) | Medium | The lever is the only compile-time stop; until it lands, a `ci_guards` arm scanning for `env::set_var` under `crates/` is the cheap interim |
| `cargo deny` work is assumed present | Medium | Low | Finding 3: `deny.toml` is branch-only; treat as a separate prerequisite, not part of the UB train |
| Cherry-picking the audit workspace conflicts | Low | Low | The three commits touch only `.ub-exorcism/`, which does not exist on `main` |

### Assumptions explicitly stated

| Assumption | Basis | Risk if wrong | Verified? |
|---|---|---|---|
| The audit's verdicts and rubric winners still hold | No `crates/` change touched the mechanisms; `memmap2`/fff-search versions identical | Would require re-deriving Delta 2/3/5 | Yes, by diff |
| `edition = "2024"` alone makes `set_var` `unsafe fn` | Rust 2024 edition change; the root manifest sets it and all members inherit | The lint lever would not be a compile-time gate | Partially: manifest read; not re-compiled |
| `lib.rs:905` races the same way UF-001 does | Same binary, same libgit2 reader chain, same lane | Would be a lower-severity logical race only | Mechanism inferred, not re-probed (EXP-801 would settle it) |
| `cargo update` cannot reach fff-search 0.10.6 | `^0.8.4` confines to 0.8.x; crates.io has no stable 0.8.x above 0.8.4 | The pin becomes urgent rather than prudent | Yes, stated last turn and unchanged |

### Open questions (for the user)

1. Should the audit workspace be cherry-picked onto a fresh branch off `main` so the
   plan, runbook and probe sources are reachable from the default branch? (Recommended;
   it is the prerequisite for the regression instruments.)
2. Should `deny.toml` and `rust-toolchain.toml` be ported to `main` as part of this work,
   or left to #254?
3. Two new issues are needed for Finding 2. File them?

## Recommendations

**Proceed** to Phase 2 (Design) with a *Delta 6* amendment to the existing plan rather
than a new plan: the winners are unchanged and re-deriving them would discard the audit's
evidence. Delta 6 must carry Findings 1-3, the corrected code-location map, the two new
writers, and an unchanged execution order.

## Next steps

1. Load `disciplined-design`; write Delta 6 (design amendment only).
2. Record Delta 6 on #252 and in the workspace file, pending the cherry-pick decision.
3. File the two Finding 2 issues.
