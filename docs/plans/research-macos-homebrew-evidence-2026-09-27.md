# Research Document: Genuine macOS evidence for the Homebrew channel

**Status**: Draft
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Reviewers**: pending
**Phase**: 1 (disciplined-research)

## Executive Summary

The release operator checklist requires genuine macOS evidence per release. None exists for any release: the only recorded install evidence is Linuxbrew. The formulae themselves are macOS-capable and include a macOS-only codesign assertion in their test block, so the gap is evidence, not capability. This item cannot be completed inside this environment: no macOS host is reachable, and Homebrew does not appear in any workflow other than the release workflow's `macos-15` runners, which build and sign but never install from the tap.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Partially | It is a verification gap, not a defect users currently hit |
| Leverages strengths? | Yes | GitHub provides `macos-15` runners already used by the release workflow |
| Meets real need? | Yes | The checklist requires it and macOS is the primary Homebrew platform |

**Proceed**: Yes (2/3); the weak answer is honest - this is evidence work rather than a user-visible fix.

## Problem Statement

### Description

Every release announces Homebrew support for macOS. The macOS-specific parts of the formulae (universal binary selection, the codesign verification in `test do`) have never been exercised on macOS in a recorded way.

### Impact

A macOS-specific breakage in the tap would be discovered by users rather than by the release process. The release currently claims more validation than it has.

### Success Criteria

1. A recorded macOS transcript showing install from the tap, version output, a real command, and upgrade to the next release.
2. The transcript is archived in the release record.
3. The Rosetta provisioning gate for the arm64 signing lane is rehearsed and recorded.

## Current State Analysis

### Existing Implementation

- `terraphim-agent.rb` and `terraphim-grep.rb` select `universal-apple-darwin` on macOS and verify SHA-256.
- `test do` runs `--version`, `learn --help`, `memory --help`, `sessions expand --help`, and, when `OS.mac?`, `codesign --verify --all-architectures --deep --strict`.
- `sign-and-notarize-macos` and `create-universal-macos` run on `macos-15` in the release workflow and produce signed, notarised bytes.
- No workflow installs from the tap on macOS; no transcript exists.

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| Agent formula | tap `Formula/terraphim-agent.rb` | macOS and Linux install plus codesign test |
| Grep formula | tap `Formula/terraphim-grep.rb` | Same shape |
| macOS build and sign | `release-binaries.yml` jobs `create-universal-macos`, `sign-and-notarize-macos` | Produce signed bytes |
| Checklist | `docs/release-operator-checklist.md` section 6 | Defines the required evidence |

### Data Flow

Tap -> formula -> `downloads.terraphim.ai` universal archive -> SHA-256 -> `bin.install` -> `brew test` codesign check.

### Integration Points

- GitHub-hosted `macos-15` runners are already in use, so a tap-install job adds no new infrastructure.
- `macos-15-intel` is the thin x86_64 lane referenced by the checklist.

## Constraints

### Technical Constraints

- `codesign --verify --all-architectures` executes only on macOS.
- Rosetta availability on arm64 runners gates the x86_64 lane; the checklist calls this a provisioning gate.
- No macOS host is reachable from this environment.

### Business Constraints

- GitHub macOS runner minutes are metered; the job should be short.
- The evidence must be per release, so the check should be automated rather than manual.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| macOS install evidence per release | recorded transcript | none |
| Runtime on a macOS runner | under 10 minutes | n/a |

## Vital Few (Essential Constraints)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| Evidence must come from a real macOS host | A Linuxbrew pass proves nothing about macOS | The formulary contains macOS-only logic |
| Evidence must be archived per release | Evidence that is not recorded did not happen | No transcript exists today |
| The check should be automated | Manual evidence will not survive future releases | The gap exists precisely because it was manual |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|---|---|
| Provisioning a permanent macOS host | Cost without need; GitHub runners already exist |
| Notarisation re-verification on the user side | Covered by the sign job |
| Intel Macs beyond the thin lane | The checklist names one lane |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|---|---|---|
| Signed universal archives | Formulae point at them | Low; the sign job already passes |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|---|---|---|---|
| GitHub `macos-15` runner | available | metered minutes, queueing | a self-hosted Mac, not present |
| Rosetta on arm64 runners | availability varies | x86_64 lane blocked | run the thin lane only on `macos-15-intel` |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Codesign test fails on real macOS | Unknown | High | That is the point; find it before users do |
| Rosetta gate blocks the Intel lane | Medium | Medium | Record as blocked on that runner, not as a pass |
| Runner cost creep | Low | Low | Keep the job to install, version and one command |

### Open Questions

1. Is a GitHub `macos-15` runner acceptable evidence, or does the checklist require a human-operated Mac? (Owner: release owner)
2. Must the thin Intel lane be exercised for this release, or only rehearsed? (Owner: release owner)

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|---|---|---|---|
| No local macOS host exists | ssh probes and `command -v` results | Effort estimate wrong | Yes |
| GitHub macOS runners are permitted | The release workflow already uses them | Blocked item | Yes |
| The formulae are macOS-capable | They contain macOS-conditional logic | Discovery of a real defect | Partially |

## Research Findings

### Key Insights

1. The gap is evidence, not formula capability.
2. Automation on existing `macos-15` runners converts a manual checklist item into a repeatable check.
3. The codesign assertion already exists in the formula; it simply has never run on macOS.
4. The Intel lane depends on Rosetta provisioning, which the checklist already identifies as a gate.

### Relevant Prior Art

- `release-binaries.yml` `create-universal-macos` and `sign-and-notarize-macos` demonstrate the existing macOS runner usage.
- The tap's `test do` block is the ready-made macOS acceptance assertion.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Trial `brew install` from the tap on a macOS runner | Prove feasibility and capture the first transcript | 2 hours |
| Rosetta check on the arm64 runner | Confirm the Intel lane can run | 1 hour |

## Recommendations

### Proceed/No-Proceed

Proceed as an automated check on an existing runner, not as a one-off manual exercise.

### Scope Recommendations

Install agent and grep, print versions, run one real command, exercise the codesign assertion, and archive the transcript in the release record.

### Risk Mitigation Recommendations

Do not report a Rosetta-blocked lane as passing; record it the same way the acceptance run handles not-executed.

## Next Steps

If approved:
1. Phase 2 design of the macOS evidence job.
2. Run it against the current release and archive the transcript.
3. Add the transcript to the release record for v1.21.16.

## Appendix

### Evidence Captured 2026-09-27

- No macOS host reachable; `bigbox` reports `Linux x86_64`.
- `command -v brew` absent locally; docker present but cannot run macOS.
- Workflows use `macos-15` and `macos-latest`; no tap-install job exists.
- Formula `test do` includes `codesign --verify --all-architectures --deep --strict` on macOS.