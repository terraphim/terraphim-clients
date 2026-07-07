# Research Document: Migrate Binary Distribution from GitHub Releases to Cloudflare R2

**Status**: Draft
**Author**: opencode (disciplined-research)
**Date**: 2026-07-07
**Repo**: `terraphim/terraphim-clients`
**Reviewers**: @AlexMikhalev

## Executive Summary

The Terraphim client binaries (`terraphim-agent`, `terraphim-grep`, `terraphim-cli`) are distributed via GitHub Releases and self-update through the `self_update` crate's GitHub backend. Autoupdate is currently broken for three independent reasons (wrong repo default, no auth-token forwarding, install-path shadowing), and GitHub's unauthenticated API rate limit (60 req/hr per IP) makes the failure mode shared-IP-hostile. `terraphim.ai` is already hosted behind Cloudflare, so serving binaries from Cloudflare R2 through a custom domain (`downloads.terraphim.ai`) eliminates rate limits, removes the need for a `GITHUB_TOKEN` secret in client binaries, and provides zero-egress-cost distribution. The existing `downloader.rs` (generic HTTP + retry) and `signature.rs` (zipsign Ed25519, storage-agnostic) make the migration low-risk.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | Removes a class of recurring autoupdate failures reported this session; unblocks offline-first UX goal |
| Leverages strengths? | Yes | `terraphim.ai` already on Cloudflare; existing generic `downloader.rs` + storage-agnostic zipsign signing already in tree |
| Meets real need? | Yes | Autoupdate currently returns 403 for every user on a shared NAT; verified live this session |

**Proceed**: Yes (3/3)

## Problem Statement

### Description
Client binaries self-update via `self_update::backends::github`, which calls `api.github.com/repos/{owner}/{repo}/releases/latest`. Three defects make this non-functional:

1. **Wrong repo default** — `terraphim_update/src/lib.rs:124` hardcodes `repo_name = "terraphim-ai"` (latest release v1.20.5). Releases actually live on `terraphim/terraphim-clients` (v1.21.9). `terraphim_agent` never overrides it.
2. **No auth-token forwarding** — `TerraphimUpdater::check_update/update()` never call `builder.auth_token()`. Only the unused `check_for_updates_auto()` helper reads `GITHUB_TOKEN`. Unauthenticated clients share a 60 req/hr/IP budget and hit `403 rate limit exceeded`.
3. **Install-path shadowing** — updater installs to `/usr/local/bin/{bin}` but `cargo install`/`rustup` copies live in `~/.cargo/bin`, which precedes `/usr/local/bin` on `$PATH`. A "successful" update is silently masked by the stale copy.

Secondary: GitHub Releases couples distribution to a GitHub token, which we do not want embedded in distributed binaries.

### Impact
- Every `terraphim-agent check-update` / `terraphim-agent update` fails for users behind a shared IP.
- The `terraphim-grep` self-update subcommand was silently regressed on `main` (exists only on unmerged tag v1.21.8).
- Version-reporting bug: v1.21.9 binaries report `1.21.0` because the release workflow tags but does not bump `CARGO_PKG_VERSION`.

### Success Criteria
1. `terraphim-agent update` and `terraphim-grep update` complete end-to-end from a clean network with **no GitHub credentials** and **no rate-limit failures**.
2. Distribution storage has **no per-request cost** and **no per-IP throttling**.
3. Existing zipsign Ed25519 signature verification continues to gate every install.
4. The release pipeline produces a single source of truth for "what is the latest version" that is independent of GitHub.
5. Rollback: `terraphim-agent` can fall back to GitHub Releases if R2 is unreachable (degradation, not hard failure).

## Current State Analysis

### Existing Implementation
The `terraphim_update` crate (`crates/terraphim_update/`, 8 modules) wraps `self_update 0.42`:

