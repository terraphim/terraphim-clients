# Implementation Plan: Truthful crates.io channel statement and installer steering

**Status**: Draft
**Research Doc**: `docs/plans/research-crates-io-parity-2026-09-27.md`
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Phase**: 2 (disciplined-design)
**Estimated Effort**: 4 hours

## Overview

### Summary

Make every documented install path either deliver 1.21.16 or state plainly what it delivers. The cargo path keeps crates.io as-is (publishing the family is a separate programme), and users are steered to the path that does deliver 1.21.16.

### Approach

Documentation and steering only. No crate is published, nothing is yanked, and no dependency pin changes. The site's cargo block gains an accurate version note and a pointer to the installer; the crate READMEs carry the same note so it survives being read on crates.io itself.

### Scope

**In Scope:**
- An accurate statement on the site and in crate READMEs of what `cargo install` delivers today.
- A pointer from the cargo instruction to the installer and the self-update path.
- A validation check that the documented versions match reality at release time.

**Out of Scope:**
- Publishing any crate to crates.io, including the family.
- Changing `registry = "terraphim"` pins or `[patch.crates-io]`.
- Yanking or deleting existing versions.

**Avoid At All Cost** (from 5/25 analysis):

| Rejected | Why |
|---|---|
| Publishing the dependency family now | Irreversible, cross-repo, owned by `terraphim-core #71` |
| A one-off manual publish to "catch up" | Creates an unreproducible channel state |
| Removing registry pins to make publishing easy | Changes how the product builds; needs its own research |
| A caveat so vague it means nothing ("may lag") | The point is a precise, checkable statement |

## Architecture

### Component Diagram

```
crates.io (unchanged)      installer (fixed separately)      self-update (works)
        |                            |                              |
        +---------- site cargo block + crate READMEs ---------------+
                     states true version and preferred path
```

### Data Flow

Release -> crates.io newest_version (observed) and channel manifest version (observed) -> site/README note generated or hand-checked at release time -> published.

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| State the exact version, not a vague caveat | A precise claim is checkable and honest | "may be behind" phrasing |
| Note lives in crate READMEs as well as the site | The README is what a user sees on crates.io | Site-only |
| Validate the claim in CI | Prevents the note drifting from reality | Trust that someone will notice |
| Leave crates.io untouched | Avoids irreversible action owned elsewhere | Catch-up publish |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| Publishing terraphim-grep alone as a proof | Its graph still needs the family; half-published state | Confusing version matrix |
| Auto-generating the README note from crates.io at build time | Adds network dependency to the build | Flaky builds |
| A separate "channels" documentation page | Out of vital few; the install section is where users are | Documentation sprawl |

### Simplicity Check

**What if this could be easy?** It is: one sentence per surface stating the true version and the preferred path, plus one CI assertion that the sentence is still true. Nothing else is needed to make the channel honest.

**Senior Engineer Test**: A senior engineer would approve; the alternative is an irreversible cross-repo publish for a documentation problem.

**Nothing Speculative Checklist**:
- [x] No features the user didn't request
- [x] No abstractions "in case we need them later"
- [x] No flexibility "just in case"
- [x] No error handling for scenarios that cannot occur
- [x] No premature optimization

## File Changes

### New Files

| File | Purpose |
|------|---------|
| `scripts/check-documented-versions.py` | Asserts the documented crates.io version statement matches the live observation |

### Modified Files

| File | Changes |
|------|---------|
| Site install section | Accurate cargo note plus installer pointer |
| `crates/terraphim_agent/README.md` | Channel note |
| `crates/terraphim_cli/README.md` | Channel note |
| `crates/terraphim_grep/README.md` | Channel note |
| `.github/workflows/ci.yml` or a scheduled workflow | Run the checker |

### Deleted Files

| File | Reason |
|------|--------|
| none | - |

## API Design

### New Script Interface

```
check-documented-versions.py [--site-url URL] [--crates a,b,c]
  exit 0 when every documented claim matches the live version
  exit 1 with a diff-style report when a claim is stale
```

### Error Types

```
// exit 0  all claims accurate
// exit 1  one or more claims stale; prints expected vs observed
// exit 2  crates.io or the site unreachable (does not fail the claim, reports inconclusive)
```

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_parses_crate_version` | `scripts/check-documented-versions.py` self-test | Version extraction from the API payload |
| `test_detects_stale_claim` | same | A stale claim yields exit 1 |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_documented_versions_match_live` | CI job invoking the checker | No drift at merge time |
| `test_readme_note_present` | same | Each crate README carries the note |

### Property Tests

Not applicable.

## Implementation Steps

### Step 1: Version checker

**Files:** `scripts/check-documented-versions.py`
**Description:** Query crates.io with a descriptive User-Agent for each documented crate, extract the documented version claims from the site and READMEs, and compare.
**Tests:** unit tests above
**Estimated:** 1.5 hours

### Step 2: Documentation

**Files:** site install section, three crate READMEs
**Description:** Add the precise statement and the steer to the installer.
**Tests:** `test_readme_note_present`
**Dependencies:** Step 1
**Estimated:** 1.5 hours

### Step 3: CI wiring

**Files:** workflow file
**Description:** Run the checker on merge and on a schedule; fail on drift.
**Dependencies:** Steps 1 and 2
**Estimated:** 1 hour

## Rollback Plan

1. Revert the documentation commit; nothing irreversible was done.
2. The checker can be disabled by removing its workflow step.

## Migration (if applicable)

Not applicable.

## Dependencies

### New Dependencies

| Dependency | Version | Justification |
|------------|---------|---------------|
| none | - | Python standard library only |

### Dependency Updates

| Dependency | From | To | Reason |
|------------|------|-----|--------|
| none | - | - | - |

## Performance Considerations

Not applicable; the checker runs once per merge.

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Confirm policy option (a) family publish, (b) document lag, or (c) document and steer | Pending | release owner |
| Decide whether the site footnote or the README is the primary statement | Pending | release owner |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Human approval received