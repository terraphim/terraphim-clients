//! No-network / no-write tests for `UpdatePolicy::PackageManaged` (Gitea
//! #247), exercising `TerraphimUpdater` end-to-end under an *injected*
//! policy via `UpdaterConfig::with_policy`. None of these tests depend on
//! real `/usr` state.
//!
//! - A real local `std::net::TcpListener` server (no mocks) counts every
//!   accepted connection, proving `check_update()`/`update()`/
//!   `check_and_update()` make zero network requests when the policy is
//!   `PackageManaged`. A `SelfManaged` control on the same harness proves the
//!   harness actually observes real requests (so a zero count isn't
//!   trivially true).
//! - A zero-write assertion against the real install destination
//!   (`current_exe().parent()/<bin_name>`, the same path
//!   `install_verified_archive` would write to) proves no install occurs.

use std::io::Read;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use serial_test::serial;
use terraphim_update::policy::{PackageManager, UpdatePolicy};
use terraphim_update::{
    TerraphimUpdater, UpdateStatus, UpdaterConfig, check_for_updates_auto_with_policy,
};

/// A real local HTTP server that counts every accepted connection and
/// replies 404 to everything (the content doesn't matter -- what matters is
/// whether a connection was ever made).
struct CountingServer {
    addr: String,
    count: Arc<AtomicUsize>,
}

impl CountingServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                count_clone.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                use std::io::Write;
                let body = b"not found";
                let resp = format!(
                    "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(body);
            }
        });
        Self { addr, count }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn request_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

fn package_managed_policy() -> UpdatePolicy {
    UpdatePolicy::PackageManaged {
        manager: PackageManager::Pacman,
        update_command: "sudo pacman -Syu".to_string(),
    }
}

/// The real destination `install_verified_archive` writes to for a given
/// `bin_name`: `current_exe().parent()/<bin_name>` (and its hyphenated
/// variant). This is the narrowest real (non-faked) test seam available
/// today -- `UpdaterConfig` has no injectable destination path, so we assert
/// against the actual path production code would use.
fn install_destination_candidates(bin_name: &str) -> Vec<std::path::PathBuf> {
    let dir = std::env::current_exe()
        .expect("current_exe")
        .parent()
        .expect("parent")
        .to_path_buf();
    vec![dir.join(bin_name), dir.join(bin_name.replace('_', "-"))]
}

#[tokio::test]
async fn package_managed_check_update_makes_zero_requests() {
    let server = CountingServer::start();
    let bin_name = "terraphim-managed-mode-test-check";
    let config = UpdaterConfig::new(bin_name)
        .with_manifest_base_url(server.base_url())
        .with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = updater.check_update().await.expect("check_update");
    assert!(
        matches!(status, UpdateStatus::PackageManaged { .. }),
        "expected PackageManaged, got {status:?}"
    );
    assert_eq!(
        server.request_count(),
        0,
        "check_update must make zero network requests when package-managed"
    );
}

#[tokio::test]
async fn package_managed_update_makes_zero_requests_and_zero_writes() {
    let server = CountingServer::start();
    let bin_name = "terraphim-managed-mode-test-update";
    let destinations = install_destination_candidates(bin_name);
    for dest in &destinations {
        assert!(!dest.exists(), "precondition: {dest:?} must not exist");
    }

    let config = UpdaterConfig::new(bin_name)
        .with_manifest_base_url(server.base_url())
        .with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = updater.update().await.expect("update");
    assert!(
        matches!(status, UpdateStatus::PackageManaged { .. }),
        "expected PackageManaged, got {status:?}"
    );
    assert_eq!(
        server.request_count(),
        0,
        "update must make zero network requests when package-managed"
    );
    for dest in &destinations {
        assert!(
            !dest.exists(),
            "update must not write {dest:?} when package-managed"
        );
    }
}

