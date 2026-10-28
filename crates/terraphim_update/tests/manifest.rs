//! Integration tests for the manifest backend.
//!
//! Spins up a real local HTTP server (std::net) — no mocks — to exercise
//! `fetch_manifest` against live bytes, retry-on-5xx, and 404 handling.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use serde::Deserialize;

use terraphim_update::manifest::{
    ManifestConfig, fetch_manifest, resolve_asset_url, validate_manifest,
};

#[derive(Debug, Deserialize)]
struct Pre12115ReleaseManifest {
    version: String,
    released_at: String,
    assets: HashMap<String, String>,
    #[serde(default)]
    notes_url: Option<String>,
}

#[test]
fn test_generated_legacy_manifest_executes_pre12115_wire_contract() {
    let root = tempfile::tempdir().expect("temporary stage");
    let assets = root.path().join("release-assets");
    std::fs::create_dir(&assets).expect("asset directory");
    let targets = terraphim_update::manifest::all_target_triples();
    for (index, target) in targets.iter().enumerate() {
        let extension = if target == "x86_64-pc-windows-msvc" {
            ".zip"
        } else {
            ".tar.gz"
        };
        std::fs::write(
            assets.join(format!("terraphim-agent-1.21.15-{target}{extension}")),
            format!("sealed-payload-{index}"),
        )
        .expect("fixture asset");
    }
    let strict = root.path().join("terraphim-agent.v2.candidate.json");
    let legacy = root.path().join("terraphim-agent.v1.candidate.json");
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let built = Command::new(repository.join("scripts/build-manifest.sh"))
        .args([
            "1.21.15",
            "terraphim-agent",
            assets.to_str().expect("UTF-8 asset path"),
            strict.to_str().expect("UTF-8 strict path"),
        ])
        .env("SOURCE_DATE_EPOCH", "1789689600")
        .output()
        .expect("run strict manifest builder");
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    let derived = Command::new("python3")
        .args([
            repository
                .join("scripts/build-legacy-manifest.py")
                .to_str()
                .expect("UTF-8 script path"),
            strict.to_str().expect("UTF-8 strict path"),
            legacy.to_str().expect("UTF-8 legacy path"),
        ])
        .output()
        .expect("run legacy manifest builder");
    assert!(
        derived.status.success(),
        "{}",
        String::from_utf8_lossy(&derived.stderr)
    );

    let bytes = std::fs::read(&legacy).expect("generated legacy bytes");
    let old: Pre12115ReleaseManifest =
        serde_json::from_slice(&bytes).expect("<=1.21.14 wire shape must deserialize");
    assert_eq!(old.version, "1.21.15");
    assert_eq!(old.released_at, "2026-09-18T00:00:00Z");
    assert_eq!(
        old.notes_url.as_deref(),
        Some("https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.15")
    );
    let current_target = terraphim_update::manifest::current_target_triples()[0].clone();
    let advertised = old.assets.get(&current_target).expect("current target");
    let expected_name = format!(
        "terraphim-agent-1.21.15-{current_target}{}",
        if current_target == "x86_64-pc-windows-msvc" {
            ".zip"
        } else {
            ".tar.gz"
        }
    );
    assert_eq!(advertised, &format!("terraphim-agent/{expected_name}"));
    let resolved = format!("https://downloads.terraphim.ai/{advertised}");
    assert_eq!(
        resolved,
        format!("https://downloads.terraphim.ai/terraphim-agent/{expected_name}")
    );
    let installed = root.path().join("installed-payload");
    std::fs::copy(assets.join(&expected_name), &installed).expect("old install copy");
    assert_eq!(
        std::fs::read(installed).expect("installed bytes"),
        std::fs::read(assets.join(expected_name)).expect("advertised bytes")
    );

    let strict_bytes = std::fs::read(strict).expect("strict bytes");
    assert!(
        serde_json::from_slice::<Pre12115ReleaseManifest>(&strict_bytes).is_err(),
        "strict object-valued assets must not masquerade as the old wire shape"
    );
}

