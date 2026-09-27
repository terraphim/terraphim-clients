# Implementation Plan: Public-release acceptance run

**Status**: Draft
**Research Doc**: `docs/plans/research-public-release-acceptance-2026-09-27.md`
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Phase**: 2 (disciplined-design)
**Estimated Effort**: 1 day

## Overview

### Summary

One script that executes every documented public entry point and fails when any of them disagrees with the published manifest, reporting unexecutable entry points honestly as not-executed.

### Approach

A single Bash orchestrator plus small per-entry-point probes. Each probe prints a structured line and a final verdict table. Runs locally before announcement and in CI after the release workflow.

### Scope

**In Scope:**
- Installer entry point in a clean container.
- Cargo entry point: observed crates.io version versus the channel version, with the documented claim checked.
- Homebrew entry point: formula existence and checksum parity, plus real install where a Linuxbrew host exists.
- Updater entry point: shipped binary reports current against the R2 channel.
- Honest not-executed reporting for macOS and Windows.

**Out of Scope:**
- Re-validating sealed stages, signatures or promotion provenance.
- Building anything; the run consumes published artefacts only.
- macOS execution, tracked separately.

**Avoid At All Cost** (from 5/25 analysis):

| Rejected | Why |
|---|---|
| A test framework dependency | Bash plus a container is enough |
| Treating skipped checks as passes | This is the exact failure the item exists to prevent |
| Caching downloaded archives between runs | Hides a broken fetch |
| A machine-readable dashboard | The release record needs transcripts, not a service |

## Architecture

### Component Diagram

```
scripts/acceptance-public-release.sh  (orchestrator, prints table, sets exit code)
   |
   +-- probe-installer   -> docker run debian:bookworm-slim: documented curl command, assert version
   +-- probe-cargo       -> crates.io observed version vs documented claim
   +-- probe-homebrew    -> formula presence + manifest checksum parity (+ real install if host)
   +-- probe-updater     -> channel binary check-update against downloads.terraphim.ai
   +-- probe-site        -> every documented command string is accounted for
```

### Data Flow

Documented entry point list -> probe per entry point -> observed facts -> comparison with `stable-v2.json` -> verdict table -> exit code.

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| Container for the installer probe | The host has masking artefacts in `~/.cargo/bin` | Running on the dev host |
| Manifest is the comparison standard | Generated truth, already validated | Hardcoded expected version |
| Three-state result: pass, fail, not-executed | Honesty about what was really run | Boolean pass/fail |
| Raw transcripts archived | Evidence for the release record | Summary only |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| A nightly full matrix of every OS | Cost without a corresponding release | Infrastructure churn |
| Screenshot or UI checks of the site | Out of vital few | Brittle, unrelated |
| Auto-opening issues on failure | Not asked for; the release owner decides | Noise |

### Simplicity Check

**What if this could be easy?** It is: five probes, each a handful of lines, one table, one exit code. The only subtlety is refusing to call a skip a pass, which is one extra state.

**Senior Engineer Test**: Yes; anything heavier would be a test framework for five checks.

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
| `scripts/acceptance-public-release.sh` | Orchestrator and verdict table |
| `scripts/probes/installer.sh` | Clean-container installer probe |
| `scripts/probes/cargo-claim.sh` | crates.io claim probe |
| `scripts/probes/homebrew.sh` | Formula and checksum probe |
| `scripts/probes/updater.sh` | Updater probe |

### Modified Files

| File | Changes |
|------|---------|
| `docs/release-operator-checklist.md` | Replace the manual entry-point section with a pointer to the run |
| `.github/workflows/` | Invoke the run after the release workflow and on a schedule |

### Deleted Files

| File | Reason |
|------|--------|
| none | - |

## API Design

```
scripts/acceptance-public-release.sh [--version X.Y.Z] [--skip-network] [--json OUT]
  exit 0  every executed probe passed
  exit 1  at least one executed probe failed
  exit 2  a probe could not execute (reported, does not silently pass)
  exit 3  usage error
```

Structured output per probe:

```
PROBE installer  PASS   installed=1.21.16 manifest=1.21.16 sha=71cb8745...
PROBE cargo      FAIL   documented=1.21.16 observed=1.21.1
PROBE homebrew   FAIL   formula=terraphim-ai missing (tap has terraphim-agent)
PROBE updater    PASS   check-update reports current
PROBE macos      NOT-EXECUTED  no macOS host
```

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_probe_result_tristate` | `scripts/tests/test-acceptance.sh` | Pass, fail and not-executed are distinct |
| `test_exit_codes` | same | Exit code reflects the worst probe outcome |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_installer_probe_clean_container` | local and CI | Real install from the documented command |
| `test_all_documented_commands_covered` | local and CI | No documented command is unaccounted for |
| `test_run_against_live_channel` | pre-announcement run | End-to-end truth |

No mocks: every probe contacts the live public channel; the container is stock and unfurnished.

### Property Tests

Not applicable.

## Implementation Steps

### Step 1: Orchestrator and result model

**Files:** `scripts/acceptance-public-release.sh`
**Description:** Probe registry, tristate results, verdict table, exit codes, JSON output.
**Tests:** unit tests
**Estimated:** 2 hours

### Step 2: Installer probe

**Files:** `scripts/probes/installer.sh`
**Description:** `docker run --rm debian:bookworm-slim` with the documented one-liner; assert version equals manifest and the binary runs.
**Tests:** integration test
**Estimated:** 2 hours

### Step 3: Cargo, homebrew and updater probes

**Files:** `scripts/probes/cargo-claim.sh`, `homebrew.sh`, `updater.sh`
**Description:** crates.io claim check; formula presence and checksum parity with Linuxbrew execution where available; updater check against the channel.
**Tests:** integration tests
**Estimated:** 3 hours

### Step 4: Site coverage probe

**Files:** `scripts/probes/site-coverage.sh`
**Description:** Extract documented commands from the site and assert each maps to a probe.
**Tests:** integration test
**Estimated:** 1 hour

### Step 5: Wire into the release process

**Files:** `docs/release-operator-checklist.md`, workflow
**Description:** Run before announcement and after the release workflow.
**Dependencies:** Steps 1 to 4
**Estimated:** 1 hour

## Rollback Plan

1. The run is additive and consumes published artefacts; removing it cannot damage a release.
2. If a probe proves unreliable, it can be downgraded to not-executed with a stated reason rather than deleted.

## Migration (if applicable)

Not applicable.

## Dependencies

### New Dependencies

| Dependency | Version | Justification |
|------------|---------|---------------|
| docker | present locally | Clean-host installer probe |
| curl, python3 | present | Manifest parsing |

### Dependency Updates

| Dependency | From | To | Reason |
|------------|------|-----|--------|
| none | - | - | - |

## Performance Considerations

| Metric | Target | Measurement |
|--------|--------|-------------|
| Total runtime | < 10 minutes | timed per run |
| Installer probe runtime | < 3 minutes | timed in container |

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Whether the run gates or follows the release workflow | Pending | release owner |
| Linuxbrew host for non-macOS brew evidence | Pending | release owner |
| Site source ownership for the coverage probe | Pending | release owner |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Human approval received