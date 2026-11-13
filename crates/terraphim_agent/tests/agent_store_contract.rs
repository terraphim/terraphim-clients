//! Real file contracts for the shared CLI/MCP agent store.
use terraphim_agent::agent_store::AgentStore;

#[test]
fn memory_readback_restart_scope_and_invalid_inputs() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    assert!(store.memory_list(20, 0).unwrap().is_empty());
    let text = "Risk requires a Decision — café, Ελληνικά.\nKeep the public brief intact.";
    let saved = store
        .memory_capture(text, Some("public-project-brief"))
        .unwrap();
    let id = saved.id.clone();
    let restarted = AgentStore::open(root.path()).unwrap();
    assert_eq!(restarted.memory_show(&id).unwrap().content, text);
    assert_eq!(saved.tags, ["provenance:public-project-brief"]);
    assert!(
        AgentStore::open(other.path())
            .unwrap()
            .memory_show(&id)
            .is_err()
    );
    let before = std::fs::read(store.memory_path()).unwrap();
    for content in ["", " \n", &"a".repeat(1_000_001)] {
        assert!(store.memory_capture(content, None).is_err());
        assert_eq!(std::fs::read(store.memory_path()).unwrap(), before);
    }
    assert!(store.memory_show("../cli-agent.json").is_err());
    assert!(store.memory_list(101, 0).is_err());
}

#[test]
fn corrupt_store_is_not_replaced_with_an_empty_success() {
    let root = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    store
        .memory_capture("Keep the existing record.", None)
        .unwrap();
    std::fs::write(store.memory_path(), b"{corrupt").unwrap();
    assert!(store.memory_list(20, 0).is_err());
    assert!(store.memory_capture("must not replace it", None).is_err());
    assert_eq!(std::fs::read(store.memory_path()).unwrap(), b"{corrupt");
}

#[test]
fn actual_concurrent_transactions_retain_every_successful_capture() {
    let root = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    let workers: Vec<_> = (0..12)
        .map(|i| {
            let root = root.path().to_owned();
            std::thread::spawn(move || {
                let store = AgentStore::open(&root).unwrap();
                store.memory_capture(&format!("Public decision {i}"), None)
            })
        })
        .collect();
    let successes: Vec<_> = workers
        .into_iter()
        .filter_map(|task| task.join().unwrap().ok())
        .collect();
    assert!(!successes.is_empty());
    let actual = store.memory_list(100, 0).unwrap();
    assert_eq!(actual.len(), successes.len());
    for memory in successes {
        assert_eq!(
            store.memory_show(&memory.id).unwrap().content,
            memory.content
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_memory_or_learning_paths_cannot_escape_the_explicit_scope() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let store = AgentStore::open(root.path()).unwrap();
    std::fs::create_dir(root.path().join("terraphim")).unwrap();
    symlink(outside.path(), root.path().join("terraphim/evolution")).unwrap();
    assert!(store.memory_capture("must stay inside", None).is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}
