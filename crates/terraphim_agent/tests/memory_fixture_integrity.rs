//! Integrity checks for the committed memory benchmark fixture
//! (terraphim-clients#259, acceptance bullet 1 of #255).
//!
//! No mocks: the tests read the committed files under
//! `tests/fixtures/memory_bench/` and parse them with the real
//! `terraphim_agent_evolution::MemoryItem` serde implementation.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use terraphim_agent_evolution::MemoryItem;

const MAX_ITEMS: usize = 200;
const MIN_QUERIES: usize = 20;
const MAX_QUERIES: usize = 50;

#[derive(Debug, Deserialize)]
struct Query {
    query: String,
    expected_ids: Vec<String>,
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("memory_bench")
}

fn read(name: &str) -> String {
    let path = fixture_dir().join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn corpus_items() -> Vec<MemoryItem> {
    read("corpus.jsonl")
        .lines()
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str::<MemoryItem>(line)
                .unwrap_or_else(|e| panic!("corpus.jsonl line {} is not a MemoryItem: {e}", i + 1))
        })
        .collect()
}

fn queries() -> Vec<Query> {
    read("queries.jsonl")
        .lines()
        .enumerate()
        .map(|(i, line)| {
            serde_json::from_str::<Query>(line)
                .unwrap_or_else(|e| panic!("queries.jsonl line {} is not a query: {e}", i + 1))
        })
        .collect()
}

#[test]
fn every_corpus_line_parses_and_round_trips_as_memory_item() {
    let items = corpus_items();
    assert!(!items.is_empty(), "corpus.jsonl is empty");
    assert!(
        items.len() <= MAX_ITEMS,
        "corpus has {} items; the cap is {MAX_ITEMS}",
        items.len()
    );
    for item in &items {
        assert!(
            !item.content.trim().is_empty(),
            "item {} has empty content",
            item.id
        );
        let json = serde_json::to_string(item).expect("serialise");
        let back: MemoryItem = serde_json::from_str(&json).expect("re-parse");
        assert_eq!(back.id, item.id);
        assert_eq!(back.content, item.content);
    }
}

#[test]
fn corpus_ids_are_unique() {
    let items = corpus_items();
    let mut seen = HashSet::new();
    for item in &items {
        assert!(
            seen.insert(item.id.clone()),
            "duplicate corpus id {}",
            item.id
        );
    }
}

#[test]
fn every_expected_id_exists_in_the_corpus() {
    let ids: HashSet<String> = corpus_items().into_iter().map(|i| i.id).collect();
    let qs = queries();
    assert!(
        (MIN_QUERIES..=MAX_QUERIES).contains(&qs.len()),
        "queries.jsonl has {} records; expected {MIN_QUERIES}..={MAX_QUERIES}",
        qs.len()
    );
    let mut seen_queries = HashSet::new();
    for q in &qs {
        assert!(!q.query.trim().is_empty(), "empty query text");
        assert!(
            seen_queries.insert(q.query.clone()),
            "duplicate query: {}",
            q.query
        );
        assert!(
            !q.expected_ids.is_empty(),
            "query has no expected ids: {}",
            q.query
        );
        for id in &q.expected_ids {
            assert!(
                ids.contains(id),
                "expected id {id} is not in corpus.jsonl (query: {})",
                q.query
            );
        }
    }
}

#[test]
fn readme_sha256_matches_corpus_file() {
    let readme = read("README.md");
    let re = Regex::new(r"corpus\.jsonl SHA-256: ([0-9a-f]{64})").unwrap();
    let recorded = re
        .captures(&readme)
        .map(|c| c[1].to_string())
        .expect("README.md must contain a line 'corpus.jsonl SHA-256: <64 hex>'");
    let bytes = std::fs::read(fixture_dir().join("corpus.jsonl")).expect("read corpus bytes");
    let actual = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        recorded, actual,
        "README.md records SHA-256 {recorded} but corpus.jsonl hashes to {actual}; rerun scripts/build_memory_fixture.sh and update the README"
    );
}

