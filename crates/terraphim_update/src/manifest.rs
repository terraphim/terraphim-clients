//! Storage-agnostic release manifest backend.
//!
//! Fetches a tiny per-binary JSON manifest from an HTTP host (Cloudflare R2
//! served via a custom domain by default) and resolves the download URL for
//! the current compile target. Decouples version discovery from any specific
//! provider API (no GitHub API, no S3 ListObjectsV2, no embedded secrets).
//!
//! The manifest lives at `{base_url}/{bin_name}/stable.json`, e.g.
//! `https://downloads.terraphim.ai/terraphim-agent/stable.json`.

use std::collections::HashMap;
use std::env::consts::{ARCH, OS};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Maximum manifest fetch attempts before giving up.
const MAX_FETCH_ATTEMPTS: u32 = 3;

/// Which distribution backend to use for update checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateBackend {
    /// Cloudflare R2 (or any HTTP host) via JSON manifest. No secrets, no
    /// per-IP rate limit. Default.
    #[default]
    R2,
    /// GitHub Releases. Used as a fallback when the manifest host is
    /// unreachable; requires `GITHUB_TOKEN` to avoid rate limiting.
    GitHub,
}

/// The per-binary release manifest served at `{base_url}/{bin}/stable.json`.
///
/// ```json
/// {
///   "version": "1.21.9",
///   "released_at": "2026-07-06T17:38:00Z",
///   "assets": {
///     "x86_64-unknown-linux-gnu": "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-gnu.tar.gz"
///   },
///   "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseManifest {
    /// Latest semantic version (no leading 'v').
    pub version: String,
    /// ISO-8601 release timestamp (informational).
    pub released_at: String,
    /// Map of Rust target triple -> asset key relative to `base_url`.
    pub assets: HashMap<String, String>,
    /// Optional human-readable release-notes URL.
    #[serde(default)]
    pub notes_url: Option<String>,
}

/// Configuration for the manifest backend.
#[derive(Debug, Clone)]
pub struct ManifestConfig {
    /// Base URL serving the bucket, e.g. `https://downloads.terraphim.ai`.
    /// No trailing slash.
    pub base_url: String,
    /// Binary name, e.g. `terraphim-agent`.
    pub bin_name: String,
    /// Manifest filename (default `stable.json`).
    pub manifest_name: String,
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            bin_name: String::new(),
            manifest_name: "stable.json".to_string(),
        }
    }
}

/// Default public base URL. R2 served via the Cloudflare custom domain
/// `downloads.terraphim.ai` (free egress).
pub const DEFAULT_BASE_URL: &str = "https://downloads.terraphim.ai";

impl ManifestConfig {
    /// Construct a new config for `bin_name` with defaults.
    pub fn new(bin_name: impl Into<String>) -> Self {
        Self {
            bin_name: bin_name.into(),
            ..Default::default()
        }
    }

    /// Override the base URL (e.g. for a staging bucket).
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }

    /// Override the manifest filename.
    pub fn with_manifest_name(mut self, name: impl Into<String>) -> Self {
        self.manifest_name = name.into();
        self
    }

    /// Construct the full manifest URL: `{base}/{bin}/{manifest_name}`.
    pub fn manifest_url(&self) -> String {
        format!(
            "{}/{}/{}",
            self.base_url.trim_end_matches('/'),
            self.bin_name.trim_end_matches('/'),
            self.manifest_name
        )
    }

    /// Construct the full asset URL for a relative asset key.
    pub fn asset_url(&self, asset_key: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            asset_key.trim_start_matches('/')
        )
    }
}

/// Errors produced by the manifest backend.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Network failure after all retries.
    #[error("manifest fetch failed: {0}")]
    Fetch(String),

    /// Malformed JSON or missing required fields.
    #[error("manifest parse failed: {0}")]
    Parse(String),

    /// Manifest carries no asset for the current target triple.
    #[error("no asset in manifest for target {target}")]
    NoAssetForTarget { target: String },

    /// Generic I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Fetch and parse the latest release manifest.
