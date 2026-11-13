//! Role-scoped thesaurus cache flushing for `terraphim-agent cache flush`.
//!
//! A role's thesaurus is persisted under the **thesaurus** name, not the role
//! name. A role whose `kg.automata_path` points at a compiled-corrections file
//! is stored as `thesaurus_compiled_corrections.json`, so a flush that
//! deletes `thesaurus_<role name>.json` removes nothing and still reports
//! success (terraphim-clients#460).
//!
//! Two rules follow from the persistence layer:
//!
//! 1. the key is computed the same way the persistence computes it
//!    (`Persistable::get_key` on a `Thesaurus` with that name), and
//! 2. a thesaurus is written to *every* configured profile, so a flush has to
//!    delete from every profile too, not only from the fastest one.

use anyhow::{Context, Result};
use terraphim_automata::AutomataPath;
use terraphim_config::{Config, Role};
use terraphim_persistence::{DeviceStorage, Persistable};
use terraphim_types::Thesaurus;

/// Resolve a role by exact name, then by case-insensitive shortname, then by
/// case-insensitive name.
fn resolve_role<'a>(config: &'a Config, query: &str) -> Option<&'a Role> {
    config
        .roles
        .values()
        .find(|role| role.name.as_str() == query)
        .or_else(|| {
            config.roles.values().find(|role| {
                role.shortname
                    .as_deref()
                    .is_some_and(|shortname| shortname.eq_ignore_ascii_case(query))
            })
        })
        .or_else(|| {
            config
                .roles
                .values()
                .find(|role| role.name.as_str().eq_ignore_ascii_case(query))
        })
}

/// The thesaurus name stored inside a compiled automata file, if it is readable.
fn thesaurus_name_in_file(path: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
    value
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
}

/// The thesaurus names of the compiled automata files the agent knows.
///
/// A role whose `kg.automata_path` points at a **remote** file cannot be read
/// from the local filesystem, but the file's `name` is what the persistence
/// layer keys the thesaurus by - the same value `load_thesaurus` reads. The
/// canonical file is the one the onboarding templates point at, and this
/// repository carries a copy of it (`test-fixtures/term_to_id.json`), so the
/// name is known without a network call.
///
/// A test checks the table against the repository's copy, so the two cannot
/// drift apart.
pub const KNOWN_REMOTE_THESAURUS_NAMES: &[(&str, &str)] = &[(
    "https://system-operator.s3.eu-west-2.amazonaws.com/term_to_id.json",
    "Engineering",
)];

/// The cache keys a role's thesaurus may live under, and what could not be
/// determined.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleThesaurusKeys {
    /// The keys the role's thesaurus may be persisted under.
    pub keys: Vec<String>,
    /// A candidate key that could not be determined, with the reason.
    ///
    /// A caller must not report a complete flush while this is set: the
    /// thesaurus may live under a name that was not looked for
    /// (terraphim-clients#514).
    pub undetermined: Option<String>,
}

/// The persisted cache keys a role's thesaurus may live under.
///
/// Both candidates are returned because either can be present:
///
/// * the role-derived name (`ai engineer` -> `thesaurus_ai_engineer.json`),
///   which is what a role without an automata path uses and what older
///   versions wrote, and
/// * the thesaurus name inside the role's compiled automata file
///   (`compiled_corrections` -> `thesaurus_compiled_corrections.json`).
///
/// The keys are produced by `Persistable::get_key`, so they cannot drift from
/// the names the persistence layer actually uses.
pub fn role_thesaurus_keys(config: &Config, role_query: &str) -> Result<RoleThesaurusKeys> {
    let role = resolve_role(config, role_query).with_context(|| {
        format!(
            "Role '{role_query}' is not in the configuration, so its cached thesaurus \
             cannot be located; nothing was flushed"
        )
    })?;

    let mut names = vec![role.name.as_lowercase().to_string()];
    let mut undetermined = None;
    match role.kg.as_ref().and_then(|kg| kg.automata_path.as_ref()) {
        Some(AutomataPath::Local(path)) => {
            if let Some(name) = thesaurus_name_in_file(path) {
                names.push(name);
            }
        }
        Some(AutomataPath::Remote(url)) => {
            match KNOWN_REMOTE_THESAURUS_NAMES
                .iter()
                .find(|(known, _)| known == url)
            {
                Some((_, name)) => names.push((*name).to_string()),
                None => {
                    undetermined = Some(format!(
                        "the thesaurus name of the remote automata file '{url}' is not known, \
                         so a cached thesaurus under that name cannot be located"
                    ));
                }
            }
        }
        None => {}
    }

    let mut keys: Vec<String> = Vec::new();
    for name in names {
        let key = Thesaurus::new(name).get_key();
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    Ok(RoleThesaurusKeys { keys, undetermined })
}

/// Delete `key` from every configured persistence profile.
///
/// Returns how many entries were actually removed. A key that is not present
/// in a profile is not counted, so the caller can report truthfully instead of
/// claiming a flush that removed nothing.
pub async fn delete_key_everywhere(storage: &DeviceStorage, key: &str) -> Result<FlushOutcome> {
    let mut outcome = FlushOutcome::default();
    for (profile, (operator, _latency)) in &storage.ops {
        match operator.exists(key).await {
            Ok(true) => {
                operator.delete(key).await.with_context(|| {
                    format!("Failed to delete '{key}' from profile '{profile}'")
                })?;
                outcome.removed += 1;
            }
            Ok(false) => {}
            Err(error) => {
                // `log::warn!` rather than `tracing::warn!`: the agent installs
                // only the `log` facade, so a tracing warning is dropped on the
                // floor (terraphim-clients#514).
                log::warn!("Could not check '{key}' in profile '{profile}': {error}");
                outcome
                    .unreachable_profiles
                    .push((profile.clone(), error.to_string()));
            }
        }
    }
    Ok(outcome)
}

/// What a flush removed, and what it could not reach.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlushOutcome {
    /// How many entries were actually removed.
    pub removed: usize,
    /// The profiles that could not be reached, with the reason.
    ///
    /// A profile that could not be reached may still hold a copy of the
    /// thesaurus, so a flush that reports success while this is not empty is
    /// lying (terraphim-clients#514).
    pub unreachable_profiles: Vec<(String, String)>,
}
