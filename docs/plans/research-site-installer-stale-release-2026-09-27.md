# Research Document: Public installer path ships stale bytes (v1.21.3 instead of v1.21.16)

**Status**: Draft
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Reviewers**: pending
**Phase**: 1 (disciplined-research)

## Executive Summary

The published release channel is sound: all 20 archives on `downloads.terraphim.ai` for terraphim-agent, terraphim-grep and terraphim-cli verify byte-exact (SHA-256 and size) at 1.21.16, and the Homebrew tap formulae carry checksums that match those manifests. The entry point a user is told to use is not sound. The headline instruction on terraphim.ai, `curl -fsSL https://raw.githubusercontent.com/terraphim/terraphim-ai/main/scripts/install.sh | bash`, resolves releases against the `terraphim/terraphim-ai` repository, whose latest release is **v1.21.3 published 2026-08-16**. That installer therefore delivers bytes from an older line than the release this project just shipped. A user following the site today does not get 1.21.16.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | This is the difference between "we shipped a release" and "a user can install it"; it is the public face of the whole release train |
| Leverages strengths? | Yes | The channel, provenance, pointer and validator machinery already exists and passed; only the entry point is miswired |
| Meets real need? | Yes | terraphim.ai advertises the installer as the primary path alongside cargo/brew; today it yields v1.21.3 |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

`terraphim.ai` presents three install paths. Two of them do not deliver the released version, and one names an artefact that does not exist:

| Site instruction | What it actually resolves to | Verified |
|---|---|---|
| `curl -fsSL .../terraphim-ai/main/scripts/install.sh \| bash` | `terraphim-ai` latest release **v1.21.3** (2026-08-16), asset naming `terraphim-agent-linux-x86_64` | Yes, 2026-09-27 |
| `cargo install terraphim-agent` / `terraphim-cli` | crates.io newest **1.21.1** (2026-08-10) | Yes |
| `brew tap terraphim/terraphim && brew install terraphim-ai` | **No formula named `terraphim-ai`**; tap holds terraphim-agent, terraphim-grep, terraphim-server | Yes |

### Impact

A user who follows the primary documented instruction installs 1.21.3-era bytes from a different repository. They miss 13 patch releases of fixes, and the bug reports that follow will describe behaviour this codebase no longer has. This is the highest-impact remaining defect in the public release.

### Success Criteria

1. The documented installer, executed verbatim on a clean Linux host, installs 1.21.16 bytes whose SHA-256 matches the published manifest for the corresponding platform.
2. The installer and every other documented command fail loudly rather than silently installing an older line when 1.21.16 is unavailable.
3. No site instruction references a repository, formula or crate that cannot deliver 1.21.16.

## Current State Analysis

### Existing Implementation

Two distinct release lines exist and the site conflates them:

- **`terraphim/terraphim-clients`** (this repository): produces terraphim-agent, terraphim-grep, terraphim-cli. Tag `v1.21.16`, GitHub release published 2026-09-25T20:07:17Z with 21 assets; R2 channel objects and six stable pointers verified.
- **`terraphim/terraphim-ai`**: latest release v1.21.3 (2026-08-16, 41 assets). The public installer script lives in this repository at `scripts/install.sh` (HTTP 200 on `main`) and hardcodes `GITHUB_API_BASE="https://api.github.com/repos/terraphim/terraphim-ai"`.

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| Public installer | `terraphim/terraphim-ai` `scripts/install.sh` (main) | Resolves and downloads a release asset |
| Release producer | `terraphim-clients` `.github/workflows/release-binaries.yml` | Builds and seals the 20 archives |
| Promotion | `terraphim-clients` `scripts/promote-release.sh` | Publishes to GitHub release and R2 with stable pointers |
| Channel validator | `terraphim-clients` `scripts/validate-r2-manifests.py` | Verifies every channel object and pointer |
| Updater base URL | `terraphim-clients` `crates/terraphim_update/src/manifest.rs:186` | `DEFAULT_BASE_URL = "https://downloads.terraphim.ai"` |
| Tap formulae | `terraphim/homebrew-terraphim` `Formula/*.rb` | Agent, grep, server |

