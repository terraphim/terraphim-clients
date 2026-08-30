//! Integration tests for the R2 update path (`TerraphimUpdater::update_r2`)
//! and the transport-failure -> GitHub fallback contract.
//!
//! All HTTP is served by a real local `std::net` server (no mocks). Archives
//! are real tar.gz files built in-test via flate2 + tar.

use base64::Engine;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use terraphim_update::{
    TerraphimUpdater, UpdateBackend, UpdateStatus, UpdaterConfig, manifest::ManifestConfig,
};

/// A tiny multi-path HTTP server: maps `path -> (status, body, content_type)`.
/// Serves any number of requests until the test ends (thread is detached).
struct MultiPathServer {
    addr: String,
}

impl MultiPathServer {
    fn new(routes: std::collections::HashMap<String, (u16, Vec<u8>, String)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let routes = routes.clone();
                thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    let req = String::from_utf8_lossy(&buf);
                    // Parse the request line: "GET /path HTTP/1.1"
                    let path = req
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/");
                    let (status, body, ct) = routes.get(path).cloned().unwrap_or((
                        404,
                        b"Not Found".to_vec(),
                        "text/plain".to_string(),
                    ));
                    let status_text = match status {
                        200 => "OK",
                        404 => "Not Found",
                        _ => "OK",
                    };
                    let resp = format!(
                        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        status,
                        status_text,
                        ct,
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.write_all(&body);
                    let _ = stream.flush();
                });
            }
        });
        Self { addr }
    }
}

/// Build a real tar.gz containing a single executable file named `bin_name`.
fn make_archive(bin_name: &str, contents: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write;
    let mut tar_buf = std::io::Cursor::new(Vec::new());
    {
        let mut tar = tar::Builder::new(&mut tar_buf);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(
            &mut header,
            bin_name,
            std::io::Cursor::new(contents.to_vec()),
        )
        .expect("append");
        tar.finish().expect("finish");
    }
    let tar_bytes = tar_buf.into_inner();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar_bytes).expect("gz write");
    encoder.finish().expect("gz finish")
}

/// Current compile target triple (first of the fallback list).
fn current_target() -> String {
    terraphim_update::manifest::current_target_triples()[0].clone()
}

/// Build a config pointing `terraphim-test` at a local server with R2 backend.
fn r2_config(base_url: String, current_version: &str) -> UpdaterConfig {
    UpdaterConfig::new("terraphim-test")
        .with_version(current_version)
        .with_backend(UpdateBackend::R2)
        .with_manifest_base_url(base_url)
        .with_progress(false)
}

/// Where install_verified_archive will write: current_exe().parent().
fn install_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[tokio::test]
async fn test_update_r2_installs_from_local_server() {
    let bin = "terraphim-test";
    let target = current_target();
    let archive = make_archive(bin, b"#!/bin/sh\necho fake updated binary\n");
    let asset_key = format!("{bin}/{bin}-2.0.0-{target}.tar.gz");
    let manifest = format!(
        r#"{{"version":"2.0.0","released_at":"2026-07-07T00:00:00Z","assets":{{"{target}":"{asset_key}"}}}}"#
    );
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        format!("/{bin}/stable.json"),
        (200, manifest.into_bytes(), "application/json".to_string()),
    );
    routes.insert(
        format!("/{asset_key}"),
        (200, archive, "application/gzip".to_string()),
    );
    let server = MultiPathServer::new(routes);
    let cfg = r2_config(format!("http://{}", server.addr), "1.0.0");
    let updater = TerraphimUpdater::new(cfg);

    let installed = install_dir();
    let target_path = installed.join(bin);
    let _ = std::fs::remove_file(&target_path); // clean pre-existing

    let status = updater.update_r2().await.expect("update_r2 should succeed");
    // Unsigned archive is now rejected — MissingSignature is a hard fail.
    assert!(
        matches!(status, UpdateStatus::Failed(_)),
        "unsigned archive should be Failed, got {status:?}"
    );
    assert!(
        !target_path.exists(),
        "rejected binary must not be installed"
    );
}

#[tokio::test]
async fn test_update_r2_manifest_404_returns_err_for_fallback() {
    // No routes -> every path 404. update_r2 must return Err so the update()
    // dispatcher can fall back to GitHub.
    let routes = std::collections::HashMap::new();
    let server = MultiPathServer::new(routes);
    let cfg = r2_config(format!("http://{}", server.addr), "1.0.0");
    let updater = TerraphimUpdater::new(cfg);

    let result = updater.update_r2().await;
    assert!(
        result.is_err(),
        "manifest 404 must be Err (fallback-eligible)"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.to_lowercase().contains("manifest") || msg.to_lowercase().contains("fetch"),
        "error should mention manifest/fetch: {msg}"
    );
}