#[tokio::test]
async fn package_managed_check_and_update_makes_zero_requests_and_zero_writes() {
    let server = CountingServer::start();
    let bin_name = "terraphim-managed-mode-test-full";
    let destinations = install_destination_candidates(bin_name);
    for dest in &destinations {
        assert!(!dest.exists(), "precondition: {dest:?} must not exist");
    }

    let config = UpdaterConfig::new(bin_name)
        .with_manifest_base_url(server.base_url())
        .with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = updater.check_and_update().await.expect("check_and_update");
    assert!(
        matches!(status, UpdateStatus::PackageManaged { .. }),
        "expected PackageManaged, got {status:?}"
    );
    assert_eq!(
        server.request_count(),
        0,
        "check_and_update must make zero network requests when package-managed"
    );
    for dest in &destinations {
        assert!(
            !dest.exists(),
            "check_and_update must not write {dest:?} when package-managed"
        );
    }
}

/// Regression control: proves the harness actually observes real requests,
/// so the zero-count assertions above aren't trivially true. Uses the
/// default (`SelfManaged` in this test environment) policy against the same
/// counting server.
#[tokio::test]
async fn self_managed_check_update_makes_a_request_control() {
    let server = CountingServer::start();
    let config = UpdaterConfig::new("terraphim-managed-mode-control")
        .with_manifest_base_url(server.base_url());
    assert_eq!(
        config.policy,
        UpdatePolicy::SelfManaged,
        "test environment must not accidentally resolve to PackageManaged"
    );
    let updater = TerraphimUpdater::new(config);

    // The manifest fetch will fail (404, not valid JSON) but the important
    // thing is that a real request was made.
    let _ = updater.check_update().await;
    assert!(
        server.request_count() > 0,
        "expected the SelfManaged control to make at least one real request"
    );
}

/// Message contract: `Display` for the returned `UpdateStatus::PackageManaged`
/// (and `policy::guidance`) must contain the manager's real update command.
#[tokio::test]
async fn package_managed_status_display_contains_pacman_command() {
    let server = CountingServer::start();
    let config = UpdaterConfig::new("terraphim-managed-mode-message")
        .with_manifest_base_url(server.base_url())
        .with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = updater.check_update().await.expect("check_update");
    assert!(status.to_string().contains("sudo pacman -Syu"));
}

/// Direct-call regression for `TerraphimUpdater::update_r2` (Gitea #247
/// review finding: "shared updater API policy must be fail-closed for
/// managed mode"). `check_and_update`/`update` are already guarded, but
/// `update_r2` is itself `pub` and independently callable -- a caller that
/// reaches it directly (bypassing the outer dispatch) must still get the
/// refusal, not a live install.
#[tokio::test]
async fn package_managed_update_r2_direct_call_makes_zero_requests_and_zero_writes() {
    let server = CountingServer::start();
    let bin_name = "terraphim-managed-mode-test-update-r2-direct";
    let destinations = install_destination_candidates(bin_name);
    for dest in &destinations {
        assert!(!dest.exists(), "precondition: {dest:?} must not exist");
    }

    let config = UpdaterConfig::new(bin_name)
        .with_manifest_base_url(server.base_url())
        .with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = updater.update_r2().await.expect("update_r2");
    assert!(
        matches!(status, UpdateStatus::PackageManaged { .. }),
        "expected PackageManaged, got {status:?}"
    );
    assert_eq!(
        server.request_count(),
        0,
        "update_r2 must make zero network requests when package-managed"
    );
    for dest in &destinations {
        assert!(
            !dest.exists(),
            "update_r2 must not write {dest:?} when package-managed"
        );
    }
}

/// Direct-call regression for `TerraphimUpdater::check_update_r2`, the R2
/// counterpart to the `update_r2` test above.
#[tokio::test]
async fn package_managed_check_update_r2_direct_call_makes_zero_requests() {
    let server = CountingServer::start();
    let config = UpdaterConfig::new("terraphim-managed-mode-test-check-r2-direct")
        .with_manifest_base_url(server.base_url())
        .with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = updater.check_update_r2().await.expect("check_update_r2");
    assert!(
        matches!(status, UpdateStatus::PackageManaged { .. }),
        "expected PackageManaged, got {status:?}"
    );
    assert_eq!(
        server.request_count(),
        0,
        "check_update_r2 must make zero network requests when package-managed"
    );
}

