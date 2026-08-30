//! Integration tests for the manifest backend.
//!
//! Spins up a real local HTTP server (std::net) — no mocks — to exercise
//! `fetch_manifest` against live bytes, retry-on-5xx, and 404 handling.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use terraphim_update::manifest::{ManifestConfig, fetch_manifest, resolve_asset_url};

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
    r#"{
        "version": "1.21.9",
        "released_at": "2026-07-06T17:38:00Z",
        "assets": {
            "x86_64-unknown-linux-gnu": "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-gnu.tar.gz",
            "x86_64-unknown-linux-musl": "terraphim-agent/terraphim-agent-1.21.9-x86_64-unknown-linux-musl.tar.gz",
            "aarch64-unknown-linux-musl": "terraphim-agent/terraphim-agent-1.21.9-aarch64-unknown-linux-musl.tar.gz"
        },
        "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.9"
    }"#
    .to_string()
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
    assert_eq!(manifest.assets.len(), 3);
    assert!(manifest.notes_url.is_some());
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
