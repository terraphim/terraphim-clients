# The dependency unsafe baseline (#447)

**Recorded**: 2026-11-13
**Tool**: `cargo-geiger 0.14.0` (installed into a scratch root from crates.io)
**Scope**: the `terraphim-clients` workspace
**Reconciled**: 2026-11-13, candidate HEAD `05b7530` - the workspace's own
`unsafe` inventory was corrected; the `cargo-geiger` run was **not** repeated.

## Reconciliation addendum (2026-11-13, candidate HEAD `05b7530`)

The rest of this document is the **original dated Gitea #447 baseline**, kept
as written; this addendum holds the current inventory of the workspace's own
`unsafe` items - **13, in three files** (7 + 4 + 2):

| File | Items | Why |
|------|-------|-----|
| `crates/terraphim_agent/tests/support/server_process.rs` | 7 | the libc FFI the test-server helper needs (one `unsafe extern` block and six expressions) |
| `crates/terraphim_agent/tests/transaction_lock.rs` | 4 | the POSIX fork regression (one `unsafe extern` block and three expressions) - new in this reconciliation |
| `crates/terraphim_update/tests/managed_mode_home_env.rs` | 2 | the `HOME` mutation of the one-test binary (#503) |

- The original record's prose said "eight"; its own allow-list counts were
  7 + 2 = **9** items - the eight was the `unsafe {` **block** count (6 + 2)
  of the research measurement, which does not count the `unsafe extern` block
  the guard's counter counts as an item.
- The third file is new: the reconciliation preserved the fork regression
  test, in its own integration-test crate, so the crate attribute cannot
  reach it.
- No new `cargo-geiger` measurement was taken; the dependency-graph figures
  below are the ones recorded on 2026-11-13 with `cargo-geiger 0.14.0`.

## The workspace's own `unsafe`

The baseline for the code this repository owns:

| Target | `unsafe` items |
|--------|----------------|
| the nine library and binary targets | **0** |
| the integration-test targets | 13, in three files (the addendum above; the original dated record said 8, in two files) |

The library and binary targets now carry `#![forbid(unsafe_code)]`
(terraphim-clients#447), so the compiler refuses any `unsafe` there - the
"0" is enforced, not merely observed.

The remaining items are in test crates, which are separate compilation units
that a crate attribute cannot reach; the two-file list below is the original
dated record (the current three-file list is the addendum above):

| File | Items | Why |
|------|-------|-----|
| `crates/terraphim_agent/tests/support/server_process.rs` | 7 | the libc FFI the test-server helper needs (one `unsafe extern` block and six expressions) |
| `crates/terraphim_update/tests/managed_mode_home_env.rs` | 2 | the `HOME` mutation of the one-test binary (#503) |

They are listed, with their counts, in the `no_new_unsafe_sites` guard - so
the list cannot grow without a deliberate edit.

## The dependency graph

| Metric | Value |
|--------|-------|
| packages in `Cargo.lock` | 802 |
| registry packages | 793 |
| workspace (path) packages | 9 |

## What `cargo-geiger` could not report here

`cargo-geiger 0.14.0` is installed and runs, but it cannot produce the unsafe
counts for this graph in this environment:

```
$ cargo-geiger --output-format Json --manifest-path .../crates/terraphim_agent/Cargo.toml
Failed to match (ignoring source) package: registry+https://github.com/rust-lang/crates.io-index#atomic@0.6.1
...
error: Cargo("1 job failed")
```

The cause is the registry **source replacement**: third-party crates are
fetched from the Gitea mirror (`[source.crates-io] replace-with =
"terraphim-mirror"`), so the build's dependency info names the mirror while
`cargo metadata` names crates.io - and cargo-geiger's package matching cannot
reconcile the two. There is exactly one `atomic 0.6.1` in the lock, so this
is not a duplicate-version problem.

Producing the dependency unsafe counts therefore needs one of:

- a crates.io-only environment for the analysis (the mirror's crates are not
  interchangeable with the crates.io ones for the tool's matching), or
- a registry mirror that keeps the crates.io source id, or
- a `cargo-geiger` version that matches packages across a source replacement.

Until one of those exists, the **baseline is the lock file**: the 793 registry
packages and their versions are the set the unsafe counts are measured against,
and a future `cargo-geiger` run must be compared with them.

## The gate that does not need the tool

The dependency baseline is a record, not a gate. The gate for this repository's
own code is the compiler attribute plus the exact-list guard:

- `#![forbid(unsafe_code)]` on every library and binary target ✓,
- `no_new_unsafe_sites` for the integration-test targets ✓.

Neither needs cargo-geiger, a registry token, or a network ✓ - which is why
they can be enforced today, while the dependency baseline waits for the
tool's environment.
