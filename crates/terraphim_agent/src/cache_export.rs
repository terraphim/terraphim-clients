//! Listing and exporting the compiled thesaurus cache.
//!
//! `terraphim-agent` compiles every role's knowledge graph and persists it as
//! `thesaurus_<name>.json` in the configured persistence profiles. There was no
//! way to read that out, so tools that consume a thesaurus file - notably
//! `terraphim-grep --thesaurus` - could not use a role's vocabulary without
//! querying the SQLite store by hand (terraphim-clients#406).
//!
//! The export format is the persisted format, which is exactly what
//! `terraphim_automata::load_thesaurus` reads.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use terraphim_persistence::DeviceStorage;
use terraphim_types::Thesaurus;
use tracing::warn;

/// One persisted thesaurus entry.
#[derive(Debug, Clone)]
pub struct CachedThesaurus {
    /// The persistence key, e.g. `thesaurus_code_reviewer.json`.
    pub key: String,
    /// The thesaurus name inside the entry.
    pub name: String,
    /// How many terms the entry holds.
    pub entries: usize,
    /// The hash of the sources it was compiled from, when recorded.
    pub source_hash: Option<String>,
}

/// Read every persisted thesaurus from every configured profile.
///
/// The same entry can live in more than one profile (a thesaurus is saved to
/// all of them), so the profiles are merged by key.
pub async fn list_cached_thesauri(storage: &DeviceStorage) -> Result<Vec<CachedThesaurus>> {
    let mut merged: BTreeMap<String, CachedThesaurus> = BTreeMap::new();

    for (profile, (operator, _latency)) in &storage.ops {
        let entries = match operator.list("thesaurus_").await {
            Ok(entries) => entries,
            Err(error) => {
                warn!("Could not list cache entries in profile '{profile}': {error}");
                continue;
            }
        };
        for entry in entries {
            let key = entry.path();
            if !key.ends_with(".json") {
                continue;
            }
            let Ok(contents) = operator.read(key).await else {
                continue;
            };
            let Ok(contents) = String::from_utf8(contents.to_vec()) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) else {
                continue;
            };
            let name = value
                .get("name")
                .and_then(|name| name.as_str())
                .unwrap_or_default()
                .to_string();
            let entries = value
                .get("data")
                .and_then(|data| data.as_object())
                .map(|data| data.len())
                .unwrap_or(0);
            let source_hash = value
                .get("source_hash")
                .and_then(|hash| hash.as_str())
                .map(str::to_string);

            merged.entry(key.to_string()).or_insert(CachedThesaurus {
                key: key.to_string(),
                name,
                entries,
                source_hash,
            });
        }
    }

    Ok(merged.into_values().collect())
}

/// Serialize a thesaurus in the format `terraphim_automata::load_thesaurus` reads.
pub fn thesaurus_json(thesaurus: &Thesaurus) -> Result<String> {
    serde_json::to_string_pretty(thesaurus).context("Failed to serialize thesaurus")
}
