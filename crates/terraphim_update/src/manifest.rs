//! Storage-agnostic release manifest backend.
//!
//! Fetches a tiny per-binary JSON manifest from an HTTP host (Cloudflare R2
//! served via a custom domain by default) and resolves the download URL for
//! the current compile target. Decouples version discovery from any specific
//! provider API (no GitHub API, no S3 ListObjectsV2, no embedded secrets).
//!
//! The strict manifest lives at `{base_url}/{bin_name}/stable-v2.json`, e.g.
//! `https://downloads.terraphim.ai/terraphim-agent/stable-v2.json`. The legacy
//! `stable.json` pointer remains string-valued for pre-1.21.15 clients.

use std::collections::{BTreeMap, BTreeSet};
use std::env::consts::{ARCH, OS};
use std::fmt;
use std::io::Read;
use std::time::Duration;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Maximum manifest fetch attempts before giving up.
const MAX_FETCH_ATTEMPTS: u32 = 3;
/// Maximum accepted stable manifest size. Release manifests are a few KiB;
/// this bound prevents untrusted hosts from forcing unbounded allocation.
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

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

/// The per-binary strict release manifest served at
/// `{base_url}/{bin}/stable-v2.json`.
///
/// ```json
/// {
///   "version": "1.21.9",
///   "released_at": "2026-07-06T17:38:00Z",
///   "assets": {
///     "x86_64-unknown-linux-gnu": {
///       "path": "terraphim-agent/terraphim-agent-1.21.15-x86_64-unknown-linux-gnu.tar.gz",
///       "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
///       "size": 123456
///     }
///   },
///   "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    /// Latest semantic version (no leading 'v').
    pub version: String,
    /// ISO-8601 release timestamp (informational).
    pub released_at: String,
    /// Map of Rust target triple to an integrity-bearing immutable asset.
    #[serde(deserialize_with = "deserialize_assets")]
    pub assets: BTreeMap<String, ReleaseAsset>,
    /// Human-readable release-notes URL.
    pub notes_url: String,
}

/// Immutable release asset metadata. All fields are required and unknown
/// fields are rejected so integrity checks cannot silently disappear.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAsset {
    /// Object path relative to the manifest base URL.
    pub path: String,
    /// Lowercase SHA-256 digest of the final signed archive bytes.
    #[serde(deserialize_with = "deserialize_sha256")]
    pub sha256: String,
    /// Exact positive byte length of the final signed archive.
    #[serde(deserialize_with = "deserialize_positive_size")]
    pub size: u64,
}

/// A selected asset together with its fully qualified download URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAsset {
    /// Fully qualified download URL.
    pub url: String,
    /// Relative object path from the manifest.
    pub path: String,
    /// Expected lowercase SHA-256 digest.
    pub sha256: String,
    /// Expected byte length.
    pub size: u64,
}

fn deserialize_sha256<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(value)
    } else {
        Err(de::Error::custom(
            "sha256 must be exactly 64 lowercase hexadecimal characters",
        ))
    }
}

fn deserialize_positive_size<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value == 0 {
        Err(de::Error::custom("asset size must be positive"))
    } else {
        Ok(value)
    }
}

fn deserialize_assets<'de, D>(deserializer: D) -> Result<BTreeMap<String, ReleaseAsset>, D::Error>
where
    D: Deserializer<'de>,
{
    struct AssetsVisitor;

    impl<'de> Visitor<'de> for AssetsVisitor {
        type Value = BTreeMap<String, ReleaseAsset>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map of unique target triples to strict release assets")
        }

        fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut assets = BTreeMap::new();
            while let Some((target, asset)) = access.next_entry::<String, ReleaseAsset>()? {
                if assets.insert(target.clone(), asset).is_some() {
                    return Err(de::Error::custom(format!(
                        "duplicate manifest target {target:?}"
                    )));
                }
            }
            Ok(assets)
        }
    }

    deserializer.deserialize_map(AssetsVisitor)
}

/// Configuration for the manifest backend.
#[derive(Debug, Clone)]
pub struct ManifestConfig {
    /// Base URL serving the bucket, e.g. `https://downloads.terraphim.ai`.
    /// No trailing slash.
    pub base_url: String,
    /// Binary name, e.g. `terraphim-agent`.
    pub bin_name: String,
    /// Manifest filename (default `stable-v2.json`).
    pub manifest_name: String,
}

