//! Real CLI/file regression: concept lookup must retrieve synonym-only documents.
#![cfg(feature = "code-search")]

use std::process::Command;

#[test]
fn alias_retrieves_canonical_file_without_duplicate_lines_or_log_matches() {
    let root = tempfile::tempdir().unwrap();
    let corpus = root.path().join("corpus");
    std::fs::create_dir(&corpus).unwrap();
    std::fs::write(
        corpus.join("brief.md"),
        "Decision: launch the public workspace.\nDecision and judgment share one concept.\n",
    )
    .unwrap();
    let graph = root.path().join("thesaurus.json");
    std::fs::write(
        &graph,
        r#"{"name":"public-demo","data":{"decision":{"id":1,"nterm":"decision"},"judgment":{"id":1,"nterm":"decision"},"choice":{"id":1,"nterm":"decision"}}}"#,
    )
    .unwrap();
    // This evidence-looking file is deliberately outside the searched corpus.
    std::fs::write(root.path().join("receipt.json"), "judgment").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_terraphim-grep"))
        .args(["judgment", "--search-only", "--json", "--thesaurus"])
        .arg(&graph)
        .arg("--paths")
        .arg(&corpus)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let chunks = result["chunks"].as_array().unwrap();
    assert_eq!(chunks.len(), 2, "actual output: {result}");
    assert!(chunks.iter().all(|c| c["source"] == "brief.md"));
    assert!(chunks.iter().any(|c| c["line_start"] == 1));
    assert!(chunks.iter().any(|c| c["line_start"] == 2));
    assert!(result["answer"].is_null());
}

#[test]
fn late_sorting_alias_in_large_concept_still_retrieves_canonical_text() {
    let root = tempfile::tempdir().unwrap();
    let corpus = root.path().join("corpus");
    std::fs::create_dir(&corpus).unwrap();
    std::fs::write(corpus.join("brief.md"), "Decision: public preview.\n").unwrap();
    let mut data = serde_json::Map::new();
    for name in (0..40)
        .map(|i| format!("alias{i:02}"))
        .chain(["decision".into(), "zzjudgment".into()])
    {
        data.insert(name, serde_json::json!({"id":1,"nterm":"decision"}));
    }
    let graph = root.path().join("thesaurus.json");
    std::fs::write(
        &graph,
        serde_json::to_vec(&serde_json::json!({"name":"large", "data":data})).unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_terraphim-grep"))
        .args(["zzjudgment", "--search-only", "--json", "--thesaurus"])
        .arg(&graph)
        .arg("--paths")
        .arg(&corpus)
        .output()
        .unwrap();
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["chunks"].as_array().unwrap().len(), 1, "{result}");
    assert_eq!(result["chunks"][0]["source"], "brief.md");
}

#[test]
fn synonym_evidence_counts_towards_synthesis_coverage() {
    let root = tempfile::tempdir().unwrap();
    let corpus = root.path().join("corpus");
    std::fs::create_dir(&corpus).unwrap();
    for index in 0..3 {
        std::fs::write(
            corpus.join(format!("brief{index}.md")),
            "Decision: public preview.\n",
        )
        .unwrap();
    }
    let graph = root.path().join("thesaurus.json");
    std::fs::write(&graph, r#"{"name":"public-demo","data":{"decision":{"id":1,"nterm":"decision"},"judgment":{"id":1,"nterm":"decision"}}}"#).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_terraphim-grep"))
        .args(["judgment", "--answer", "--json", "--thesaurus"])
        .arg(graph)
        .arg("--paths")
        .arg(corpus)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("OLLAMA_BASE_URL")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["chunks"].as_array().unwrap().len(), 3);
    assert_ne!(result["sufficiency"], "RlmInsufficient", "{result}");
    assert!(
        result["answer"].is_null(),
        "no configured provider must not invent output"
    );
}
