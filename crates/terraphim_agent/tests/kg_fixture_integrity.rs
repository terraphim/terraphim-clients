//! KG fixture integrity: every thesaurus key has exactly one owner.
//!
//! The `terraphim_automata` builder (`index_inner`) inserts, for every `.md`
//! file containing a `synonyms::` line, BOTH the file stem AND each synonym
//! into a `HashMap`. Last writer wins, and the walk order differs by machine —
//! so two fixtures claiming the same key resolve nondeterministically.
//! Symptom: `pnpm install` rewrites to `bun install` on one host and to
//! `npm install` on another (terraphim-agents#110 / #111).
//!
//! This test reproduces the builder's key-insertion rules directly from the
//! fixtures and fails if any key is claimed by two or more files, listing ALL
//! collisions at once. It deliberately does not call the builder itself:
//! the builder resolves collisions silently (last writer wins), so the only
//! observable of a collision is which host you ran it on.
//!
//! Key rules mirrored from `terraphim_automata-1.20.4/src/builder.rs`
//! (`index_inner`, `concept_from_path`):
//! - a line is a synonyms directive iff it contains `"::"` and the part
//!   before the first `"::"` equals `synonyms` exactly;
//! - the value side is split on `,`, each synonym trimmed, empties dropped;
//! - the file stem is inserted as a key for every such line in the file;
//! - keys are normalized like `terraphim_types::NormalizedTermValue::new`
//!   (trim + lowercase) before insertion.
//!
//! Refs: terraphim-agents#110 (design: .docs/handoff-110-replace-matcher-bun.md)

// Include only the kg_paths helper rather than the whole `support` module:
// `support/mod.rs` also exposes binary/server helpers that this binary does
// not use, and without a crate-level `#[allow(dead_code)]` (still unmerged on
// main, see #115) they would fail workspace `clippy --all-targets -D warnings`.
#[path = "support/kg_paths.rs"]
mod kg_paths;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use kg_paths::crate_kg_dir;

/// Normalization mirroring `terraphim_types::NormalizedTermValue::new`.
fn normalize(term: &str) -> String {
    term.trim().to_lowercase()
}

/// Returns `Some((stem, synonyms))` if the file declares a thesaurus concept.
///
/// A file is a concept iff it contains at least one valid `synonyms::` line.
/// Returns the file stem plus the union of synonyms across ALL such lines
/// (the builder inserts on every matching line, not just the first).
fn concept_keys(contents: &str, stem: &str) -> Option<Vec<String>> {
    let mut keys = vec![normalize(stem)];
    let mut is_concept = false;
    for line in contents.lines() {
        let Some((keyword, value)) = line.split_once("::") else {
            continue;
        };
        if keyword != "synonyms" {
            continue;
        }
        is_concept = true;
        for synonym in value.split(',') {
            let synonym = synonym.trim();
            if !synonym.is_empty() {
                keys.push(normalize(synonym));
            }
        }
    }
    is_concept.then_some(keys)
}

/// Recursively collect `*.md` files under `dir`.
fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}) failed: {e}", dir.display()));
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

#[test]
fn every_thesaurus_key_has_exactly_one_owner() {
    let kg_dir = crate_kg_dir();
    let mut files = Vec::new();
    markdown_files(&kg_dir, &mut files);
    assert!(
        !files.is_empty(),
        "no KG fixtures found under {} -- the walk is broken, not the fixtures",
        kg_dir.display()
    );

    // key -> set of files claiming it (sorted for deterministic failure output)
    let mut owners: BTreeMap<String, BTreeMap<String, ()>> = BTreeMap::new();
    let mut concept_count = 0usize;

    for path in &files {
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", path.display()));
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(keys) = concept_keys(&contents, stem) else {
            continue; // not a thesaurus concept; inserts nothing
        };
        concept_count += 1;
        let owner = path
            .strip_prefix(&kg_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        for key in keys {
            owners.entry(key).or_default().insert(owner.clone(), ());
        }
    }

    assert!(
        concept_count > 0,
        "no fixtures with synonyms:: lines found under {} -- \
         the fixtures stopped declaring concepts",
        kg_dir.display()
    );

    let collisions: Vec<(&String, &BTreeMap<String, ()>)> = owners
        .iter()
        .filter(|(_, claimants)| claimants.len() > 1)
        .collect();

    assert!(
        collisions.is_empty(),
        "KG thesaurus keys owned by multiple fixtures (the automata builder \
         resolves these last-writer-wins, so replacement output depends on \
         directory walk order):\n{}",
        collisions
            .iter()
            .map(|(key, claimants)| {
                format!(
                    "  '{key}' claimed by: {}",
                    claimants.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    );
}
