# Implementation Plan: #113 — stop tests spawning `cargo`

**Status**: Draft
**Research**: `.docs/research/2026-08-29-nested-cargo-in-tests.md`
**Date**: 2026-08-29
**Estimated effort**: 2-3 hours for the mechanical pass; triage of resulting failures is separate

## Summary

Replace every nested `cargo` invocation in tests with `env!("CARGO_BIN_EXE_<name>")`, stop two tests
writing into the source tree, and mark the two tests targeting the absent `terraphim_server` as
ignored with a reason.

## Approach

Strictly mechanical, in one commit per concern, because the tests have not run in months and will
almost certainly fail once they do. Keeping the swap free of behaviour changes means a failure
afterwards is unambiguously pre-existing rather than something this change introduced.

### Scope

**In:** the `CARGO_BIN_EXE_*` swap across 22 files; tempdir for the two source-tree writers;
`#[ignore]` on the two `terraphim_server` tests.

**Out:** switching CI to `--all-targets` (PR #84's job); fixing failures the swap reveals; bringing
`terraphim_server` into the workspace.

**Avoid at all cost:**
- Mixing behaviour fixes into the mechanical swap — it makes the diff unreviewable and blurs blame
  for any new failure
- Deleting the `terraphim_server` tests — irreversible, and the decision is not mine
- "While I'm here" edits to test assertions
- Enabling these tests in CI in the same change

## Key decisions

| Decision | Rationale | Rejected |
|---|---|---|
| `env!("CARGO_BIN_EXE_<name>")` | Cargo builds the binary before the test and hands over the path; no lock, no subprocess build | A shared helper crate — more churn than the problem warrants |
| `#[ignore]` the `terraphim_server` tests | Reversible; preserves intent; the binary genuinely is not in this workspace | Deleting (destructive, not my call); making them pass (needs `terraphim_server` here) |
| `#[cfg(feature = "server")]` for the `--features server` tests | `CARGO_BIN_EXE_*` yields the binary as built for the test target; gating is honest about what is exercised | Blind swap — would silently assert against a binary lacking the feature |
| Separate commits per concern | The tests are unrun; isolate mechanical from semantic | One big commit |

### Simplicity check

The core is a textual substitution: `Command::new("cargo").args(["run", "-p", X, "--"])` becomes
`Command::new(env!("CARGO_BIN_EXE_x"))`. No new crates, helpers, or abstractions. The only judgement
is the three feature-gated cases and the two orphans.

## File changes

| Group | Files | Change |
|---|---|---|
| Agent CLI spawns | 14 in `crates/terraphim_agent/tests/` | `CARGO_BIN_EXE_terraphim-agent` |
| MCP server spawns | 6 in `crates/terraphim_mcp_server/tests/` | `CARGO_BIN_EXE_terraphim_mcp_server` |
| Session-analyzer spawns | 2 in `crates/terraphim-session-analyzer/tests/` | `CARGO_BIN_EXE_tsa` |
| Source-tree writers | `cross_mode_consistency_test.rs:411`, `kg_ranking_integration_test.rs:278` | write under `tempfile::tempdir()` |
| Orphans | the two `cargo build -p terraphim_server` tests | `#[ignore = "terraphim_server is not a member of this workspace"]` |

## Test strategy

The subject here *is* the test suite, so verification is about the harness, not new assertions.

| Check | Command | Expectation |
|---|---|---|
| Nothing spawns cargo any more | `rg 'Command::new\("cargo"\)' crates/*/tests` | no matches |
| Suite terminates | `cargo test --workspace --all-targets --no-fail-fast` | **completes** — the headline criterion; today it hangs forever |
| Working tree stays clean | `git status --short` after the run | empty (guards against the source-tree writers) |
| No regression in what CI runs today | `cargo test --workspace --lib`, `packaged_install_graph_regression`, `ci_guards` | unchanged |
| Build/lint unaffected | `cargo check`/`clippy --all-targets --all-features` | green |

**Success is "the suite finishes", not "the suite passes."** Failures afterwards are pre-existing and
get triaged separately — attempting both in one change is how the scope runs away.

## Steps

1. Mechanical swap, agent crate (14 files) — verify no `Command::new("cargo")` remains there.
2. Mechanical swap, mcp_server (6) and session-analyzer (2).
3. Tempdir the two source-tree writers; assert `git status` clean after a run.
4. `#[ignore]` the two orphans with a reason.
5. Run the full suite to completion; record pass/fail counts as the new baseline.

## Rollback

Each step is an independent commit and reverts cleanly. Nothing outside `crates/*/tests/` changes, so
no shipped code is affected.

## Open items

| Item | Status |
|---|---|
| Fate of the two `terraphim_server` tests | Taking `#[ignore]` as the reversible default; delete-or-relocate is Alex's call |
| Failures revealed once the suite runs | Expected; to be triaged as separate issues, not fixed here |
