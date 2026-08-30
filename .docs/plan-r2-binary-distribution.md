# Implementation Plan: R2 Binary Distribution with Generic Manifest Backend

**Status**: Draft
**Research Doc**: `.docs/research-r2-binary-distribution.md`
**Author**: opencode (disciplined-design)
**Date**: 2026-07-07
**Repo**: `terraphim/terraphim-clients`
**Estimated Effort**: 2-3 days (implementation) + 0.5 day (infra setup)

## Overview

### Summary
Replace the `self_update` GitHub backend with a storage-agnostic **manifest backend** that fetches a tiny per-binary JSON file from Cloudflare R2 (served via `downloads.terraphim.ai`), downloads the matching archive, verifies the existing zipsign Ed25519 signature, and installs to the *currently running* binary's location. Add an R2 upload step to the release workflow. Keep GitHub Releases as an automatic fallback.

### Approach
Interpretation **A** from the research doc: generic HTTP + JSON manifest. No fork of `self_update`, no public S3 ListObjectsV2, no embedded secrets.

### Scope

**In Scope:**
- New `manifest` module in `terraphim_update` (R2/HTTP backend)
- Backend selector (`r2` default, `github` fallback) in `UpdaterConfig`
- Fix `UpdaterConfig` default repo + add `auth_token` plumbing for the GitHub fallback
- Fix `platform.rs` install-path shadowing (prefer `current_exe()` parent)
- New `r2.rs` workflow step (upload artifacts + manifest via `rclone`)
- Release-pipeline version-bump fix (so binaries report the tag version)
- GitHub Releases fallback wiring in `TerraphimUpdater`
- Manifest schema + signing-of-asset (reuse zipsign; manifest itself unsigned initially)

**Out of Scope:**
- macOS notarisation changes, Windows code-signing, delta updates, scheduler/notification rewrites, auto-rollback trigger, manifest signing (deferred), grep subcommand re-add (gated on Open Question 1)

**Avoid At All Cost** (5/25 anti-list):
- Forking `self_update` to add an R2 `EndPoint`
- Exposing the S3 ListObjectsV2 API publicly
- Embedding any R2 access key in distributed binaries
- A runtime Cloudflare Worker dependency (static hosting only)
- Rewriting the scheduler/rollback/notification modules

## Architecture

### Component Diagram
```
                        ┌───────────────────────────┐
   client (terraphim-   │  TerraphimUpdater         │
   agent / grep / cli)  │   ├─ backend = "r2" ──────┼─► manifest::fetch_latest()
                        │   │                        │     GET downloads.terraphim.ai/<bin>/stable.json
                        │   │                        │     ▼
                        │   │                        │   compare semver → decide
                        │   │                        │     ▼
                        │   │                        │   downloader::download_with_retry(asset_url)
                        │   │                        │     ▼
                        │   │                        │   signature::verify_archive_signature()  [zipsign Ed25519]
                        │   │                        │     ▼
                        │   │                        │   install → current_exe().parent()/<bin>
                        │   │                        │
                        │   └─ backend = "github" ──┼─► (existing path, fallback only)
                        └───────────────────────────┘

  Release pipeline:
    build (6 targets) → sign macOS → upload artifacts to R2 → write stable.json → (also) GH release
                                                              ▲
                                              Cloudflare R2 bucket "terraphim-releases"
                                              served via downloads.terraphim.ai (free egress)
```

### Data Flow
```
[terraphim-agent update]
  → manifest::fetch_latest("terraphim-agent")
     → GET https://downloads.terraphim.ai/terraphim-agent/stable.json   (public, no auth)
     → parse { version, assets[target], notes_url }
  → semver compare(current, manifest.version)
  → if newer:
     → resolve asset URL for current target triple
     → downloader::download_with_retry(url, tmpfile)        [existing, unchanged]
     → signature::verify_archive_signature(tmpfile)          [existing, unchanged]
     → install_verified_archive(tmpfile, bin)                [existing, unchanged]
        → install dir = current_exe().parent()                [FIXED, was hardcoded /usr/local/bin]
  → on R2 failure (network/404/parse): fall back to github backend
```