- `lib.rs` — `UpdaterConfig` + `TerraphimUpdater`. GitHub-only. Constructs `self_update::backends::github::Update`, downloads, verifies, installs.
- `downloader.rs` — **generic HTTP downloader** (`ureq`) with retry, exponential backoff, progress. Storage-agnostic. Already used by the verification flow.
- `signature.rs` — zipsign-api Ed25519 verification of `.tar.gz` (signature embedded as GZIP comment). **Fully storage-agnostic** — operates on a local file path. Embedded public key: `1uLjooBMO+HlpKeiD16WOtT3COWeC8J/o2ERmDiEMc4=`.
- `platform.rs` — install-path resolution (`/usr/local/bin` → `~/.local/bin` fallback). **Does not consider `current_exe()` parent**, causing the cargo-bin shadowing bug.
- `config.rs` / `scheduler.rs` / `notification.rs` / `rollback.rs` / `state.rs` — update scheduling, history, backup. Backend-agnostic.

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| Updater core | `crates/terraphim_update/src/lib.rs` | `TerraphimUpdater`, GitHub-bound |
| Config struct | `crates/terraphim_update/src/lib.rs:88-144` | `UpdaterConfig { repo_owner, repo_name, ... }` |
| Generic downloader | `crates/terraphim_update/src/downloader.rs` | Reusable HTTP fetch w/ retry |
| Signature verify | `crates/terraphim_update/src/signature.rs` | zipsign Ed25519 (storage-agnostic) |
| Install-path logic | `crates/terraphim_update/src/platform.rs:37-65` | `/usr/local/bin` first, broken |
| Agent CLI wiring | `crates/terraphim_agent/src/main.rs:1700-2050` | `Command::CheckUpdate`, `Command::Update`, startup check |
| Release workflow | `.github/workflows/release-binaries.yml` | Builds 6 targets, signs macOS, uploads to GH release |
| macOS signing | `scripts/sign-macos-binary.sh` | Apple notarisation (unchanged by this work) |

### Data Flow (current)
```
client binary
  -> self_update::backends::github::Update
     -> GET api.github.com/repos/terraphim/terraphim-ai/releases/latest  [403 rate limit]
     -> pick asset by target triple
     -> download github.com/.../releases/download/vX.Y.Z/<asset>.tar.gz
     -> zipsign verify (Ed25519)
     -> extract -> /usr/local/bin/<bin>   [shadowed by ~/.cargo/bin/<bin>]
```

### Integration Points
- `self_update = "0.42"` (features: archive-tar, compression-flate2, rustls, signatures).
- `ureq = "2.9"` for the generic downloader.
- Release artifacts: per-binary `.tar.gz` + raw binary, 6 targets (linux gnu/musl x86_64+aarch64, macOS x86_64/aarch64/universal, windows x86_64).
- `terraphim.ai` DNS currently resolves to Cloudflare IPs (`172.67.200.226`, `104.21.44.147`) — **Cloudflare already manages the zone**.

## Constraints

### Technical Constraints
- **Must not embed secrets in distributed binaries.** Today's `GITHUB_TOKEN` workaround is unacceptable for public clients.
- **Signature verification is mandatory.** The embedded Ed25519 public key (`1uLjooBMO+HlpKeiD16WOtT3COWeC8J/o2ERmDiEMc4=`) must continue to gate every install; no bypass paths.
- **`self_update` S3 backend URL scheme is incompatible with R2.** Verified: `backends/s3.rs:581-592` hardcodes `{bucket}.s3.{region}.amazonaws.com` / DigitalOcean / GCS only — no R2 variant. R2 uses `{account_id}.r2.cloudflarestorage.com/{bucket}`.
- **Must keep GitHub Releases as a fallback** (degradation) so a misconfigured bucket does not brick installed clients.
- **Workspace Rust edition 2024**, `rustls` TLS only (no native-tls).

### Business Constraints
- Minimise infrastructure cost. R2 egress through a Cloudflare custom domain is free; direct R2 egress is metered.
- Avoid new long-lived secrets in CI where possible; prefer short-lived or read-only public reads.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Update-check availability | >99.9%, no per-IP throttle | ~fails on shared NAT (403) |
| Update-check latency | <500ms TTFB | ~300ms GitHub (when not 403) |
| Egress cost | $0 | $0 (GH Releases free but rate-limited) |
| Client secrets required | 0 | 1 (`GITHUB_TOKEN`, not actually wired) |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| No secrets / no rate-limit in client | The whole point: autoupdate must work for every user, including shared IPs | Live 403 reproduced this session |
| Reuse existing downloader + signature code | 70% of the change surface already storage-agnostic; do not rewrite | `downloader.rs`, `signature.rs` review |
| Keep GitHub Releases fallback | Distribution resilience; do not introduce a single point of failure | Ops requirement |

### Eliminated from Scope (5/25 Rule)

