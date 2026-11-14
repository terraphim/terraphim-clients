//! Guards for the repo-local ADF agent configuration (#517).
//!
//! The orchestrator validates every agent's `model` and `fallback_model`
//! against an allow-list of provider prefixes **when the fleet loads the
//! project**. One prefix that is not on the list fails the whole load, and
//! the agents never run - including the ones that have nothing to do with
//! that agent.
//!
//! PR #409 pointed this repository's agent at `terraphim-proxy/fastest`,
//! which is not on the list, and the fleet refused to load the project:
//!
//! ```text
//! adf --local --check FAILED validation: banned LLM provider
//! 'terraphim-proxy/fastest' in model for agent 'implementation-swarm'
//! (allowed: claude-code, opencode-go, kimi-for-coding, minimax-coding-plan,
//! zai-coding-plan)
//! ```
//!
//! Nothing caught it: the configuration is only read by the orchestrator, and
//! no test in this repository looked at it. This file is that test.
//!
//! The portable arms always run. The arms that need a host tool (the
//! deployed orchestrator, pi's model registry, an opencode binary) skip with
//! a printed notice when the tool is absent, the way
//! `tests/support/binary.rs::skip_without_server` does.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The provider prefixes the orchestrator accepts.
///
/// This is the orchestrator's `ALLOWED_PROVIDER_PREFIXES`, quoted from the
/// deployed binary's own validation message. The orchestrator lives in
/// another repository and its deployed build cannot be changed from here, so
/// the allow-list is the contract this repository has to live within.
const ALLOWED_PROVIDER_PREFIXES: [&str; 5] = [
    "claude-code",
    "opencode-go",
    "kimi-for-coding",
    "minimax-coding-plan",
    "zai-coding-plan",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .to_path_buf()
}

fn config_path() -> PathBuf {
    workspace_root().join(".terraphim").join("adf.toml")
}

/// One `[[agents]]` stanza.
#[derive(Debug, Default)]
struct Agent {
    name: String,
    model: Option<String>,
    fallback_model: Option<String>,
    fallback_provider: Option<String>,
}

/// The `[[agents]]` stanzas of the configuration.
fn agents() -> Vec<Agent> {
    let path = config_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    let value: toml::Value = toml::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not valid TOML: {error}", path.display()));
    let stanzas = value
        .get("agents")
        .and_then(toml::Value::as_array)
        .unwrap_or_else(|| {
            panic!(
                "{} declares no [[agents]] stanzas; the fleet would load no agent",
                path.display()
            )
        });

    stanzas
        .iter()
        .map(|stanza| Agent {
            name: stanza
                .get("name")
                .and_then(toml::Value::as_str)
                .unwrap_or("<unnamed>")
                .to_string(),
            model: stanza
                .get("model")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            fallback_model: stanza
                .get("fallback_model")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
            fallback_provider: stanza
                .get("fallback_provider")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        })
        .collect()
}

/// The provider prefix of a `provider/model` pair.
fn provider_prefix(model: &str) -> &str {
    model.split('/').next().unwrap_or(model)
}

#[test]
fn every_agent_model_is_on_the_orchestrators_allow_list() {
    let agents = agents();
    assert!(!agents.is_empty(), "the configuration declares no agents");

    for agent in &agents {
        for (label, model) in [
            ("model", agent.model.as_deref()),
            ("fallback_model", agent.fallback_model.as_deref()),
        ] {
            let Some(model) = model else {
                continue;
            };
            let prefix = provider_prefix(model);
            assert!(
                ALLOWED_PROVIDER_PREFIXES.contains(&prefix),
                "the agent `{}` has {label} `{model}`, whose provider `{prefix}` is not on the \
                 orchestrator's allow-list {ALLOWED_PROVIDER_PREFIXES:?}: the fleet refuses to \
                 load this project and no agent runs. Refs #517.",
                agent.name
            );
        }
    }
}

#[test]
fn every_fallback_provider_is_an_absolute_path_to_an_opencode() {
    let agents = agents();

    // The fallback CLI is resolved by the orchestrator when it respawns a
    // timed-out agent, so a relative path cannot be resolved at all. This arm
    // is portable: it holds on every host.
    //
    // Whether the path *exists* is a property of the machine the fleet runs
    // on, not of this repository: the configured path names that machine's
    // opencode, and the CI runner is a different machine with its own. That
    // check is done by hand and recorded in the implementation note, not by a
    // test that would have to know which host it is on.
    for agent in &agents {
        let Some(provider) = agent.fallback_provider.as_deref() else {
            continue;
        };
        assert!(
            Path::new(provider).is_absolute(),
            "the agent `{}` has the relative fallback_provider `{provider}`; the orchestrator \
             resolves it against its own working directory. Refs #517.",
            agent.name
        );
        assert!(
            Path::new(provider)
                .file_name()
                .is_some_and(|name| name == "opencode"),
            "the agent `{}` falls back to `{provider}`, which is not an opencode binary; the \
             fallback CLI and the fallback model belong to different providers. Refs #517.",
            agent.name
        );
    }
}

/// The deployed orchestrator, when this host has one.
fn orchestrator() -> Option<PathBuf> {
    ["/home/alex/.local/bin/adf", "/usr/local/bin/adf"]
        .into_iter()
        .map(PathBuf::from)
        .find(|candidate| candidate.is_file())
}

#[test]
fn the_deployed_orchestrator_accepts_this_configuration() {
    let Some(orchestrator) = orchestrator() else {
        println!(
            "skipping: the orchestrator's own verdict: no `adf` binary on this host \
             (the CI runner has none)"
        );
        return;
    };

    let output = Command::new(&orchestrator)
        .args(["--local", "--check"])
        .current_dir(workspace_root())
        .output()
        .unwrap_or_else(|error| panic!("run {}: {error}", orchestrator.display()));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !combined.contains("FAILED validation"),
        "the orchestrator refused this project's configuration:\n{combined}\n\
         An agent's model prefix that is not on the orchestrator's allow-list fails the whole \
         fleet load. Refs #517."
    );
    assert!(
        !combined.contains("banned LLM provider"),
        "the orchestrator banned a provider in this project's configuration:\n{combined}\n\
         Refs #517."
    );
}