### Key Design Decisions

| Decision | Rationale | Alternatives Rejected |
|----------|-----------|----------------------|
| JSON manifest per binary (not S3 XML listing) | Decouples version discovery from S3 API; smallest possible payload; trivially cacheable by Cloudflare | Forking self_update S3 backend (couples to fork, exposes ListObjectsV2) |
| R2 served via custom domain `downloads.terraphim.ai` | Free egress; stable URL independent of account ID | Direct `*.r2.cloudflarestorage.com` URL (metered egress, leaks account ID) |
| `rclone` for CI upload | Single static binary, supports R2 via S3 API, idempotent `copyto` | `wrangler` (Node dep), `aws s3` (heavier) |
| Backend selector env var + config | Lets a broken R2 deploy be bypassed without a binary rebuild | Hard cutover (risky) |
| Install to `current_exe()` parent | Fixes the cargo-bin shadowing bug; correct for every install method | Keep `/usr/local/bin` first (the bug) |
| Publish manifest LAST, atomically | A published manifest never points at a missing asset | Publish manifest first (race window) |
| Reuse `downloader.rs` + `signature.rs` unmodified | They are already storage-agnostic | Rewriting them (waste, risk) |

### Eliminated Options (Essentialism)

| Option Rejected | Why Rejected | Risk of Including |
|-----------------|--------------|-------------------|
| Fork `self_update`, add `EndPoint::R2` | Upstream PR latency; not simpler than a manifest | Maintenance fork burden |
| Public S3 ListObjectsV2 for version discovery | Exposes bucket layout; heavier XML parse | Enumeration surface, parsing fragility |
| Cloudflare Worker proxy in front of GitHub | Runtime dependency; cost; complexity | Extra failure mode |
| Manifest-level Ed25519 signing | Defence-in-depth but a second key to rotate now | Scope creep; per-asset zipsign already gates integrity |
| Auto-rollback on failed verify | `rollback.rs` already supports manual; trigger logic is its own design | Scope creep |

### Simplicity Check
> "Minimum code that solves the problem. Nothing speculative."

**What if this could be easy?** It is: one new ~150-line module (`manifest.rs`), one new workflow step (~40 lines YAML), three small fixes to existing files, one `rclone` config. The existing `downloader.rs` and `signature.rs` do the heavy lifting unchanged.

**Senior Engineer Test:** Would a senior engineer call this overcomplicated? No — it removes a dependency (GitHub API in the client) and a secret (`GITHUB_TOKEN`), and the new code is smaller than what it replaces.

**Nothing Speculative Checklist:**
- [x] No features the user didn't request
- [x] No abstractions "in case we need them later" (backend enum has exactly 2 variants: r2, github)
- [x] No flexibility "just in case" (manifest schema is fixed, not extensible)
- [x] No error handling for impossible scenarios
- [x] No premature optimization

## File Changes

### New Files
| File | Purpose |
|------|---------|
| `crates/terraphim_update/src/manifest.rs` | R2/HTTP manifest backend: fetch, parse, version compare, asset resolution |
| `crates/terraphim_update/tests/manifest.rs` | Integration tests for manifest fetch+parse (uses a local HTTP fixture) |
| `.github/workflows/upload-r2.yml` | Reusable workflow: upload artifacts + manifest to R2 via `rclone` |
| `scripts/r2-rclone.conf.template` | Template for CI rclone config (filled from secrets at runtime) |
| `scripts/build-manifest.sh` | Generate `stable.json` for a binary from a list of uploaded assets |

