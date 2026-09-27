# Implementation Plan: Correct the documented Homebrew command

**Status**: Draft
**Research Doc**: `docs/plans/research-homebrew-documented-command-2026-09-27.md`
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Phase**: 2 (disciplined-design)
**Estimated Effort**: 3 hours

## Overview

### Summary

Change the documented Homebrew command to the real tap and real formula names, and decide explicitly whether terraphim-cli also ships through the tap.

### Approach

Copy correction plus, if the cli is in scope, one new formula following the existing pattern exactly. No change to how the existing formulae work.

### Scope

**In Scope:**
- Correct the tap and formula names in the documented command.
- Add a `terraphim-cli` formula if the site continues to advertise a cli install.
- Add the Homebrew command to the public-release acceptance run.

**Out of Scope:**
- Renaming the tap or moving formulae to homebrew-core.
- Fixing `terraphim-server.rb`'s terraphim-ai v1.20.5 pin.
- macOS execution evidence, which is tracked separately.

**Avoid At All Cost** (from 5/25 analysis):

| Rejected | Why |
|---|---|
| Inventing a `terraphim-ai` formula to match the wrong copy | Creates a product-name formula nobody else uses |
| Renaming the tap to `homebrew-terraphim-ai` | Breaks existing users for a naming preference |
| Duplicating the formula pattern into a shared library | Three formulae do not justify abstraction |

## Architecture

### Component Diagram

```
site copy: brew tap terraphim/terraphim && brew install terraphim-agent
     |                    |
     |                    +-> tap repository terraphim/homebrew-terraphim
     |                              Formula/terraphim-agent.rb   (v1.21.16)
     |                              Formula/terraphim-grep.rb    (v1.21.16)
     |                              Formula/terraphim-cli.rb     (new, if in scope)
     +----------------------------> downloads.terraphim.ai (primary) + GitHub release (mirror)
```

### Data Flow

`brew install <formula>` -> formula url resolved per OS/arch -> archive downloaded -> SHA-256 compared -> binary installed -> formula `test do` block executed by `brew test`.

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| Copy follows the tap, not the reverse | The tap has users and merge history; the copy is one line | Renaming the tap |
| New cli formula copies the existing pattern exactly | Proven shape, no new concepts | A shared helper |
| Acceptance run executes the documented command | Prevents copy drift | Manual review |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| A meta-formula installing all binaries | Brew formulae install one thing well | Ownership and conflict complexity |
| Serving the tap from a separate repository | Adds a hop for no benefit | Synchronisation burden |
| Adding a `livecheck` block for every formula now | Not needed for correctness | Extra moving parts |

### Simplicity Check

**What if this could be easy?** It is: correct one line, and if the cli is in scope, add one formula that is a near-copy of an existing one with different names and hashes.

**Senior Engineer Test**: Yes; anything more would be over-engineering a copy fix.

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
| `Formula/terraphim-cli.rb` (tap) | Install terraphim-cli 1.21.16, if in scope |

### Modified Files

| File | Changes |
|------|---------|
| Site install section | `brew tap terraphim/terraphim && brew install terraphim-agent` (or the agreed set) |
| `scripts/check-documented-versions.py` (from the crates item) | Also assert the Homebrew command's formula names exist |
| Release acceptance run | Add the Homebrew command |

### Deleted Files

| File | Reason |
|------|--------|
| none | - |

## API Design

No programmatic interface. The contract is the documented command string and the formula names.

```
$ brew tap terraphim/terraphim
$ brew install terraphim-agent
$ terraphim-agent --version
terraphim-agent 1.21.16
```

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| Formula Ruby syntax | `ruby -c Formula/*.rb` | Formulae parse |
| `brew audit --strict --formula` | Linuxbrew | Catch formula smells |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| Install agent from the tap on Linuxbrew | acceptance run | Real install, real checksum |
| Install grep from the tap on Linuxbrew | acceptance run | Real install, real checksum |
| Run the documented command verbatim | acceptance run | Copy and tap agree |
| Formula downloads match the manifest | acceptance run | No checksum drift |

No mocks: Homebrew is exercised for real on a Linuxbrew host; macOS execution is a separately tracked item.

### Property Tests

Not applicable.

## Implementation Steps

### Step 1: Decide the installable set

**Files:** none (decision)
**Description:** Confirm whether the documented Homebrew path installs agent only, agent plus grep, or agent plus cli.
**Estimated:** 15 minutes

### Step 2: Copy correction

**Files:** site install section
**Description:** Replace the tap and formula names with the real ones.
**Tests:** documented command run in the acceptance run
**Estimated:** 30 minutes

### Step 3: Optional cli formula

**Files:** `Formula/terraphim-cli.rb`
**Description:** Mirror `terraphim-agent.rb` with cli names, target selection and manifest checksums.
**Tests:** `ruby -c`, `brew audit`, install on Linuxbrew
**Dependencies:** Step 1
**Estimated:** 1.5 hours

### Step 4: Acceptance wiring

**Files:** acceptance run definition
**Description:** Execute the documented command and compare the installed version with the manifest.
**Dependencies:** Steps 2 and 3
**Estimated:** 1 hour

## Rollback Plan

1. Revert the copy commit; the previous (broken) command returns, which is no regression.
2. For a new formula, delete the file; no user depends on it yet.
3. Existing formulae are untouched.

## Migration (if applicable)

Not applicable.

## Dependencies

### New Dependencies

| Dependency | Version | Justification |
|------------|---------|---------------|
| none | - | Formulae use existing channels |

### Dependency Updates

| Dependency | From | To | Reason |
|------------|------|-----|--------|
| none | - | - | - |

## Performance Considerations

Not applicable.

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Which binaries the Homebrew path advertises | Pending | release owner |
| Site source location | Pending | release owner |
| Whether `terraphim-cli` gets a formula | Pending | release owner |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Human approval received