impl Default for ManifestConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            bin_name: String::new(),
            manifest_name: "stable-v2.json".to_string(),
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

    /// Parsed JSON violates the release identity or exact target contract.
    #[error("manifest validation failed: {0}")]
    Invalid(String),

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
                    let mut body = String::new();
                    resp.into_reader()
                        .take(MAX_MANIFEST_BYTES + 1)
                        .read_to_string(&mut body)
                        .map_err(|e| ManifestError::Fetch(format!("read body: {e}")))?;
                    if body.len() as u64 > MAX_MANIFEST_BYTES {
                        return Err(ManifestError::Parse(
                            "manifest exceeds 1 MiB limit".to_string(),
                        ));
                    }
                    let manifest: ReleaseManifest = serde_json::from_str(&body)
                        .map_err(|e| ManifestError::Parse(e.to_string()))?;
                    validate_manifest(&manifest, config)?;
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
    validate_manifest(manifest, config)?;
    for target in current_target_triples() {
        if let Some(asset) = manifest.assets.get(&target) {
            debug!("resolved asset for target {}", target);
            return Ok(config.asset_url(&asset.path));
        }
        debug!("target {} not in manifest; trying fallback", target);
    }
    Err(ManifestError::NoAssetForTarget {
        target: format!("{ARCH}-{OS}"),
    })
}

/// Resolve the current platform asset with its integrity metadata.
pub fn resolve_asset(
    manifest: &ReleaseManifest,
    config: &ManifestConfig,
) -> Result<ResolvedAsset, ManifestError> {
    validate_manifest(manifest, config)?;
    for target in current_target_triples() {
        if let Some(asset) = manifest.assets.get(&target) {
            return Ok(ResolvedAsset {
                url: config.asset_url(&asset.path),
                path: asset.path.clone(),
                sha256: asset.sha256.clone(),
                size: asset.size,
            });
        }
    }
    Err(ManifestError::NoAssetForTarget {
        target: format!("{ARCH}-{OS}"),
    })
}

/// Validate release identity, filenames, paths, and official exact target sets.
pub fn validate_manifest(
    manifest: &ReleaseManifest,
    config: &ManifestConfig,
) -> Result<(), ManifestError> {
    let version = semver::Version::parse(&manifest.version)
        .map_err(|error| ManifestError::Invalid(format!("invalid version: {error}")))?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err(ManifestError::Invalid(
            "stable manifest version must not be prerelease or build metadata".to_string(),
        ));
    }
    chrono::DateTime::parse_from_rfc3339(&manifest.released_at).map_err(|error| {
        ManifestError::Invalid(format!("released_at must be RFC 3339: {error}"))
    })?;
    if !manifest.notes_url.starts_with("https://") {
        return Err(ManifestError::Invalid(
            "notes_url must use HTTPS".to_string(),
        ));
    }
    if manifest.assets.is_empty() {
        return Err(ManifestError::Invalid(
            "assets must not be empty".to_string(),
        ));
    }

    for (target, asset) in &manifest.assets {
        if target.is_empty()
            || !target
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(ManifestError::Invalid(format!(
                "invalid target key {target:?}"
            )));
        }
        let extension = if target == "x86_64-pc-windows-msvc" {
            ".zip"
        } else {
            ".tar.gz"
        };
        let filename = format!(
            "{}-{}-{}{}",
            config.bin_name, manifest.version, target, extension
        );
        let expected_path = format!("{}/{filename}", config.bin_name);
        if asset.path != expected_path {
            return Err(ManifestError::Invalid(format!(
                "asset path {:?} must equal {:?}",
                asset.path, expected_path
            )));
        }
    }

    if let Some(expected) = expected_targets_for_bin(&config.bin_name) {
        let actual: BTreeSet<&str> = manifest.assets.keys().map(String::as_str).collect();
        if actual != expected {
            return Err(ManifestError::Invalid(format!(
                "{} targets do not match the exact release contract",
                config.bin_name
            )));
        }
    }
    Ok(())
}

fn expected_targets_for_bin(bin_name: &str) -> Option<BTreeSet<&'static str>> {
    let mut targets = BTreeSet::from([
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
    ]);
    match bin_name {
        "terraphim-agent" | "terraphim-grep" => {
            targets.insert("universal-apple-darwin");
            Some(targets)
        }
        "terraphim-cli" => Some(targets),
        _ => None,
    }
}

/// Ordered list of target triples to try for the current platform.
///
/// Mirrors the GNU→MUSL and native→universal fallback logic used elsewhere in
/// the updater, kept here as a standalone pub fn so the manifest module is
/// self-contained.
pub fn current_target_triples() -> Vec<String> {
    let cur = format!("{}-{}", ARCH, OS);
    target_triples_for_host(&cur)
        .into_iter()
        .map(String::from)
        .collect()
}