///
/// Performs a small number of HTTP GETs with backoff. Public read, no auth.
pub fn fetch_manifest(config: &ManifestConfig) -> Result<ReleaseManifest, ManifestError> {
    let url = config.manifest_url();
    info!("Fetching manifest from {}", url);

    let mut last_err: Option<String> = None;
    for attempt in 1..=MAX_FETCH_ATTEMPTS {
        debug!("manifest fetch attempt {}/{}", attempt, MAX_FETCH_ATTEMPTS);
        match ureq::get(&url).timeout(Duration::from_secs(15)).call() {
            Ok(resp) => {
                if resp.status() != 200 {
                    let msg = format!("HTTP {} {}", resp.status(), resp.status_text());
                    warn!("manifest fetch attempt {} failed: {}", attempt, msg);
                    last_err = Some(msg);
                } else {
                    let body = resp
                        .into_string()
                        .map_err(|e| ManifestError::Fetch(format!("read body: {e}")))?;
                    let manifest: ReleaseManifest = serde_json::from_str(&body)
                        .map_err(|e| ManifestError::Parse(e.to_string()))?;
                    debug!(
                        "manifest fetched: version {} ({} assets)",
                        manifest.version,
                        manifest.assets.len()
                    );
                    return Ok(manifest);
                }
            }
            Err(e) => {
                warn!("manifest fetch attempt {} failed: {}", attempt, e);
                last_err = Some(e.to_string());
            }
        }
        if attempt < MAX_FETCH_ATTEMPTS {
            let backoff = Duration::from_millis(500 * 2u64.pow(attempt - 1));
            debug!("backing off {:?}", backoff);
            std::thread::sleep(backoff);
        }
    }

    Err(ManifestError::Fetch(
        last_err.unwrap_or_else(|| "unknown fetch failure".to_string()),
    ))
}

/// Resolve the asset URL for the current compile target.
///
/// Walks the platform's target-triple fallback list (e.g. GNU before MUSL on
/// x86_64 linux; native before universal on macOS) and returns the first
/// target present in the manifest's `assets` map.
pub fn resolve_asset_url(
    manifest: &ReleaseManifest,
    config: &ManifestConfig,
) -> Result<String, ManifestError> {
    for target in current_target_triples() {
        if let Some(key) = manifest.assets.get(&target) {
            debug!("resolved asset for target {}", target);
            return Ok(config.asset_url(key));
        }
        debug!("target {} not in manifest; trying fallback", target);
    }
    Err(ManifestError::NoAssetForTarget {
        target: format!("{ARCH}-{OS}"),
    })
}

