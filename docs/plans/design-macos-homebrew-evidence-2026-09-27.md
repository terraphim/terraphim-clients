# Implementation Plan: macOS Homebrew evidence job

**Status**: Draft
**Research Doc**: `docs/plans/research-macos-homebrew-evidence-2026-09-27.md`
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Phase**: 2 (disciplined-design)
**Estimated Effort**: 3 hours

## Overview

### Summary

An automated job on a GitHub `macos-15` runner that installs the published formulae from the tap, asserts behaviour including the codesign check, and archives the transcript in the release record.

### Approach

Reuse the runner already used by the signing jobs. Tap the formula, install, print versions, run one real command, run `brew test`, and upload the transcript as an artefact referenced by the release record.

### Scope

**In Scope:**
- `brew tap` and `brew install` for terraphim-agent and terraphim-grep on macOS arm64.
- Version output, one real command, and `brew test` including the codesign assertion.
- Transcript artefact attached to the release record.
- Honest reporting if the thin Intel lane cannot run because Rosetta is unavailable.

**Out of Scope:**
- Provisioning a persistent macOS host.
- Notarisation re-verification beyond the formula's codesign check.
- Any change to the formulae themselves, beyond what the evidence run proves necessary.

**Avoid At All Cost** (from 5/25 analysis):

| Rejected | Why |
|---|---|
| A permanent self-hosted Mac | Cost and maintenance for a short periodic job |
| Manual evidence collection | Manual is exactly why the gap exists |
| Treating a Rosetta-blocked lane as a pass | Violates the honesty rule used elsewhere in this plan set |

## Architecture

### Component Diagram

```
release workflow (or manual dispatch)
        |
        v
job: macos-brew-evidence  (runs-on: macos-15)
   brew tap terraphim/terraphim
   brew install terraphim-agent terraphim-grep
   terraphim-agent --version ; terraphim-grep --version
   terraphim-agent session list (or equivalent real command)
   brew test terraphim-agent
   tee transcript -> upload-artifact
```

### Data Flow

Tap -> formula -> R2 universal archive -> SHA-256 -> install -> assertions -> transcript -> release record.

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| GitHub `macos-15` runner | Already used and permitted; no new infrastructure | Self-hosted Mac |
| Transcript as an uploaded artefact | Verifiable evidence attached to the release | Console output only |
| Run `brew test` explicitly | It contains the macOS codesign assertion | Version check only |
| Report the Intel lane as not-executed when Rosetta is absent | Consistent honesty rule | Silent omission |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| Testing every Homebrew version | Out of vital few | Long runtimes |
| Publishing to homebrew-core as evidence | Unrelated programme | Review burden |
| Screenshot-based UI evidence | The binaries are CLIs | Brittle artefacts |

### Simplicity Check

**What if this could be easy?** It is a short job: tap, install, assert, upload. The only judgement call is the Rosetta lane, handled by an explicit not-executed state.

**Senior Engineer Test**: Yes.

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
| `.github/workflows/macos-brew-evidence.yml` | The evidence job |
| `scripts/probes/macos-evidence.sh` | Assertions and transcript assembly |

### Modified Files

| File | Changes |
|------|---------|
| `docs/release-operator-checklist.md` | Section 6 points at the job and its artefact |
| Release record for v1.21.16 | Attach the first transcript |

### Deleted Files

| File | Reason |
|------|--------|
| none | - |

## API Design

```
scripts/probes/macos-evidence.sh --formulae "terraphim-agent terraphim-grep" --out transcript.txt
  exit 0  all assertions passed
  exit 1  an assertion failed (transcript still written)
  exit 2  lane not executable (for example Rosetta unavailable), reported as not-executed
```

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_transcript_contains_versions` | `scripts/tests/test-macos-evidence.sh` | Required lines present |
| `test_lane_skip_is_exit_2` | same | Skip is not a pass |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_install_from_tap_macos_arm64` | macOS runner | Real install |
| `test_codesign_assertion_runs` | macOS runner | The macOS-only test executes |
| `test_transcript_uploaded` | workflow | Artefact present and non-empty |

No mocks: real Homebrew, real tap, real signed bytes.

### Property Tests

Not applicable.

## Implementation Steps

### Step 1: Evidence script

**Files:** `scripts/probes/macos-evidence.sh`
**Description:** Tap, install, versions, real command, `brew test`, transcript, tristate exit codes.
**Tests:** unit tests
**Estimated:** 1 hour

### Step 2: Workflow job

**Files:** `.github/workflows/macos-brew-evidence.yml`
**Description:** `macos-15` job, manual dispatch plus invocation from the release process, artefact upload.
**Tests:** a real run
**Dependencies:** Step 1
**Estimated:** 1 hour

### Step 3: First evidence run and archival

**Files:** release record
**Description:** Run against 1.21.16 and archive the transcript.
**Dependencies:** Step 2
**Estimated:** 1 hour

## Rollback Plan

1. The job is additive and changes no published artefact.
2. If the job proves flaky, disable the workflow and record the reason; the checklist reverts to manual.

## Migration (if applicable)

Not applicable.

## Dependencies

### New Dependencies

| Dependency | Version | Justification |
|------------|---------|---------------|
| none | - | Uses Homebrew already present on macOS runners |

### Dependency Updates

| Dependency | From | To | Reason |
|------------|------|-----|--------|
| none | - | - | - |

## Performance Considerations

| Metric | Target | Measurement |
|--------|--------|-------------|
| Job runtime | < 10 minutes | runner log |
| Runner minutes per release | minimal | run count |

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Whether GitHub macOS runners satisfy the checklist's evidence requirement | Pending | release owner |
| Whether the thin Intel lane is required this release | Pending | release owner |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Human approval received