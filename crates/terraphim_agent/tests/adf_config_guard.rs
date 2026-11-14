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
fn every_fallback_provider_is_an_absolute_path_that_exists_when_opencode_does() {
    let agents = agents();

    // The fallback CLI is resolved by the orchestrator when it respawns a
    // timed-out agent, so a relative path cannot be resolved at all.
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
    }

    // The path has to exist on the host that runs the fleet. The CI runner
    // has no opencode at all, so this arm only applies where opencode is
    // installed - and then the path has to be the one that is really there.
    if !opencode_on_path() {
        println!(
            "skipping: the fallback path check: no opencode on PATH on this host, so the \
             fleet's fallback cannot run here either"
        );
        return;
    }

    for agent in &agents {
        let Some(provider) = agent.fallback_provider.as_deref() else {
            continue;
        };
        assert!(
            Path::new(provider).is_file(),
            "the agent `{}` falls back to `{provider}`, which does not exist on this host: the \
             fallback would fail to start. `which opencode` says {}. Refs #517.",
            agent.name,
            which_opencode().unwrap_or_else(|| "<nothing>".to_string()),
        );
    }
}

/// Whether an `opencode` binary is on `PATH`.
fn opencode_on_path() -> bool {
    which_opencode().is_some()
}

/// The path `PATH` resolves `opencode` to.
fn which_opencode() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|directory| {
        let candidate = directory.join("opencode");
        candidate
            .is_file()
            .then(|| candidate.to_string_lossy().to_string())
    })
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
