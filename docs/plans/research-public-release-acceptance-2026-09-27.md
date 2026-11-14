# Research Document: Public-release acceptance run (what "100% validated" means executably)

**Status**: Draft
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Reviewers**: pending
**Phase**: 1 (disciplined-research)

## Executive Summary

The channel is verified; the entry points are not, and nothing currently asserts that a user following the published instructions gets the released version. This item defines one repeatable acceptance run that exercises every documented entry point from outside the project and fails when any of them disagrees with the published manifest. It is the mechanism that keeps the other fixes honest, and it is the difference between "we tested the artefacts" and "the release is validated".

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | It converts a verified channel into a validated release |
| Leverages strengths? | Yes | The manifest, validator and promotion machinery already exist |
| Meets real need? | Yes | Three entry-point defects existed undetected until today |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

Validation today covers the channel (all 20 archives hashed from the manifest) and the updater contract. It does not cover the commands a user is told to run. The result was three defects invisible to the existing checks: the installer served v1.21.3, cargo served 1.21.1, and the documented Homebrew formula does not exist.

### Impact

Without an executable acceptance run, every future release can regress an entry point silently, and the "validated" claim rests on a human reading the site.

### Success Criteria

1. One command runs every documented entry point and reports per-entry-point pass or fail.
2. Every entry point that can execute on Linux is executed for real against the live channel.
3. Entry points that cannot execute here (macOS-specific) are reported as not-executed with the reason, never as passed.
4. Exit code is non-zero when any executed entry point disagrees with the manifest.

## Current State Analysis

### Existing Implementation

| Check | Covered | Location |
|---|---|---|
| Channel object and pointer integrity | yes | `scripts/validate-r2-manifests.py` |
| Promotion stage provenance | yes | `scripts/validate-promotion-stage.py` |
| Release archive contents | yes | `scripts/validate-release-archive.py` |
| Updater contract against the R2 channel | partly, by hand | shipped binary `check-update` |
| Documented install commands | no | none |
| Tap formulae checksums | no | none |

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| Manifest validator | `scripts/validate-r2-manifests.py` | Channel integrity |
| Promotion | `scripts/promote-release.sh` | Publishes and writes pointers |
| Updater | `crates/terraphim_update/` | Client-side update contract |
| Operator checklist | `docs/release-operator-checklist.md` | Manual steps, partly unexecutable |

### Data Flow

`stable-v2.json` (truth) -> acceptance run -> executes each documented path -> compares observed installed version and hash against the manifest -> summary report with exit code.

### Integration Points

- Docker is available locally (v29.6.0, overlay2) and `debian:bookworm-slim` is already present, so a clean-host check is cheap.
- GitHub Actions provides macOS runners (`macos-15`, `macos-latest`) in the release workflow; there is no local macOS host.
- Homebrew cannot run on this Linux host; Linuxbrew is not installed.
- `downloads.terraphim.ai` requires a descriptive User-Agent.

## Constraints

### Technical Constraints

- The run must not depend on credentials belonging to the private repositories; it validates the public surface only.
- Clean-host checks must use a container, because the dev host has `~/.cargo/bin` artefacts that could mask a failure.
- Distinguishing "not executed" from "passed" is mandatory; a silent skip is the failure mode this item exists to prevent.

### Business Constraints

- Must be runnable by a human before announcing a release, and by CI after one.
- Must not require new paid infrastructure.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Coverage of documented entry points | 100 per cent executed or explicitly not-executed | 0 per cent |
| Runtime | under 10 minutes | n/a |
| Determinism | same result on repeated runs | n/a |

## Vital Few (Essential Constraints)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| Execute, do not inspect | The defects were invisible to inspection | Three defects found only by running commands |
| Never report a skip as a pass | A false pass is worse than no check | macOS cannot execute here |
| Compare against the manifest, not a constant | Constants rot; the manifest is generated | Manifest is authoritative |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|---|---|
| Re-validating the sealed stage | Already covered by promotion-stage validation |
| Verifying signatures | Covered by existing archive and stage validation |
| A dashboard or reporting service | Out of vital few; a script and an exit code suffice |
| Windows execution | No Windows host; report as not-executed |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|---|---|---|
| Manifest schema | The comparison basis | Low |
| Installer fix (item 1) | The installer entry point can only pass after it lands | High until fixed |
| Homebrew copy fix (item 3) | Same, for the brew entry point | High until fixed |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|---|---|---|---|
| Docker | 29.6.0 local | availability on other hosts | document as a prerequisite |
| Linuxbrew | not installed | needed for the brew entry point | mark not-executed until a host exists |
| GitHub Actions macOS runner | available in CI | cost and availability | keep macOS execution in the release workflow |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Run becomes a rubber stamp | Medium | High | Fail on disagreement; record raw transcripts |
| Container checks diverge from real user hosts | Medium | Medium | Use stock `debian:bookworm-slim` with no extra packages |
| Entry point added to the site without being added here | Medium | Medium | Treat the site copy as the input list during review |

### Open Questions

1. Should the acceptance run gate the release workflow, or run after and report? (Owner: release owner)
2. Is a Linuxbrew host acceptable for the non-macOS brew evidence, given the macOS item is separate? (Owner: release owner)
3. Who owns the site copy list so a new entry point cannot be added silently? (Owner: release owner)

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|---|---|---|---|
| The manifest is the correct comparison basis | Updater and installer both target it | Wrong standard | Yes |
| Docker is acceptable as a clean host proxy | It is the closest available substitute | Missed host-specific issue | Yes, stated as a limitation |
| No local macOS host exists | ssh probes and `command -v` | Effort estimate wrong | Yes |

## Research Findings

### Key Insights

1. Existing validation is artefact-centric; the gap is entirely entry-point-centric.
2. Three of the documented entry points are Linux-executable today, one (macOS brew) is not.
3. The run doubles as the regression test for items 1, 2 and 3.
4. Docker plus a stock image is sufficient for a clean-host installer check without new infrastructure.

### Relevant Prior Art

- `validate-r2-manifests.py` establishes the comparison-against-manifest pattern and the User-Agent requirement.
- The release operator checklist already enumerates manual steps; the run replaces the executable subset.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Run the installer in `debian:bookworm-slim` today | Capture the failing baseline | 1 hour |
| Confirm the updater check can run unattended | Include it in the run | 30 minutes |

## Recommendations

### Proceed/No-Proceed

Proceed. Without this, the other fixes cannot be shown to hold.

### Scope Recommendations

Cover exactly the documented entry points plus the updater; report everything else as not-executed with a reason.

### Risk Mitigation Recommendations

Keep raw transcripts in the release record, and make the run fail rather than warn.

## Next Steps

If approved:
1. Phase 2 design of the acceptance run script and its reporting format.
2. Execute it before and after the item 1 to 3 changes to demonstrate the delta.

## Appendix

### Evidence Captured 2026-09-27

- Documented entry points: `curl ... install.sh | bash`; `cargo install terraphim-agent`; `cargo install terraphim-cli`; `brew tap terraphim/terraphim && brew install terraphim-ai`.
- Local runtimes: docker present (29.6.0, overlay2), brew absent, no macOS host.
- Release workflow uses `ubuntu-22.04` and `macos-15` runners.
- Updater contract verified live earlier in the train: shipped 1.21.16 `check-update` reported already-current.