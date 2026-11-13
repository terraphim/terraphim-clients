# Validation Document: Server-dependent tests skip when the server binary is absent

**Change**: PR for terraphim-clients#466, item 3.
**Verification input**: `docs/plans/verification-server-binary-test-skips-2026-11-11.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-11

## Acceptance Criteria and Evidence

The acceptance source is terraphim-clients#466:

> "cargo test without the server binary reports skips, not failures."

| # | Criterion (literal) | Implementation | Verification | Result |
|---|--------------------|----------------|--------------|--------|
| AC1 | "cargo test without the server binary reports skips, not failures" | `support::binary::skip_without_server` announces the skip and returns `true`; the five server-dependent suites return early instead of propagating an error | `cargo test -p terraphim_agent --all-targets --no-fail-fast` on a machine with no server binary: 21 server-binary failures removed (23 -> 2 failures, the 2 being documented out-of-scope guards); `--nocapture` shows 25 `skipping:` notices | PASS |
| AC2 | "reports skips" (the skip is visible, not silent) | One notice per test, naming the test, the missing binary and the environment variable that provides it | 25 notices observed, one per affected test, identical wording across all seven suites | PASS |
| AC3 | CI behaviour unchanged | The helper skips only when the binary is **absent**; with `TERRAPHIM_SERVER_BIN` set (as native-ci does) the suites run as before | native-ci on the branch: `native-ci / build (push): success`, and the CI lane exercises the present-binary path | PASS |
| AC4 | "not failures" (no test is weakened) | No assertion was removed or relaxed; the suites keep their assertions and simply do not start a server they cannot start | `git diff` shows only early-return plumbing and the new helper; no `assert!` was deleted or weakened | PASS |

## Requirements Traceability

| Requirement (issue text) | Design element | Implementation | Test evidence |
|---------------------------|----------------|----------------|---------------|
| "skip cleanly" | `skip_without_server` prints a notice and returns `true` | `tests/support/binary.rs` | 25 notices; 0 failures |
| "not fail" | suites return early instead of `?`-propagating an error | `server_mode_tests.rs`, `cross_mode_consistency_test.rs`, `kg_ranking_integration_test.rs`, `integration_tests.rs`, `server_process_guard_test.rs` | baseline 23 failures -> 2 |
| "when TERRAPHIM_SERVER_BIN is unset" | resolution order: env value, then `target/debug` | `resolve_server_binary` | `server_binary_resolution.rs` (6 tests) |
| "server-mode integration tests" | the five suites that resolve a server binary | see above | each suite's notice observed |

## Scenario Walkthrough

| Scenario | Expected | Observed |
|----------|----------|----------|
| Developer, no server binary | suite green, skip notices | 1192 passed, 2 out-of-scope failures, 25 notices |
| CI, `TERRAPHIM_SERVER_BIN` set | affected tests run for real | native-ci green on the branch |
| `TERRAPHIM_SERVER_BIN` points at a missing file | treat as absent, do not fall back silently | `an_environment_value_that_points_at_a_missing_file_is_absent` |
| Server present but broken | the test must fail, not skip | the helper only skips on absence; the suites' own error paths are unchanged |

## Not Validated Here

- The "server present but broken" path could not be exercised locally (no
  server binary is available on this machine); the guarantee is structural -
  the skip decision is made before the server is started, and every error after
  that point still propagates as a failure.
- Items 1 and 2 of #466 (version bump, `v1.21.23` publish-or-abandon) are out
  of scope: the issue marks the tag decision as needing an owner.

## Conclusion

The acceptance criterion of #466 item 3 is met: on a machine without the
server binary the suite no longer fails, it announces each skip, and CI keeps
running the tests for real.
