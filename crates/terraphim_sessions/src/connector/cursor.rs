//! Cursor IDE session connector
//!
//! Reads Cursor's SQLite `state.vscdb` databases to extract AI chat sessions.
//!
//! ## Storage locations
//!
//! - Linux:   `~/.config/Cursor/User/`
//! - macOS:   `~/Library/Application Support/Cursor/User/`
//! - Windows: `%APPDATA%/Cursor/User/`
//!
//! ## Schema versions
//!
//! ### v2 — `cursorDiskKV` table (Cursor ≥ 0.40)
//! Keys match `composerData:<uuid>`. Value is JSON:
//! ```json
//! {"tabs": [{"bubbles": [{"role": "user", "text": "...", "timestamp": 1234}], "model": "gpt-4"}]}
//! ```
//!
//! ### v1 — `ItemTable` table (Cursor < 0.40)
//! Keys match `%aichat%chatdata%` or `%composer%`. Value is JSON:
//! ```json
//! {"messages": [{"role": "user", "content": "...", "timestamp": 1234}]}
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{ConnectorStatus, ImportOptions, SessionConnector};
use crate::model::{ContentBlock, Message, MessageRole, Session, SessionMetadata};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rusqlite::Connection;
use serde::Deserialize;
use tracing::{debug, info, warn};

/// Cursor IDE session connector — reads `state.vscdb` SQLite databases.
#[derive(Debug, Default)]
pub struct CursorConnector;

#[async_trait]
impl SessionConnector for CursorConnector {
    fn source_id(&self) -> &str {
        "cursor"
    }

    fn display_name(&self) -> &str {
        "Cursor IDE"
    }

    fn detect(&self) -> ConnectorStatus {
        let Some(path) = self.default_path() else {
            return ConnectorStatus::NotFound;
        };
        if !path.exists() {
            return ConnectorStatus::NotFound;
        }
        let count = walkdir::WalkDir::new(&path)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .is_some_and(|name| name == "state.vscdb")
            })
            .count();
        ConnectorStatus::Available {
            path,
            sessions_estimate: Some(count),
        }
    }

    fn default_path(&self) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir().map(|h| {
                h.join("Library")
                    .join("Application Support")
                    .join("Cursor")
                    .join("User")
            })
        }

        #[cfg(target_os = "linux")]
        {
            dirs::home_dir().map(|h| h.join(".config").join("Cursor").join("User"))
        }

        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA")
                .ok()
                .map(|appdata| PathBuf::from(appdata).join("Cursor").join("User"))
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }

    async fn import(&self, options: &ImportOptions) -> Result<Vec<Session>> {
        let base_path = options
            .path
            .clone()
            .or_else(|| self.default_path())
            .ok_or_else(|| anyhow::anyhow!("No path specified and default not found"))?;

        info!("Importing Cursor sessions from: {}", base_path.display());

        // Collect all state.vscdb paths upfront (sync, lightweight)
        let db_files: Vec<PathBuf> = walkdir::WalkDir::new(&base_path)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .file_name()
                    .is_some_and(|name| name == "state.vscdb")
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        info!("Found {} Cursor databases", db_files.len());

        let limit = options.limit;

        // rusqlite is synchronous — offload to a blocking thread
        let sessions = tokio::task::spawn_blocking(move || {
            let mut all = Vec::new();
            let mut seen_ids = HashSet::new();

            for db_path in db_files {
                match parse_database(&db_path, &mut seen_ids) {
                    Ok(mut db_sessions) => all.append(&mut db_sessions),
                    Err(e) => warn!("Failed to parse {}: {}", db_path.display(), e),
                }

                if let Some(max) = limit
                    && all.len() >= max
                {
                    all.truncate(max);
                    break;
                }
            }

            info!("Imported {} Cursor sessions", all.len());
            all
        })
        .await?;

        Ok(sessions)
    }
}

// ---------------------------------------------------------------------------
// Synchronous parsing helpers (called inside spawn_blocking)
// ---------------------------------------------------------------------------