### Modified Files
| File | Changes |
|------|---------|
| `crates/terraphim_update/src/lib.rs` | Add `UpdateBackend` enum to `UpdaterConfig`; branch `check_update`/`update`/`check_and_update` by backend; fix default `repo_name` to `terraphim-clients`; forward `auth_token` in github fallback; declare `pub mod manifest;` |
| `crates/terraphim_update/src/platform.rs` | `get_binary_path`: prefer `current_exe()` parent over hardcoded `/usr/local/bin` |
| `crates/terraphim_update/Cargo.toml` | Add `serde` (already transitive) + `ureq` (already present) to direct deps; bump patch version |
| `crates/terraphim_agent/src/main.rs` | Set `UpdaterConfig.backend = UpdateBackend::R2` (default); no other change |
| `.github/workflows/release-binaries.yml` | After GH upload, call `upload-r2.yml`; bump `version.workspace` before building (fix version-report bug) |
| `Cargo.toml` (workspace) | Bump patch for the release that ships the new backend |

### Deleted Files
None.

## API Design

### Public Types (`crates/terraphim_update/src/manifest.rs`)

```rust
use serde::{Deserialize, Serialize};

/// Which distribution backend to use for update checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateBackend {
    /// Cloudflare R2 via JSON manifest on a custom domain (default, no secrets).
    #[default]
    R2,
    /// GitHub Releases (fallback; requires GITHUB_TOKEN to avoid rate limits).
    GitHub,
}

/// The per-binary release manifest served at `<base_url>/<bin>/stable.json`.
///
/// Example:
/// ```json
/// {
///   "version": "1.21.9",
///   "released_at": "2026-07-06T17:38:00Z",
///   "assets": {
///     "x86_64-unknown-linux-gnu":   "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-gnu.tar.gz",
///     "aarch64-unknown-linux-musl": "terraphim-agent/terraphim-agent-1.21.9-aarch64-unknown-linux-musl.tar.gz"
///   },
///   "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
    /// Latest semantic version (no leading 'v').
    pub version: String,
    /// ISO-8601 release timestamp.
    pub released_at: String,
    /// Map of Rust target triple -> asset key (relative to base_url).
    pub assets: std::collections::HashMap<String, String>,
    /// Optional human-readable release notes URL.
    #[serde(default)]
    pub notes_url: Option<String>,
}

/// Configuration for the R2 manifest backend.
#[derive(Debug, Clone)]
pub struct ManifestConfig {
    /// Base URL serving the bucket, e.g. `https://downloads.terraphim.ai`.
    pub base_url: String,
    /// Binary name, e.g. `terraphim-agent`.
    pub bin_name: String,
    /// Override the manifest filename (default `stable.json`).
    pub manifest_name: String,
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            base_url: "https://downloads.terraphim.ai".to_string(),
            bin_name: String::new(),
            manifest_name: "stable.json".to_string(),
        }
    }
}
```

### Public Functions

```rust
impl ManifestConfig {
    /// Construct the full manifest URL.
    pub fn manifest_url(&self) -> String;

    /// Construct the full asset URL for a relative asset key.
    pub fn asset_url(&self, asset_key: &str) -> String;
}

/// Fetch and parse the latest release manifest.
///
/// Performs a single HTTP GET with the retry policy from `downloader.rs`.
///
/// # Errors
/// - `ManifestError::Fetch` — network failure after retries
/// - `ManifestError::Parse` — malformed JSON / missing required fields
pub fn fetch_manifest(config: &ManifestConfig) -> Result<ReleaseManifest, ManifestError>;

/// Resolve the asset URL for the current compile target triple.
///
/// Falls back through target variants (e.g. gnu before musl) using the
/// existing `get_target_triples_with_fallback()` logic.
///
/// # Errors
/// - `ManifestError::NoAssetForTarget` — manifest has no asset for this platform
pub fn resolve_asset_url(
    manifest: &ReleaseManifest,
    config: &ManifestConfig,
) -> Result<String, ManifestError>;
```

### Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest fetch failed: {0}")]
    Fetch(String),

    #[error("manifest parse failed: {0}")]
    Parse(String),

    #[error("no asset in manifest for target {target}")]
    NoAssetForTarget { target: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

### `UpdaterConfig` additions (`lib.rs`)

```rust
pub struct UpdaterConfig {
    pub bin_name: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub current_version: String,
    pub show_progress: bool,
    // NEW:
    pub backend: UpdateBackend,
    pub manifest: ManifestConfig,
    pub auth_token: Option<String>, // forwarded to github fallback only
}

