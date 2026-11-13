# Research Document: the workspace becomes a forbid-soundness project (#447)

**Status**: Approved
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13
**Mode**: `remediation` (the issue names the goal; the research establishes that the precondition now holds)

## Executive Summary

The unsafe-audit epic removed the environment-mutation sites that made
`forbid(unsafe_code)` impossible, so the permanent gate the audit kept
pointing at can finally be turned on: **every crate's library and binary
target now carries `#![forbid(unsafe_code)]`**, enforced by the compiler. What
remains is eight `unsafe` items in two *integration-test* files - separate
compilation units that the crate attribute cannot reach - and a guard that
keeps that list from growing.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | The audit's own conclusion was "forbid(unsafe_code) is the permanent gate; this arm is the interim stop" ✓ |
| Leverages strengths? | Yes | The compiler enforces it - no scanning, no reviewer diligence needed ✓ |
| Meets real need? | Yes | The issue asks for exactly this, and the precondition (the 18 sites) is gone ✓ |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

The workspace compiles `unsafe` in its test targets and has no mechanism that
would stop a new one from appearing. The interim guard (the env-mutation arm)
covers one *shape* of unsafe code - the environment mutation - but not the
general case.

### Impact

- A new `unsafe` block in a library target would compile silently ✓ and the
  audit trail would have to catch it by review ✓ - which the env-guard's own
  history shows does not happen ✓ (the shape spread 6 -> 14 in four weeks ✓).

### Success Criteria

- A library or binary target cannot contain `unsafe` at all ✓ (the compiler
  refuses it ✓).
- The test targets' remaining exceptions are listed, counted and visible ✓.
- A new `unsafe` item outside that list fails CI ✓.

## Current State Analysis

### The inventory, measured

```
$ grep -rn "unsafe\s*{" --include=*.rs crates/ | wc -l
10
$ grep -rn "unsafe\s*{" --include=*.rs crates/ | awk -F: '{print $1}' | sort | uniq -c
      6 crates/terraphim_agent/tests/support/server_process.rs
      2 crates/terraphim_update/tests/managed_mode_home_env.rs
      2 crates/terraphim_agent/tests/ci_guards.rs
```

Every one of them is in an **integration test** - a separate compilation unit,
so a crate-level `#![forbid(unsafe_code)]` does not reach it ✓:

