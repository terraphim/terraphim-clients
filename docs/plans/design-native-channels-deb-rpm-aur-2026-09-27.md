# Implementation Plan: Native channels - DEB/RPM packaging and AUR submission

**Status**: Draft
**Research Doc**: `docs/plans/research-native-channels-deb-rpm-aur-2026-09-27.md`
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Phase**: 2 (disciplined-design)
**Estimated Effort**: 2 days of dry-run work; publication blocked externally

## Overview

### Summary

Produce and dry-run-validate native DEB/RPM packages and an AUR split PKGBUILD from the sealed stage, without publishing, until central signing and AUR credentials are available.

### Approach

Build both from the same sealed archives and receipts, so payload hashes are provably identical to the released bytes. Publication is a separate, explicitly blocked step.

### Scope

**In Scope:**
- nFPM packaging for terraphim-agent and terraphim-grep on x86_64 and aarch64 MUSL.
- Split PKGBUILD `terraphim-clients-bin` producing `terraphim-agent-bin` and `terraphim-grep-bin`.
- Receipts under `/usr/share/terraphim/package-manager.d/<binary>`.
- Dry-run validation: contents, ownership, lint, reproducibility, payload hashes.

**Out of Scope:**
- Repository publication, which waits on central signing.
- AUR submission, which waits on credentials.
- Omarchy package generation, which consumes the AUR output.

**Avoid At All Cost** (from 5/25 analysis):

| Rejected | Why |
|---|---|
| Publishing before the signing contract lands | Produces unsigned packages that later need replacement |
| A hand-maintained package definition separate from the sealed stage | Guarantees hash drift |
| Building an APT/YUM repository service now | Infrastructure beyond the release |
| Bundling both binaries into one package | Violates the ownership contract |

## Architecture

### Component Diagram

```
sealed stage (archives + SHA256SUMS + receipts)
        |
        +--> nFPM --> terraphim-agent_1.21.16_amd64.deb / .rpm
        |             terraphim-grep_1.21.16_amd64.deb  / .rpm
        |             (x86_64 and aarch64 MUSL payloads)
        |                                  |
        |                         publication BLOCKED on central signing
        |
        +--> PKGBUILD terraphim-clients-bin --> terraphim-agent-bin, terraphim-grep-bin
                                             --> .SRCINFO
                                             --> submission BLOCKED on AUR credentials
```

### Data Flow

Archive -> package payload -> package -> install in a clean matrix -> receipt written -> upgrade -> remove/purge -> receipt removed.

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|---|---|---|
| Package payloads are the sealed archives verbatim | Guarantees payload-hash equality | Rebuilding from source at package time |
| One package per binary | Ownership contract, no conflicts | A bundle package |
| Receipts under a fixed documented path | Machine-readable channel provenance | Documentation only |
| Dry-run until blockers clear | Avoids publishing artefacts that need replacement | Publish then fix |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|---|---|---|
| AppStream metadata and desktop entries | The binaries are CLIs | Unnecessary surface |
| systemd units | No service is packaged here | Wrong scope |
| Multi-distro repository signing keys managed here | Owned by the central signing contract | Duplicated trust roots |

### Simplicity Check

**What if this could be easy?** Use the pinned packager, feed it the sealed archives, and assert the payload hash equals the archive hash. Everything else is a clean-matrix install test.

**Senior Engineer Test**: Yes; the temptation to build repository infrastructure is explicitly rejected.

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
| `scripts/build-native-packages.sh` | nFPM packaging from the sealed stage |
| `packaging/terraphim-clients-bin/PKGBUILD` | AUR split package |
| `scripts/validate-native-packages.sh` | Dry-run matrix validation |

### Modified Files

| File | Changes |
|------|---------|
| `docs/release-operator-checklist.md` | Packaging section links the validation script and records the blockers |

### Deleted Files

| File | Reason |
|------|--------|
| none | - |

## API Design

```
scripts/build-native-packages.sh --stage DIR --out DIR [--arch amd64|arm64]
scripts/validate-native-packages.sh --packages DIR
  exit 0  all matrix checks passed
  exit 1  a check failed
  exit 2  a matrix entry could not execute (reported)
```

Package contract:

```
/usr/bin/terraphim-agent
/usr/share/doc/terraphim-agent/license
/usr/share/terraphim/package-manager.d/terraphim-agent   (content: dpkg | rpm)
```

## Test Strategy

### Unit Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_payload_hash_equals_archive` | `scripts/validate-native-packages.sh` | The packaged binary is the released binary |
| `test_no_glibc_dependency_musl` | same | MUSL packages declare no glibc or gcc dependency |

### Integration Tests

| Test | Location | Purpose |
|------|----------|---------|
| `test_deb_matrix` | clean Debian/Ubuntu containers | Inspect, install, ownership, version, upgrade, remove, purge, receipt cleanup |
| `test_rpm_matrix` | clean Fedora/RHEL-compatible containers | Same checks |
| `test_arch_split_ownership` | clean Arch chroot | Both split outputs, ownership, `.SRCINFO` drift, `namcap` |
| `test_omarchy_sync` | Omarchy checkout | `bin/sync-upstream` validates both architectures |

No mocks: real package managers in real clean containers or chroots.

### Property Tests

Not applicable.

## Implementation Steps

### Step 1: nFPM packaging

**Files:** `scripts/build-native-packages.sh`
**Description:** Consume the sealed stage, produce per-binary DEB and RPM for both architectures with receipts.
**Tests:** payload hash equality
**Estimated:** 4 hours

### Step 2: Package validation matrix

**Files:** `scripts/validate-native-packages.sh`
**Description:** Clean-matrix install, upgrade, remove, purge, receipt cleanup, architecture metadata, reproducibility.
**Tests:** integration tests
**Dependencies:** Step 1
**Estimated:** 6 hours

### Step 3: AUR split PKGBUILD

**Files:** `packaging/terraphim-clients-bin/PKGBUILD`
**Description:** Split outputs, architecture-specific source aliases, SHA-256 arrays, matching `provides` and `conflicts`, `.SRCINFO` generation.
**Tests:** chroot build, `namcap`
**Estimated:** 6 hours

### Step 4: Record blockers and evidence

**Files:** release record
**Description:** Record dry-run results and the named blockers with owners.
**Dependencies:** Steps 1 to 3
**Estimated:** 1 hour

## Rollback Plan

1. Nothing is published by this plan, so rollback is deleting the scripts and the PKGBUILD.
2. If packaging is later published, rollback is yanking the release, which is why publication waits for signing.

## Migration (if applicable)

Existing curl-installed binaries at the same path are not migrated; the packages own `/usr/bin` and installation is documented as requiring the curl install to be removed first where both would write the same path.

## Dependencies

### New Dependencies

| Dependency | Version | Justification |
|------------|---------|---------------|
| nFPM | pinned by digest | Reproducible package construction |
| namcap | distribution-provided | AUR lint |

### Dependency Updates

| Dependency | From | To | Reason |
|------------|------|-----|--------|
| none | - | - | - |

## Performance Considerations

Not applicable; packaging is a build-time concern.

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Central signing status (blocks DEB/RPM publication) | Blocked | terraphim-ai maintainer |
| AUR account and SSH key (blocks submission) | Blocked | release owner |
| Whether Omarchy is in this release | Pending | release owner |

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Human approval received