/// Ordered list of target triples to try for the current platform.
///
/// Mirrors the GNU→MUSL and native→universal fallback logic used elsewhere in
/// the updater, kept here as a standalone pub fn so the manifest module is
/// self-contained.
pub fn current_target_triples() -> Vec<String> {
    let cur = format!("{}-{}", ARCH, OS);
    match cur.as_str() {
        "x86_64-linux" => vec![
            "x86_64-unknown-linux-gnu".to_string(),
            "x86_64-unknown-linux-musl".to_string(),
        ],
        "aarch64-linux" => vec![
            "aarch64-unknown-linux-gnu".to_string(),
            "aarch64-unknown-linux-musl".to_string(),
        ],
        "x86_64-windows" => vec!["x86_64-pc-windows-msvc".to_string()],
        "x86_64-macos" => vec![
            "x86_64-apple-darwin".to_string(),
            "universal-apple-darwin".to_string(),
        ],
        "aarch64-macos" => vec![
            "aarch64-apple-darwin".to_string(),
            "universal-apple-darwin".to_string(),
        ],
        other => vec![other.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ReleaseManifest {
        let mut assets = HashMap::new();
        assets.insert(
            "x86_64-unknown-linux-gnu".to_string(),
            "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-gnu.tar.gz".to_string(),
        );
        assets.insert(
            "x86_64-unknown-linux-musl".to_string(),
            "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-musl.tar.gz".to_string(),
        );
        assets.insert(
            "aarch64-unknown-linux-musl".to_string(),
            "terraphim-agent/terraphim-agent-1.21.9-aarch64-unknown-linux-musl.tar.gz".to_string(),
        );
        ReleaseManifest {
            version: "1.21.9".to_string(),
            released_at: "2026-07-06T17:38:00Z".to_string(),
            assets,
            notes_url: Some(
                "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9".to_string(),
            ),
        }
    }

    #[test]
    fn test_manifest_url_construction() {
        let cfg = ManifestConfig::new("terraphim-agent");
        assert_eq!(
            cfg.manifest_url(),
            "https://downloads.terraphim.ai/terraphim-agent/stable.json"
        );
    }

    #[test]
    fn test_manifest_url_strips_trailing_slash() {
        let cfg = ManifestConfig::new("terraphim-agent/").with_base_url("https://x.example/");
        assert_eq!(
            cfg.manifest_url(),
            "https://x.example/terraphim-agent/stable.json"
        );
    }

    #[test]
    fn test_asset_url_construction() {
        let cfg = ManifestConfig::new("terraphim-agent");
        assert_eq!(
            cfg.asset_url("terraphim-agent/foo-1.0.0-x86_64-unknown-linux-gnu.tar.gz"),
            "https://downloads.terraphim.ai/terraphim-agent/foo-1.0.0-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn test_asset_url_strips_leading_slash() {
        let cfg = ManifestConfig::new("terraphim-agent");
        assert_eq!(
            cfg.asset_url("/terraphim-agent/foo.tar.gz"),
            "https://downloads.terraphim.ai/terraphim-agent/foo.tar.gz"
        );
    }

    #[test]
    fn test_manifest_parse_minimal() {
        let json = r#"{
            "version": "1.2.3",
            "released_at": "2026-01-01T00:00:00Z",
            "assets": { "x86_64-unknown-linux-gnu": "bin/foo-1.2.3.tar.gz" }
        }"#;
        let m: ReleaseManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.assets.len(), 1);
        assert!(m.notes_url.is_none());
    }

    #[test]
    fn test_manifest_parse_missing_version_errors() {
        let json = r#"{ "released_at": "x", "assets": {} }"#;
        let res: Result<ReleaseManifest, _> = serde_json::from_str(json);
        assert!(res.is_err());
    }

    #[test]
    fn test_manifest_roundtrip() {
        let original = sample_manifest();
        let json = serde_json::to_string(&original).unwrap();
        let back: ReleaseManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn test_resolve_asset_finds_present_target() {
        // Build the manifest from the host's own triples rather than reusing
        // `sample_manifest()`, which carries Linux assets only and so could
        // never resolve on macOS. Deriving the fixture keeps this test correct
        // on any host and cannot rot when a target is added. Refs #116.
        let cfg = ManifestConfig::new("terraphim-agent");
        let first = current_target_triples()[0].clone();
        let mut manifest = sample_manifest();
        manifest.assets.insert(
            first.clone(),
            format!("terraphim-agent/terraphim-agent-1.21.9-{first}.tar.gz"),
        );
        let url = resolve_asset_url(&manifest, &cfg).unwrap();
        assert!(url.contains(&first));
        assert!(url.starts_with("https://downloads.terraphim.ai/"));
    }

    #[test]
    fn test_resolve_asset_no_match_errors() {
        let cfg = ManifestConfig::new("terraphim-agent");
        let manifest = ReleaseManifest {
            version: "1.0.0".to_string(),
            released_at: "x".to_string(),
            assets: HashMap::new(),
            notes_url: None,
        };
        let res = resolve_asset_url(&manifest, &cfg);
        assert!(matches!(res, Err(ManifestError::NoAssetForTarget { .. })));
    }

    #[test]
    fn test_backend_default_is_r2() {
        assert_eq!(UpdateBackend::default(), UpdateBackend::R2);
    }

    #[test]
    fn test_current_target_triples_nonempty() {
        let targets = current_target_triples();
        assert!(
            !targets.is_empty(),
            "must return at least one target triple"
        );
        // Every entry looks like a Rust triple (contains at least two dashes).
        for t in &targets {
            assert!(t.matches('-').count() >= 2, "malformed triple: {t}");
        }
    }
}
