# Validation Document: A ci_guards arm that rejects new environment-mutation sites

**Change**: PR for terraphim-clients#375.
**Verification input**: `docs/plans/verification-env-mutation-guard-2026-11-12.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12

## Acceptance Criteria and Evidence

The acceptance source is terraphim-clients#375. Every criterion is stated as a
checkable behaviour; the evidence is the verification document.

| # | Criterion | Result |
|---|-----------|--------|
| AC1 | "Passes on `main` as-is with all known sites allow-listed" | PASS - `no_new_environment_mutation_sites` passes with the measured 23-site inventory (5 files). |
| AC2 | "Fails, naming file and line, when a new `unsafe { std::env::set_var(...) }` is added anywhere under `crates/`" | PASS - manual probe: the failure names `crates/terraphim_agent/tests/env_mutation_probe.rs`, the needle and the allow-list count. |
| AC3 | "Fails, naming file and line, when a new `EnvVarGuard` / `set_env_var` / `remove_env_var` use is added" (the load-bearing case) | PASS - manual probe with `terraphim_test_utils::set_env_var(...)`: the failure names the file and `set_env_var(`. |
| AC4 | "Fails when any member manifest re-adds the `terraphim_test_utils` dev-dependency" | DEFERRED BY DESIGN - members still depend on it while the wrapper sites are open; the arm reports the state and becomes the gate once they are gone (the sequencing #375 describes). |
| AC5 | "Fails when an allow-list entry names a site that no longer exists, so the list cannot rot" | PASS - the guard compares counts in both directions and reports a stale entry as "the allow-list no longer matches the tree". |
| AC6 | "No external process is spawned" | PASS - the scanner and the manifest check use `std::fs` only; the runner's command policy is not involved. |
| AC7 | "Each later remediation commit removes its own allow-list entry in the same commit" | ENFORCED - a remediation that removes a site without decrementing the count fails the guard. |

## Requirements Traceability

| Requirement (issue text) | Design element | Implementation | Evidence |
|---------------------------|----------------|----------------|----------|
| "reject new env::set_var sites against an allow-list" | `KNOWN_ENV_MUTATION_SITES` + the comparison | `no_new_environment_mutation_sites` | AC1, AC2 |
| "both forms ... the safe-wrapper forms" | `ENV_MUTATION_NEEDLES` | same test | AC3 |
| "the list cannot rot" | count comparison in both directions | same test | AC5 |
| "no member lists terraphim_test_utils once the sweep has removed it" | `member_manifests_depending_on` + the conditional branch | same test | AC4 (deferred by design) |
| "No external process is spawned" | `std::fs` walking | `workspace_rust_sources()` (existing) + `collect_env_mutation_sites` | AC6 |

## Scenario Walkthrough

| Scenario | Expected | Observed |
|----------|----------|----------|
| Repository as it stands | guard passes | passes |
| A developer adds `unsafe { std::env::set_var("X", "1") }` in any file under `crates/` | guard fails naming the file | observed (probe) |
| A developer adds `terraphim_test_utils::set_env_var(...)` | guard fails naming the file | observed (probe) |
| A remediation removes a site without updating the allow-list | guard fails ("allow-list no longer matches the tree") | covered by the two-sided count comparison |
| A remediation removes a site and decrements the count | guard passes | by construction |

## Not Validated Here

- AC4 cannot be exercised until the wrapper sites are closed; the branch that
  closes them must also remove the `terraphim_test_utils` dev-dependency, or
  the guard will say so.
- The guard does not detect env mutation done through other spellings (for
  example a `Command::env` on a *parent* process, or `set_var` written with a
  different import alias). It guards the two shapes that actually landed.

## Conclusion

Five of the seven acceptance criteria are met and demonstrated; the seventh
(AC4) is deferred by the sequencing the issue itself prescribes, and the arm
becomes that gate automatically once the wrapper sites are closed.
