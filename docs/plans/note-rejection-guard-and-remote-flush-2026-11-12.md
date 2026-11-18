# Research, Design, Verification and Validation: the rejected-learning guard and the remote flush key (#514)

**Issue**: terraphim-clients#514
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (three measured defects, two of them in this session's own work)

## Research

The issue reports three defects, two of them in the PRs merged earlier in this
session (#505 and #508). All three were confirmed in the code.

### 1. `promote_to_l2` had no rejection guard

#508 added the guard to `promote_to_l3` and `approve`, but not to
`promote_to_l2` (or `promote_to_l1`). `SharedLearningSub::Promote { to: "l2" }`
calls `store.promote_to_l2` directly, so
`terraphim-agent learn shared promote --to l2 <id>` promoted a **rejected**
learning from L1 to L2 and returned `Ok(())`. The learning then had
`trust_level = L2`, `should_sync_to_wiki()` was true, and it was listed by
`learn shared list --trust-level l2` as peer-validated - despite the
rejection.

The invariant #508 stated - "promotion is not a way to route around the
rejection" - was therefore only half-enforced.

### 2. A role with a remote automata path was flushed under the wrong key

`role_thesaurus_keys` only inspected `AutomataPath::Local`. The default
`Terraphim Engineer` role points at a **remote** compiled-corrections file:

```rust
// crates/terraphim_agent/src/onboarding/templates.rs
automata_path: Some(AutomataPath::Remote(
    "https://system-operator.s3.eu-west-2.amazonaws.com/term_to_id.json".to_string(),
)),
```

The service loads that file and persists the thesaurus under the **name inside
it** - `Engineering` (the repository's own copy of the same file,
`test-fixtures/term_to_id.json`, carries `"name": "Engineering"`), so the
persisted key is `thesaurus_engineering.json`. The flush only tried
`thesaurus_terraphim_engineer.json`, so it reported "No cached thesaurus
matched role" and left the real entry in place - the original #460 symptom
for the primary real-world role.

### 3. An unreachable profile was skipped silently, and the warning was invisible

`delete_key_everywhere` swallowed an `Err` from `operator.exists(key)` with
`tracing::warn!` and still returned `Ok(removed)`, so the caller printed
"Flushed N cache entries for role" as if the flush were complete. The warning
was invisible on top of that: the agent installs only the `log` facade
(`crates/terraphim_agent/src/logging.rs` calls `log::set_boxed_logger`) and
there is no `tracing_subscriber` anywhere in the crate - so `tracing::warn!`
was dropped on the floor. The sibling code in the same file family uses
`log::warn!` (main.rs, the role-less flush path).

## Design

### The rejection guard

The check is factored into one helper, so a future promotion path cannot
forget it:

```rust
fn ensure_not_rejected(id: &str, learning: &SharedLearning) -> Result<(), StoreError>
```

`promote_to_l1`, `promote_to_l2`, `promote_to_l3` and `approve` all call it.
Lifting a rejection stays a decision of its own (`approve_after_rejection`).

### The remote flush key

The name of a remote compiled file cannot be read from the local filesystem,
but it is known for the file the agent ships with: the canonical
`term_to_id.json` is described by the repository's own copy
(`test-fixtures/term_to_id.json`). So:

```rust
pub const KNOWN_REMOTE_THESAURUS_NAMES: &[(&str, &str)] = &[(
    "https://system-operator.s3.eu-west-2.amazonaws.com/term_to_id.json",
    "Engineering",
)];
```

A **test** checks the table against the repository's copy, so the two cannot
drift apart.

A remote file that is *not* in the table is reported: `role_thesaurus_keys`
now returns `RoleThesaurusKeys { keys, undetermined }`, and the command says
that the thesaurus name could not be determined - and exits non-zero - rather
than claiming a complete flush.

### Truthful reporting

`delete_key_everywhere` returns `FlushOutcome { removed, unreachable_profiles }`:

- the warning uses `log::warn!` (the facade the agent actually installs),
- a profile that could not be reached is named on stderr and the command exits
  non-zero - the profile may still hold a copy of the thesaurus,
- the role-less flush path reports the deletes that failed the same way.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1239 passed, 0 failed |
| `cargo test -p terraphim_agent --lib shared_learning` | 87 passed |
| `cargo test -p terraphim_agent --test cache_flush_tests` | 10 passed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

New tests:

- the store test now asserts that `promote_to_l2` and `promote_to_l1` refuse a
  rejected learning, with the same error as `promote_to_l3`;
- a role with the canonical remote automata path resolves
  `thesaurus_engineering.json`;
- a role with an unknown remote automata path is reported as undetermined, with
  the URL in the message;
- the known-name table agrees with the repository's copy of the file;
- a delete in a reachable profile reports no unreachable profile.

## Validation

| Acceptance criterion (issue #514) | Result |
|---|---|
| "the rejection guard ... covers `promote_to_l2`" | PASS - and `promote_to_l1` too, through one shared helper |
| "Derive the candidate key for a remote automata_path as well" | PASS - the canonical remote file resolves to `thesaurus_engineering.json` |
| "Failing that, the command must not claim the role was flushed ... and exit non-zero" | PASS - an unknown remote file is reported and the command exits non-zero |
| "Use `log::warn!` instead of `tracing::warn!`" | PASS |
| "do not let an unreachable profile pass as success" | PASS - the profile is named and the command exits non-zero |

## Limits

- The known-name table covers the file the agent ships with. A role pointing
  at some other remote file is reported as undetermined rather than flushed -
  the honest answer, but the flush does not delete that role's thesaurus.
  Reading the name over the network would fix it; the agent's HTTP client is
  behind the optional `server` feature, so the flush path cannot use it.
- The name in the remote file is not read at flush time, so a *changed* name
  upstream would make the table stale. The test that ties the table to the
  repository's copy catches that when the copy is updated.
- The unreachable-profile detection is per profile: it reports that the
  profile could not be reached, not which keys are still there.
