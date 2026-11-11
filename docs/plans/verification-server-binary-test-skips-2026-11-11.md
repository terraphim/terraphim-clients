# Verification Document: Server-dependent tests skip when the server binary is absent

**Change**: PR for terraphim-clients#466, item 3 ("make the server-mode integration tests skip cleanly (not fail) when TERRAPHIM_SERVER_BIN is unset").
**Design input**: `docs/plans/design-server-binary-test-skips-2026-11-11.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-11
**Scope**: test code only; no production behaviour changes.

## Author Gate

| Gate | Command | Result |
|------|---------|--------|
| Format | `cargo fmt --all -- --check` | PASS |
| Lint | `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | PASS (exit 0) |
| Package tests | `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1192 passed, 2 failed (both out of scope, see below) |

### Before / after, same machine, no server binary

| | Tests passed | Tests failed |
|---|---|---|
| Before (`main` aaa1720) | 1165 | 23 |
| After (this branch) | 1192 | 2 |

The 21 removed failures are exactly the server-binary failures enumerated in the
research document. The two remaining failures are the out-of-scope environment
guards listed there.

### Skip notices

`cargo test -p terraphim_agent --all-targets -- --nocapture` prints one notice
per skipped test (25 notices, one per affected test):

```
skipping: server_mode_tests: no terraphim_server binary (set TERRAPHIM_SERVER_BIN, or install one into target/debug); refs #113, #466
skipping: cross_mode_consistency_test: ...
skipping: kg_ranking_integration_test: ...
skipping: integration_tests: ...
skipping: extract_functionality_validation: ...
skipping: drop_stops_and_reaps_server: ...
skipping: panic_unwinding_stops_and_reaps_server: ...
```

### New unit tests

`crates/terraphim_agent/tests/server_binary_resolution.rs` - 6 tests, all
passing:

| Test | Proves |
|------|--------|
| `an_environment_value_that_points_at_a_file_wins` | `TERRAPHIM_SERVER_BIN` takes precedence over the fallback |
| `the_fallback_is_used_when_no_environment_value_is_set` | `target/debug/terraphim_server` is the fallback |
| `an_environment_value_that_points_at_a_missing_file_is_absent` | a misconfigured environment value does not silently fall back |
| `a_missing_fallback_without_an_environment_value_is_absent` | absent means absent |
| `a_directory_is_not_a_usable_binary` | a directory does not count as a binary |
| `the_resolved_path_is_usable_as_a_command` | the resolved path is usable by `Command::new` |

## Changed-Surface Coverage

| Surface | Change | Covering test |
|---------|--------|---------------|
| `support::binary::resolve_server_binary` | new pure resolver | `server_binary_resolution.rs` (6 tests) |
| `support::binary::server_binary_path` | new | used by all five suites; covered indirectly by every skip notice above |
| `support::binary::skip_without_server` | new | every affected suite; 25 notices observed |
| `support::binary::server_binary` / `agent_binary` | unchanged semantics, now reachable through `support::binary` | used by `extract_functionality_validation.rs` |

The resolver is the only new logic; everything else is call-site plumbing that
the 25 observed skip notices exercise.

## Known Failures (out of scope, documented in the research)

1. `coverage_tool_pinning_matches_local_toolchain` - local `cargo-llvm-cov` is
   0.8.7 while `.gitea/workflows/native-ci.yml` pins 0.8.5. A toolchain-drift
   guard, not a server-binary problem.
2. `cleanup_script_only_signals_verified_checkout_processes` - "copying test
   executable failed" in `ci_guards`; unrelated to the server binary.

Both are filed as separate issues so the tracker reflects them.

## Reviewer Spot Verification (reproduce, do not trust)

```bash
cd crates/terraphim_agent
cargo test --test server_binary_resolution -- --nocapture   # 6 passed
cargo test --all-targets --no-fail-fast                     # 1192 passed, 2 failed (documented)
```

## Caveats

- The skip is a runtime early return: Rust's harness has no "skipped" state, so
  the runner reports these tests as passed with a `skipping:` line. This is the
  repository's existing convention (`extract_functionality_validation.rs`
  before this change).
- The CI lane installs `terraphim_server` and exports
  `TERRAPHIM_SERVER_BIN`, so the affected tests run for real there; the change
  cannot silently disable them in CI.
