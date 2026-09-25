//! Real-binary tests for the package-managed update refusal contract.
//!
//! The packaging lifecycle gates (`.github/scripts/nfpm/tests/
//! test_client_nfpm_native.sh` and `test_client_nfpm_native_actual.sh`)
//! install these binaries via dpkg/rpm and require an explicit `update` to
//! refuse: non-zero exit, an exact stderr line, and no write to the installed
//! executable. `check-update` must keep reporting the managed status on
//! stdout with a zero exit.
//!
//! These tests lock that contract against the real compiled binary using the
//! updater's own receipt detection (`<prefix>/share/terraphim/package-manager.d/
//! <bin-name>` relative to the executable's `<prefix>/bin/<bin-name>` layout):
//! the binary is staged into a temporary prefix with a real receipt file, so
//! no network is touched, no system path is written, and nothing is mocked.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const BIN_NAME: &str = "terraphim-agent";

/// Stage the real compiled binary into `<tmp>/bin/terraphim-agent` with a
/// package-manager receipt beside it, exactly as the DEB/RPM packages lay it
/// out under `/usr`.
fn stage_managed_binary(manager: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let bin_dir = tmp.path().join("bin");
    let receipt_dir = tmp.path().join("share/terraphim/package-manager.d");
    fs::create_dir_all(&bin_dir).expect("create bin dir");
    fs::create_dir_all(&receipt_dir).expect("create receipt dir");
    let bin = bin_dir.join(BIN_NAME);
    fs::copy(env!("CARGO_BIN_EXE_terraphim-agent"), &bin).expect("copy real binary");
    fs::write(receipt_dir.join(BIN_NAME), manager).expect("write receipt");
    (tmp, bin)
}

/// Drive `update` under a receipt and assert the full refusal contract:
/// non-zero exit, the exact stderr line the gates `grep -Fxq` on, and a
/// byte-identical executable afterwards.
fn assert_update_refusal(manager: &str, guidance: &str) {
    let (_tmp, bin) = stage_managed_binary(manager);
    let before = fs::read(&bin).expect("read binary before update");

    let output = Command::new(&bin)
        .arg("update")
        .output()
        .expect("run update");

    assert!(
        !output.status.success(),
        "update under a {} receipt must exit non-zero, got {:?}",
        manager,
        output.status.code()
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = format!(
        "{BIN_NAME} update was refused: [OK] Managed by {manager}; run `{guidance}` to update"
    );
    assert!(
        stderr.lines().any(|line| line == expected),
        "stderr must contain the exact refusal line {expected:?}; stderr:\n{stderr}"
    );

    let after = fs::read(&bin).expect("read binary after update");
    assert_eq!(
        before, after,
        "update under a {manager} receipt must not rewrite the binary"
    );
}

#[test]
fn update_refuses_under_dpkg_receipt() {
    assert_update_refusal("dpkg", "sudo apt update && sudo apt upgrade");
}

#[test]
fn update_refuses_under_rpm_receipt() {
    assert_update_refusal("rpm", "sudo dnf upgrade");
}

#[test]
fn check_update_reports_managed_on_stdout_with_zero_exit() {
    let (_tmp, bin) = stage_managed_binary("dpkg");

    let output = Command::new(&bin)
        .arg("check-update")
        .output()
        .expect("run check-update");

    assert!(
        output.status.success(),
        "check-update under a dpkg receipt must exit zero, got {:?}",
        output.status.code()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = "[OK] Managed by dpkg; run `sudo apt update && sudo apt upgrade` to update";
    assert!(
        stdout.lines().any(|line| line == expected),
        "stdout must contain the exact managed line {expected:?}; stdout:\n{stdout}"
    );
}
