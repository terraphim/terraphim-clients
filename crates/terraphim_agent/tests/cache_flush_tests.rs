//! `terraphim-agent cache flush --role` must flush the thesaurus the role
//! actually uses, and must say so truthfully.
//!
//! A role's thesaurus is persisted under the *thesaurus* name: a role whose
//! `kg.automata_path` points at a compiled-corrections file is stored as
//! `thesaurus_compiled_corrections.json`, so the old flush
//! (`thesaurus_<role name>.json`, fastest profile only) removed nothing and
//! still printed "Flushed cache for role". Refs terraphim-clients#460.

use std::path::PathBuf;

use terraphim_agent::cache_flush::{delete_key_everywhere, role_thesaurus_keys};
use terraphim_config::Config;

mod support;

/// A thesaurus file as `learn compile` writes it: the `name` field is what the
/// persistence keys the entry under.
fn compiled_thesaurus_file(name: &str, path: &PathBuf) -> PathBuf {
    let contents = serde_json::json!({
        "name": name,
        "data": {
            "npm install": { "id": 1, "nterm": "bun install" }
        }
    });
    std::fs::write(
        path,
        serde_json::to_string_pretty(&contents).expect("serialize"),
    )
    .expect("write");
    path.clone()
}

fn config_with_role(
    role_name: &str,
    shortname: Option<&str>,
    automata: Option<&PathBuf>,
) -> Config {
    let mut role = terraphim_config::Role {
        shortname: shortname.map(str::to_string),
        name: terraphim_types::RoleName::new(role_name),
        relevance_function: terraphim_types::RelevanceFunction::TitleScorer,
        terraphim_it: false,
        theme: "default".to_string(),
        kg: None,
        haystacks: vec![],
        llm_enabled: false,
        llm_api_key: None,
        llm_model: None,
        llm_auto_summarize: false,
        llm_chat_enabled: false,
        llm_chat_system_prompt: None,
        llm_chat_model: None,
        llm_context_window: None,
        llm_router_enabled: false,
        llm_router_config: None,
        extra: ahash::AHashMap::new(),
    };
    if let Some(automata) = automata {
        role.kg = Some(terraphim_config::KnowledgeGraph {
            automata_path: Some(terraphim_automata::AutomataPath::from_local(
                automata.clone(),
            )),
            knowledge_graph_local: None,
            public: false,
            publish: false,
        });
    }
    let mut config: Config = serde_json::from_str(
        r#"{"id":"Desktop","global_shortcut":null,"roles":{},"default_role":"Code Reviewer","selected_role":"Code Reviewer"}"#,
    )
    .expect("config json");
    config
        .roles
        .insert(terraphim_types::RoleName::new(role_name), role);
    config
}

#[test]
fn role_with_compiled_automata_resolves_the_thesaurus_key() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let compiled = compiled_thesaurus_file(
        "compiled corrections",
        &tmp.path().join("compiled_corrections.json"),
    );
    let config = config_with_role("Stale Cache Probe", None, Some(&compiled));

    let keys = role_thesaurus_keys(&config, "Stale Cache Probe").expect("keys");

    assert!(
        keys.keys
            .contains(&"thesaurus_compiled_corrections.json".to_string()),
        "the compiled thesaurus key must be resolved, got {keys:?}"
    );
    assert!(
        keys.keys
            .contains(&"thesaurus_stale_cache_probe.json".to_string()),
        "the role-derived key must be included as well, got {keys:?}"
    );
    assert!(
        keys.undetermined.is_none(),
        "a local automata file is readable, so nothing is undetermined: {keys:?}"
    );
}

#[test]
fn role_without_automata_resolves_the_role_derived_key() {
    let config = config_with_role("AI Engineer", Some("ai"), None);

    let keys = role_thesaurus_keys(&config, "AI Engineer").expect("keys");

    assert_eq!(keys.keys, vec!["thesaurus_ai_engineer.json".to_string()]);
    assert!(keys.undetermined.is_none(), "got {keys:?}");
}

#[test]
fn role_lookup_accepts_case_and_shortname() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let compiled = compiled_thesaurus_file(
        "compiled corrections",
        &tmp.path().join("compiled_corrections.json"),
    );
    let config = config_with_role("Stale Cache Probe", Some("probe"), Some(&compiled));

    for query in ["Stale Cache Probe", "probe", "PROBE", "stale cache probe"] {
        let keys = role_thesaurus_keys(&config, query)
            .unwrap_or_else(|error| panic!("query {query:?} should resolve: {error}"));
        assert!(
            keys.keys
                .contains(&"thesaurus_compiled_corrections.json".to_string()),
            "query {query:?} resolved to {keys:?}"
        );
    }
}

#[test]
fn unknown_role_is_rejected_instead_of_flushing_nothing() {
    let config = config_with_role("AI Engineer", None, None);

    let error = role_thesaurus_keys(&config, "No Such Role").expect_err("must not resolve");

    assert!(
        error.to_string().contains("No Such Role"),
        "the error must name the role, got: {error}"
    );
}

#[tokio::test]
async fn deleting_an_absent_key_removes_nothing() {
    let storage = terraphim_persistence::DeviceStorage::init_memory_only()
        .await
        .expect("memory storage");

    let outcome = delete_key_everywhere(storage, "thesaurus_does_not_exist.json")
        .await
        .expect("delete");

    assert_eq!(
        outcome.removed, 0,
        "an absent key must not be counted as removed"
    );
    assert!(
        outcome.unreachable_profiles.is_empty(),
        "a reachable profile is not an unreachable one: {outcome:?}"
    );
}