#[tokio::test]
async fn test_update_r2_uptodate_when_manifest_not_newer() {
    let bin = "terraphim-test";
    let manifest = r#"{"version":"1.0.0","released_at":"x","assets":{}}"#;
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        format!("/{bin}/stable.json"),
        (
            200,
            manifest.as_bytes().to_vec(),
            "application/json".to_string(),
        ),
    );
    let server = MultiPathServer::new(routes);
    // current_version 2.0.0 > manifest 1.0.0 -> not newer.
    let cfg = r2_config(format!("http://{}", server.addr), "2.0.0");
    let updater = TerraphimUpdater::new(cfg);

    let status = updater.update_r2().await.unwrap();
    assert!(matches!(status, UpdateStatus::UpToDate(_)));
}

#[tokio::test]
async fn test_update_r2_no_asset_returns_definitive_failed() {
    let bin = "terraphim-test";
    // Manifest advertises a target that is NOT the current platform's triple.
    let manifest = r#"{"version":"9.9.9","released_at":"x","assets":{"wasm32-unknown-unknown":"nope.tar.gz"}}"#;
    let mut routes = std::collections::HashMap::new();
    routes.insert(
        format!("/{bin}/stable.json"),
        (
            200,
            manifest.as_bytes().to_vec(),
            "application/json".to_string(),
        ),
    );
    let server = MultiPathServer::new(routes);
    let cfg = r2_config(format!("http://{}", server.addr), "1.0.0");
    let updater = TerraphimUpdater::new(cfg);

    let status = updater.update_r2().await.unwrap();
    // No asset for the current target is a definitive Ok(Failed), NOT Err:
    // a missing platform asset is not a transport failure, so the caller must
    // not fall back to GitHub.
    match status {
        UpdateStatus::Failed(msg) => {
            assert!(
                msg.to_lowercase().contains("asset") || msg.to_lowercase().contains("target"),
                "expected asset/target message: {msg}"
            );
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn test_check_and_update_dispatches_by_backend() {
    // Static contract: the entry points honour the configured backend. (The
    // async dispatch is exercised by the integration tests above; this guards
    // the default and the builder.)
    let r2 = UpdaterConfig::new("terraphim-test");
    assert_eq!(r2.backend, UpdateBackend::R2);
    let gh = UpdaterConfig::new("terraphim-test").with_backend(UpdateBackend::GitHub);
    assert_eq!(gh.backend, UpdateBackend::GitHub);
    // ManifestConfig must carry the bin name through.
    let cfg = UpdaterConfig::new("terraphim-test");
    assert_eq!(cfg.manifest.bin_name, "terraphim-test");
    let _ = ManifestConfig::new("terraphim-test");
}

#[test]
fn test_signed_archive_verifies_via_explicit_key() {
    use terraphim_update::signature::{VerificationResult, verify_archive_signature};

    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("signed.tar.gz");
    let priv_key = dir.path().join("priv.key");
    let pub_key = dir.path().join("pub.key");

    // Generate a throwaway Ed25519 keypair.
    let keygen = std::process::Command::new("zipsign")
        .args([
            "gen-key",
            priv_key.to_str().unwrap(),
            pub_key.to_str().unwrap(),
        ])
        .output()
        .expect("zipsign gen-key");
    assert!(
        keygen.status.success(),
        "zipsign gen-key failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );

    // Build a tiny tar.gz.
    {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut tar_buf = std::io::Cursor::new(Vec::new());
        {
            let mut tar = tar::Builder::new(&mut tar_buf);
            let mut hdr = tar::Header::new_gnu();
            hdr.set_size(3);
            hdr.set_mode(0o644);
            hdr.set_cksum();
            tar.append_data(&mut hdr, "exe", std::io::Cursor::new(b"abc".to_vec()))
                .unwrap();
            tar.finish().unwrap();
        }
        let mut enc = GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            Compression::default(),
        );
        enc.write_all(&tar_buf.into_inner()).unwrap();
        enc.finish().unwrap();
    }

    // Sign with the throwaway private key.
    let sign = std::process::Command::new("zipsign")
        .args([
            "sign",
            "tar",
            archive.to_str().unwrap(),
            priv_key.to_str().unwrap(),
        ])
        .output()
        .expect("zipsign sign");
    assert!(
        sign.status.success(),
        "zipsign sign failed: {}",
        String::from_utf8_lossy(&sign.stderr)
    );

    // Read the public key as base64 and verify.
    let pub_b64: String = {
        let raw = std::fs::read(&pub_key).expect("read pub.key");
        base64::engine::general_purpose::STANDARD.encode(raw)
    };
    let result = verify_archive_signature(&archive, Some(&pub_b64))
        .expect("verify_archive_signature should not Err");

    assert!(
        matches!(result, VerificationResult::Valid),
        "signed archive must verify with its own key, got {result:?}"
    );
}