#[test]
fn test_manifest_rejects_legacy_assets_and_unknown_keys() {
    let legacy = r#"{
        "version":"1.21.15",
        "released_at":"2026-09-18T00:00:00Z",
        "assets":{"x86_64-unknown-linux-gnu":"terraphim-agent/legacy.tar.gz"},
        "notes_url":"https://example.invalid/v1.21.15"
    }"#;
    let unknown = r#"{
        "version":"1.21.15",
        "released_at":"2026-09-18T00:00:00Z",
        "assets":{"x86_64-unknown-linux-gnu":{
            "path":"terraphim-agent/terraphim-agent-1.21.15-x86_64-unknown-linux-gnu.tar.gz",
            "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "size":42,
            "signature":"not-part-of-the-schema"
        }},
        "notes_url":"https://example.invalid/v1.21.15",
        "channel":"stable"
    }"#;

    for body in [legacy, unknown] {
        let result = serde_json::from_str::<terraphim_update::manifest::ReleaseManifest>(body);
        assert!(
            result.is_err(),
            "strict manifest unexpectedly accepted {body}"
        );
    }
}

#[test]
fn test_manifest_rejects_duplicate_targets_invalid_integrity_and_zero_size() {
    let valid = r#"{
        "version":"1.21.15","released_at":"2026-09-18T00:00:00Z",
        "assets":{
            "x86_64-unknown-linux-gnu":{"path":"x/x-1.21.15-x86_64-unknown-linux-gnu.tar.gz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1}
        },"notes_url":"https://example.invalid/v1.21.15"
    }"#;
    let duplicate = r#"{
        "version":"1.21.15","released_at":"2026-09-18T00:00:00Z",
        "assets":{
            "x86_64-unknown-linux-gnu":{"path":"x/x-1.21.15-x86_64-unknown-linux-gnu.tar.gz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":1},
            "x86_64-unknown-linux-gnu":{"path":"x/x-1.21.15-x86_64-unknown-linux-gnu.tar.gz","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","size":2}
        },"notes_url":"https://example.invalid/v1.21.15"
    }"#;
    let uppercase_sha = valid.replacen(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        1,
    );
    let zero_size = valid.replacen("\"size\":1", "\"size\":0", 1);
    for body in [duplicate.to_string(), uppercase_sha, zero_size] {
        assert!(
            serde_json::from_str::<terraphim_update::manifest::ReleaseManifest>(&body).is_err(),
            "invalid manifest unexpectedly parsed: {body}"
        );
    }
}

#[test]
fn test_official_manifest_requires_exact_targets_and_filename_identity() {
    let mut manifest: terraphim_update::manifest::ReleaseManifest =
        serde_json::from_str(&sample_manifest_json()).expect("strict fixture");
    let config = ManifestConfig::new("terraphim-agent");
    validate_manifest(&manifest, &config).expect("complete fixture");

    manifest.assets.remove("aarch64-unknown-linux-musl");
    assert!(validate_manifest(&manifest, &config).is_err());

    let mut wrong_path: terraphim_update::manifest::ReleaseManifest =
        serde_json::from_str(&sample_manifest_json()).expect("strict fixture");
    let asset = wrong_path
        .assets
        .get_mut("x86_64-unknown-linux-gnu")
        .expect("asset");
    asset.path =
        "terraphim-agent/terraphim-agent-1.21.14-x86_64-unknown-linux-gnu.tar.gz".to_string();
    assert!(validate_manifest(&wrong_path, &config).is_err());
}

/// Minimal single-connection HTTP/1.1 server for one request, running on its
/// own thread. Returns the configured status + body once, then shuts down.
struct OneShotServer {
    addr: String,
    _handle: thread::JoinHandle<()>,
}

struct OneShotConfig {
    status: u16,
    body: String,
    content_type: String,
}

impl OneShotServer {
    fn start(cfg: OneShotConfig) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr").to_string();
        let handle = thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                serve(stream, &cfg);
            }
        });
        Self {
            addr,
            _handle: handle,
        }
    }
}

fn serve(mut stream: TcpStream, cfg: &OneShotConfig) {
    // Read and discard the request.
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);
    let status_text = match cfg.status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        cfg.status,
        status_text,
        cfg.content_type,
        cfg.body.len(),
        cfg.body
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// Server that returns 500 for the first N requests, then 200. Counts via an
/// AtomicUsize so the test can assert retry behaviour.
struct FlakyServer {
    addr: String,
    attempts: Arc<AtomicUsize>,
}

impl FlakyServer {
    fn start(fail_first: usize, body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("local addr").to_string();
        let fail_count = Arc::new(AtomicUsize::new(fail_first));
        let attempts = Arc::new(AtomicUsize::new(0));
        let fc = fail_count.clone();
        let att = attempts.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let n = att.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                if n < fc.load(Ordering::SeqCst) {
                    let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
                let _ = stream.flush();
                if n >= fc.load(Ordering::SeqCst) {
                    break;
                }
            }
        });
        Self { addr, attempts }
    }
}

