//! Resolution rules for the external `terraphim_server` binary.
//!
//! The rules are pure so they can be asserted without touching the process
//! environment: mutating the environment in a test is `unsafe` and this
//! package has suites that run concurrently (terraphim-clients#466).

mod support;

use std::path::PathBuf;
use support::binary::resolve_server_binary;

#[test]
fn an_environment_value_that_points_at_a_file_wins() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let binary = tmp.path().join("terraphim_server");
    std::fs::write(&binary, "#!/bin/sh\n").expect("write fake binary");
    let fallback = tmp.path().join("target/debug/terraphim_server");
    std::fs::create_dir_all(fallback.parent().expect("parent")).expect("create fallback dir");
    std::fs::write(&fallback, "#!/bin/sh\n").expect("write fake fallback");

    let resolved = resolve_server_binary(Some(binary.to_str().expect("utf8 path")), &fallback);

    assert_eq!(resolved, Some(binary));
}

#[test]
fn the_fallback_is_used_when_no_environment_value_is_set() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fallback = tmp.path().join("terraphim_server");
    std::fs::write(&fallback, "#!/bin/sh\n").expect("write fake fallback");

    let resolved = resolve_server_binary(None, &fallback);

    assert_eq!(resolved, Some(fallback));
}

#[test]
fn an_environment_value_that_points_at_a_missing_file_is_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let missing = tmp.path().join("not-installed");
    let fallback = tmp.path().join("terraphim_server");
    std::fs::write(&fallback, "#!/bin/sh\n").expect("write fake fallback");

    // A misconfigured environment value must not silently fall back to a
    // different binary; the test skips instead.
    let resolved = resolve_server_binary(Some(missing.to_str().expect("utf8 path")), &fallback);

    assert_eq!(resolved, None);
}

#[test]
fn a_missing_fallback_without_an_environment_value_is_absent() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fallback = tmp.path().join("terraphim_server");

    assert_eq!(resolve_server_binary(None, &fallback), None);
}

#[test]
fn a_directory_is_not_a_usable_binary() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let directory = tmp.path().join("terraphim_server");
    std::fs::create_dir_all(&directory).expect("create directory");

    assert_eq!(
        resolve_server_binary(None, &directory),
        None,
        "a directory must not count as a server binary"
    );
}

#[test]
fn the_resolved_path_is_usable_as_a_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let binary = tmp.path().join("terraphim_server");
    std::fs::write(&binary, "#!/bin/sh\n").expect("write fake binary");

    let resolved: PathBuf =
        resolve_server_binary(Some(binary.to_str().expect("utf8 path")), &binary)
            .expect("binary resolves");

    assert!(resolved.is_file());
}