/// P2 regression: `update_with_verification` is a separate public entry
/// point from `check_and_update`/`update` (used by the GitHub-backend
/// install path) and must independently honor `self.config.policy`. Bounded
/// with a timeout: if the guard regresses, this reaches the real self_update
/// GitHub backend (no injectable base URL exists for it), which would hang
/// or fail slowly in a network-isolated sandbox rather than return
/// instantly.
#[tokio::test]
async fn package_managed_update_with_verification_makes_zero_writes() {
    let bin_name = "terraphim-managed-mode-test-verify";
    let destinations = install_destination_candidates(bin_name);
    for dest in &destinations {
        assert!(!dest.exists(), "precondition: {dest:?} must not exist");
    }

    let config = UpdaterConfig::new(bin_name).with_policy(package_managed_policy());
    let updater = TerraphimUpdater::new(config);

    let status = tokio::time::timeout(Duration::from_secs(5), updater.update_with_verification())
        .await
        .expect("update_with_verification must short-circuit instantly, not reach the network")
        .expect("update_with_verification");
    assert!(
        matches!(status, UpdateStatus::PackageManaged { .. }),
        "expected PackageManaged, got {status:?}"
    );
    for dest in &destinations {
        assert!(
            !dest.exists(),
            "update_with_verification must not write {dest:?} when package-managed"
        );
    }
}

/// P1 regression: `check_for_updates_auto` (and therefore every caller that
/// invokes it -- REPL `/update check`, `terraphim-cli check-update`,
/// `check_for_updates_startup`, the update scheduler's periodic check) must
/// short-circuit on a package-managed policy before ever touching
/// `platform::get_binary_path` (which can create `~/.local/bin`) or the
/// self_update GitHub backend. `check_for_updates_auto_with_policy` is the
/// injectable seam this test exercises directly with a fake, temp-dir-rooted
/// `HOME` so the zero-write assertion is real and doesn't depend on the
/// sandbox's actual home directory already lacking `.local/bin`.
#[tokio::test]
#[serial(update_home_env)]
async fn package_managed_check_for_updates_auto_with_policy_makes_no_home_writes() {
    let temp_home = tempfile::tempdir().expect("tempdir");
    let original_home = std::env::var("HOME").ok();
    // SAFETY: serialized via #[serial] within this test binary; no other
    // thread in this process mutates HOME concurrently.
    unsafe {
        std::env::set_var("HOME", temp_home.path());
    }
    let local_bin = temp_home.path().join(".local/bin");
    assert!(
        !local_bin.exists(),
        "precondition: fake HOME must start without .local/bin"
    );

    let policy = package_managed_policy();
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        check_for_updates_auto_with_policy("terraphim-managed-mode-fake-bin", "0.0.1", &policy),
    )
    .await;

    // SAFETY: restore before any assertion can panic and unwind past this.
    unsafe {
        match &original_home {
            Some(home) => std::env::set_var("HOME", home),
            None => std::env::remove_var("HOME"),
        }
    }

    let status = result
        .expect("check_for_updates_auto_with_policy must short-circuit instantly, not reach the network")
        .expect("check_for_updates_auto_with_policy");
    match status {
        UpdateStatus::PackageManaged { update_command, .. } => {
            assert!(
                update_command.contains("sudo pacman -Syu"),
                "unexpected update command: {update_command}"
            );
        }
        other => panic!("expected PackageManaged, got {other:?}"),
    }
    assert!(
        !local_bin.exists(),
        "check_for_updates_auto_with_policy must not create ~/.local/bin when package-managed"
    );
}
