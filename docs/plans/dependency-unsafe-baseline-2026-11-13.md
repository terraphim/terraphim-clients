# The dependency unsafe baseline (#447)

**Recorded**: 2026-11-13
**Tool**: `cargo-geiger 0.14.0` (installed into a scratch root from crates.io)
**Scope**: the `terraphim-clients` workspace

## The workspace's own `unsafe`

The baseline for the code this repository owns:

| Target | `unsafe` items |
|--------|----------------|
| the nine library and binary targets | **0** |
| the integration-test targets | 8, in two files (see below) |

The library and binary targets now carry `#![forbid(unsafe_code)]`
(terraphim-clients#447), so the compiler refuses any `unsafe` there - the
"0" is enforced, not merely observed.

The eight remaining items are in test crates, which are separate compilation
units that a crate attribute cannot reach:

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
