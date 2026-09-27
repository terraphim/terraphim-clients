# Research Document: Optional native channels (DEB/RPM and AUR)

**Status**: Draft
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Reviewers**: pending
**Phase**: 1 (disciplined-research)

## Executive Summary

The release covers curl, cargo (stale, item 2), Homebrew (copy broken, item 3) and the self-updater. It does not cover native distribution: no DEB or RPM packages are published, and Arch users have no AUR package. Both were deferred for external reasons - DEB/RPM waits on central signing in the terraphim-ai repository, and the AUR submission needs `ssh://aur@aur.archlinux.org` credentials that are not available. Neither blocks the public release being correct for the channels it already claims; both extend reach. This item therefore plans them, states the blockers precisely, and does not pretend they are closeable here.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Partially | Reach expansion rather than defect repair |
| Leverages strengths? | Yes | The sealed stage already produces per-target archives with receipts |
| Meets real need? | Yes for Arch and enterprise Linux users | Two of the three documented OS families lack native packages |

**Proceed**: Yes (2/3). Recommended sequencing is after the entry-point defects, which affect every user.

## Problem Statement

### Description

| Channel | State | Blocker |
|---|---|---|
| DEB/RPM | Not published | Awaits central signing in the terraphim-ai repository |
| AUR | Not published | No `aur@aur.archlinux.org` SSH key or account access available |
| Omarchy | Not published | Consumes the AUR package; not on any critical path |

### Impact

Debian, Ubuntu, Fedora and RHEL users install through curl or Homebrew rather than their native package manager, and Arch users have no packaged option. The impact is reach, not correctness.

### Success Criteria

1. Native packages install, upgrade and remove cleanly on the documented matrices, owning only their binary, licence and receipt.
2. Receipts record `dpkg` or `rpm` under the documented path.
3. AUR submission regenerates `.SRCINFO` and pushes to `master` with the PKGBUILD.

## Current State Analysis

### Existing Implementation

- The sealed stage produces per-target archives with SHA-256 sidecars, receipts and signatures.
- `nFPM` packaging work exists in the build pipeline but publication waits on external signing.
- `crates/terraphim_grep/RELEASE_RESEARCH.md` and `RELEASE_DESIGN.md` contain prior analysis of packaging for grep, including the dependency-ordered publish pattern.

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| Package build | release workflow `build-client-packages` | Produces DEB/RPM artefacts |
| Sealed stage | `client-release-stage-*` | Archives, SHA256SUMS, receipts |
| Prior packaging research | `crates/terraphim_grep/RELEASE_RESEARCH.md` | Established packaging context |

### Data Flow

Sealed archives -> nFPM -> DEB/RPM -> repository publication (blocked) -> package manager install.

AUR: PKGBUILD sources the published archives -> `.SRCINFO` -> push to `master` (blocked on credentials).

### Integration Points

- Native package managers (`dpkg`, `rpm`), their lint tools, and chroot-based build environments.
- `ssh://aur@aur.archlinux.org/terraphim-clients-bin.git` for submission.
- The central signing contract in the terraphim-ai repository.

## Constraints

### Technical Constraints

- Each package must own only its binary, licence and receipt; MUSL packages declare no glibc or gcc runtime dependency.
- Split PKGBUILD outputs (`terraphim-agent-bin`, `terraphim-grep-bin`) must declare matching `provides` and `conflicts`.
- Reproducibility and exact payload SHA equivalence must hold against the sealed archives.

### Business Constraints

- Publication waits on external signing and external credentials; both are outside this repository.
- Package publication is user-visible and less reversible than a binary download.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Install, upgrade, remove on the documented matrices | clean | not published |
| Reproducible payload hashes | exact | not published |

## Vital Few (Essential Constraints)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| Package owns only its own files | Prevents conflicts with existing installs | Stated contract in the tracking issues |
| Payload hashes equal the sealed archives | Guarantees the packaged binary is the released binary | Reproducibility requirement |
| Blocker ownership must be explicit | Prevents this item from silently stalling the release | Two external blockers exist |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|---|---|
| Publishing to a distro's official repositories | Separate, lengthy review process |
| Building a full APT/YUM repository service | Infrastructure beyond this release |
| Windows package managers | Not requested and no host to validate |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|---|---|---|
| Sealed stage receipts | Package payload basis | Low |
| AUR PKGBUILD | Depends on published archive URLs | Low |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|---|---|---|---|
| Central signing (terraphim-ai) | pending | blocks DEB/RPM publication | none |
| AUR account and SSH key | absent | blocks AUR submission | none |
| nFPM | pinned | packaging drift | pin by digest |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Package conflicts with a curl-installed binary | Medium | Medium | Document coexistence; packages own `/usr/bin` only |
| AUR package drifts from the release | Medium | Medium | Generate `.SRCINFO` from the same manifest |
| Blockers persist indefinitely | Medium | Low (reach only) | State them in the release record rather than implying readiness |

### Open Questions

1. What is the current status of central signing, and who owns it? (Owner: terraphim-ai maintainer)
2. Can AUR credentials be provisioned into 1Password for automated submission? (Owner: release owner)
3. Is Omarchy in scope for this release or the next? (Owner: release owner)

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|---|---|---|---|
| DEB/RPM publication genuinely waits on central signing | Recorded in the tracking issues | Effort misdirected | Partially |
| No AUR credentials are available | No AUR key present in the environment | Unnecessary deferral | Yes |
| Neither channel blocks user-facing correctness | Their binaries are also reachable via curl and Homebrew | Priority misjudged | Yes |

## Research Findings

### Key Insights

1. Both channels extend reach; neither repairs a current defect.
2. Both have hard external dependencies, so planning must state them rather than absorb them.
3. The packaging contract is already written down in the tracking issues, which reduces design risk.

### Relevant Prior Art

- `crates/terraphim_grep/RELEASE_RESEARCH.md` and `RELEASE_DESIGN.md` for the packaging context and the dependency-ordered publish pattern.
- The release operator checklist's packaging section.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Dry-run nFPM on the sealed archives | Confirm package contents and receipts | 4 hours |
| PKGBUILD build in a clean chroot | Confirm split output ownership | 4 hours |

## Recommendations

### Proceed/No-Proceed

Proceed with design and dry-run validation; do not publish until the blockers clear.

### Scope Recommendations

Keep DEB/RPM and AUR as one research item with two designs, because they share the sealed stage and the receipt contract. Treat Omarchy as a downstream consumer of the AUR package.

### Risk Mitigation Recommendations

Record the blockers in the release record with named owners, so the public release is not described as incomplete for reasons nobody can act on.

## Next Steps

If approved:
1. Phase 2 designs for the DEB/RPM and AUR paths.
2. Dry-run validation that changes no public surface.
3. Re-assess once central signing and AUR credentials are available.

## Appendix

### Evidence Captured 2026-09-27

- No AUR-specific SSH key in the environment; no Arch tooling present.
- The release workflow contains `build-client-packages`, whose publication path waits on external signing.
- Receipt contract: `dpkg` or `rpm` recorded under `/usr/share/terraphim/package-manager.d/<binary>`.