fn sample_manifest_json() -> String {
    // Assets are derived from `all_target_triples()` so adding a new platform
    // to the updater (e.g., RISC-V, FreeBSD) automatically extends this fixture.
    // Without this derivation, `test_resolve_asset_url_against_local_manifest`
    // would fail on macOS runners with `NoAssetForTarget { target: "aarch64-macos" }`.
    let entries: Vec<String> = terraphim_update::manifest::all_target_triples()
        .into_iter()
        .map(|target| {
            let extension = if target == "x86_64-pc-windows-msvc" {
                ".zip"
            } else {
                ".tar.gz"
            };
            format!(
                "    \"{target}\": {{\"path\":\"terraphim-agent/terraphim-agent-1.21.9-{target}{extension}\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"size\":1}}"
            )
        })
        .collect();
    format!(
        r#"{{
    "version": "1.21.9",
    "released_at": "2026-07-06T17:38:00Z",
    "assets": {{
{}
    }},
    "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
}}"#,
        entries.join(",\n")
    )
}

#[test]
fn test_fetch_manifest_from_local_server() {
    let server = OneShotServer::start(OneShotConfig {
        status: 200,
        body: sample_manifest_json(),
        content_type: "application/json".to_string(),
    });
    let cfg =
        ManifestConfig::new("terraphim-agent").with_base_url(format!("http://{}", server.addr));
    let manifest = fetch_manifest(&cfg).expect("manifest fetch should succeed");
    assert_eq!(manifest.version, "1.21.9");
    assert_eq!(
        manifest.assets.len(),
        terraphim_update::manifest::all_target_triples().len(),
        "sample manifest must cover every target in all_target_triples() (linux + macos + windows)"
    );
    assert!(manifest.notes_url.ends_with("/v1.21.9"));
}

#[test]
fn test_default_manifest_pointer_is_strict_v2() {
    let cfg = ManifestConfig::new("terraphim-agent");
    assert_eq!(
        cfg.manifest_url(),
        "https://downloads.terraphim.ai/terraphim-agent/stable-v2.json"
    );
}

#[test]
fn test_fetch_manifest_retry_on_500_then_succeed() {
    let body = sample_manifest_json();
    let server = FlakyServer::start(2, body);
    let cfg =
        ManifestConfig::new("terraphim-agent").with_base_url(format!("http://{}", server.addr));
    let result = fetch_manifest(&cfg);
    assert!(result.is_ok(), "should succeed after retries: {:?}", result);
    // At least 3 attempts (2 failures + 1 success).
    assert!(
        server.attempts.load(Ordering::SeqCst) >= 3,
        "expected >=3 attempts, got {}",
        server.attempts.load(Ordering::SeqCst)
    );
}

#[test]
fn test_fetch_manifest_404_errors() {
    let server = OneShotServer::start(OneShotConfig {
        status: 404,
        body: String::new(),
        content_type: "text/plain".to_string(),
    });
    let cfg =
        ManifestConfig::new("terraphim-agent").with_base_url(format!("http://{}", server.addr));
    let result = fetch_manifest(&cfg);
    assert!(result.is_err(), "404 should error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("manifest fetch failed"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_resolve_asset_url_against_local_manifest() {
    // Build the manifest in-memory and resolve without any network.
    let json = sample_manifest_json();
    let manifest: terraphim_update::manifest::ReleaseManifest =
        serde_json::from_str(&json).unwrap();
    let cfg = ManifestConfig::new("terraphim-agent");
    let url = resolve_asset_url(&manifest, &cfg).expect("should resolve for current platform");
    assert!(url.starts_with("https://downloads.terraphim.ai/"));
    assert!(url.ends_with(".tar.gz"));
}

#[test]
fn test_resolve_asset_url_rejects_unvalidated_traversal_path() {
    let mut manifest: terraphim_update::manifest::ReleaseManifest =
        serde_json::from_str(&sample_manifest_json()).expect("strict fixture");
    let target = terraphim_update::manifest::current_target_triples()[0].clone();
    let asset = manifest.assets.get_mut(&target).expect("current target");
    asset.path = "../escaped.tar.gz".to_string();
    let cfg = ManifestConfig::new("terraphim-agent");

    let error = resolve_asset_url(&manifest, &cfg).expect_err("traversal must fail");
    assert!(error.to_string().contains("validation failed"));
}