fn parse_database(db_path: &Path, seen_ids: &mut HashSet<String>) -> Result<Vec<Session>> {
    debug!("Parsing database: {}", db_path.display());

    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open database: {}", db_path.display()))?;

    let mut sessions = Vec::new();

    // v2: cursorDiskKV table with composerData: keys
    sessions.extend(parse_composer_data(&conn, db_path, seen_ids)?);

    // v1: ItemTable with aichat/composer keys
    sessions.extend(parse_legacy_format(&conn, db_path, seen_ids)?);

    Ok(sessions)
}

/// Parse schema v2 — `cursorDiskKV` table, `composerData:<id>` keys.
fn parse_composer_data(
    conn: &Connection,
    db_path: &Path,
    seen_ids: &mut HashSet<String>,
) -> Result<Vec<Session>> {
    let table_exists: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='cursorDiskKV'")?
        .exists([])?;

    if !table_exists {
        return Ok(vec![]);
    }

    let mut stmt =
        conn.prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")?;

    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut sessions = Vec::new();
    for (key, value) in rows {
        let composer_id = key
            .strip_prefix("composerData:")
            .unwrap_or(&key)
            .to_string();

        if !seen_ids.insert(composer_id.clone()) {
            continue;
        }

        match serde_json::from_str::<ComposerData>(&value) {
            Ok(data) => {
                if let Some(session) = composer_to_session(&composer_id, data, db_path) {
                    sessions.push(session);
                }
            }
            Err(e) => debug!("Failed to parse composer data {}: {}", composer_id, e),
        }
    }

    Ok(sessions)
}

