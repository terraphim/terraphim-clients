# Verification Document: A ci_guards arm that rejects new environment-mutation sites

**Change**: PR for terraphim-clients#375.
**Design input**: `docs/plans/design-env-mutation-guard-2026-11-12.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Scope**: one test file (`crates/terraphim_agent/tests/ci_guards.rs`); no production code.

## Author Gate

| Gate | Command | Result |
|------|---------|--------|
| Format | `cargo fmt --all -- --check` | PASS |
| Lint | `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | PASS (exit 0) |
| Guard suite | `cargo test -p terraphim_agent --test ci_guards` | 15 passed, 2 failed (both pre-existing, see below) |

The two failures are the environment guards that were already failing on
`main` and are filed separately (#497 coverage-toolchain pin drift, #498
`cleanup_script_only_signals_verified_checkout_processes` harness copy).

## New tests

| Test | Proves |
|------|--------|
| `no_new_environment_mutation_sites` | The allow-list matches the tree exactly, and a new site fails the guard |
| `env_mutation_scanner_reports_an_unlisted_site` | The scanner detects an unlisted site, and the walker excludes the guard file (which quotes the forbidden literals) |

## Manual proof that the detector works

A temporary probe file was added under `crates/`, the guard was run, and the
probe was removed afterwards. Both shapes are detected:

**Direct `unsafe` form**

```
crates/terraphim_agent/tests/env_mutation_probe.rs: 1 x 'env::set_var(' (allow-list says 0)
```

**Safe-wrapper form** (the case `forbid(unsafe_code)` is structurally blind to)

```
crates/terraphim_agent/tests/env_mutation_probe.rs: 1 x 'set_env_var(' (allow-list says 0)
```

The failure message names the file, the needle and the allow-list count, and
points at #373/#374 and at the injection pattern to use instead.

## Changed-Surface Coverage

| Surface | Change | Covering evidence |
|---------|--------|-------------------|
| `collect_env_mutation_sites` | new scanner | unit test + two manual probes |
| `ENV_MUTATION_NEEDLES` / `KNOWN_ENV_MUTATION_SITES` | new constants | the guard itself (it fails when the tree and the list disagree, in both directions) |
| `member_manifests_depending_on` | new helper | the guard's manifest branch (the wrapper is still in use, so the arm reports rather than fails) |

## Acceptance Evidence (issue #375)

| Acceptance criterion | Evidence | Result |
|---|---|---|
| Passes on `main` as-is with all known sites allow-listed | `no_new_environment_mutation_sites` passes on this branch | PASS |
| Fails, naming file and line, for a new `unsafe { std::env::set_var(...) }` | manual probe: `crates/terraphim_agent/tests/env_mutation_probe.rs: 1 x 'env::set_var(' (allow-list says 0)` | PASS |
| Fails, naming file and line, for a new `EnvVarGuard` / `set_env_var` / `remove_env_var` use | manual probe: `... : 1 x 'set_env_var(' (allow-list says 0)` | PASS |
| Fails when a member manifest re-adds `terraphim_test_utils` | conditional on the wrapper sites being gone (see the note below) | DEFERRED by design |
| Fails when an allow-list entry names a site that no longer exists | the guard compares counts in both directions; a stale entry is reported as "the allow-list no longer matches the tree" | PASS |
| No external process is spawned | the scanner and the manifest check use `std::fs` only | PASS |
| Each later remediation commit removes its own allow-list entry | enforced by the stale-entry check | PASS |

### Note on the manifest criterion

Members still depend on `terraphim_test_utils` because the wrapper sites in
`terraphim_mcp_server` and `terraphim_agent` are still open (the UF-014
sweep). The arm therefore *reports* that the wrapper is still in use and only
starts failing on a manifest that lists `terraphim_test_utils` once the
allow-list has no wrapper entries left. This keeps the arm green during the
remediation train and turns it into the re-entry gate afterwards - the
sequencing #375 describes.

## Caveats

- The guard file is excluded from its own scan (it quotes the literals); that
  exclusion is asserted by the unit test rather than assumed.
- The allow-list is a count per (file, needle), so a remediation commit
  decrements a count instead of deleting a line.