impl UpdaterConfig {
    pub fn new(bin_name: impl Into<String>) -> Self {
        Self {
            // ...existing...
            repo_name: "terraphim-clients".to_string(), // FIX: was "terraphim-ai"
            backend: UpdateBackend::R2,                 // default to R2
            manifest: ManifestConfig {
                bin_name: bin_name.into(),
                ..Default::default()
            },
            auth_token: std::env::var("GITHUB_TOKEN").ok(), // pickup for fallback
        }
    }

    pub fn with_backend(mut self, b: UpdateBackend) -> Self { self.backend = b; self }
    pub fn with_manifest_base_url(mut self, url: impl Into<String>) -> Self {
        self.manifest.base_url = url.into(); self
    }
}
```

## Test Strategy

### Unit Tests (`manifest.rs`)
| Test | Purpose |
|------|---------|
| `test_manifest_url_construction` | `<base>/<bin>/stable.json` |
| `test_manifest_parse_minimal` | version + 1 asset |
| `test_manifest_parse_missing_version_errors` | required-field validation |
| `test_resolve_asset_exact_target` | x86_64-unknown-linux-gnu found |
| `test_resolve_asset_fallback_gnu_to_musl` | fallback chain |
| `test_resolve_asset_no_match_errors` | `NoAssetForTarget` |
| `test_backend_default_is_r2` | `UpdaterConfig::default().backend == R2` |
| `test_default_repo_is_terraphim_clients` | regression guard for the wrong-repo bug |

### Integration Tests (`tests/manifest.rs`)
| Test | Purpose |
|------|---------|
| `test_fetch_manifest_from_local_server` | spin up `httptest`/`wiremock` server, serve fixture JSON, assert parse |
| `test_fetch_manifest_retry_on_500` | server 500s twice then 200; assert success + attempt count |
| `test_fetch_manifest_404_errors` | clean `ManifestError::Fetch` |
| `test_full_update_flow_against_local_server` | manifest + signed fake archive + verify + install to tmpdir |
| `test_github_fallback_when_r2_down` | R2 server down → github mock returns a release → update succeeds |

### Property Tests
```rust
proptest! {
    #[test]
    fn manifest_roundtrip(m: ReleaseManifest_ARB) {
        let json = serde_json::to_string(&m).unwrap();
        let back: ReleaseManifest = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(m.version, back.version);
    }
}
```

### Workflow Tests (CI)
| Test | Purpose |
|------|---------|
| `r2-manifest-health` (scheduled) | `terraphim-agent update --check-only` against live `downloads.terraphim.ai` once hourly |
| `manifest-points-at-existing-assets` | in workflow: after upload, HEAD every asset URL in the manifest before publishing |

> **No mocks.** Per project rules, integration tests use real local HTTP servers (`wiremock`) or real R2 in CI, not mocked network.

## Implementation Steps

### Step 1: Manifest module + types
**Files:** `crates/terraphim_update/src/manifest.rs`, `crates/terraphim_update/src/lib.rs` (add `pub mod manifest;`)
**Description:** `ReleaseManifest`, `ManifestConfig`, `ManifestError`, `fetch_manifest`, `resolve_asset_url`. Pure logic + `ureq` GET via existing `downloader.rs`.
**Tests:** all unit tests above.
**Estimated:** 4 hours
**Dependencies:** none

### Step 2: Backend selector in `UpdaterConfig`
**Files:** `crates/terraphim_update/src/lib.rs`
**Description:** Add `UpdateBackend`, `backend`, `manifest`, `auth_token` fields; fix `repo_name` default; branch `check_update`/`update`/`check_and_update` to dispatch to `manifest` or existing github path.
**Tests:** `test_backend_default_is_r2`, `test_default_repo_is_terraphim_clients`; ensure existing github tests still pass.
**Estimated:** 3 hours
**Dependencies:** Step 1

### Step 3: Wire R2 path into `TerraphimUpdater`
**Files:** `crates/terraphim_update/src/lib.rs`
**Description:** New `check_update_r2()` / `update_r2()` methods: fetch manifest → semver compare → download asset (`downloader.rs`) → verify (`signature.rs`) → install (`install_verified_archive`, unchanged). On `ManifestError`, fall back to github path.
**Tests:** `test_full_update_flow_against_local_server`, `test_github_fallback_when_r2_down`.
**Estimated:** 4 hours
**Dependencies:** Step 2

### Step 4: Fix install-path shadowing
**Files:** `crates/terraphim_update/src/platform.rs`
**Description:** `get_binary_path` returns `current_exe().parent().join(bin)` when that dir is writable and the running exe matches `bin`; else existing `/usr/local/bin` → `~/.local/bin` chain.
**Tests:** `test_install_path_prefers_current_exe`, existing path tests unchanged.
**Estimated:** 1 hour
**Dependencies:** none (parallel-safe with 1-3)

### Step 5: Release-pipeline version bump
**Files:** `.github/workflows/release-binaries.yml`
**Description:** Before `cargo build`, run `sed -i`/`cargo workspaces version` to set `version.workspace` to `${{ inputs.version }}` so `--version` matches the tag.
**Tests:** workflow run produces binaries whose `--version` == input version.
**Estimated:** 1 hour
**Dependencies:** none

### Step 6: R2 upload workflow + manifest builder
**Files:** `.github/workflows/upload-r2.yml`, `scripts/build-manifest.sh`, `scripts/r2-rclone.conf.template`
**Description:** Reusable workflow: input = `version`, `bin`, `assets_dir`. Configure `rclone` from `R2_*` secrets, `rclone copyto` each asset to `terraphim-releases/<bin>/<asset>`, then generate `stable.json` and `rclone copyto` it last. Verify with `curl HEAD` each URL before completing.
**Tests:** `manifest-points-at-existing-assets` post-step.
**Estimated:** 3 hours
**Dependencies:** Infra spike (R2 bucket + custom domain created)

### Step 7: Agent CLI wiring + docs
**Files:** `crates/terraphim_agent/src/main.rs` (set default backend), `crates/terraphim_agent/README.md` (or `CHANGELOG.md`), `crates/terraphim_update/CHANGELOG.md`
**Description:** Ensure `UpdaterConfig::new("terraphim-agent")` defaults to R2; document `TERRAPHIM_UPDATE_BACKEND=r2|github` and `TERRAPHIM_UPDATE_BASE_URL` env overrides.
**Tests:** manual `terraphim-agent update --check-only` against staging bucket.
**Estimated:** 2 hours
**Dependencies:** Steps 2, 6

### Step 8: Fallback hardening + e2e CI
**Files:** `.github/workflows/ci.yml` (add `r2-manifest-health` job)
**Description:** Scheduled job that hits live manifest; PR job that runs `terraphim-agent update` against a staging bucket.
**Estimated:** 2 hours
**Dependencies:** Step 6

## Rollback Plan

If the R2 backend misbehaves after shipping:
1. Set default backend back to `GitHub` via a one-line revert in `UpdaterConfig::new` (or instruct users to `export TERRAPHIM_UPDATE_BACKEND=github`).
2. The github path remains fully functional (now with the repo-name + auth-token fixes), so no client is bricked.
3. Take the bucket offline; existing clients transparentently fall back.

Feature flag: `TERRAPHIM_UPDATE_BACKEND` env var (values: `r2` | `github`).

## Migration (if applicable)

### One-time infra setup (manual, ~30 min)
1. Cloudflare dashboard → R2 → create bucket `terraphim-releases`.
2. R2 → Settings → Custom Domain → add `downloads.terraphim.ai` (requires the `terraphim.ai` zone, already on Cloudflare).
3. Create an R2 API token with **Object Read & Write** scoped to the bucket; store as GitHub Actions secrets `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_ENDPOINT` (`https://<account>.r2.cloudflarestorage.com`).
4. Backfill: re-upload the existing v1.21.9 artifacts + generate `stable.json` (use `scripts/build-manifest.sh`).