/// Parse schema v1 — `ItemTable`, keys matching chat or composer patterns.
fn parse_legacy_format(
    conn: &Connection,
    db_path: &Path,
    seen_ids: &mut HashSet<String>,
) -> Result<Vec<Session>> {
    let table_exists: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='ItemTable'")?
        .exists([])?;

    if !table_exists {
        return Ok(vec![]);
    }

    let mut stmt = conn.prepare(
        "SELECT key, value FROM ItemTable \
         WHERE key LIKE '%aichat%chatdata%' OR key LIKE '%composer%'",
    )?;

    let rows: Vec<(String, Vec<u8>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut sessions = Vec::new();
    for (key, raw) in rows {
        if !seen_ids.insert(key.clone()) {
            continue;
        }

        let Ok(value) = String::from_utf8(raw) else {
            continue;
        };

        match serde_json::from_str::<LegacyChatData>(&value) {
            Ok(data) => {
                if let Some(session) = legacy_to_session(&key, data, db_path) {
                    sessions.push(session);
                }
            }
            Err(e) => debug!("Failed to parse legacy chat data {}: {}", key, e),
        }
    }

    Ok(sessions)
}

fn composer_to_session(id: &str, data: ComposerData, db_path: &Path) -> Option<Session> {
    let tabs = data.tabs.unwrap_or_default();
    if tabs.is_empty() {
        return None;
    }

    let mut messages: Vec<Message> = Vec::new();
    let mut idx = 0usize;

    for tab in &tabs {
        for bubble in &tab.bubbles {
            let content = bubble
                .text
                .clone()
                .or_else(|| bubble.content.clone())
                .or_else(|| bubble.message.clone())
                .unwrap_or_default();

            if content.is_empty() {
                continue;
            }

            let role = normalize_role(&bubble.role);
            let created_at = bubble
                .timestamp
                .and_then(|ts| jiff::Timestamp::from_millisecond(ts as i64).ok());

            messages.push(Message {
                idx,
                role,
                author: bubble.model.clone(),
                content: content.clone(),
                blocks: vec![ContentBlock::Text { text: content }],
                created_at,
                extra: serde_json::Value::Null,
            });
            idx += 1;
        }
    }

    if messages.is_empty() {
        return None;
    }

    let title = messages.first().map(|m| truncate_title(&m.content));
    let started_at = messages.first().and_then(|m| m.created_at);
    let ended_at = messages.last().and_then(|m| m.created_at);

    let metadata = SessionMetadata::new(
        None,
        None,
        vec!["cursor".to_string(), "composer".to_string()],
        serde_json::json!({"unified_mode": data.unified_mode}),
    );

    Some(Session {
        id: format!("cursor:{id}"),
        source: "cursor".to_string(),
        external_id: id.to_string(),
        title,
        source_path: db_path.to_path_buf(),
        started_at,
        ended_at,
        messages,
        metadata,
    })
}

fn legacy_to_session(key: &str, data: LegacyChatData, db_path: &Path) -> Option<Session> {
    let messages: Vec<Message> = data
        .messages
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .filter_map(|(idx, msg)| {
            let content = msg.content.unwrap_or_default();
            if content.is_empty() {
                return None;
            }
            let role = normalize_role(&msg.role);
            let created_at = msg
                .timestamp
                .and_then(|ts| jiff::Timestamp::from_millisecond(ts as i64).ok());
            Some(Message {
                idx,
                role,
                author: msg.model,
                content: content.clone(),
                blocks: vec![ContentBlock::Text { text: content }],
                created_at,
                extra: serde_json::Value::Null,
            })
        })
        .collect();

    if messages.is_empty() {
        return None;
    }

    let title = messages.first().map(|m| truncate_title(&m.content));
    let started_at = messages.first().and_then(|m| m.created_at);
    let ended_at = messages.last().and_then(|m| m.created_at);

    let metadata = SessionMetadata::new(
        None,
        None,
        vec!["cursor".to_string(), "legacy".to_string()],
        serde_json::Value::Null,
    );

    Some(Session {
        id: format!("cursor:{key}"),
        source: "cursor".to_string(),
        external_id: key.to_string(),
        title,
        source_path: db_path.to_path_buf(),
        started_at,
        ended_at,
        messages,
        metadata,
    })
}

fn normalize_role(role: &str) -> MessageRole {
    match role.to_lowercase().as_str() {
        "user" | "human" => MessageRole::User,
        "assistant" | "ai" | "bot" | "model" => MessageRole::Assistant,
        _ => MessageRole::Other,
    }
}

fn truncate_title(content: &str) -> String {
    const MAX_CHARS_BYTES: usize = 60;
    if content.len() <= MAX_CHARS_BYTES {
        return content.to_string();
    }
    // `content[..60]` would panic when byte 60 falls inside a multibyte
    // UTF-8 scalar (CJK, emoji, accented Latin). Walk back to the nearest
    // char boundary. Mirrors the established idiom in
    // `terraphim_rlm/src/query_loop.rs:truncate` and
    // `terraphim_sessions/src/search.rs`.
    let mut boundary = MAX_CHARS_BYTES.min(content.len());
    while boundary > 0 && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }
    format!("{}...", &content[..boundary])
}

// ---------------------------------------------------------------------------
// Schema structs
// ---------------------------------------------------------------------------

