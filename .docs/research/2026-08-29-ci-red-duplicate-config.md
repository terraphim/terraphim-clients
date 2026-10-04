# Research: #118 — CI red since #112, duplicate `terraphim_config`

**Status**: Draft
**Date**: 2026-08-29
**Issue**: #118 (caused by #112)

## Executive Summary

CI resolves a `terraphim_config` 1.20.4 (crates.io) copy alongside 1.20.2 (Gitea), producing two
`ConfigState` types and a clippy failure. A **cold resolve reproduces neither** — locally, with no
`Cargo.lock`, cargo picks Gitea 1.20.2 and yields zero duplicates. The resolution logic is therefore
sound; the divergence is environmental, and `Cargo.lock` being gitignored is what allows CI's
environment to differ from a verified one at all.

## Essential Questions Check

| Question | Answer | Evidence |
|---|---|---|
| Energizing? | Yes | It is my regression; `main` is red |
| Leverages strengths? | Yes | Already have the full dependency-graph picture from #112 |
| Meets real need? | Yes | Red `main` blocks every merge, and PR #84 is queued behind it |

**Proceed**: Yes (3/3).

## Problem Statement

`native-ci` passed on `58810594` and has failed on every commit since the #112 merge. Failing step:
`cargo clippy --workspace --all-targets -- -D warnings`:

```
error[E0308]: mismatched types
  --> crates/terraphim_cli/src/service.rs:230:45
  --> .../terraphim_config-1.20.2/src/lib.rs:1109:1      (Gitea)
  --> .../terraphim_service-1.21.1/src/lib.rs:131:12
```

Both `terraphim_config` 1.20.2 and 1.20.4 are in the CI graph. `terraphim_service 1.21.1` will not
accept a `ConfigState` from the other copy.

**Success criteria**: `native-ci` green on `main`; a recurrence produces a named error rather than a
type mismatch.

## Current State — verified this session

| Check | Result |
|---|---|
| `cargo check/clippy --workspace --all-targets --all-features`, locally | green |
| `cargo clippy --workspace --all-targets -- -D warnings` (CI's exact flags), locally | green |
| Fresh clone, **no `Cargo.lock`**, cold resolve | **green, 0 duplicate terraphim crates** |
| Cold lock's `terraphim_config` | `1.20.2`, `sparse+https://git.terraphim.cloud/...` |
| Local lock vs cold lock, terraphim crates | **identical, 0 differences, 0 duplicates** |
| `cargo test -p terraphim_agent --test packaged_install_graph_regression` | passes (67s) |

So the manifests resolve correctly from scratch. The committed source is not the problem.

### Why `Cargo.lock` is ignored

`.gitignore:2` has carried `Cargo.lock` since `81ec742` ("chore: scaffold terraphim-clients
workspace (#1910 E5)") — a scaffolding default, not a considered decision. This workspace ships
binaries (`crates/terraphim_agent/Cargo.toml:123 [[bin]] terraphim-agent`), the case where Cargo's
own guidance is to commit the lock. **`terraphim-ai` commits its lock**; `terraphim-core` and
`terraphim-agents` do not.

### Runner facts

- Runs 205-209: 80-93s, success. Runs 210-218: **0-1s, failure, no logs** — those jobs never started
  (`started_at == completed_at`), a scheduling problem, not a build failure.
- Run 219 executed for 15s and produced the errors above. Only this run is evidence about the build.
- 3 of 6 `terraphim-native` runners online; `bigbox-runner` offline.

## Vital Few

| Constraint | Why vital | Evidence |
|---|---|---|
| CI must build the same graph a human verified | The whole failure is CI resolving differently from every local check | Cold lock == local lock, yet CI differs |
| Duplicate terraphim crates must fail loudly | Symptom was `expected ConfigState, found ConfigState`, which reads as a code bug and cost hours | #112 hit the identical confusion |
| Fix must not re-open the crates.io 1.20.4 path | Gitea's 1.20.4 is **yanked**, so widening the pin to `^1.20.4` would resolve to crates.io | Registry index: `1.20.2 ok, 1.20.4 YANKED` |

## Eliminated from scope

| Eliminated | Why |
|---|---|
| Unyanking 1.20.4 on Gitea | Cold resolve shows nothing needs it; unyanking without knowing why it was yanked is a worse risk |
| Chasing the 0s runner-scheduling failures | Separate infrastructure fault; run 219 shows the build fault independently |
| Changing the `=1.20.2` exact pins | They produce a correct cold resolve; changing them is speculative |
| `--all-features` in CI | Would surface #113's deadlock; out of scope here |

## Risks and unknowns

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Root cause is Linux-specific resolution, not a stale runner lock | Medium | Medium | A committed lock pins both cases identically, so the fix covers either |
| Committed lock drifts and becomes noise in diffs | Medium | Low | Normal for binary-shipping repos; `terraphim-ai` already lives with it |
| Lock hides a genuine future resolution conflict | Low | Medium | The duplicate guard fails loudly when the graph regresses |

### Assumptions

| Assumption | Basis | Risk if wrong | Verified |
|---|---|---|---|
| The local lock is a correct resolution worth committing | Byte-identical to a cold resolve; full gate green against it | Would commit a bad graph | **Yes** |
| Committing the lock overrides whatever the runner has | Once tracked, checkout writes the file | Fix does not take | No — CI will confirm |
| Nothing requires `terraphim_config ^1.20.4` | Cold resolve picks 1.20.2 and is duplicate-free | Patch silently skipped again | Partly — macOS only |

### Open question

**Is CI's 1.20.4 a stale `Cargo.lock` in a persistent runner workspace, or Linux-specific
resolution?** I cannot see the runner filesystem. A gitignored lock surviving between runs on a
reused workspace fits the evidence exactly: the failure mixes a *new* manifest (`terraphim_service
1.21.1`) with an *old* resolution (`terraphim_config 1.20.4`), which is a stale-lock signature. The
proposed fix addresses both, so answering it is not a prerequisite — but if CI stays red after the
lock lands, the answer is "Linux-specific" and the next step is a Linux cold resolve.

## Recommendation

Proceed. Two changes: commit a verified `Cargo.lock`, and add a duplicate-crate guard to CI so this
class fails with a clear message. Do not touch the patch pins.