| Eliminated Item | Why Eliminated |
|-----------------|---------------|
| Migrate macOS notarisation | Already works; orthogonal (`sign-macos-binary.sh`) |
| Rewrite `scheduler.rs` / `notification.rs` | Backend-agnostic already; no change needed |
| Add R2 variant to upstream `self_update` S3 backend | Upstream PR is slow; a local generic backend is simpler and avoids the S3 XML listing entirely |
| Host a full release-notes/changelog system | Out of scope; manifest carries a notes URL only |
| Auto-rollback on failed verify | `rollback.rs` already supports manual rollback; auto-trigger deferred |
| Windows code-signing | Not currently done; separate initiative |
| Bucket lifecycle / retention policy automation | Manual via Cloudflare dashboard initially |
| Delta/binary-diff updates | Over-engineering for current artifact sizes (10-45MB) |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `terraphim_agent` CLI command enum | Must keep `CheckUpdate`/`Update` subcommands stable | Low — additive change |
| `terraphim_grep` main.rs (HEAD has no update subcommand) | Decision: re-add subcommand or rely on agent only | Medium — see Open Questions |
| Release workflow `release-binaries.yml` | Must add R2 upload step | Low — additive |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `self_update` | 0.42.0 | Low — we stop using its GitHub backend; keep crate for `Release`/extract helpers | Hand-rolled if ever dropped |
| `ureq` | 2.9 | Low — already a dependency of `downloader.rs` | `reqwest` (heavier) |
| `zipsign-api` | 0.2 | None — unchanged | — |
| Cloudflare R2 (S3-compatible) | n/a | Low — well-documented S3 API; `rclone`/`aws s3 cp`/`wrangler` all work | Backblaze B2, GCS |
| `rclone` (CI upload) | any | Low — single static binary, no SDK needed | `aws s3 cp`, `wrangler r2 object put` |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| R2 public-read bucket misconfigured → 403/404 on update | Medium | High | Keep GitHub Releases fallback; health-check step in workflow |
| Manifest out of sync with uploaded artifacts | Medium | High | Workflow uploads artifacts **then** manifest atomically; manifest is single source of truth |
| Stale `~/.cargo/bin` copy shadows `/usr/local/bin` update (existing bug) | High | High | Fix `platform.rs` to prefer `current_exe()` parent; out of scope-flagged but should be fixed in same PR |
| Cloudflare custom-domain misconfiguration | Low | High | R2 → custom domain is a dashboard toggle; verify with `curl` before shipping |
| Bucket object-key version skew (prefix vs flat) | Low | Low | Use `{bin}/{bin}-{version}-{target}.tar.gz` key layout consistently |

### Open Questions

1. **Does `terraphim-grep` need its own `update` subcommand?** On `main` (v1.21.9) it does not; the v1.21.8 tag (unmerged) had it. Decision needed: re-add, or route grep updates through `terraphim-agent`. — **Owner: Alex**
2. **R2 bucket naming / Cloudflare account ID** — needed to construct URLs. — **Owner: Alex**
3. **Custom domain choice** — `downloads.terraphim.ai` (recommended) vs `releases.terraphim.ai`. — **Owner: Alex**
4. **Should the manifest be signed** (beyond the per-asset zipsign signature)? Adds defence-in-depth but another key to rotate. — Deferred unless requested.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| `terraphim.ai` Cloudflare zone can add a subdomain CNAME to R2 | Zone already on Cloudflare (DNS verified) | Must move zone; high effort | Partially — DNS verified, dashboard access unverified |
| R2 egress is free through a custom domain | Cloudflare R2 pricing docs (zero egress via worker/custom domain) | Cost surprise if wrong | No (docs-based) |
| The generic `downloader.rs` handles the artifact sizes (≤45MB) | Code streams to disk in 8KB chunks (`downloader.rs:228`) | None | Yes (code review) |
| zipsign signatures survive being copied to R2 byte-for-byte | Signatures are GZIP-comment-embedded; byte-exact copies preserve them | None | Yes (format-level) |
| `self_update`'s archive extraction can be reused standalone | Used internally by the GitHub backend | May need a thin wrapper | To verify in spike |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| **A. Generic HTTP + JSON manifest (recommended)** | Fetch `https://downloads.terraphim.ai/terraphim-agent/stable.json`, compare version, download archive URL from manifest, verify with existing zipsign, install. No S3 API exposed publicly. | **CHOSEN** — simplest, host-agnostic, no upstream coupling, reuses `downloader.rs`/`signature.rs` |
| B. Fork `self_update` S3 backend, add `EndPoint::R2` | Reuses listing logic; but exposes S3 ListObjectsV2 publicly and couples to a fork | Rejected — more moving parts, public bucket-listing surface |
| C. Stay on GitHub, just forward `GITHUB_TOKEN` and fix repo default | Minimal code change; but keeps the rate-limit/secrets problem | Rejected — does not meet success criteria 1 & 2 |
| D. Cloudflare Worker that proxies GitHub Releases | Keeps GH as source; hides rate limit behind Worker | Rejected — adds a runtime dependency; R2 is simpler and cheaper |

## Research Findings

