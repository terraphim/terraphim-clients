//! Fake-root tests for per-binary package-manager receipt detection.

use std::fs;
use std::path::{Path, PathBuf};

use terraphim_update::policy::{
    PackageManager, UpdatePolicy, detect_update_policy, inferred_prefix,
};

fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, contents).expect("write file");
}

fn install_binary(root: &Path, prefix: &str, bin_name: &str) -> (PathBuf, PathBuf) {
    let prefix = root.join(prefix);
    let exe = prefix.join("bin").join(bin_name);
    write_file(&exe, b"binary");
    (prefix, exe)
}

fn write_receipt(prefix: &Path, bin_name: &str, contents: &[u8]) {
    write_file(
        &prefix
            .join("share/terraphim/package-manager.d")
            .join(bin_name),
        contents,
    );
}

fn assert_managed(policy: UpdatePolicy, manager: PackageManager, command: &str) {
    match policy {
        UpdatePolicy::PackageManaged {
            manager: actual,
            update_command,
        } => {
            assert_eq!(actual, manager);
            assert_eq!(update_command, command);
        }
        other => panic!("expected PackageManaged, got {other:?}"),
    }
}

#[test]
fn per_binary_agent_receipt_under_resolved_prefix_is_package_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    write_receipt(&prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&exe);

    assert_managed(policy, PackageManager::Pacman, "sudo pacman -Syu");
}

#[test]
fn per_binary_grep_receipt_under_resolved_prefix_is_package_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-grep");
    write_receipt(&prefix, "terraphim-grep", b"dpkg\n");

    let policy = detect_update_policy(&exe);

    assert_managed(
        policy,
        PackageManager::Dpkg,
        "sudo apt update && sudo apt upgrade",
    );
}

#[test]
fn all_supported_receipt_values_are_accepted() {
    let cases = [
        (
            b"pacman".as_slice(),
            PackageManager::Pacman,
            "sudo pacman -Syu",
        ),
        (
            b"pacman\n".as_slice(),
            PackageManager::Pacman,
            "sudo pacman -Syu",
        ),
        (
            b"pacman\r\n".as_slice(),
            PackageManager::Pacman,
            "sudo pacman -Syu",
        ),
        (
            b"dpkg\n".as_slice(),
            PackageManager::Dpkg,
            "sudo apt update && sudo apt upgrade",
        ),
        (
            b"dpkg".as_slice(),
            PackageManager::Dpkg,
            "sudo apt update && sudo apt upgrade",
        ),
        (
            b"dpkg\r\n".as_slice(),
            PackageManager::Dpkg,
            "sudo apt update && sudo apt upgrade",
        ),
        (b"rpm".as_slice(), PackageManager::Rpm, "sudo dnf upgrade"),
        (b"rpm\n".as_slice(), PackageManager::Rpm, "sudo dnf upgrade"),
        (
            b"rpm\r\n".as_slice(),
            PackageManager::Rpm,
            "sudo dnf upgrade",
        ),
        (
            b"homebrew".as_slice(),
            PackageManager::Homebrew,
            "brew upgrade terraphim-agent",
        ),
        (
            b"homebrew\n".as_slice(),
            PackageManager::Homebrew,
            "brew upgrade terraphim-agent",
        ),
        (
            b"homebrew\r\n".as_slice(),
            PackageManager::Homebrew,
            "brew upgrade terraphim-agent",
        ),
    ];

    for (contents, manager, command) in cases {
        let root = tempfile::tempdir().expect("tempdir");
        let (prefix, exe) = install_binary(root.path(), "opt/terraphim", "terraphim-agent");
        write_receipt(&prefix, "terraphim-agent", contents);

        let policy = detect_update_policy(&exe);

        assert_managed(policy, manager, command);
    }
}

#[test]
fn homebrew_update_command_uses_actual_agent_binary_name() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "opt/terraphim", "terraphim-agent");
    write_receipt(&prefix, "terraphim-agent", b"homebrew\n");

    let policy = detect_update_policy(&exe);

    assert_managed(
        policy,
        PackageManager::Homebrew,
        "brew upgrade terraphim-agent",
    );
}

#[test]
fn homebrew_update_command_uses_actual_grep_binary_name() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "opt/terraphim", "terraphim-grep");
    write_receipt(&prefix, "terraphim-grep", b"homebrew\n");

    let policy = detect_update_policy(&exe);

    assert_managed(
        policy,
        PackageManager::Homebrew,
        "brew upgrade terraphim-grep",
    );
}