/// All target triples the updater publishes assets for, deduplicated.
///
/// Used by the test fixture to derive the manifest's asset count so adding
/// a new platform here (or to `target_triples_for_host`) automatically extends
/// the fixture without a magic-number update.
pub fn all_target_triples() -> Vec<String> {
    [
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-musl",
        "universal-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

/// Static map from `ARCH-OS` host string to the target triples we publish
/// assets for. Single source of truth for both `current_target_triples()` and
/// `all_target_triples()`; adding a new platform is a one-line change here.
///
/// Unknown hosts return an empty list -- callers that depend on a match (the
/// manifest module's `resolve_asset_url`) treat an empty result as
/// "no asset for this target", which is the correct behaviour for an
/// unsupported platform.
fn target_triples_for_host(host: &str) -> Vec<&'static str> {
    match host {
        "x86_64-linux" => vec!["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"],
        "aarch64-linux" => vec!["aarch64-unknown-linux-gnu", "aarch64-unknown-linux-musl"],
        "x86_64-windows" => vec!["x86_64-pc-windows-msvc"],
        "x86_64-macos" => vec!["x86_64-apple-darwin", "universal-apple-darwin"],
        "aarch64-macos" => vec!["aarch64-apple-darwin", "universal-apple-darwin"],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> ReleaseManifest {
        let mut assets = BTreeMap::new();
        assets.insert(
            "x86_64-unknown-linux-gnu".to_string(),
            ReleaseAsset {
                path: "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-gnu.tar.gz"
                    .to_string(),
                sha256: "a".repeat(64),
                size: 1,
            },
        );
        assets.insert(
            "x86_64-unknown-linux-musl".to_string(),
            ReleaseAsset {
                path: "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-musl.tar.gz"
                    .to_string(),
                sha256: "b".repeat(64),
                size: 2,
            },
        );
        assets.insert(
            "aarch64-unknown-linux-musl".to_string(),
            ReleaseAsset {
                path: "terraphim-agent/terraphim-agent-1.21.9-aarch64-unknown-linux-musl.tar.gz"
                    .to_string(),
                sha256: "c".repeat(64),
                size: 3,
            },
        );
        ReleaseManifest {
            version: "1.21.9".to_string(),
            released_at: "2026-07-06T17:38:00Z".to_string(),
            assets,
            notes_url: "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
                .to_string(),
        }
    }

    #[test]
    fn test_manifest_url_construction() {
        let cfg = ManifestConfig::new("terraphim-agent");
        assert_eq!(
            cfg.manifest_url(),
            "https://downloads.terraphim.ai/terraphim-agent/stable-v2.json"
        );
    }

    #[test]
    fn test_manifest_url_strips_trailing_slash() {
        let cfg = ManifestConfig::new("terraphim-agent/").with_base_url("https://x.example/");
        assert_eq!(
            cfg.manifest_url(),
            "https://x.example/terraphim-agent/stable-v2.json"
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
            "assets": { "x86_64-unknown-linux-gnu": {
                "path": "bin/bin-1.2.3-x86_64-unknown-linux-gnu.tar.gz",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "size": 1
            } },
            "notes_url": "https://example.invalid/v1.2.3"
        }"#;
        let m: ReleaseManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.assets.len(), 1);
        assert_eq!(m.notes_url, "https://example.invalid/v1.2.3");
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
        let cfg = ManifestConfig::new("test-client");
        let first = current_target_triples()[0].clone();
        let extension = if first == "x86_64-pc-windows-msvc" {
            ".zip"
        } else {
            ".tar.gz"
        };
        let manifest = ReleaseManifest {
            version: "1.21.9".to_string(),
            released_at: "2026-09-18T00:00:00Z".to_string(),
            assets: BTreeMap::from([(
                first.clone(),
                ReleaseAsset {
                    path: format!("test-client/test-client-1.21.9-{first}{extension}"),
                    sha256: "d".repeat(64),
                    size: 4,
                },
            )]),
            notes_url: "https://example.invalid/v1.21.9".to_string(),
        };
        let url = resolve_asset_url(&manifest, &cfg).unwrap();
        assert!(url.contains(&first));
        assert!(url.starts_with("https://downloads.terraphim.ai/"));
    }

    #[test]
    fn test_resolve_asset_no_match_errors() {
        let cfg = ManifestConfig::new("test-client");
        let manifest = ReleaseManifest {
            version: "1.0.0".to_string(),
            released_at: "2026-09-18T00:00:00Z".to_string(),
            assets: BTreeMap::from([(
                "wasm32-unknown-unknown".to_string(),
                ReleaseAsset {
                    path: "test-client/test-client-1.0.0-wasm32-unknown-unknown.tar.gz".to_string(),
                    sha256: "d".repeat(64),
                    size: 4,
                },
            )]),
            notes_url: "https://example.invalid/v1.0.0".to_string(),
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
