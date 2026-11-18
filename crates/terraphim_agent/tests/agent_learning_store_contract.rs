//! Actual failed command, stored annotation, explicit compilation and KG export.
use terraphim_agent::agent_store::{AgentStore, LearningRecord};

#[test]
fn failed_observation_annotation_and_explicit_rule_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    let failed = std::process::Command::new("printf")
        .arg("%Z")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    let stderr = String::from_utf8(failed.stderr).unwrap();
    let saved = store
        .learning_capture("printf '%Z'", &stderr, failed.status.code().unwrap())
        .unwrap();
    let LearningRecord::Failure(record) = saved.record else {
        panic!("expected actual failure record")
    };
    assert_eq!(record.command, "printf '%Z'");
    assert_eq!(record.exit_code, 1);
    assert_eq!(record.error_output.trim(), stderr.trim());
    assert!(saved.path.is_file());
    let corrected = std::process::Command::new("printf")
        .args(["%s\n", "reviewed correction"])
        .output()
        .unwrap();
    assert!(corrected.status.success());
    assert_eq!(corrected.stdout, b"reviewed correction\n");
    let annotation = store
        .learning_correct(&record.id, "printf '%s\\n' 'reviewed correction'")
        .unwrap();
    let LearningRecord::Failure(actual) = annotation.record else {
        panic!("expected annotated failure")
    };
    assert_eq!(
        actual.correction.as_deref(),
        Some("printf '%s\\n' 'reviewed correction'")
    );
    assert_eq!(store.correction_compile().unwrap().mappings, 0);
    assert_eq!(store.learning_query("printf", 10).unwrap().len(), 1);
    store
        .correction_add(
            "printf '%Z'",
            "printf '%s\\n' 'reviewed correction'",
            "Actual public fixture observation",
        )
        .unwrap();
    let compiled = store.correction_compile().unwrap();
    assert_eq!(compiled.mappings, 1);
    let disk: terraphim_types::Thesaurus =
        serde_json::from_slice(&std::fs::read(compiled.path).unwrap()).unwrap();
    assert_eq!(disk.len(), 1);
    let mapped = disk
        .get(&terraphim_types::NormalizedTermValue::from("printf '%Z'"))
        .unwrap();
    assert_eq!(mapped.display(), "printf '%s\\n' 'reviewed correction'");
    let exported = store.correction_export_kg().unwrap();
    assert_eq!(exported.files.len(), 1);
    assert!(
        exported.files[0]
            .markdown
            .contains("synonyms:: printf '%Z'")
    );
    assert_eq!(
        std::fs::read_to_string(&exported.files[0].path).unwrap(),
        exported.files[0].markdown
    );
    assert_eq!(store.learning_query("printf", 10).unwrap().len(), 2);
    assert!(!root.path().join(".claude").exists());
    assert!(!root.path().join(".opencode").exists());
}

#[test]
fn scope_unknown_ids_malformed_records_and_unsupported_kg_values_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    assert!(store.learning_capture("printf x", "", 1).is_err());
    assert!(store.learning_capture("printf x", "no failure", 0).is_err());
    assert!(store.learning_correct("../escape", "printf x").is_err());
    assert!(store.learning_correct("missing", "printf x").is_err());
    assert_eq!(store.learning_query("printf", 10).unwrap().len(), 0);
    assert!(!root.path().join("terraphim/staging").exists());
    store.correction_add("a,b", "safe", "").unwrap();
    assert_eq!(store.correction_compile().unwrap().mappings, 1);
    assert!(store.correction_export_kg().is_err());
    let directory = root.path().join("terraphim/learnings");
    std::fs::write(directory.join("correction-broken.md"), b"not a correction").unwrap();
    assert!(store.learning_query("safe", 10).is_err());
    assert!(store.correction_compile().is_err());
    assert!(store.correction_add("must not", "write", "").is_err());
}

#[test]
fn cli_learning_mutations_and_mcp_store_share_the_same_transaction_lock() {
    use terraphim_agent::learnings::{
        CorrectionType, LearningCaptureConfig, capture_correction, capture_failed_command,
        correct_learning,
    };
    let root = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    let saved = store
        .learning_capture("printf '%Z'", "actual fixture error", 1)
        .unwrap();
    let LearningRecord::Failure(record) = saved.record else {
        panic!("failure")
    };
    let directory = root.path().join("terraphim/learnings");
    let config = LearningCaptureConfig::new(
        root.path().join("absent/project/learnings"),
        directory.clone(),
    );
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(".write.lock"))
        .unwrap();
    lock.lock().unwrap();
    let before = std::fs::read(&saved.path).unwrap();
    assert!(capture_failed_command("must not capture", "failure", 1, &config).is_err());
    assert!(
        capture_correction(
            CorrectionType::ToolPreference,
            "must not",
            "capture",
            "",
            &config
        )
        .is_err()
    );
    assert!(correct_learning(&directory, &record.id, "must not replace").is_err());
    assert!(
        store
            .learning_correct(&record.id, "must not replace")
            .is_err()
    );
    assert_eq!(std::fs::read(&saved.path).unwrap(), before);
    drop(lock);
    correct_learning(&directory, &record.id, "CLI correction after unlock").unwrap();
    let repeated_path =
        capture_failed_command("printf '%Z'", "repeated observed error", 1, &config).unwrap();
    let repeated = terraphim_agent::learnings::CapturedLearning::from_markdown(
        &std::fs::read_to_string(repeated_path).unwrap(),
    )
    .unwrap();
    let importance = repeated.importance.unwrap();
    assert_eq!(
        importance.repetition_count, 1,
        "reader must see history while outer mutation lock is held"
    );
    assert!(importance.has_correction);

    let values = store.learning_query("printf", 10).unwrap();
    assert!(values.iter().any(|value| matches!(&value.record, LearningRecord::Failure(record) if record.correction.as_deref()==Some("CLI correction after unlock"))));
}

#[test]
fn unsupported_markdown_roundtrips_are_refused_before_destination_changes() {
    let root = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    for (original, corrected, context) in [
        ("printf `date`", "printf safe", ""),
        ("printf original", "printf `date`", ""),
        (
            "printf original",
            "printf safe",
            "first\n## Unrelated\ntruncated",
        ),
    ] {
        assert!(store.correction_add(original, corrected, context).is_err());
    }
    for (command, error) in [
        ("printf first\nsecond", "error"),
        ("printf x", "first\n```\ntruncated"),
        ("printf x\nexit_code: 0", "error"),
    ] {
        assert!(store.learning_capture(command, error, 1).is_err());
    }
    assert!(store.learning_query("printf", 100).unwrap().is_empty());
    let saved = store
        .learning_capture("printf '%Z'", "observed failure\n", 1)
        .unwrap();
    let LearningRecord::Failure(record) = saved.record else {
        panic!("failure")
    };
    let before = std::fs::read(&saved.path).unwrap();
    assert!(store.learning_correct(&record.id, "first\nsecond").is_err());
    assert_eq!(std::fs::read(saved.path).unwrap(), before);
}
