//! The one test that mutates `HOME` lives in its own binary, on purpose.
//!
//! `std::env::set_var` is `unsafe`: it is not thread safe, and any other
//! thread in the same process may read the variable while it is being
//! replaced (on glibc, `getenv` is unlocked while `setenv` may realloc
//! `__environ`). `#[serial]` does not close that hole -- it only excludes
//! other `#[serial]` tests that share the same key, not the unannotated
//! siblings in the same binary (terraphim-clients#374).
//!
//! Moving the test into a binary that contains **no other test** removes the
//! concurrent readers entirely, which makes the SAFETY claim below true
//! instead of aspirational. The `ci_guards` arm
//! `home_mutating_test_file_holds_a_single_test` keeps the property from
//! decaying when a second test is added to this file.
//!
//! The test itself is the P1 regression: `check_for_updates_auto` (and every
//! caller that invokes it -- REPL `/update check`, `terraphim-cli
//! check-update`, `check_for_updates_startup`, the update scheduler's periodic
//! check) must short-circuit on a package-managed policy before ever touching
//! `platform::get_binary_path` (which can create `~/.local/bin`) or the
//! self_update GitHub backend. `check_for_updates_auto_with_policy` is the
//! injectable seam this test exercises directly with a fake, temp-dir-rooted
//! `HOME` so the zero-write assertion is real and does not depend on the
//! sandbox's actual home directory already lacking `.local/bin`.

use std::time::Duration;

use serial_test::serial;
use terraphim_update::policy::{PackageManager, UpdatePolicy};
use terraphim_update::{UpdateStatus, check_for_updates_auto_with_policy};

fn package_managed_policy() -> UpdatePolicy {
    UpdatePolicy::PackageManaged {
        manager: PackageManager::Pacman,
        update_command: "sudo pacman -Syu".to_string(),
    }
}

#[tokio::test]
#[serial]
async fn package_managed_check_for_updates_auto_with_policy_makes_no_home_writes() {
    let temp_home = tempfile::tempdir().expect("tempdir");
    let original_home = std::env::var("HOME").ok();
    // SAFETY: this binary contains exactly one test, so no other thread in
    // this process can read or write HOME concurrently. The ci_guards arm
    // `home_mutating_test_file_holds_a_single_test` keeps that true.
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