/// v2 — `cursorDiskKV` format
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComposerData {
    tabs: Option<Vec<ComposerTab>>,
    unified_mode: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ComposerTab {
    bubbles: Vec<Bubble>,
    #[allow(dead_code)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bubble {
    role: String,
    text: Option<String>,
    content: Option<String>,
    message: Option<String>,
    timestamp: Option<u64>,
    model: Option<String>,
}

/// v1 — `ItemTable` format
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyChatData {
    messages: Option<Vec<LegacyMessage>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyMessage {
    role: String,
    content: Option<String>,
    timestamp: Option<u64>,
    model: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use tempfile::tempdir;

    fn create_v2_db(path: &Path) -> Result<()> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE cursorDiskKV (key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);",
        )?;

        let data = serde_json::json!({
            "tabs": [
                {
                    "model": "gpt-4",
                    "bubbles": [
                        {"role": "user", "text": "Write a Rust hello world", "timestamp": 1_700_000_000_000u64},
                        {"role": "assistant", "text": "Here is hello world in Rust", "timestamp": 1_700_000_001_000u64, "model": "gpt-4"}
                    ]
                }
            ],
            "unifiedMode": false
        });

        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params!["composerData:test-uuid-123", data.to_string()],
        )?;

        Ok(())
    }

    fn create_v1_db(path: &Path) -> Result<()> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE ItemTable (key TEXT NOT NULL UNIQUE, value BLOB NOT NULL);",
        )?;

        let data = serde_json::json!({
            "messages": [
                {"role": "user", "content": "Explain ownership in Rust", "timestamp": 1_600_000_000_000u64},
                {"role": "assistant", "content": "Rust ownership is...", "timestamp": 1_600_000_001_000u64, "model": "claude-3-opus"}
            ]
        });

        conn.execute(
            "INSERT INTO ItemTable (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "workbench.panel.aichat.view.aichat.chatdata",
                data.to_string().as_bytes().to_vec()
            ],
        )?;

        Ok(())
    }

    #[test]
    fn test_source_id_and_display_name() {
        let c = CursorConnector;
        assert_eq!(c.source_id(), "cursor");
        assert_eq!(c.display_name(), "Cursor IDE");
    }

    #[test]
    fn test_normalize_role_user_variants() {
        assert!(matches!(normalize_role("user"), MessageRole::User));
        assert!(matches!(normalize_role("User"), MessageRole::User));
        assert!(matches!(normalize_role("human"), MessageRole::User));
        assert!(matches!(normalize_role("HUMAN"), MessageRole::User));
    }

    #[test]
    fn test_normalize_role_assistant_variants() {
        assert!(matches!(
            normalize_role("assistant"),
            MessageRole::Assistant
        ));
        assert!(matches!(normalize_role("AI"), MessageRole::Assistant));
        assert!(matches!(normalize_role("bot"), MessageRole::Assistant));
        assert!(matches!(normalize_role("model"), MessageRole::Assistant));
    }

    #[test]
    fn test_normalize_role_unknown() {
        assert!(matches!(normalize_role("system"), MessageRole::Other));
        assert!(matches!(normalize_role("unknown"), MessageRole::Other));
    }

    #[test]
    fn test_parse_v2_composer_data() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("state.vscdb");
        create_v2_db(&db_path)?;

        let mut seen = HashSet::new();
        let conn = Connection::open(&db_path)?;
        let sessions = parse_composer_data(&conn, &db_path, &mut seen)?;

        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.external_id, "test-uuid-123");
        assert_eq!(s.source, "cursor");
        assert_eq!(s.messages.len(), 2);
        assert!(matches!(s.messages[0].role, MessageRole::User));
        assert!(matches!(s.messages[1].role, MessageRole::Assistant));
        assert_eq!(s.messages[0].content, "Write a Rust hello world");
        assert_eq!(s.messages[1].content, "Here is hello world in Rust");
        Ok(())
    }

    #[test]
    fn test_parse_v1_legacy_format() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("state.vscdb");
        create_v1_db(&db_path)?;

        let mut seen = HashSet::new();
        let conn = Connection::open(&db_path)?;
        let sessions = parse_legacy_format(&conn, &db_path, &mut seen)?;

        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.source, "cursor");
        assert_eq!(s.messages.len(), 2);
        assert!(matches!(s.messages[0].role, MessageRole::User));
        assert!(matches!(s.messages[1].role, MessageRole::Assistant));
        assert_eq!(s.messages[0].content, "Explain ownership in Rust");
        Ok(())
    }

    #[test]
    fn test_deduplication_across_calls() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("state.vscdb");
        create_v2_db(&db_path)?;

        let mut seen = HashSet::new();
        let conn = Connection::open(&db_path)?;

        let first = parse_composer_data(&conn, &db_path, &mut seen)?;
        assert_eq!(first.len(), 1);

        // Second call with same `seen` set — must return 0 (deduplication)
        let second = parse_composer_data(&conn, &db_path, &mut seen)?;
        assert_eq!(second.len(), 0);
        Ok(())
    }

    #[test]
    fn test_empty_database_returns_no_sessions() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("state.vscdb");
        // Create a valid SQLite db with neither table
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("CREATE TABLE unrelated (id INTEGER);")?;
        drop(conn);

        let mut seen = HashSet::new();
        let sessions = parse_database(&db_path, &mut seen)?;
        assert!(sessions.is_empty());
        Ok(())
    }

    #[test]
    fn test_v2_empty_tabs_skipped() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("state.vscdb");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE cursorDiskKV (key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);",
        )?;
        // A composer entry with no tabs
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params![
                "composerData:empty-uuid",
                r#"{"tabs": [], "unifiedMode": false}"#
            ],
        )?;
        drop(conn);

        let mut seen = HashSet::new();
        let sessions = parse_database(&db_path, &mut seen)?;
        assert!(sessions.is_empty());
        Ok(())
    }

    #[test]
    fn test_v2_corrupted_json_does_not_panic() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("state.vscdb");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(
            "CREATE TABLE cursorDiskKV (key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);",
        )?;
        conn.execute(
            "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
            rusqlite::params!["composerData:bad-uuid", "not valid json {{{"],
        )?;
        drop(conn);

        // Must not panic; bad row is silently skipped
        let mut seen = HashSet::new();
        let sessions = parse_database(&db_path, &mut seen)?;
        assert!(sessions.is_empty());
        Ok(())
    }

    #[test]
    fn test_truncate_title_long_content() {
        let long = "a".repeat(100);
        let title = truncate_title(&long);
        assert!(title.ends_with("..."));
        assert!(title.len() <= 63); // 60 chars + "..."
    }

    #[test]
    fn test_truncate_title_short_content() {
        let short = "Hello Rust";
        let title = truncate_title(short);
        assert_eq!(title, "Hello Rust");
    }

    #[test]
    fn test_truncate_title_multibyte_cjk_does_not_panic() {
        // Regression: byte index 60 falls mid-CJK-char. Each 中 is 3 bytes;
        // 30 of them = 90 bytes (> 60). 60 % 3 == 0 -> safe here, but mix
        // so the boundary lands inside a scalar.
        let s = format!("{}{}", "a", "中".repeat(40)); // 1 + 120 = 121 bytes
        let title = truncate_title(&s);
        assert!(title.ends_with("..."));
        assert!(title.is_char_boundary(title.len()));
        // prefix must be a valid UTF-8 prefix of the input
        assert!(s.starts_with(title.trim_end_matches("...")));
    }

    #[test]
    fn test_truncate_title_emoji_does_not_panic() {
        // Regression from compound-review + quality-coordinator: emoji at
        // byte 60 caused `end byte index 60 is not a char boundary; it is
        // inside '😀' (bytes 57..61)` panic.
        let s = format!("{}{}", "a", "😀".repeat(30)); // 1 + 120 = 121 bytes
        let title = truncate_title(&s);
        assert!(title.ends_with("..."));
        assert!(title.is_char_boundary(title.len()));
        assert!(s.starts_with(title.trim_end_matches("...")));
        assert!(title.len() <= 63);
    }

    #[tokio::test]
    async fn test_import_with_limit() -> Result<()> {
        let dir = tempdir()?;

        // Create 3 separate databases each with one session
        for i in 0..3u32 {
            let db_path = dir.path().join(format!("db_{i}")).join("state.vscdb");
            std::fs::create_dir_all(db_path.parent().unwrap())?;
            let conn = Connection::open(&db_path)?;
            conn.execute_batch(
                "CREATE TABLE cursorDiskKV (key TEXT NOT NULL UNIQUE, value TEXT NOT NULL);",
            )?;
            let data = serde_json::json!({
                "tabs": [{"model": "gpt-4", "bubbles": [
                    {"role": "user", "text": format!("question {i}"), "timestamp": 1_700_000_000_000u64}
                ]}]
            });
            conn.execute(
                "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                rusqlite::params![format!("composerData:uuid-{i}"), data.to_string()],
            )?;
        }

        let connector = CursorConnector;
        let options = ImportOptions::new()
            .with_path(dir.path().to_path_buf())
            .with_limit(2);
        let sessions = connector.import(&options).await?;

        assert_eq!(sessions.len(), 2);
        Ok(())
    }
}
