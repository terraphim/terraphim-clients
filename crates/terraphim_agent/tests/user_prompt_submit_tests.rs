//! Integration tests for user-prompt-submit hook patterns.
//!
//! Tests that `terraphim-agent learn hook --learn-hook-type user-prompt-submit`
//! correctly captures tool preference corrections from user prompts and writes
//! `CorrectionType::ToolPreference` files.
//!
//! These tests are hermetic: each test steers the agent binary's data dir
//! through `TERRAPHIM_DEFAULT_DATA_PATH` (which the production hook honours
//! via `LearningCaptureConfig::default()`, Refs #144), so the test reads back
//! from the same path the hook writes to. This avoids platform-specific
//! `dirs::data_dir()` behaviour (macOS/Windows ignore `XDG_DATA_HOME`).

mod support;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use support::cli_test_env::{create_hermetic_root, set_hermetic_env};

fn agent_binary() -> String {
    if let Ok(bin) = std::env::var("TERRAPHIM_AGENT_BIN") {
        return bin;
    }

    let output = Command::new("cargo")
        .args(["build", "-p", "terraphim_agent"])
        .output()
        .expect("cargo build should succeed");
    if !output.status.success() {
        panic!(
            "cargo build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    workspace_root
        .join("target/debug/terraphim-agent")
        .to_string_lossy()
        .to_string()
}

/// Derive the learnings dir from the same env var the helper sets on the
/// spawned cmd. The hook (post-#144) uses this var via
/// `LearningCaptureConfig::default()` to compute `global_dir`.
fn hermetic_learnings_dir(root: &Path) -> PathBuf {
    root.join("data").join("terraphim").join("learnings")
}

/// Run the user-prompt-submit hook with a JSON payload, returning whether it succeeded.
fn run_user_prompt_submit(binary: &str, prompt: &str, root: &PathBuf) -> bool {
    let json = format!(r#"{{"user_prompt":"{}"}}"#, prompt);
    let mut cmd = Command::new(binary);
    cmd.args(["learn", "hook", "--learn-hook-type", "user-prompt-submit"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    set_hermetic_env(&mut cmd, root).expect("set hermetic env");
    let output = cmd
        .spawn()
        .expect("should spawn hook process")
        .communicate(json.into_bytes())
        .expect("should communicate with hook process");

    output.status.success()
}

trait Communicate {
    fn communicate(self, stdin: Vec<u8>) -> std::io::Result<std::process::Output>;
}

impl Communicate for std::process::Child {
    fn communicate(mut self, stdin: Vec<u8>) -> std::io::Result<std::process::Output> {
        use std::io::Write;
        if let Some(mut child_stdin) = self.stdin.take() {
            child_stdin.write_all(&stdin)?;
        }
        self.wait_with_output()
    }
}

/// Return all correction markdown files under the hermetic learnings dir.
fn find_correction_files(learnings_dir: &Path) -> Vec<std::path::PathBuf> {
    if !learnings_dir.exists() {
        return vec![];
    }
    std::fs::read_dir(learnings_dir)
        .expect("should read learnings dir")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.starts_with("correction-") && name.ends_with(".md"))
        })
        .collect()
}

#[test]
fn user_prompt_submit_use_instead_of_creates_tool_preference() {
    let binary = agent_binary();
    let root = create_hermetic_root().expect("create hermetic root");
    let learnings = hermetic_learnings_dir(&root);

    let success = run_user_prompt_submit(&binary, "use uv instead of pip", &root);
    assert!(success, "hook should exit 0");

    let files = find_correction_files(&learnings);
    assert_eq!(
        files.len(),
        1,
        "expected exactly one correction file under {}, found: {:?}",
        learnings.display(),
        files
    );

    let content = std::fs::read_to_string(&files[0]).expect("should read correction file");
    assert!(
        content.contains("tool-preference"),
        "correction should be ToolPreference, got:\n{}",
        content
    );
    assert!(
        content.contains("uv"),
        "correction should contain corrected tool 'uv', got:\n{}",
        content
    );
    assert!(
        content.contains("pip"),
        "correction should contain original tool 'pip', got:\n{}",
        content
    );
}

#[test]
fn user_prompt_submit_use_not_creates_tool_preference() {
    let binary = agent_binary();
    let root = create_hermetic_root().expect("create hermetic root");
    let learnings = hermetic_learnings_dir(&root);

    let success = run_user_prompt_submit(&binary, "use cargo not make", &root);
    assert!(success, "hook should exit 0");

    let files = find_correction_files(&learnings);
    assert_eq!(
        files.len(),
        1,
        "expected exactly one correction file under {}, found: {:?}",
        learnings.display(),
        files
    );

    let content = std::fs::read_to_string(&files[0]).expect("should read correction file");
    assert!(
        content.contains("tool-preference"),
        "correction should be ToolPreference, got:\n{}",
        content
    );
    assert!(
        content.contains("cargo"),
        "correction should contain corrected tool 'cargo', got:\n{}",
        content
    );
    assert!(
        content.contains("make"),
        "correction should contain original tool 'make', got:\n{}",
        content
    );
}

#[test]
fn user_prompt_submit_prefer_over_creates_tool_preference() {
    let binary = agent_binary();
    let root = create_hermetic_root().expect("create hermetic root");
    let learnings = hermetic_learnings_dir(&root);

    let success = run_user_prompt_submit(&binary, "prefer bunx over npx", &root);
    assert!(success, "hook should exit 0");

    let files = find_correction_files(&learnings);
    assert_eq!(
        files.len(),
        1,
        "expected exactly one correction file under {}, found: {:?}",
        learnings.display(),
        files
    );

    let content = std::fs::read_to_string(&files[0]).expect("should read correction file");
    assert!(
        content.contains("tool-preference"),
        "correction should be ToolPreference, got:\n{}",
        content
    );
    assert!(
        content.contains("bunx"),
        "correction should contain corrected tool 'bunx', got:\n{}",
        content
    );
    assert!(
        content.contains("npx"),
        "correction should contain original tool 'npx', got:\n{}",
        content
    );
}

#[test]
fn user_prompt_submit_personal_preference_does_not_capture() {
    let binary = agent_binary();
    let root = create_hermetic_root().expect("create hermetic root");
    let learnings = hermetic_learnings_dir(&root);

    let success = run_user_prompt_submit(&binary, "I prefer tea over coffee", &root);
    assert!(success, "hook should exit 0 (fail-open)");

    let files = find_correction_files(&learnings);
    assert!(
        files.is_empty(),
        "personal preference should NOT create a correction file, found: {:?}",
        files
    );
}
