//! Catalogue-backed resource listing.
//!
//! `resources/list` must not search. Searching re-indexes every haystack, and
//! the previous implementation amplified that into 54 full scans per request
//! (four broad terms plus one `get_document_by_id` per result). The skills
//! tooling already emits a catalogue of one JSON object per document, so the
//! server can answer `resources/list` from that file instead.
//!
//! Resolution order:
//!
//! 1. `TERRAPHIM_CATALOG_PATH`, when set and readable.
//! 2. `<cwd>/index/catalog.jsonl` (the estate `.terraphim/` / `index/` layout
//!    produced by the skills exporter).
//!
//! The parsed catalogue is cached in-process and reloaded only when the file's
//! `mtime` changes, so repeated `resources/list` calls are free.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use serde::Deserialize;
use terraphim_types::Document;
use tokio::sync::{Mutex, OnceCell};

/// Maximum number of resources returned per page.
pub const PAGE_SIZE: usize = 50;

#[derive(Debug, Deserialize)]
struct CatalogEntry {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

struct CachedCatalogue {
    path: PathBuf,
    mtime: Option<SystemTime>,
    documents: Arc<Vec<Document>>,
}

static CATALOGUE: OnceCell<Mutex<Option<CachedCatalogue>>> = OnceCell::const_new();

fn catalog_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("TERRAPHIM_CATALOG_PATH") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }

    let candidate = std::env::current_dir()
        .ok()?
        .join("index")
        .join("catalog.jsonl");
    candidate.is_file().then_some(candidate)
}

fn entry_to_document(entry: CatalogEntry) -> Document {
    let title = if !entry.title.is_empty() {
        entry.title
    } else if !entry.name.is_empty() {
        entry.name
    } else {
        entry.id.clone()
    };

    Document {
        id: entry.id,
        title,
        url: entry.path.unwrap_or_default(),
        description: entry.description,
        ..Default::default()
    }
}

/// Load (and cache) the document catalogue, if one is available.
pub async fn documents() -> Option<Arc<Vec<Document>>> {
    let path = catalog_path()?;
    let mtime = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());

    let cell = CATALOGUE.get_or_init(|| async { Mutex::new(None) }).await;
    let mut guard = cell.lock().await;

    if let Some(cached) = guard.as_ref()
        && cached.path == path
        && cached.mtime == mtime
    {
        return Some(cached.documents.clone());
    }

    let raw = tokio::fs::read_to_string(&path).await.ok()?;
    let documents = Arc::new(parse_catalogue(&raw));
    *guard = Some(CachedCatalogue {
        path,
        mtime,
        documents: documents.clone(),
    });
    Some(documents)
}

/// Parse catalogue JSON Lines into documents, skipping malformed lines.
fn parse_catalogue(raw: &str) -> Vec<Document> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<CatalogEntry>(line).ok())
        .map(entry_to_document)
        .collect()
}

/// Resolve a single catalogue document, reading its body from disk.
///
/// Used by `read_resource` as a fallback when the persistence layer does not
/// hold the id (catalogue ids are derived from the haystack, not from the
/// persistence key space).
pub async fn document_by_id(id: &str) -> Option<Document> {
    let documents = documents().await?;
    let document = documents.iter().find(|doc| doc.id == id)?.clone();

    if document.url.is_empty() {
        return Some(document);
    }

    match tokio::fs::read_to_string(&document.url).await {
        Ok(body) => Some(Document { body, ..document }),
        Err(_) => Some(document),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_skips_malformed_lines() {
        let raw = concat!(
            r#"{"id":"alpha","name":"alpha","title":"Alpha","description":"first","path":"/tmp/alpha.md"}"#,
            "\n",
            "not json at all\n",
            "\n",
            r#"{"id":"beta","name":"beta-name"}"#,
            "\n",
        );

        let documents = parse_catalogue(raw);
        assert_eq!(documents.len(), 2, "malformed lines must be skipped");

        assert_eq!(documents[0].id, "alpha");
        assert_eq!(documents[0].title, "Alpha");
        assert_eq!(documents[0].url, "/tmp/alpha.md");
        assert_eq!(documents[0].description.as_deref(), Some("first"));

        // No title: fall back to the name, then to the id.
        assert_eq!(documents[1].title, "beta-name");
        assert!(documents[1].url.is_empty());
    }

    #[test]
    fn title_falls_back_to_id_when_name_is_absent() {
        let documents = parse_catalogue(r#"{"id":"gamma"}"#);
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].title, "gamma");
    }
}
