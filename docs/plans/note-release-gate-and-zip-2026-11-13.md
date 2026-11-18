# Research, Design, Verification and Validation: the release gate, the exact pin and the zip advisories (#516)

**Issue**: terraphim-clients#516
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13
**Mode**: `remediation` (explicit defects from the post-merge review, local change)

## Research

The issue reports four release-plumbing defects, three of them P1.

### 1. The pre-publish gate was wired into no workflow

`scripts/release/verify-agent-package.sh` is the only thing between a broken
package and the registry, yet nothing called it: the release notes told the
operator to run it by hand, and `.gitea/workflows/publish-registry.yml`
published with `cargo publish -p "${CRATE}" --registry terraphim` and no gate
step at all. The gate needs registry credentials, which the publish workflow
already has.

### 2. The `=1.23.0` exact pin creates an undocumented publish ordering

`crates/terraphim_agent/Cargo.toml` requires `terraphim_sessions = "=1.23.0"`.
A semver probe shows that this matches exactly one version: if `1.23.0` is not
on the registry, `cargo install terraphim_agent` fails with no fallback. The
publish workflow takes a crate name as an input and publishes it - with
nothing recording that `terraphim_sessions 1.23.0` has to go first.

### 3. The `zip = "8"` pin keeps four advisories open

`crates/terraphim_update/Cargo.toml` declares `zip = "8"` directly, and
`zipsign-api 0.2.1` (also a direct dependency) pulls `zip 7.2.0`. The lock
therefore carries **both** a vulnerable zip 8 and a vulnerable zip 7, and
`RUSTSEC-2026-0367`, `-0368`, `-0369` and `-0429` are patched only at
`>= 9.0.0`. A lock-only change can never close them: the requirement lives in
the manifest.

The registry index says the way out is available:

| Crate | Versions | Requirement on `zip` |
|---|---|---|
| `zipsign-api` | 0.1.5 | (no zip) |
| `zipsign-api` | 0.2.1 | `>=2.0.0, <8.0.0` |
| `zipsign-api` | **0.2.2** | **`^9`** |
| `zip` | **9.x** | - |

so `zipsign-api 0.2.2` follows zip into the fixed major, and the direct pin
can move with it.

### 4. The guard test's name and scope no longer matched what it guards

`native_ci_aliases_gitea_token_for_packaged_graph_lanes` guarded two lanes -
the broad `--workspace --all-targets` lane and the coverage lane - after the
packaged-graph lane was removed. The name still said "packaged graph".

## Design

**`.gitea/workflows/publish-registry.yml`**

- A gate step runs `scripts/release/verify-agent-package.sh` before the
  publish, scoped to the `terraphim_agent` crate, with the registry token the
  workflow already has. A failing gate refuses to publish.

**`crates/terraphim_update/Cargo.toml` and `Cargo.lock`**

- The direct requirement becomes `zip = ">=9.0, <9.2"`, and `zipsign-api`
  resolves to 0.2.2 (its `zip ^9` requirement is satisfied by the same
  version). The lock now carries **one** zip - 9.1.3 - and no zip 7 or 8.

  The upper bound is what the registry mirror can resolve *today*: the newest
  zip 9.x needs `lzma-rust2 0.25`, and the mirror's newest `lzma-rust2` is
  0.23.0. The bound can go when the mirror catches up; the comment in the
  manifest says so.

**`crates/terraphim_update/src/lib.rs`**

- `extract_zip` had to be adapted to zip 9 (`mangled_name` now returns a
  `Result`), and the adaptation is also the safer one: the entry is written to
  the archive's **enclosed** name, which zip refuses to produce for a path
  that escapes the target directory. The previous `mangled_name` only
  sanitised the characters of the file name - a `../../etc/passwd` entry still
  escaped the target when it was joined onto it. An entry without a safe name
  is now skipped with a warning.

**`crates/terraphim_agent/tests/ci_guards.rs`**

- The guard test is renamed to `native_ci_aliases_gitea_token_for_the_registry_resolving_lanes` and its doc comment describes the two lanes it actually guards (the ones that build the workspace and therefore resolve the private registry).

## Verification

| Check | Result |
|---|---|
| `cargo build --offline --workspace` | clean |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | clean |
| `cargo test --offline -p terraphim_update --all-targets` | the three pre-existing downloader failures (#502, they need network access to git.terraphim.cloud), everything else passes |
| the lock's zip entries | one: `zip 9.1.3` - no zip 7 or 8 left |
| `zipsign-api` in the lock | 0.2.2 (needs `zip ^9`) and 0.1.5 (no zip) |

## Validation

| Acceptance criterion (issue #516) | Result |
|---|---|
| "Add a gate step ... that runs `scripts/release/verify-agent-package.sh` and refuses to publish on non-zero exit" | PASS |
| "bump `zip` to 9 ... and check zipsign-api 0.2's zip requirement" | PASS - `zipsign-api 0.2.2` requires `zip ^9` ✓ and the lock carries one zip 9 ✓ |
| "Rename the test and refresh its doc comment" | PASS |
| "Either publish `terraphim_sessions 1.23.0` first ... or relax to `>=1.23.0, <1.24.0`" | NOT DONE - see the limits |

## Limits

- The `=1.23.0` exact pin is **not** relaxed here. The two options in the
  issue are "publish 1.23.0 first and record the ordering" and "relax the
  requirement"; the first is a release action (the owner's), and the second
  changes what a published `terraphim_agent` resolves to - a policy decision
  about the release line. Neither belongs in a bug fix that touches the
  workspace's dependency graph.
- The `zip` upper bound (`<9.2`) is a mirror limitation, not a security
  choice: the newest zip 9.x needs `lzma-rust2 0.25`, which the mirror does
  not carry. When the mirror catches up, the bound can go.
- The gate step runs the `#[ignore]`d package test, which packages and installs
  the agent - it makes the publish run minutes longer.
- The gate verifies the **local** package against the canonical registry, not
  the published artefact; the post-publish verification the issue also asks for
  needs the published version and is not part of this change.
- The extraction change (`enclosed_name`) skips archive entries whose path
  escapes the target directory. The release archives are produced by this
  repository and contain no such entries, but an archive from elsewhere would
 be extracted partially rather than unsafely.