/// A flush that removes nothing must not claim success.
///
/// The old implementation printed "Flushed cache for role" after a delete that
/// silently did nothing, which is what made the stale-cache report look like a
/// fixed problem (terraphim-clients#460).
#[test]
fn cli_flush_reports_when_nothing_matched() {
    use std::process::Command;

    use support::cli_test_env::set_hermetic_env;

    let root = support::cli_test_env::create_hermetic_root().expect("hermetic root");
    let mut command = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    set_hermetic_env(&mut command, &root).expect("hermetic env");

    let output = command
        .args(["cache", "flush", "--role", "Terraphim Engineer"])
        .output()
        .expect("run terraphim-agent cache flush");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "flush should exit 0\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("No cached thesaurus matched role 'Terraphim Engineer'"),
        "a no-op flush must say so instead of claiming success\nstdout: {stdout}"
    );
    assert!(
        !stdout.contains("Flushed cache for role"),
        "the old unconditional success claim must be gone\nstdout: {stdout}"
    );
}

/// A role that is not in the configuration cannot be mapped to a key, and the
/// command must say so instead of flushing nothing successfully.
#[test]
fn cli_flush_rejects_an_unknown_role() {
    use std::process::Command;

    use support::cli_test_env::set_hermetic_env;

    let root = support::cli_test_env::create_hermetic_root().expect("hermetic root");
    let mut command = Command::new(env!("CARGO_BIN_EXE_terraphim-agent"));
    set_hermetic_env(&mut command, &root).expect("hermetic env");

    let output = command
        .args(["cache", "flush", "--role", "No Such Role"])
        .output()
        .expect("run terraphim-agent cache flush");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "an unmappable role must not exit 0\nstdout: {}\nstderr: {stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("No Such Role"),
        "the error must name the role, got: {stderr}"
    );
}

/// A role whose automata path is a **remote** file is flushed under the name
/// inside that file, not under the role name (terraphim-clients#514).
///
/// The default `Terraphim Engineer` role points at the canonical remote
/// compiled-corrections file, whose `name` is `Engineering`, so the persisted
/// key is `thesaurus_engineering.json`.
#[test]
fn role_with_a_remote_automata_path_resolves_the_known_thesaurus_key() {
    let mut config = config_with_role("Terraphim Engineer", Some("terra"), None);
    let role = config
        .roles
        .get_mut(&terraphim_types::RoleName::new("Terraphim Engineer"))
        .expect("role");
    role.kg = Some(terraphim_config::KnowledgeGraph {
        automata_path: Some(terraphim_automata::AutomataPath::Remote(
            "https://system-operator.s3.eu-west-2.amazonaws.com/term_to_id.json".to_string(),
        )),
        knowledge_graph_local: None,
        public: false,
        publish: false,
    });

    let keys = role_thesaurus_keys(&config, "Terraphim Engineer").expect("keys");

    assert!(
        keys.keys
            .contains(&"thesaurus_engineering.json".to_string()),
        "the remote thesaurus key must be resolved, got {keys:?}"
    );
    assert!(
        keys.undetermined.is_none(),
        "the canonical remote file is known, so nothing is undetermined: {keys:?}"
    );
}

/// A remote file whose name is not known must not be reported as a complete
/// flush (terraphim-clients#514).
#[test]
fn role_with_an_unknown_remote_automata_path_is_reported_as_undetermined() {
    let mut config = config_with_role("Terraphim Engineer", Some("terra"), None);
    let role = config
        .roles
        .get_mut(&terraphim_types::RoleName::new("Terraphim Engineer"))
        .expect("role");
    role.kg = Some(terraphim_config::KnowledgeGraph {
        automata_path: Some(terraphim_automata::AutomataPath::Remote(
            "https://example.invalid/other_corrections.json".to_string(),
        )),
        knowledge_graph_local: None,
        public: false,
        publish: false,
    });

    let keys = role_thesaurus_keys(&config, "Terraphim Engineer").expect("keys");

    assert!(
        keys.undetermined.is_some(),
        "an unknown remote file must be reported as undetermined: {keys:?}"
    );
    let reason = keys.undetermined.expect("reason");
    assert!(
        reason.contains("example.invalid"),
        "the reason must name the file, got: {reason}"
    );
}

/// The known-name table must describe the file the repository carries.
///
/// The table is what lets the flush find a remote thesaurus without a network
/// call, so it has to agree with the repository's copy of that file
/// (terraphim-clients#514).
#[test]
fn the_known_remote_names_match_the_repositorys_copy_of_the_file() {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let fixture = workspace.join("test-fixtures/term_to_id.json");

    let contents = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|error| panic!("read {}: {error}", fixture.display()));
    let value: serde_json::Value = serde_json::from_str(&contents).expect("fixture json");
    let name = value
        .get("name")
        .and_then(|name| name.as_str())
        .expect("the fixture carries a name");

    for (url, expected) in terraphim_agent::cache_flush::KNOWN_REMOTE_THESAURUS_NAMES {
        if url.ends_with("term_to_id.json") {
            assert_eq!(
                *expected,
                name,
                "the table must agree with {} ({url})",
                fixture.display()
            );
        }
    }
}
