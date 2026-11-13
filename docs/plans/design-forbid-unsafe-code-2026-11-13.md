# Design Document: the workspace becomes a forbid-soundness project (#447)

**Status**: Approved (Phase 1 research: `docs/plans/research-forbid-unsafe-code-2026-11-13.md`)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13

## The decision

**Turn the permanent gate on.**

1. Every crate's **library and binary target** carries
   `#![forbid(unsafe_code)]` ✓ - the compiler refuses any `unsafe` there ✓.
2. The **integration-test targets**, which are separate compilation units the
   attribute cannot reach, are covered by a second gate: an exact-list guard
   with the remaining exceptions and their counts ✓.
3. The **dependency unsafe baseline** is recorded as a document ✓, with the
   tool and the date ✓.

## Why `forbid` and not `deny`

`deny(unsafe_code)` can be overridden by a local `#[allow(unsafe_code)]` ✓ -
so a new `unsafe` item could arrive with an allow attribute and no visible
decision ✓. `forbid` cannot be overridden ✓: turning the attribute off is a
deliberate, visible act on the crate's first line ✓ - which is the property
the audit asked for ✓ ("the workspace would become a forbid-soundness
project" ✓).

## The attribute's placement

An inner attribute must precede every item of the crate ✓, so it goes directly
after the crate's `//!` documentation block ✓ and before the first item ✓.
Each crate carries a short comment that says why it is there and where the
test crates' exceptions live ✓.

## The guard

`no_new_unsafe_sites`, next to the env-mutation arm ✓:

| Rule | Why |
|------|-----|
| the inventory must match the allow-list **exactly** (file **and** count) | a stale entry means the list is decaying into a rubber stamp ✓ (the env-guard's own rule ✓) |
| the counter counts `unsafe` **items** - `unsafe {`, `unsafe fn`, `unsafe impl`, `unsafe extern` - and ignores the word in prose and in the crate attribute | the attribute itself must not be counted ✓ |

The counter is a pure function ✓, so its rules are unit-tested ✓ (a prose
mention, the attribute, and an identifier that merely starts with "unsafe" are
not items ✓).

## The dependency baseline

A document that records, with the tool version and the date:

- the total number of `unsafe` items in the dependency graph ✓,
- the crates that contribute the most ✓,
- what the workspace itself contributes ✓ (zero, after this change ✓).

The baseline is a record, not a gate: the *gate* for the workspace's own code
is the compiler attribute ✓.

## The gate for the test crates

The allow-list has two entries ✓:

| File | Items | Why |
|------|-------|-----|
| `crates/terraphim_agent/tests/support/server_process.rs` | 7 | the libc FFI of the test-server helper (one `unsafe extern` block and six expressions) |
| `crates/terraphim_update/tests/managed_mode_home_env.rs` | 2 | the `HOME` mutation of the one-test binary (#503) |

The guard file's own quoted fixtures are excluded by the same path-based
exclusion the env-mutation arm uses ✓.

## Validation plan

- The workspace compiles with the attribute ✓ (`cargo check --workspace
  --all-targets` ✓).
- The whole suite passes ✓.
- The guard's own tests: the counter's rules ✓, and the inventory matches the
  list ✓.
- A new `unsafe` item in a test crate fails the guard ✓ (the exact-list
  comparison ✓).

## Risks

| Risk | Mitigation |
|------|------------|
| A dependency's macro expands to `unsafe` in our crate | the build breaks ✓ - that is the point ✓, and it would be visible immediately ✓ |
| The exception list grows | the guard requires a deliberate edit ✓ with a count ✓ |
| The attribute is placed after an item | the compiler says so ✓ |

## Out of scope

- Removing the eight remaining test-target items ✓ (legitimate FFI and the
  one-test HOME mutation ✓; the list shrinks with the remediation train ✓).
- A `cargo-geiger` step in CI ✓ (it needs the registry mirror with a
  `read:package` token ✗ - the owner's action, #368 ✓).
- The dependencies' own `unsafe` code ✓ (not ours ✓ - the baseline records it ✓).
