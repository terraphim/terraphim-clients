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
/// all of them), so the profiles are merged by key. When the copies disagree -
/// one profile was written before the last rebuild, say - the most complete
/// one is reported, so the listing does not depend on the order the profiles
/// happen to be iterated in.
pub async fn list_cached_thesauri(storage: &DeviceStorage) -> Result<Vec<CachedThesaurus>> {
    let mut merged: BTreeMap<String, CachedThesaurus> = BTreeMap::new();

    // The profiles live in a `HashMap`, so they are visited in name order to
    // keep the listing deterministic.
    let mut profiles: Vec<&String> = storage.ops.keys().collect();
    profiles.sort();

    for profile in profiles {
        let (operator, _latency) = &storage.ops[profile];
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

            merge_entry(
                &mut merged,
                CachedThesaurus {
                    key: key.to_string(),
                    name,
                    entries,
                    source_hash,
                },
            );
        }
    }

    Ok(merged.into_values().collect())
}

/// Merge one profile's copy of a persisted thesaurus into the listing.
///
/// Profiles can hold different copies of the same key - one was written
/// before the last rebuild, say - so the most complete copy wins. That keeps
/// the listing independent of the order the profiles are visited in.
fn merge_entry(merged: &mut BTreeMap<String, CachedThesaurus>, candidate: CachedThesaurus) {
    merged
        .entry(candidate.key.clone())
        .and_modify(|existing| {
            if candidate.entries > existing.entries {
                *existing = candidate.clone();
            }
        })
        .or_insert(candidate);
}

/// Serialize a thesaurus in the format `terraphim_automata::load_thesaurus` reads.
pub fn thesaurus_json(thesaurus: &Thesaurus) -> Result<String> {
    serde_json::to_string_pretty(thesaurus).context("Failed to serialize thesaurus")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, entries: usize, hash: &str) -> CachedThesaurus {
        CachedThesaurus {
            key: key.to_string(),
            name: "role".to_string(),
            entries,
            source_hash: Some(hash.to_string()),
        }
    }

    /// Profiles can hold different copies of the same key; the listing must
    /// report the most complete one, whichever profile it came from.
    #[test]
    fn the_most_complete_copy_of_a_key_wins() {
        let mut merged = BTreeMap::new();

        merge_entry(&mut merged, entry("thesaurus_role.json", 2, "old"));
        merge_entry(&mut merged, entry("thesaurus_role.json", 5, "new"));

        let listed = merged.get("thesaurus_role.json").expect("merged entry");
        assert_eq!(listed.entries, 5, "the larger copy must win");
        assert_eq!(
            listed.source_hash.as_deref(),
            Some("new"),
            "the winning copy's hash must be reported with it"
        );

        // A smaller copy arriving later must not displace the larger one.
        merge_entry(&mut merged, entry("thesaurus_role.json", 1, "older"));
        let listed = merged.get("thesaurus_role.json").expect("merged entry");
        assert_eq!(listed.entries, 5, "a smaller copy must not win");
    }

    /// Different keys are listed separately.
    #[test]
    fn distinct_keys_are_listed_separately() {
        let mut merged = BTreeMap::new();

        merge_entry(&mut merged, entry("thesaurus_a.json", 1, "a"));
        merge_entry(&mut merged, entry("thesaurus_b.json", 2, "b"));

        assert_eq!(merged.len(), 2);
        assert_eq!(
            merged.get("thesaurus_a.json").map(|entry| entry.entries),
            Some(1)
        );
        assert_eq!(
            merged.get("thesaurus_b.json").map(|entry| entry.entries),
            Some(2)
        );
    }
}