/// The fixture is committed to a repository that is mirrored publicly, so the
/// structural shapes the build script redacts must not appear in either file.
/// Each pattern matches both the raw shape and its redacted form; every match
/// must be the redacted form.
#[test]
fn fixture_carries_no_unredacted_hosts_paths_or_credentials() {
    // Ids are UUID-timestamp pairs and are not redaction targets, so the scan
    // runs over the free-text fields only: content, tags and query text.
    let mut parts: Vec<String> = corpus_items()
        .into_iter()
        .map(|i| format!("{}\n{}", i.content, i.tags.join(" ")))
        .collect();
    parts.extend(queries().into_iter().map(|q| q.query));
    let text = parts.join("\n");
    let checks: &[(&str, &str, &str)] = &[
        (
            r"/Users/[A-Za-z0-9._\[\]-]+",
            "/Users/[USER]",
            "macOS home directory",
        ),
        (
            r"/home/[A-Za-z0-9._\[\]-]+",
            "/home/[USER]",
            "Linux home directory",
        ),
        (
            r#"op://[^\s"'`)]+"#,
            "op://[REDACTED]",
            "1Password reference",
        ),
        (
            r"(?i)\bbearer\s+[A-Za-z0-9._~+/=\[\]-]+",
            "[REDACTED]",
            "bearer token",
        ),
        (
            r"zestic-ai/[A-Za-z0-9._\[\]-]+",
            "zestic-ai/[CLIENT]",
            "client directory",
        ),
    ];
    for (pattern, allowed, label) in checks {
        let re = Regex::new(pattern).unwrap();
        for m in re.find_iter(&text) {
            let found = m.as_str();
            let ok = if *label == "bearer token" {
                found.to_ascii_lowercase().ends_with("bearer [redacted]")
            } else {
                found == *allowed
            };
            assert!(ok, "unredacted {label} in fixture: {found}");
        }
    }

    // Shapes that must not appear at all.
    let forbidden: &[(&str, &str)] = &[
        (r"\b[0-9a-fA-F]{32,}\b", "long hexadecimal run"),
        (r"AKIA[A-Z0-9]{16}", "AWS access key"),
        (r"sk-[A-Za-z0-9-_]{20,}", "OpenAI-style key"),
        (r"gh[po]_[A-Za-z0-9]{36}", "GitHub token"),
    ];
    for (pattern, label) in forbidden {
        let re = Regex::new(pattern).unwrap();
        if let Some(m) = re.find(&text) {
            panic!("unredacted {label} in fixture: {}", m.as_str());
        }
    }

    // user@host pairs and e-mail addresses must be the redacted pair.
    let at = Regex::new(r"[A-Za-z0-9._%+\[\]-]+@[A-Za-z0-9\[][A-Za-z0-9.\[\]-]*").unwrap();
    for m in at.find_iter(&text) {
        assert_eq!(
            m.as_str(),
            "[USER]@[HOST]",
            "unredacted user@host or e-mail in fixture"
        );
    }

    // Host names: every dotted label run ending in a host TLD must be gone,
    // and every URL host must be a redacted placeholder. Source-file names
    // (`lib.rs`, `build.sh`, `Cargo.lock`) end in extensions, not TLDs.
    const HOST_TLDS: &[&str] = &[
        "cloud",
        "ai",
        "com",
        "io",
        "net",
        "org",
        "dev",
        "engineer",
        "local",
        "lan",
        "internal",
        "localhost",
    ];
    let dotted = Regex::new(r"\b[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+\b").unwrap();
    for m in dotted.find_iter(&text) {
        let lower = m.as_str().to_ascii_lowercase();
        let labels: Vec<&str> = lower.split('.').collect();
        let tld = labels.last().copied().unwrap_or_default();
        let numeric = labels.iter().all(|l| l.chars().all(|c| c.is_ascii_digit()));
        assert!(
            numeric || !HOST_TLDS.contains(&tld),
            "unredacted host name {} in fixture",
            m.as_str()
        );
    }
    assert!(
        !Regex::new(r"\blocalhost\b").unwrap().is_match(&text),
        "bare localhost in fixture"
    );
    let url_host = Regex::new(r#"://([^/\s"'`:]+)"#).unwrap();
    for c in url_host.captures_iter(&text) {
        let host = &c[1];
        assert!(
            matches!(
                host,
                "[HOST]" | "[USER]@[HOST]" | "[IP]" | "[REDACTED]" | "127.0.0.1" | "0.0.0.0"
            ),
            "unredacted URL host {host} in fixture"
        );
    }

    // Project paths: after a home directory, `~`, or a deployment root the
    // only permitted tail is `/[PROJECT]`, optionally followed by one generic
    // file name (a shell dotfile or a config/log extension).
    fn is_generic_file_name(name: &str) -> bool {
        const DOTFILES: &[&str] = &[".profile", ".bashrc", ".zshrc", ".gitconfig", ".env"];
        const EXTENSIONS: &[&str] = &[
            "toml", "lock", "json", "yml", "yaml", "ini", "conf", "cfg", "log", "md", "txt", "db",
        ];
        DOTFILES.contains(&name)
            || name
                .rsplit_once('.')
                .is_some_and(|(stem, ext)| !stem.is_empty() && EXTENSIONS.contains(&ext))
    }
    let project = Regex::new(
        r#"(/Users/\[USER\]|/home/\[USER\]|~|/opt|/srv|/data|/var/lib)(/[^\s"'`:;|()\[\],<>*\\#]+)?"#,
    )
    .unwrap();
    for c in project.captures_iter(&text) {
        let Some(tail) = c.get(2) else { continue };
        let parts: Vec<&str> = tail.as_str().trim_start_matches('/').split('/').collect();
        let ok = match parts.as_slice() {
            ["[PROJECT]"] => true,
            ["[PROJECT]", name] => is_generic_file_name(name),
            [name] => is_generic_file_name(name),
            _ => false,
        };
        assert!(
            ok,
            "unredacted project path {}{} in fixture",
            &c[1],
            tail.as_str()
        );
    }

    // IPv4 other than loopback and the unspecified address.
    let ipv4 = Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap();
    for m in ipv4.find_iter(&text) {
        let s = m.as_str();
        assert!(
            s == "127.0.0.1" || s == "0.0.0.0",
            "unredacted IPv4 address {s} in fixture"
        );
    }
}
