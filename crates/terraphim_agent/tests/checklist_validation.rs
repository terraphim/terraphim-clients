//! End-to-end tests for `terraphim-agent validate --checklist`.
//!
//! Structural contract (exit codes, report shape, JSON fields) always runs. Assertions
//! about *which* categories a text credits need a knowledge graph and are skipped when
//! the host has none - CI runners have none, and a test that silently passes because the
//! graph is missing would be worse than no test.

use std::path::PathBuf;

use assert_cmd::Command;

fn agent() -> Command {
    Command::cargo_bin("terraphim-agent").expect("built binary")
}

/// The knowledge graph this role resolves to, if the host has one.
fn kg_dir() -> Option<PathBuf> {
    let dir = std::env::var("TERRAPHIM_KG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").expect("HOME")).join(".config/terraphim/kg")
        });
    // Readable, not merely present: a graph directory can hold nothing but dangling
    // symlinks (that is exactly what the CI runner has), and treating that as a graph
    // would run assertions that cannot pass and blame the code for it.
    let usable = std::fs::read_dir(&dir).ok()?.flatten().any(|e| {
        let path = e.path();
        path.extension().is_some_and(|x| x == "md") && std::fs::read_to_string(&path).is_ok()
    });
    usable.then_some(dir)
}

#[test]
fn unknown_checklist_exits_2() {
    // Regression: the generic error path used to exit 1, indistinguishable from a
    // runtime failure, and before that the exit status was misread as 0 (#476).
    let out = agent()
        .args(["validate", "--checklist", "code_reveiw", "text"])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(2),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Unknown checklist"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn unknown_checklist_lists_available_names() {
    let out = agent()
        .args(["validate", "--checklist", "nope", "text"])
        .output()
        .expect("run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("code_review"), "stderr: {stderr}");
    assert!(stderr.contains("security"), "stderr: {stderr}");
}

#[test]
fn known_checklist_exits_0_even_when_the_text_fails_it() {
    // "ran and failed" is a successful run reporting passed:false - not a usage error.
    let out = agent()
        .args(["validate", "--checklist", "code_review", "nothing relevant"])
        .output()
        .expect("run");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Passed: false"), "stdout: {stdout}");
}

#[test]
fn report_score_matches_the_listed_items() {
    let out = agent()
        .args(["validate", "--checklist", "code_review", "nothing relevant"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let satisfied = stdout.matches('✓').count();
    let missing = stdout.matches('✗').count();
    assert!(
        stdout.contains(&format!("Score: {}/{}", satisfied, satisfied + missing)),
        "score line disagrees with the item lists: {stdout}"
    );
}

#[test]
fn json_result_carries_evidence_and_unreachable() {
    let out = agent()
        .args([
            "--format",
            "json",
            "validate",
            "--checklist",
            "code_review",
            "unit tests",
            "--json",
        ])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .unwrap_or_else(|| panic!("no JSON in output: {stdout}"));
    // Parsed as text: the crate has serde_json as a normal dependency, not a dev one,
    // and pulling it in for one assertion is not worth a new test dependency.
    for field in [
        "\"satisfied\"",
        "\"missing\"",
        "\"evidence\"",
        "\"unreachable\"",
    ] {
        assert!(
            line.contains(field),
            "JSON result is missing {field}: {line}"
        );
    }
}

#[test]
fn a_security_sentence_credits_the_security_category() {
    // The #475 repro. The graph has no `security` pattern, and the matcher returns the
    // fragment `check` for this text, so only the phrase fallback can credit it.
    if kg_dir().is_none() {
        eprintln!("skipping: no knowledge graph on this host");
        return;
    }
    let out = agent()
        .args([
            "validate",
            "--checklist",
            "code_review",
            "Ran a security check on the endpoint.",
        ])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("✓ security (phrase:security)"),
        "security was not credited by phrase: {stdout}"
    );
}

#[test]
fn a_text_addressing_every_category_passes_the_checklist() {
    if kg_dir().is_none() {
        eprintln!("skipping: no knowledge graph on this host");
        return;
    }
    let text = "Added unit tests and updated documentation. Handled error handling paths. \
                Ran a security review. Checked performance.";
    let out = agent()
        .args(["validate", "--checklist", "code_review", text])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Passed: true"),
        "checklist still cannot pass: {stdout}"
    );
}

#[test]
fn unreachable_categories_are_labelled_in_the_report() {
    if kg_dir().is_none() {
        eprintln!("skipping: no knowledge graph on this host");
        return;
    }
    let out = agent()
        .args(["validate", "--checklist", "code_review", "nothing relevant"])
        .output()
        .expect("run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no knowledge-graph concept can credit this"),
        "unreachable categories are not distinguished from missing text: {stdout}"
    );
}