### Data Migration
- Existing GitHub release assets are unchanged (kept as fallback). R2 is additive.
- No database changes.

## Dependencies

### New Dependencies
| Crate | Version | Justification |
|-------|---------|---------------|
| (none) | — | `ureq`, `serde`, `serde_json`, `thiserror`, `semver` already in tree |

### Dependency Updates
| Crate | From | To | Reason |
|-------|------|-----|--------|
| (none) | — | — | — |

CI-only (not shipped):
| Tool | Version | Justification |
|------|---------|---------------|
| `rclone` | latest stable | R2 upload (S3-compatible) |

## Performance Considerations

### Expected Performance
| Metric | Target | Measurement |
|--------|--------|-------------|
| Manifest fetch | <300 ms (cached at Cloudflare edge) | `terraphim-agent update --check-only` |
| Asset download | bandwidth-bound; R2 edge cache | CI timing |
| Client binary size delta | <20 KB (new module + serde structs) | `du` before/after |

### Benchmarks to Add
```rust
// benches/manifest_parse.rs -- criterion
fn bench_parse_manifest(c: &mut Criterion) {
    let json = include_str!("../fixtures/stable.json");
    c.bench_function("parse", |b| b.iter(|| serde_json::from_str::<ReleaseManifest>(json).unwrap()));
}
```

