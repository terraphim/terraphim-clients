//! Exercise authored memory through the actual CLI and persisted store.

use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_terraphim-agent"))
        .args(["--format", "json"])
        .args(args)
        .env("TERRAPHIM_DEFAULT_DATA_PATH", root)
        .current_dir(root)
        .output()
        .expect("run real agent")
}

fn json(output: Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("machine-readable result")
}

#[test]
fn capture_preserves_authored_content_across_processes_and_stores() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let content = "Validate the public garden brief with python3 validate.py --brief brief.md.\nKeep the source decisions intact.";
    let captured = json(run(
        root.path(),
        &[
            "memory",
            "capture",
            "--content",
            content,
            "--provenance-tag",
            "public-fixture-v1",
        ],
    ));
    let id = captured["memory_id"].as_str().unwrap();
    let shown = json(run(root.path(), &["memory", "show", id]));
    assert_eq!(shown["memory_item"]["content"], content);
    assert_eq!(
        shown["memory_item"]["tags"][0],
        "provenance:public-fixture-v1"
    );
    let store = root.path().join("terraphim/evolution/cli-agent.json");
    assert!(
        store.is_file(),
        "explicit data root owns the persisted store"
    );
    let listed = json(run(root.path(), &["memory", "list"]));
    assert!(listed.to_string().contains(id));
    let exported = json(run(root.path(), &["memory", "export", "--format", "json"]));
    assert!(
        exported
            .to_string()
            .contains("Keep the source decisions intact.")
    );
    let isolated = json(run(other.path(), &["memory", "list"]));
    assert!(!isolated.to_string().contains(id));
    assert!(
        !other
            .path()
            .join("terraphim/evolution/cli-agent.json")
            .exists()
    );
}

#[test]
fn capture_reads_utf8_file_without_replacing_content_with_provenance() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("lesson.md");
    let content = "# Validation lesson\nUse the café fixture — keep Ελληνικά intact.\n";
    std::fs::write(&input, content).unwrap();
    let captured = json(run(
        root.path(),
        &["memory", "capture", "--file", input.to_str().unwrap()],
    ));
    let shown = json(run(
        root.path(),
        &["memory", "show", captured["memory_id"].as_str().unwrap()],
    ));
    assert_eq!(shown["memory_item"]["content"], content);
}

#[test]
fn capture_refuses_missing_blank_conflicting_and_unreadable_input() {
    for args in [
        vec!["memory", "capture"],
        vec!["memory", "capture", "--content", " \n\t"],
        vec!["memory", "capture", "--file", "missing-lesson.md"],
        vec![
            "memory",
            "capture",
            "--content",
            "lesson",
            "--file",
            "lesson.md",
        ],
    ] {
        let root = tempfile::tempdir().unwrap();
        let output = run(root.path(), &args);
        assert!(!output.status.success(), "must reject {args:?}");
        assert!(
            !root
                .path()
                .join("terraphim/evolution/cli-agent.json")
                .exists()
        );
    }
}

#[test]
fn invalid_files_leave_existing_memories_unchanged() {
    let root = tempfile::tempdir().unwrap();
    json(run(
        root.path(),
        &["memory", "capture", "--content", "Keep this authored note."],
    ));
    let store = root.path().join("terraphim/evolution/cli-agent.json");
    let before = std::fs::read(&store).unwrap();
    for bytes in [vec![0xff, 0xfe], vec![b'a'; 1_000_001], b" \n\t".to_vec()] {
        let input = root.path().join("invalid-lesson.md");
        std::fs::write(&input, bytes).unwrap();
        let output = run(
            root.path(),
            &["memory", "capture", "--file", input.to_str().unwrap()],
        );
        assert!(!output.status.success());
        assert_eq!(std::fs::read(&store).unwrap(), before);
    }
}