### Data Flow

Site copy -> `install.sh` -> GitHub Releases API on `terraphim-ai` -> asset named `terraphim-agent-linux-x86_64` -> `~/.local/bin`.

Intended flow -> published manifest on `downloads.terraphim.ai` (`<binary>/stable-v2.json`) -> archive `<binary>-1.21.16-<target>.tar.gz` -> verified SHA-256 -> installed binary.

The two flows share no component. The installer neither consults the manifest nor verifies a published checksum.

### Integration Points

- GitHub Releases REST API (`/repos/terraphim/terraphim-ai/releases/latest`) - returns v1.21.3.
- Asset naming: the installer builds `${tool}-${OS}-${ARCH}` (for example `terraphim-agent-linux-x86_64`) with no version component; the sealed stage ships `${tool}-${version}-${target}.tar.gz` (`terraphim-agent-1.21.16-x86_64-unknown-linux-gnu.tar.gz`) plus a Windows `.zip`.
- `downloads.terraphim.ai` is fronted by Cloudflare bot management, which 403s `Python-urllib/*`; the validator sends `terraphim-r2-manifest-validator/1.0`.
- The self-updater already targets the R2 manifest and reports correct behaviour when installed at 1.21.16.

## Constraints

### Technical Constraints

- The installer must work with `bash` and `curl` or `wget` only; it is piped to a shell and cannot assume extras.
- Windows is supported only through WSL by the current script; the channel publishes `.zip` for `x86_64-pc-windows-msvc`.
- The channel manifest is the single source of truth for version, per-platform path and SHA-256; any installer must read it rather than infer URLs.
- The script lives in `terraphim-ai`, while the artefacts live in `terraphim-clients`; a change therefore spans repositories.

### Business Constraints

- `terraphim-ai` is a public repository and the documented installer path; editing it is a public-surface change requiring the same care as the release itself.
- No new hosted infrastructure should be introduced.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Installed version via site command | 1.21.16 | v1.21.3 |
| Archive integrity check | SHA-256 against published manifest | none |
| Silent stale install | impossible | silent |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| The installer must read the published manifest | It is the only artefact that names the current version, path and hash for every platform | `stable-v2.json` verified today for all three binaries |
| The installer must verify SHA-256 before installing | Prevents a compromised or partial download from being executed | All 20 archives verified byte-exact |
| The installer must fail closed on a version mismatch | A silent older install is worse than a clear failure | v1.21.3 vs v1.21.16 today |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Rewriting the installer in Rust or shipping a compiled installer | Out of the vital few; bash+curl already works |
| Introducing a CDN or new hosting | Cloudflare R2 already serves the channel |
| Changing the archive naming scheme | It is validated by the tap, the updater and the validator |
| Rebuilding or re-promoting 1.21.16 | It is verified; this is an entry-point defect |
| Publishing terraphim-ai v1.21.16 to satisfy the old installer | Duplicates the release line and perpetuates two sources of truth |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|---|---|---|
| `stable-v2.json` schema | Installer correctness depends on `assets[target].path` and `.sha256` | Low; schema is stable and validated |
| `terraphim-clients` promotion workflow | Must keep writing the manifest the installer reads | Low |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|---|---|---|---|
| `downloads.terraphim.ai` (Cloudflare R2) | live | Cloudflare bot management may 403 default UAs | Send a descriptive User-Agent |
| GitHub Releases API | v3 | Rate limits for anonymous callers | Manifest on R2 is preferred over the API |
| `bash`, `curl`, `tar`, `sha256sum`/`shasum` | any modern | macOS lacks `sha256sum` | Fall back to `shasum -a 256` |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Site copy and installer drift again after the fix | High | High | Make the manifest the only source; add an automated check |
| Changing a public installer breaks existing users | Medium | Medium | Keep CLI flags and install directory semantics; test the matrix |
| Installer in a different repository is missed by this project's CI | High | Medium | Add a cross-repository validation job or a scheduled check that executes the documented command |
| macOS `sha256sum` absence | High on macOS | Low | Detect and use `shasum -a 256` |

### Open Questions