#[test]
fn missing_receipt_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (_prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");

    let policy = detect_update_policy(&exe);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn malformed_receipts_are_self_managed() {
    let invalid_contents: &[&[u8]] = &[
        b"",
        b" pacman",
        b"pacman ",
        b"pacman\t",
        b"pacman\n\n",
        b"pacman\r\n\r\n",
        b"pacman\n ",
        b"pacman\t\n",
        b"pacman\nextra",
        b"pacman extra",
        b"pacman\0",
        b"pacman\xff",
        b"dpkg ",
        b"rpm\n\n",
        b"homebrew\n\n",
        b"homebrew\r\n\r\n",
        b"PACMAN",
        b"Pacman",
        b"pacmanx",
        b" pacman stuff ",
        b"apt",
    ];

    for contents in invalid_contents {
        let root = tempfile::tempdir().expect("tempdir");
        let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
        write_receipt(&prefix, "terraphim-agent", contents);

        let policy = detect_update_policy(&exe);

        assert_eq!(
            policy,
            UpdatePolicy::SelfManaged,
            "expected SelfManaged for receipt content {contents:?}"
        );
    }
}

#[test]
fn obsolete_global_marker_without_per_binary_receipt_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    write_file(&prefix.join("share/terraphim/package-manager"), b"pacman\n");

    let policy = detect_update_policy(&exe);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn prefix_only_without_receipt_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (_prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");

    let policy = detect_update_policy(&exe);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn unrelated_per_binary_receipt_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    write_receipt(&prefix, "terraphim-grep", b"pacman\n");

    let policy = detect_update_policy(&exe);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn receipt_under_different_prefix_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (_prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    let other_prefix = root.path().join("opt/terraphim");
    write_receipt(&other_prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&exe);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn binary_name_mismatch_is_self_managed_even_with_receipt() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    write_receipt(&prefix, "terraphim-grep", b"pacman\n");

    let policy = detect_update_policy(&exe);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn actual_hyphenated_executable_receipt_cannot_be_bypassed_by_caller_spelling() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    write_receipt(&prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&exe);

    assert_managed(policy, PackageManager::Pacman, "sudo pacman -Syu");
}

#[test]
fn traversal_resolving_into_prefix_is_package_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, exe) = install_binary(root.path(), "usr", "terraphim-agent");
    let other = prefix.join("other");
    fs::create_dir_all(&other).unwrap();
    let traversal_exe = other.join("..").join("bin").join("terraphim-agent");
    assert_eq!(fs::canonicalize(&traversal_exe).unwrap(), exe);
    write_receipt(&prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&traversal_exe);

    assert_managed(policy, PackageManager::Pacman, "sudo pacman -Syu");
}

#[cfg(unix)]
#[test]
fn symlink_resolving_into_prefix_is_package_managed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, real_exe) = install_binary(root.path(), "usr", "terraphim-agent");
    let link_dir = root.path().join("home/user/bin");
    fs::create_dir_all(&link_dir).unwrap();
    let link = link_dir.join("terraphim-agent");
    symlink(&real_exe, &link).expect("symlink");
    write_receipt(&prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&link);

    assert_managed(policy, PackageManager::Pacman, "sudo pacman -Syu");
}

#[cfg(unix)]
#[test]
fn symlink_resolving_outside_receipt_prefix_is_self_managed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let (managed_prefix, _managed_exe) = install_binary(root.path(), "usr", "terraphim-agent");
    let (_outside_prefix, outside_exe) =
        install_binary(root.path(), "home/user/.local", "terraphim-agent");
    let link = managed_prefix.join("bin/terraphim-agent-link");
    symlink(&outside_exe, &link).expect("symlink");
    write_receipt(&managed_prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&link);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[cfg(unix)]
#[test]
fn dangling_symlink_executable_never_panics() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let (prefix, _exe) = install_binary(root.path(), "usr", "terraphim-agent");
    let link = prefix.join("bin/terraphim-agent-dangling");
    let nonexistent_target: PathBuf = root.path().join("nowhere/terraphim-agent");
    symlink(&nonexistent_target, &link).expect("symlink");
    write_receipt(&prefix, "terraphim-agent-dangling", b"pacman\n");

    let policy = detect_update_policy(&link);

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[cfg(unix)]
#[test]
fn symlinked_bin_directory_uses_resolved_prefix_receipt() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let (real_prefix, _real_exe) = install_binary(root.path(), "opt/terraphim", "terraphim-agent");
    let link_prefix = root.path().join("usr");
    fs::create_dir_all(&link_prefix).unwrap();
    symlink(real_prefix.join("bin"), link_prefix.join("bin")).expect("symlink bin dir");
    write_receipt(&real_prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&link_prefix.join("bin/terraphim-agent"));

    assert_managed(policy, PackageManager::Pacman, "sudo pacman -Syu");
}

#[cfg(unix)]
#[test]
fn symlinked_bin_directory_ignores_link_prefix_receipt() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let (real_prefix, _real_exe) = install_binary(root.path(), "opt/terraphim", "terraphim-agent");
    let link_prefix = root.path().join("usr");
    fs::create_dir_all(&link_prefix).unwrap();
    symlink(real_prefix.join("bin"), link_prefix.join("bin")).expect("symlink bin dir");
    write_receipt(&link_prefix, "terraphim-agent", b"pacman\n");

    let policy = detect_update_policy(&link_prefix.join("bin/terraphim-agent"));

    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn inferred_prefix_strips_trailing_bin_and_binary_name() {
    let exe =
        Path::new("/home/linuxbrew/.linuxbrew/Cellar/terraphim-agent/1.2.3/bin/terraphim-agent");

    let prefix = inferred_prefix(exe);

    assert_eq!(
        prefix,
        Some(PathBuf::from(
            "/home/linuxbrew/.linuxbrew/Cellar/terraphim-agent/1.2.3"
        ))
    );
}

#[test]
fn guidance_message_contains_manager_update_command() {
    let policy = UpdatePolicy::PackageManaged {
        manager: PackageManager::Rpm,
        update_command: "sudo dnf upgrade".to_string(),
    };
    let msg = terraphim_update::policy::guidance(&policy, "terraphim-agent");
    assert!(
        msg.contains("sudo dnf upgrade"),
        "guidance message missing update command: {msg}"
    );
}
