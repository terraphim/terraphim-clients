# Implementation, Verification and Validation: the workspace becomes a forbid-soundness project (#447)

**Issue**: terraphim-clients#447
**Research**: `docs/plans/research-forbid-unsafe-code-2026-11-13.md`
**Design**: `docs/plans/design-forbid-unsafe-code-2026-11-13.md`
**Baseline**: `docs/plans/dependency-unsafe-baseline-2026-11-13.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13

> **Reconciliation pointer (2026-11-13, candidate HEAD `05b7530`).** The
> allow-list and the counts below are the **original dated Gitea #447 record**
> and are historical. The **current** inventory is in the baseline's
> reconciliation addendum: `docs/plans/dependency-unsafe-baseline-2026-11-13.md`.

## What was implemented

### The compiler gate

Every crate's library and binary target now carries:

```rust
// The workspace is a forbid-soundness project: no `unsafe` in the library or
// binary target of this crate (terraphim-clients#447, #310). The integration
// tests under `tests/` are separate crates and keep their own, listed
// exceptions - see the `no_new_unsafe_sites` guard.
#![forbid(unsafe_code)]
```

| Crate | Target |
|-------|--------|
| terraphim_agent | src/lib.rs |
| terraphim_cli | src/main.rs |
| terraphim_command_runtime | src/lib.rs |
| terraphim_grep | src/lib.rs |
| terraphim_hooks | src/lib.rs |
| terraphim_mcp_server | src/lib.rs |
| terraphim_negative_contribution | src/lib.rs |
| terraphim_sessions | src/lib.rs |
| terraphim_update | src/lib.rs |

`forbid` and not `deny`: `deny` can be overridden by a local
`#[allow(unsafe_code)]`, so a new `unsafe` item could arrive with an allow
attribute and no visible decision. `forbid` cannot be overridden - turning it
off is a deliberate act on the crate's first line.

### The guard for the test crates

`no_new_unsafe_sites`, next to the env-mutation arm:

- the inventory of `unsafe` items under `crates/` must match the allow-list
  **exactly** - file *and* count - so a new item needs a deliberate entry and
  a vanished item needs its entry removed;
- the counter counts `unsafe` **items** (`unsafe {`, `unsafe fn`, `unsafe impl`,
  `unsafe trait`, `unsafe extern`) and ignores the word in prose, in the crate
  attribute `#![forbid(unsafe_code)]`, and inside a longer identifier.

The allow-list has two entries (the #447 list ✓; the current list is in the
baseline addendum ✓):

| File | Items |
|------|-------|
| `crates/terraphim_agent/tests/support/server_process.rs` | 7 (one `unsafe extern` block and six expressions - the libc FFI of the test-server helper) |
| `crates/terraphim_update/tests/managed_mode_home_env.rs` | 2 (the `HOME` mutation of the one-test binary, #503) |

### The baseline

`docs/plans/dependency-unsafe-baseline-2026-11-13.md` records the tool
(`cargo-geiger 0.14.0`, installed into a scratch root), the workspace's own
`unsafe` inventory (0 in the library and binary targets, 8 in the test
targets), the dependency graph (802 packages, 793 of them registry packages),
and - with the evidence - what cargo-geiger cannot report in this environment.

## Verification

| Check | Result |
|---|---|
| `cargo check --offline --workspace --all-targets` | clean with the attributes ✓ |
| `cargo test --offline --workspace --all-targets --no-fail-fast` | **1989 passed, 0 failed** ✓ |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | clean ✓ |
| `cargo fmt --all -- --check` | clean ✓ |
| `cargo test -p terraphim_agent --test ci_guards` | 29 passed ✓ (the new arm and the counter's rules ✓) |

The counter's rules are unit-tested: an `unsafe` block, an `unsafe fn`, an
`unsafe impl` and an `unsafe extern` are items; the crate attribute, a prose
mention and an identifier that merely starts with "unsafe" are not ✓.

## Validation

| Acceptance criterion (issue #447) | Result |
|---|---|
| "Install cargo-geiger" | PASS - `cargo-geiger 0.14.0` is installed (into a scratch root, from crates.io) ✓ |
| "record the dependency unsafe baseline" | PARTIAL - the workspace's own inventory and the dependency graph are recorded ✓; the tool's unsafe counts for the graph cannot be produced here (the registry source replacement breaks its package matching ✓ - documented with the evidence ✓) |
| "add `#![forbid(unsafe_code)]` per crate once sites 0001-0018 are removed" | PASS - the sites are removed ✓ (the library and binary targets hold zero `unsafe` ✓), and the attribute is on all nine crates ✓ |
| "After the audit, the workspace would become a forbid-soundness project" | PASS for the shipped code ✓; the test crates keep eight listed items ✓ |

## Limits

- The dependency unsafe **counts** are not recorded: cargo-geiger cannot match
  the packages across the registry source replacement on this host ✓ (the
  error and its cause are in the baseline ✓). The tool needs a crates.io-only
  environment, a mirror that keeps the crates.io source id, or a version that
  matches across a replacement ✓.
- `cargo-geiger` is not installed as part of this change ✓ - it was installed
  into a scratch root to record the baseline ✓; adding it to CI needs the
  registry mirror with a `read:package` token ✗ (#368 ✓, the owner's action ✓).
- The eight remaining items are legitimate ✓ (libc FFI ✓ and the one-test
  HOME mutation ✓); they are listed with their counts ✓ and the list shrinks
  as the remediation train runs ✓.
- The attribute reaches the crate's own unit tests ✓ (they are part of the
  library ✓) but not the integration tests ✓ (separate crates ✓) - the guard
  covers those ✓.
