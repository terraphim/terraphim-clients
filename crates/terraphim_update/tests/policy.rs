//! Fake-root tests for the pure `detect_update_policy` function (Gitea #247).
//!
//! Every test builds a `tempfile::TempDir` containing stand-in marker file
//! and `bin/`-equivalent directories, and calls `detect_update_policy`
//! directly with paths rooted in that tempdir. None of these tests touch the
//! real `/usr` tree or process environment.

use std::fs;
use std::path::{Path, PathBuf};

use terraphim_update::policy::{PackageManager, UpdatePolicy, detect_update_policy};

/// Create a file with the given contents, creating parent directories first.
fn write_file(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(path, contents).expect("write file");
}

#[test]
fn valid_marker_and_matching_prefix_is_package_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let exe = usr_bin.join("terraphim-agent");
    write_file(&exe, b"binary");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&exe, &marker, &prefixes);

    match policy {
        UpdatePolicy::PackageManaged {
            manager,
            update_command,
        } => {
            assert_eq!(manager, PackageManager::Pacman);
            assert_eq!(update_command, "sudo pacman -Syu");
        }
        other => panic!("expected PackageManaged, got {other:?}"),
    }
}

#[test]
fn marker_only_without_matching_prefix_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    // Executable lives outside any managed prefix (stand-in ~/.local/bin).
    let local_bin = root.path().join("home/user/.local/bin");
    fs::create_dir_all(&local_bin).unwrap();
    let exe = local_bin.join("terraphim-agent");
    write_file(&exe, b"binary");

    // Marker is valid.
    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];

    let policy = detect_update_policy(&exe, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn prefix_only_without_marker_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let exe = usr_bin.join("terraphim-agent");
    write_file(&exe, b"binary");

    // Marker absent entirely.
    let marker = root.path().join("share/terraphim/package-manager");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&exe, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn empty_marker_with_matching_prefix_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let exe = usr_bin.join("terraphim-agent");
    write_file(&exe, b"binary");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&exe, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn invalid_marker_content_is_self_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let exe = usr_bin.join("terraphim-agent");
    write_file(&exe, b"binary");
    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];

    let invalid_contents: &[&[u8]] = &[
        b"dpkg",           // unsupported manager
        b"pacman\nextra",  // multiple lines / trailing garbage
        b"PACMAN",         // wrong case
        b"pacmanx",        // partial/prefix match, not exact
        b" pacman stuff ", // trailing garbage around a valid token
    ];

    for (i, contents) in invalid_contents.iter().enumerate() {
        let marker = root.path().join(format!("share/terraphim/marker-{i}"));
        write_file(&marker, contents);
        let policy = detect_update_policy(&exe, &marker, &prefixes);
        assert_eq!(
            policy,
            UpdatePolicy::SelfManaged,
            "expected SelfManaged for marker content {contents:?}"
        );
    }
}

#[test]
fn traversal_resolving_into_prefix_is_package_managed() {
    let root = tempfile::tempdir().expect("tempdir");
    let usr = root.path().join("usr");
    let usr_bin = usr.join("bin");
    let usr_other = usr.join("other");
    fs::create_dir_all(&usr_bin).unwrap();
    fs::create_dir_all(&usr_other).unwrap();
    let exe = usr_bin.join("terraphim-agent");
    write_file(&exe, b"binary");

    // Path expressed via `..`-traversal that still resolves into usr_bin.
    let traversal_exe = usr_other.join("..").join("bin").join("terraphim-agent");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&traversal_exe, &marker, &prefixes);

    assert!(
        matches!(
            policy,
            UpdatePolicy::PackageManaged {
                manager: PackageManager::Pacman,
                ..
            }
        ),
        "expected PackageManaged, got {policy:?}"
    );
}

#[cfg(unix)]
#[test]
fn symlink_resolving_into_prefix_is_package_managed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let real_exe = usr_bin.join("terraphim-agent");
    write_file(&real_exe, b"binary");

    let link_dir = root.path().join("home/user/bin");
    fs::create_dir_all(&link_dir).unwrap();
    let link = link_dir.join("terraphim-agent-link");
    symlink(&real_exe, &link).expect("symlink");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&link, &marker, &prefixes);

    assert!(
        matches!(
            policy,
            UpdatePolicy::PackageManaged {
                manager: PackageManager::Pacman,
                ..
            }
        ),
        "expected PackageManaged, got {policy:?}"
    );
}

#[cfg(unix)]
#[test]
fn symlink_resolving_outside_prefix_is_self_managed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();

    let outside_dir = root.path().join("home/user/.local/bin");
    fs::create_dir_all(&outside_dir).unwrap();
    let outside_target = outside_dir.join("terraphim-agent");
    write_file(&outside_target, b"binary");

    let link = usr_bin.join("terraphim-agent-link");
    symlink(&outside_target, &link).expect("symlink");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&link, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn sibling_prefix_string_match_is_self_managed() {
    // `/usr/bin-evil` must never be treated as a descendant of `/usr/bin`:
    // this pins that the comparison is path-component-wise, not a raw
    // string `starts_with`.
    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    let usr_bin_evil = root.path().join("usr/bin-evil");
    fs::create_dir_all(&usr_bin).unwrap();
    fs::create_dir_all(&usr_bin_evil).unwrap();
    let exe = usr_bin_evil.join("terraphim-agent");
    write_file(&exe, b"binary");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&exe, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn missing_marker_file_never_panics() {
    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();
    let exe = usr_bin.join("terraphim-agent");
    write_file(&exe, b"binary");

    // Marker's parent directories don't even exist.
    let marker = root
        .path()
        .join("nonexistent/deeply/nested/package-manager");
    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];

    let policy = detect_update_policy(&exe, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[cfg(unix)]
#[test]
fn dangling_symlink_executable_never_panics() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().expect("tempdir");
    let usr_bin = root.path().join("usr/bin");
    fs::create_dir_all(&usr_bin).unwrap();

    let link = usr_bin.join("terraphim-agent-dangling");
    let nonexistent_target: PathBuf = root.path().join("nowhere/terraphim-agent");
    symlink(&nonexistent_target, &link).expect("symlink");

    let marker = root.path().join("share/terraphim/package-manager");
    write_file(&marker, b"pacman\n");

    let prefixes = [(PackageManager::Pacman, usr_bin.as_path())];
    let policy = detect_update_policy(&link, &marker, &prefixes);
    assert_eq!(policy, UpdatePolicy::SelfManaged);
}

#[test]
fn guidance_message_contains_pacman_update_command() {
    let policy = UpdatePolicy::PackageManaged {
        manager: PackageManager::Pacman,
        update_command: "sudo pacman -Syu".to_string(),
    };
    let msg = terraphim_update::policy::guidance(&policy, "terraphim-agent");
    assert!(
        msg.contains("sudo pacman -Syu"),
        "guidance message missing update command: {msg}"
    );
}
