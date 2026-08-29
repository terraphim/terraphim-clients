# Research: #113 — integration tests spawn nested `cargo`

**Status**: Draft
**Date**: 2026-08-29
**Issue**: #113

## Executive Summary

#113 described one file. It is **22 test files across 3 crates**, in four distinct patterns. One
pattern cannot be fixed at all — two tests build `terraphim_server`, which is **not a member of this
workspace** — so those tests have never been able to pass here. None of the 22 run in CI today, so
the entire value of this work is unblocking a full test gate (PR #84).

## Essential Questions Check

| Question | Answer | Evidence |
|---|---|---|
| Energizing? | Yes | It is the reason the gate can only run `--lib` |
| Leverages strengths? | Yes | Already mapped the workspace and CI during #112/#118 |
| Meets real need? | Yes | PR #84 switches CI to `--all-targets` and will hang the runner until this lands |

**Proceed**: Yes (3/3).

## Problem

Tests invoke the binary under test by shelling out to cargo:

```rust
let mut cmd = Command::new("cargo");
cmd.args(["run", "-p", "terraphim_agent", "--"]).args(args);
```

Under `cargo test`, the outer cargo holds the build-directory lock; the nested `cargo run` blocks on
it and never returns. Observed during #112: `cargo test --workspace --all-features` hangs in
`comprehensive_cli_tests` indefinitely.

## Scope — measured, not estimated

| Pattern | Count | Fixable via `CARGO_BIN_EXE_*` |
|---|---|---|
| `cargo run -p terraphim_agent --` | 6 | yes |
| `cargo build --bin terraphim-agent` | 4 | yes — the binary already exists at test time |
| `cargo run -p terraphim_agent --features server --` | 3 | **needs care** (see below) |
| `cargo build -p terraphim_server` | 2 | **no — not a workspace member** |
| `cargo build -p terraphim_agent` | 2 | yes |
| `cargo run --bin tsa --` | 3 | yes |
| `cargo run --bin terraphim-agent --` | 2 | yes |
| `cargo build -p terraphim_agent --bin terraphim-agent` | 1 | yes |

By crate: `terraphim_agent` 14, `terraphim_mcp_server` 6, `terraphim-session-analyzer` 2.

Binaries available as `CARGO_BIN_EXE_<name>`: `terraphim-agent`, `terraphim-cli`, `terraphim-grep`,
`terraphim-lsp`, `terraphim_mcp_server`, `tsa`. Every target the tests want is present **except**
`terraphim_server`.

### `terraphim_server` does not exist here

`rg 'terraphim_server' Cargo.toml crates/*/Cargo.toml` returns nothing. It is not a member, not a
dependency, not on the registry as far as these manifests are concerned. So
`cargo build -p terraphim_server` in `cross_mode_consistency_test.rs` and its sibling can never
succeed — matching the observed `Error: Failed to compile server`. These are not deadlocks; they are
tests for a binary this repo does not build.

### The `--features server` variant

`terraphim_agent` has `server = ["dep:reqwest", "dep:urlencoding"]`, not in `default`
(`["repl-interactive", "llm", "repl-sessions"]`) but included in `repl-full`. `CARGO_BIN_EXE_*`
resolves to the binary built with **the feature set the test target itself was built under**, not an
arbitrary one. So a test that today asks for `--features server` gets whatever the harness built. It
must either be gated on the feature (`#[cfg(feature = "server")]`) or assert the behaviour is
present, rather than silently testing a binary without it.

### Tests write into the source tree

`cross_mode_consistency_test.rs:411` and `kg_ranking_integration_test.rs:278` both do
`fs::write("docs/src/kg/test_ranking_kg.md", ...)`, leaving an untracked file after any run. It was
committed by accident once during #112 and had to be amended out.

## Current CI exposure — none

`native-ci` runs `cargo test --workspace --lib`, plus `packaged_install_graph_regression` and
`ci_guards` by name. **None of the 22 files run in CI.** They compile (`--all-targets` passes) but are
never executed, so their pass/fail state is unknown and has been for some time.

This reframes the work: fixing #113 delivers no immediate CI improvement. Its value is that PR #84
("run all workspace targets in test gate") is unmergeable until it lands, and that ~22 files of
integration coverage are currently dead weight.

## Vital Few

| Constraint | Why vital | Evidence |
|---|---|---|
| No test may spawn `cargo` | The deadlock is unconditional under `cargo test` | `comprehensive_cli_tests` hangs indefinitely |
| Tests must not write into the source tree | Already caused an accidental commit | #112, amended out |
| Tests for a non-existent binary must stop pretending | 2 tests can never pass here | `terraphim_server` absent from all manifests |

## Eliminated from scope

| Eliminated | Why |
|---|---|
| Making the `terraphim_server` tests pass | The binary is not in this workspace; bringing it in is a much larger decision |
| Turning CI to `--all-targets` | That is PR #84's job; doing both at once conflates two changes |
| Fixing whatever the 22 tests find once they run | Unknown until they run; separate work |

## Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Tests fail once actually executed | **High** — unrun for months | Medium | Land the mechanical fix first, then triage failures separately |
| `--features server` tests silently assert against a binary lacking the feature | Medium | Medium | `#[cfg(feature = "server")]` gate rather than a blind swap |
| A 22-file mechanical change hides a semantic one | Medium | Medium | Keep the swap purely mechanical; no behaviour edits in the same commit |

## Open question

**What should happen to the two `terraphim_server` tests?** They cannot pass in this workspace.
Options: `#[ignore]` with a reason, delete them, or move them to whichever repo builds
`terraphim_server`. This needs a decision — it is the only part of #113 that is not mechanical.

## Recommendation

Proceed, but in two separable pieces: the mechanical `CARGO_BIN_EXE_*` swap plus tempdir fixes
(large, low-risk, reviewable), and a decision on the two orphaned server tests. Do not attempt to fix
whatever failures surface afterwards in the same change.