1. Should the installer move into `terraphim-clients`, or stay in `terraphim-ai` and be repointed? (Owner: release owner)
2. Should the installer default to `latest` from the manifest, or pin a version unless overridden? (Owner: release owner)
3. Is the site source of truth a repository that can be changed in the same pass, so the `brew install terraphim-ai` line is corrected at the same time? (Owner: release owner; requires locating the site source)

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|---|---|---|---|
| The site's installer URL is intended to serve the clients binaries | The script installs terraphim-agent and terraphim-cli | Wrong remedy chosen | Partially; the script's own help text names those tools |
| R2 `stable-v2.json` is the durable source of truth | Updater contract and pointer design | Installer targets the wrong source | Yes |
| `terraphim-ai` v1.21.3 is genuinely older, not a parallel product | Asset names and repo description | Two products conflated | Yes, by version and date |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|---|---|---|
| Repoint the existing `terraphim-ai` installer at the clients manifest | Smallest change; keeps the documented URL stable | Chosen as primary candidate |
| Move the installer into `terraphim-clients` and change the documented URL | Colocates installer with artefacts; breaks the widely quoted URL | Rejected unless the URL can be redirected |
| Publish terraphim-ai v1.21.16 with the expected asset names | Makes the old installer work untouched; creates a second release line | Rejected: two sources of truth |

## Research Findings

### Key Insights

1. The release itself is complete and verified; the failure is in discovery and entry, not in the artefacts.
2. The site instructs users to install from a different repository and a different product line.
3. The tap is correct and checksum-verified, but the documented formula name does not exist.
4. The installer never verifies a checksum, so today it cannot detect that it is serving old bytes.
5. Local environment note: `~/.cargo/bin/terraphim-agent` is a user-work artefact and must not be touched as part of this work.

### Relevant Prior Art

- `scripts/promote-release.sh` + `stable-v2.json`: the manifest already carries version, path and sha256 per platform, which is exactly what an installer needs.
- `crates/terraphim_update/src/manifest.rs`: proves a client can consume the manifest and self-update.
- `scripts/validate-r2-manifests.py`: proves channel integrity can be asserted automatically, including the Cloudflare User-Agent requirement.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Execute the documented command in a clean container and capture the installed version | Establish the failing baseline objectively | 1 hour |
| Confirm manifest schema stability across all three binaries | Guarantee the installer can rely on it | 30 minutes |
| Locate the site source for the install section | Enable the copy fix in the same pass | 1 hour |

## Recommendations

### Proceed/No-Proceed

Proceed. This is the highest-value outstanding item: the channel is verified, and one entry point contradicts it.

### Scope Recommendations

Treat "public release complete" as "the documented command installs the released version with verified integrity". Keep the existing CLI surface; change what the installer reads, not how a user invokes it.

### Risk Mitigation Recommendations

Make the installer fail closed on version or hash mismatch, and add an automated check that executes the documented command so drift is caught without a human.

## Next Steps

If approved:
1. Phase 2 design: specify manifest-driven resolution, checksum verification, failure modes, and the exact file changes.
2. Phase 2.5 specification interview: confirm installer location and default version policy.
3. Implement and validate in a clean container, then re-verify the site copy.

## Appendix

### Reference Materials

- `https://terraphim.ai/` install section
- `https://raw.githubusercontent.com/terraphim/terraphim-ai/main/scripts/install.sh`
- `https://downloads.terraphim.ai/terraphim-agent/stable-v2.json`
- `terraphim/terraphim-ai` latest release: v1.21.3, 2026-08-16

### Evidence Captured 2026-09-27

- All 20 channel archives SHA-256 and size verified against `stable-v2.json` (agent 7, grep 7, cli 6).
- Tap checksums for agent and grep match the manifest for `universal-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-musl`.
- `terraphim-ai` latest release v1.21.3; asset naming in the installer is `${tool}-${OS}-${ARCH}`; the channel uses `${tool}-${version}-${target}.tar.gz`.
- Site commands observed verbatim: `cargo install terraphim-agent`, `cargo install terraphim-cli`, `brew tap terraphim/terraphim && brew install terraphim-ai`, and the `curl ... | bash` one-liner.