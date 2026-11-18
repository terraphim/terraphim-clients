//! The CLI contract of `terraphim-agent learn install-hook` (#517).
//!
//! The command installs a hook file for the agents that are wired through a
//! shell hook, and prints the extension-package guidance for pi. For pi it
//! installs **nothing**, and that has to be visible in the exit code: a
//! scripted install that sees exit 0 concludes the hook is installed.
//!
//! The tests spawn the real binary against a hermetic HOME, so nothing is
//! written outside the temporary root. No mocks.

mod support;

use std::path::Path;
use std::process::{Command, Output};

use support::cli_test_env::{create_hermetic_root, set_hermetic_env};

/// Run `learn install-hook <agent>` against a hermetic HOME.
fn install_hook(agent: &str) -> (Output, std::path::PathBuf) {
    let root = create_hermetic_root().expect("hermetic root");
    let mut command = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    command.args(["learn", "install-hook", agent]);
    set_hermetic_env(&mut command, &root).expect("hermetic environment");
    let output = command.output().expect("run the agent binary");
    (output, root)
}

/// Every file under the hermetic root, relative to it.
fn files_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(relative) = path.strip_prefix(root) {
                found.push(relative.to_string_lossy().to_string());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn install_hook_pi_exits_non_zero_and_installs_nothing() {
    let (output, root) = install_hook("pi");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_ne!(
        output.status.code(),
        Some(0),
        "`learn install-hook pi` installed nothing, so it must not report success\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert_eq!(
        output.status.code(),
        Some(1),
        "the failure has to be the taxonomy's general error (1); the exit-code taxonomy \
         belongs to Gitea #181 and this change must not mint a new code\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // The guidance is still printed: it is the installation contract.
    assert!(
        stdout.contains("pi install"),
        "the pi guidance must still be printed\nstdout: {stdout}"
    );

    // And the failure says what a caller has to do.
    assert!(
        stderr.contains("was not installed"),
        "the error must say that nothing was installed\nstderr: {stderr}"
    );

    // The hermetic environment itself writes the settings the agent reads;
    // what must not exist is a hook.
    let installed = files_under(&root);
    let hooks: Vec<&String> = installed
        .iter()
        .filter(|path| path.contains("terraphim-hook.sh") || path.contains(".pi"))
        .collect();
    assert!(
        hooks.is_empty(),
        "`learn install-hook pi` must not install anything, but it wrote: {hooks:?} \
         (the files under the root: {installed:?})"
    );
}

#[test]
fn install_hook_claude_installs_the_hook_and_exits_zero() {
    // The control: the agents that do have a shell hook still install it and
    // still report success. Refs #517.
    let (output, root) = install_hook("claude");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "`learn install-hook claude` installs a real hook and must exit 0\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    let installed = files_under(&root);
    assert!(
        installed
            .iter()
            .any(|path| path.ends_with("claude/terraphim-hook.sh")),
        "the hook must be written under the hermetic home, found: {installed:?}"
    );
}