### Key Insights
1. **70% of the updater is already storage-agnostic.** `downloader.rs`, `signature.rs`, `scheduler.rs`, `rollback.rs`, `config.rs` never touch GitHub. Only `lib.rs`'s `check_update`/`update`/`update_with_verification` and the `UpdaterConfig` repo fields are GitHub-coupled.
2. **`self_update 0.42`'s S3 backend is a dead end for R2** — hardcoded AWS URL pattern, no `EndPoint::R2`, relies on public ListObjectsV2. A manifest-based generic backend is strictly simpler.
3. **A 3-field JSON manifest per binary is the entire "latest version" source of truth**, decoupling version discovery from GitHub. Example:
   ```json
   {
     "version": "1.21.9",
     "released_at": "2026-07-06T17:38:00Z",
     "assets": {
       "x86_64-unknown-linux-gnu":   "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-gnu.tar.gz",
       "aarch64-unknown-linux-musl": "terraphim-agent/terraphim-agent-1.21.9-aarch64-unknown-linux-musl.tar.gz"
     },
     "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
   }
   ```
4. **R2 free egress requires a custom domain** (Cloudflare toggle), not the direct `*.r2.cloudflarestorage.com` URL. This is a one-time dashboard config.
5. **The install-path shadowing bug is independent but should be fixed in the same PR** — otherwise a "successful" R2 update is still invisible to `cargo install` users.

### Relevant Prior Art
- **rustup / cargo-dist** — rustup uses a static `release-stable.toml` on `static.rust-lang.org`; same manifest pattern. Proves the approach scales.
- **gh-cli** — self-updates from GitHub Releases with token; opposite of our goal.
- **Cloudflare R2 docs** — S3-compatible API at `{account}.r2.cloudflarestorage.com`; custom domain via Cloudflare dashboard; egress free when served through Cloudflare.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Create R2 bucket + custom domain, upload one test artifact, `curl` it | Verify egress + URL scheme + no auth | 30 min |
| Prototype manifest fetch+parse in `terraphim_update` | Validate `serde` structs + ureq GET | 1 hour |
| Confirm `self_update` extraction helpers are callable standalone | Decide reuse vs. small extract fn | 1 hour |

## Recommendations

### Proceed/No-Proceed
**PROCEED.** All success criteria are achievable with Interpretation A (generic HTTP + JSON manifest) at low risk, reusing the majority of existing code.

### Scope Recommendations
- **In:** generic manifest backend, R2 upload step in workflow, fix `UpdaterConfig` repo default, fix `platform.rs` install-path shadowing, fix the version-bump-in-release-pipeline issue, GitHub Releases fallback.
- **Gated on Q1:** re-adding `terraphim-grep` update subcommand (decide vs. agent-only).
- **Out:** everything in the Eliminated table.

### Risk Mitigation Recommendations
- Ship behind a config flag `update_backend = "r2" | "github"` (default `r2`, fallback `github`) so a bad manifest cannot brick clients.
- Workflow step order: upload artifacts → verify each is fetchable → publish manifest (atomic single PUT). Never publish a manifest pointing at missing assets.
- Add a `terraphim-agent update --check-only` health probe CI job that hits the live manifest.

## Next Steps

If approved:
1. Resolve Open Questions 1-3 (grep subcommand, bucket name, custom domain).
2. Run the three technical spikes (≤3 hours total).
3. Proceed to Phase 2: Implementation Plan (companion document).

## Appendix

### Reference Materials
- `self_update` 0.42 S3 backend: `~/.cargo/registry/src/.../self_update-0.42.0/src/backends/s3.rs` (URL hardcoding at L581-592)
- Cloudflare R2: `https://developers.cloudflare.com/r2/`
- rustup distribution: `https://static.rust-lang.org/dist/channel-rust-stable.toml`

### Code Snippets — current GitHub coupling (the code to replace)

`crates/terraphim_update/src/lib.rs:120-128` — wrong default + GitHub-only:
```rust
pub fn new(bin_name: impl Into<String>) -> Self {
    Self {
        bin_name: bin_name.into(),
        repo_owner: "terraphim".to_string(),
        repo_name: "terraphim-ai".to_string(),   // WRONG repo; should be terraphim-clients
        current_version: cargo_crate_version!().to_string(),
        show_progress: true,
    }
}
```

`crates/terraphim_update/src/lib.rs:178-185` — no auth token forwarded:
```rust
let mut builder = self_update::backends::github::Update::configure();
builder.repo_owner(&repo_owner);
builder.repo_name(&repo_name);
builder.bin_name(&bin_name_for_asset);
builder.current_version(&current_version);
builder.show_download_progress(show_progress);
// <-- builder.auth_token(...) NEVER called => 403 on shared IP
```

`crates/terraphim_update/src/platform.rs:42-51` — install-path shadowing:
```rust
let system_path = format!("/usr/local/bin/{}", binary_name);
// ... returns /usr/local/bin even when current_exe lives in ~/.cargo/bin
// => updated binary is shadowed by the stale cargo copy on $PATH
```