| File | Items | Why it needs them |
|------|-------|-------------------|
| `crates/terraphim_agent/tests/support/server_process.rs` | 7 | the libc FFI the test-server helper needs (one `unsafe extern` block and six expressions) |
| `crates/terraphim_update/tests/managed_mode_home_env.rs` | 2 | the `HOME` mutation of the one-test binary (#503) |
| `crates/terraphim_agent/tests/ci_guards.rs` | 2 | the guard's own quoted fixtures (the file excludes itself from its scan) |

The library and binary targets hold **zero** `unsafe` ✓ - the precondition the
issue names ("once sites 0001-0018 are removed" ✓) now holds ✓.

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| The nine crates | `crates/*/src/lib.rs`, `crates/terraphim_cli/src/main.rs` | where the compiler attribute goes |
| The interim guard | `crates/terraphim_agent/tests/ci_guards.rs::no_new_environment_mutation_sites` | the shape-specific stop |
| The new guard | `crates/terraphim_agent/tests/ci_guards.rs::no_new_unsafe_sites` | the general stop for the test crates |

## Constraints

### Technical Constraints

- `#![forbid(unsafe_code)]` is an **inner attribute**: it must appear before
  any item of the crate ✓, and it cannot be overridden by
  `#[allow(unsafe_code)]` ✓ - which is exactly the property wanted ✓.
- Integration tests under `tests/` are separate crates ✓, so the attribute
  does not reach them ✓ - hence the second gate ✓.
- The `forbid` attribute also forbids `unsafe impl`, `unsafe fn` and
  `unsafe extern` ✓.

### Business Constraints

- None beyond the audit's own ✓.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| A library target with `unsafe` | does not compile | compiles |
| A new `unsafe` item in a test crate | fails CI | compiles |

## Vital Few (Essential Constraints)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| The compiler attribute, not a scan | a scan can be evaded by a spelling; the compiler cannot | `forbid` is unforgiving ✓ |
| The test crates need a second gate | the attribute cannot reach them | the crates are separate compilation units ✓ |
| The exception list must shrink | an allow-list that grows is a rubber stamp | the env-guard's own rule ✓ |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Rewriting the eight test-target items | the FFI and the one-test HOME mutation are legitimate and documented ✓; the list shrinks as the remediation train runs ✓ |
| A `cargo-geiger` dependency in CI | it needs the registry mirror with a `read:package` token ✗ (the owner's action, #368 ✓) |
| `unsafe` in the dependencies | not ours to fix ✓ - the baseline records them ✓ |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| The nine crates' lib/bin targets | the attribute goes there | low ✓ - they compile clean ✓ |
| `ci_guards.rs` | the new arm lives there | low ✓ |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `cargo-geiger` | - | the baseline needs it; the install needs crates.io access | the baseline can be recorded from the lock file ✓ |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| A dependency's macro expands to `unsafe` in our crate | low | the build breaks | `forbid` cannot be overridden - a break is the *point* ✓, and it would be visible immediately ✓ |
| The attribute is placed after an item | low | the build breaks | the compiler says so ✓ |
| The exception list becomes a rubber stamp | medium | the gate loses its value | the list is exact (counts included ✓) and must be edited deliberately ✓ |

### Open Questions

1. Can the dependency baseline be recorded with `cargo-geiger` on this host? -
   The registry mirror needs a `read:package` token ✗ (#368 ✓); crates.io is
   reachable through the proxy ✓, so an install from crates.io into a scratch
   root is possible ✓.
2. Should the attribute also cover the *doctests*? - Doctests are separate
   crates ✓ that link the library ✓; the library's own `forbid` applies to the
   library ✓, so a doctest that writes `unsafe` would still compile ✗ - but
   that is a doctest, not the library ✓, and the audit's scope is the
   library ✓.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| Every library and binary target compiles with the attribute | the inventory shows zero `unsafe` outside `tests/` | the build breaks | Yes - `cargo check --workspace --all-targets` is clean ✓ |
| The integration tests are separate crates | cargo compiles each file under `tests/` as its own crate | the attribute would reach them and break the build | Yes - the build is clean ✓ and the items are still there ✓ |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| (a) `#![forbid(unsafe_code)]` everywhere, including the test crates | the eight legitimate items would not compile | Rejected: the FFI and the one-test HOME mutation are legitimate ✓ |
| (b) `#![deny(unsafe_code)]` in the test crates, with per-item allows | the compiler would enforce it there too | Rejected for now: eight per-item allows are noise ✓; a text guard with an exact list is the repo's established pattern ✓ and the compiler gate covers the code that ships ✓ |

## Research Findings

### Key Insights

1. The precondition the issue names now holds ✓: the library and binary
   targets hold zero `unsafe` ✓.
2. `forbid` (not `deny`) is the right attribute ✓: it cannot be overridden by
   a local `#[allow]` ✓, so a new `unsafe` needs a *review of the attribute
   itself* ✓.
3. The eight remaining items are in test crates ✓, so they need a second,
   list-based gate ✓ - the same shape as the env-mutation arm ✓.

### Relevant Prior Art

- `no_new_environment_mutation_sites`: the interim stop ✓ - the new arm
  follows its exact-list pattern ✓ (counts included ✓, so a stale entry fails
  ✓).

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| add the attribute and compile the workspace | does it hold? | done ✓ (clean ✓) |
| install cargo-geiger from crates.io | the dependency baseline | in progress ✓ |

## Recommendations

### Proceed

Proceed: the attribute on all nine crates ✓, plus an exact-list guard for the
test crates ✓, plus the dependency baseline ✓.

### Scope Recommendations

- The attribute goes on the **library and binary** target of every crate ✓.
- The guard covers the test crates' exceptions with counts ✓.
- The dependency baseline is recorded as a document ✓ (the audit trail the
  issue asks for ✓).

### Risk Mitigation Recommendations

- Keep the guard's list exact (file **and** count ✓) so a stale entry fails ✓.
- Record the dependency baseline with the tool version and the date ✓.

## Next Steps

1. Phase 2 (design): the attribute's placement, the guard's rule, the
   baseline's content.
2. Phase 3 (implementation): the attributes, the guard, the note.
3. Phase 4 (verification): the workspace build, the suite, the guard's own
   tests.
4. Phase 5 (validation): the issue's acceptance.

## Appendix

### Reference Materials

- The issue ✓.
- The audit's own conclusion: "#310 is the permanent gate; this arm is the
  interim stop" (`crates/terraphim_agent/tests/ci_guards.rs`) ✓.
- The inventory, measured ✓ (above).