## Open Items

| Item | Status | Owner |
|------|--------|-------|
| Q1: grep `update` subcommand — re-add or agent-only? | Pending | Alex |
| ~~Q2: R2 bucket name~~ | **DONE** `terraphim-releases` | opencode |
| ~~Q3: custom domain~~ | **DONE** `downloads.terraphim.ai` (TLS 1.2+) | opencode |
| ~~Spike: create bucket + curl one artifact~~ | **DONE** 2026-07-07 — bucket, domain, 15 assets, 3 manifests all live | opencode |
| Spike: confirm `self_update` extract helpers reusable | Not started | opencode |

### Infra Spike Result (2026-07-07)
- **Cloudflare account ID**: `4a345f44f6a673abdaf28eea80da7588`
- **Zone** `terraphim.ai` ID: `b489b841cea3c6a7270890a7e2310e5d`
- **Bucket**: `terraphim-releases` (Standard storage class)
- **Custom domain**: `downloads.terraphim.ai` → bucket (min TLS 1.2)
- **Object key layout**: `{bin}/{filename}` and `{bin}/stable.json`
- **Verified**: `curl https://downloads.terraphim.ai/terraphim-agent/stable.json` → 200, `application/json`; asset HEAD → 200, `application/gzip`, correct `content-length`.
- **Backfilled**: 15 v1.21.9 tar.gz assets (3 binaries × 5 targets) + 3 real manifests.
- **wrangler upload command**: `bunx wrangler r2 object put terraphim-releases/{key} --file {path} --remote` (`--remote` required for custom-domain access; without it objects land in local worker storage).
- **CI secrets needed**: `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` (create an R2 API token scoped to the bucket; store in GitHub Actions secrets).

## Approval

- [ ] Technical review complete
- [ ] Test strategy approved
- [ ] Performance targets agreed
- [ ] Open Questions 1-3 resolved
- [ ] Human approval received

---

## Sequencing Summary (for the implementer)

```
Day 0 (parallel):  Infra spike (R2 bucket + custom domain) ──┐
                  Step 4 (install-path fix) ───────────────┐ │
                  Step 5 (version bump in workflow) ──────┐ │ │
Day 1:            Step 1 (manifest module) ──► Step 2 ──► Step 3
Day 2:            Step 6 (R2 upload workflow) ──► Step 7 ──► Step 8
Gate:             quality-gate skill before merge
```

Total new code: ~150 lines (`manifest.rs`) + ~40 lines (`lib.rs` dispatch) + ~10 lines (`platform.rs`) + ~60 lines (workflow YAML). Replaces ~0 lines of working code (github path kept as fallback). Net: removes a hard external dependency from the client hot path.
