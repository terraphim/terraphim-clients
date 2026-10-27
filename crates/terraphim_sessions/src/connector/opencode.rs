//! OpenCode Session Connector
//!
//! Imports OpenCode sessions from its SQLite database and legacy JSONL prompt
//! history. The SQLite database is preferred during automatic detection.

use std::path::{Path, PathBuf};

use super::{ConnectorStatus, ImportOptions, SessionConnector};
use crate::model::{ContentBlock, Message, MessageRole, Session, SessionMetadata};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;

/// OpenCode connector.
#[derive(Debug, Default)]
pub struct OpenCodeConnector;

/// OpenCode prompt history entry in the legacy JSONL format.
#[derive(Debug, Clone, Deserialize)]
struct LegacyEntry {
    /// User input prompt.
    #[serde(default)]
    input: Option<String>,
    /// Additional parts, usually empty.
    #[serde(default)]
    parts: Vec<serde_json::Value>,
    /// Mode, for example `normal`.
    #[serde(default)]
    mode: Option<String>,
}

#[async_trait]
impl SessionConnector for OpenCodeConnector {
    fn source_id(&self) -> &str {
        "opencode"
    }

    fn display_name(&self) -> &str {
        "OpenCode"
    }

    fn detect(&self) -> ConnectorStatus {
        if let Some(database) = self.sqlite_db_path()
            && database.exists()
        {
            return ConnectorStatus::Available {
                path: database,
                sessions_estimate: None,
            };
        }

        if let Some(path) = self.legacy_path() {
            let history_file = path.join("prompt-history.jsonl");
            if history_file.exists() {
                let count = std::fs::read_to_string(&history_file)
                    .map(|content| {
                        content
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .count()
                    })
                    .unwrap_or(0);
                return ConnectorStatus::Available {
                    path,
                    sessions_estimate: Some(usize::from(count > 0)),
                };
            }
        }

        ConnectorStatus::NotFound
    }

    fn default_path(&self) -> Option<PathBuf> {
        self.sqlite_db_path().or_else(|| self.legacy_path())
    }

    async fn import(&self, options: &ImportOptions) -> Result<Vec<Session>> {
        let mut sessions = if options.path.is_some() {
            self.import_jsonl(options).await?
        } else if let Some(database) = self.sqlite_db_path()
            && database.exists()
        {
            self.import_sqlite(&database, options).await?
        } else {
            self.import_jsonl(options).await?
        };

        crate::redaction::redact_sessions(&mut sessions);
        Ok(sessions)
    }
}

impl OpenCodeConnector {
    /// Path to OpenCode's current SQLite database.
    fn sqlite_db_path(&self) -> Option<PathBuf> {
        dirs::data_local_dir().map(|directory| directory.join("opencode").join("opencode.db"))
    }

    /// Path to OpenCode's legacy JSONL state directory.
    fn legacy_path(&self) -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".local").join("state").join("opencode"))
    }

    #[cfg(feature = "opencode-connector")]
    async fn import_sqlite(&self, db_path: &Path, options: &ImportOptions) -> Result<Vec<Session>> {
        let db_path = db_path.to_path_buf();
        let limit = options.limit;
        tokio::task::spawn_blocking(move || Self::import_sqlite_sync(&db_path, limit))
            .await
            .context("OpenCode SQLite import task failed")?
    }

    #[cfg(feature = "opencode-connector")]
    fn import_sqlite_sync(db_path: &Path, limit: Option<usize>) -> Result<Vec<Session>> {
        use std::collections::{BTreeMap, HashMap};

        use rusqlite::Connection;

        struct PendingMessage {
            role: MessageRole,
            created_at: Option<jiff::Timestamp>,
            parts: Vec<String>,
        }

        type PendingSession = (Session, Vec<PendingMessage>, HashMap<String, usize>);

        let uri = format!("file:{}?mode=ro", db_path.display());
        let connection = Connection::open_with_flags(
            &uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .context("Failed to open OpenCode SQLite DB")?;

        let mut statement = connection.prepare(
            "SELECT s.id, s.title, s.directory, s.time_created, s.model, s.cost, \
                    s.tokens_input, s.tokens_output, m.id, m.time_created, m.data, \
                    p.id, p.time_created, p.data \
             FROM session s JOIN message m ON m.session_id = s.id \
             JOIN part p ON p.message_id = m.id \
             WHERE json_extract(p.data, '$.type') = 'text' \
             ORDER BY s.id, m.time_created, p.id",
        )?;

        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<f64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, String>(13)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut session_map: BTreeMap<String, PendingSession> = BTreeMap::new();

        for (
            session_id,
            title,
            directory,
            _session_created,
            model,
            cost,
            tokens_input,
            tokens_output,
            message_id,
            message_created,
            message_data,
            _part_id,
            _part_created,
            part_data,
        ) in rows
        {
            let role = serde_json::from_str::<serde_json::Value>(&message_data)
                .ok()
                .and_then(|value| {
                    value
                        .get("role")
                        .and_then(|role| role.as_str().map(String::from))
                })
                .map(|role| match role.as_str() {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    _ => MessageRole::Other,
                })
                .unwrap_or(MessageRole::Other);
            let text = serde_json::from_str::<serde_json::Value>(&part_data)
                .ok()
                .and_then(|value| {
                    value
                        .get("text")
                        .and_then(|text| text.as_str().map(String::from))
                })
                .unwrap_or_default();
            if text.is_empty() {
                continue;
            }

            let timestamp = jiff::Timestamp::from_second(message_created / 1000).ok();
            let (_, messages, message_indices) =
                session_map.entry(session_id.clone()).or_insert_with(|| {
                    let mut extra = serde_json::Map::new();
                    if let Some(cost) = cost {
                        extra.insert("cost".into(), serde_json::json!(cost));
                    }
                    if let Some(tokens_input) = tokens_input {
                        extra.insert("tokens_input".into(), serde_json::json!(tokens_input));
                    }
                    if let Some(tokens_output) = tokens_output {
                        extra.insert("tokens_output".into(), serde_json::json!(tokens_output));
                    }
                    (
                        Session {
                            id: format!("opencode:{session_id}"),
                            source: "opencode".into(),
                            external_id: session_id,
                            title,
                            source_path: db_path.to_path_buf(),
                            started_at: timestamp,
                            ended_at: timestamp,
                            messages: Vec::new(),
                            metadata: SessionMetadata::new(
                                directory,
                                model,
                                vec!["opencode".into()],
                                serde_json::Value::Object(extra),
                            ),
                        },
                        Vec::new(),
                        HashMap::new(),
                    )
                });
            let message_index = *message_indices.entry(message_id).or_insert_with(|| {
                let index = messages.len();
                messages.push(PendingMessage {
                    role,
                    created_at: timestamp,
                    parts: Vec::new(),
                });
                index
            });
            messages[message_index].parts.push(text);
        }

        let mut sessions = Vec::new();
        for (_, (mut session, messages, _)) in session_map {
            for (idx, message) in messages.into_iter().enumerate() {
                let content = message.parts.join("\n");
                session.messages.push(Message {
                    idx,
                    role: message.role,
                    author: None,
                    content: content.clone(),
                    blocks: vec![ContentBlock::Text { text: content }],
                    created_at: message.created_at,
                    extra: serde_json::Value::Null,
                });
            }
            session.started_at = session
                .messages
                .first()
                .and_then(|message| message.created_at);
            session.ended_at = session
                .messages
                .last()
                .and_then(|message| message.created_at);
            if !session.messages.is_empty() {
                sessions.push(session);
                if limit.is_some_and(|limit| limit > 0 && sessions.len() >= limit) {
                    break;
                }
            }
        }

        Ok(sessions)
    }

    #[cfg(not(feature = "opencode-connector"))]
    async fn import_sqlite(
        &self,
        _db_path: &Path,
        _options: &ImportOptions,
    ) -> Result<Vec<Session>> {
        Ok(Vec::new())
    }

    async fn import_jsonl(&self, options: &ImportOptions) -> Result<Vec<Session>> {
        let path = options
            .path
            .clone()
            .or_else(|| self.legacy_path())
            .ok_or_else(|| anyhow::anyhow!("No OpenCode legacy data path found"))?;
        let history_file = path.join("prompt-history.jsonl");
        if !history_file.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&history_file).await?;
        let mut messages = Vec::new();
        for (idx, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<LegacyEntry>(line)
                && let Some(input) = entry.input
                && !input.is_empty()
            {
                messages.push(Message {
                    idx,
                    role: MessageRole::User,
                    author: None,
                    content: input.clone(),
                    blocks: vec![ContentBlock::Text { text: input }],
                    created_at: None,
                    extra: serde_json::json!({
                        "mode": entry.mode,
                        "parts": entry.parts,
                    }),
                });
            }
        }

        if let Some(limit) = options.limit
            && limit > 0
        {
            messages.truncate(limit);
        }
        if messages.is_empty() {
            return Ok(Vec::new());
        }

        Ok(vec![Session {
            id: "opencode:prompt-history".into(),
            source: "opencode".into(),
            external_id: "prompt-history".into(),
            title: Some("OpenCode Prompt History".into()),
            source_path: history_file,
            started_at: None,
            ended_at: None,
            messages,
            metadata: SessionMetadata::new(
                None,
                None,
                vec!["opencode".into()],
                serde_json::json!({"type": "prompt-history"}),
            ),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connector_source_id() {
        let connector = OpenCodeConnector;
        assert_eq!(connector.source_id(), "opencode");
        assert_eq!(connector.display_name(), "OpenCode");
    }

    #[test]
    fn test_parse_entry() {
        let json = r#"{"input":"use ask user tool to ask all questions one by one","parts":[],"mode":"normal"}"#;
        let entry: LegacyEntry = serde_json::from_str(json).unwrap();
        assert_eq!(
            entry.input,
            Some("use ask user tool to ask all questions one by one".to_string())
        );
        assert_eq!(entry.mode, Some("normal".to_string()));
        assert!(entry.parts.is_empty());
    }

    #[test]
    fn test_sqlite_path() {
        assert!(
            OpenCodeConnector
                .sqlite_db_path()
                .unwrap()
                .ends_with("opencode/opencode.db")
        );
    }

    #[test]
    fn test_legacy_path() {
        assert!(
            OpenCodeConnector
                .legacy_path()
                .unwrap()
                .ends_with(".local/state/opencode")
        );
    }

    #[tokio::test]
    async fn test_import_from_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let history_file = dir.path().join("prompt-history.jsonl");
        let content = [
            r#"{"input":"hello world","parts":[],"mode":"normal"}"#,
            r#"{"input":"second prompt","parts":[],"mode":"insert"}"#,
        ]
        .join("\n");
        tokio::fs::write(&history_file, content).await.unwrap();

        let options = ImportOptions::default().with_path(dir.path().to_path_buf());
        let sessions = OpenCodeConnector.import(&options).await.unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].messages.len(), 2);
        assert_eq!(sessions[0].messages[0].role, MessageRole::User);
        assert_eq!(sessions[0].messages[0].content, "hello world");
        assert_eq!(sessions[0].messages[1].content, "second prompt");
    }

    #[tokio::test]
    async fn test_import_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("prompt-history.jsonl"), "")
            .await
            .unwrap();

        let options = ImportOptions::default().with_path(dir.path().to_path_buf());
        let sessions = OpenCodeConnector.import(&options).await.unwrap();

        assert!(sessions.is_empty());
    }

    #[tokio::test]
    async fn test_import_with_limit() {
        let dir = tempfile::tempdir().unwrap();
        let content = [
            r#"{"input":"first","parts":[],"mode":"normal"}"#,
            r#"{"input":"second","parts":[],"mode":"normal"}"#,
            r#"{"input":"third","parts":[],"mode":"normal"}"#,
        ]
        .join("\n");
        tokio::fs::write(dir.path().join("prompt-history.jsonl"), content)
            .await
            .unwrap();

        let options = ImportOptions::default()
            .with_path(dir.path().to_path_buf())
            .with_limit(2);
        let sessions = OpenCodeConnector.import(&options).await.unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].messages.len(), 2);
    }

    #[cfg(feature = "opencode-connector")]
    #[tokio::test(flavor = "current_thread")]
    async fn test_import_sqlite_runs_on_blocking_pool() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("opencode.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY, title TEXT, directory TEXT,
                    time_created INTEGER, model TEXT, cost REAL,
                    tokens_input INTEGER, tokens_output INTEGER
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                INSERT INTO session VALUES
                    ('s1', 'Blocking pool', NULL, NULL, NULL, NULL, NULL, NULL);
                INSERT INTO message VALUES
                    ('m1', 's1', 1000, '{\"role\":\"user\"}');
                INSERT INTO part VALUES
                    ('p1', 'm1', 1000, '{\"type\":\"text\",\"text\":\"question\"}');",
            )
            .unwrap();
        drop(connection);

        let sessions = OpenCodeConnector
            .import_sqlite(&database, &ImportOptions::default())
            .await
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].messages[0].content, "question");
    }

    #[cfg(feature = "opencode-connector")]
    #[tokio::test]
    async fn test_import_sqlite_preserves_chronological_message_order() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("opencode.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY, title TEXT, directory TEXT,
                    time_created INTEGER, model TEXT, cost REAL,
                    tokens_input INTEGER, tokens_output INTEGER
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                INSERT INTO session VALUES
                    ('s1', 'Ordered', NULL, NULL, NULL, NULL, NULL, NULL);
                INSERT INTO message VALUES
                    ('z-first', 's1', 1000, '{\"role\":\"user\"}'),
                    ('a-second', 's1', 3000, '{\"role\":\"assistant\"}');
                INSERT INTO part VALUES
                    ('p1', 'z-first', 1000, '{\"type\":\"text\",\"text\":\"first part\"}'),
                    ('p2', 'z-first', 2000, '{\"type\":\"text\",\"text\":\"continued\"}'),
                    ('p3', 'a-second', 3000, '{\"type\":\"text\",\"text\":\"second\"}');",
            )
            .unwrap();
        drop(connection);

        let sessions = OpenCodeConnector
            .import_sqlite(&database, &ImportOptions::default())
            .await
            .unwrap();

        let session = &sessions[0];
        assert_eq!(session.messages[0].content, "first part\ncontinued");
        assert_eq!(session.messages[1].content, "second");
        assert_eq!(
            session.messages[0].created_at,
            jiff::Timestamp::from_second(1).ok()
        );
        assert_eq!(
            session.messages[1].created_at,
            jiff::Timestamp::from_second(3).ok()
        );
        assert_eq!(session.started_at, session.messages[0].created_at);
        assert_eq!(session.ended_at, session.messages[1].created_at);
    }

    #[cfg(feature = "opencode-connector")]
    #[tokio::test]
    async fn test_import_sqlite_accepts_nullable_session_metadata() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("opencode.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY, title TEXT, directory TEXT,
                    time_created INTEGER, model TEXT, cost REAL,
                    tokens_input INTEGER, tokens_output INTEGER
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                INSERT INTO session VALUES
                    ('s1', NULL, NULL, NULL, NULL, NULL, NULL, NULL);
                INSERT INTO message VALUES
                    ('m1', 's1', 1000, '{\"role\":\"user\"}');
                INSERT INTO part VALUES
                    ('p1', 'm1', 1000, '{\"type\":\"text\",\"text\":\"question\"}');",
            )
            .unwrap();
        drop(connection);

        let sessions = OpenCodeConnector
            .import_sqlite(&database, &ImportOptions::default())
            .await
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, None);
        assert_eq!(sessions[0].metadata.project_path, None);
        assert_eq!(sessions[0].metadata.model, None);
        assert_eq!(sessions[0].metadata.extra, serde_json::json!({}));
        assert_eq!(sessions[0].messages[0].content, "question");
    }

    #[cfg(feature = "opencode-connector")]
    #[tokio::test]
    async fn test_sqlite_limit_applies_to_sessions_not_messages() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("opencode.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY, title TEXT NOT NULL, directory TEXT NOT NULL,
                    time_created INTEGER NOT NULL, model TEXT, cost REAL NOT NULL,
                    tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                INSERT INTO session VALUES
                    ('s1', 'First session', '/tmp/first', 1000, NULL, 0, 0, 0),
                    ('s2', 'Second session', '/tmp/second', 3000, NULL, 0, 0, 0);
                INSERT INTO message VALUES
                    ('m1', 's1', 1000, '{\"role\":\"user\"}'),
                    ('m2', 's1', 2000, '{\"role\":\"assistant\"}'),
                    ('m3', 's2', 3000, '{\"role\":\"user\"}');
                INSERT INTO part VALUES
                    ('p1', 'm1', 1000, '{\"type\":\"text\",\"text\":\"question\"}'),
                    ('p2', 'm2', 2000, '{\"type\":\"text\",\"text\":\"answer\"}'),
                    ('p3', 'm3', 3000, '{\"type\":\"text\",\"text\":\"later\"}');",
            )
            .unwrap();
        drop(connection);

        let sessions = OpenCodeConnector
            .import_sqlite(&database, &ImportOptions::default().with_limit(1))
            .await
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "opencode:s1");
        assert_eq!(sessions[0].messages.len(), 2);
        assert_eq!(sessions[0].messages[0].content, "question");
        assert_eq!(sessions[0].messages[1].content, "answer");
    }

    #[cfg(feature = "opencode-connector")]
    #[tokio::test]
    async fn test_import_sqlite() {
        use rusqlite::Connection;

        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("opencode.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE session (
                    id TEXT PRIMARY KEY, title TEXT NOT NULL, directory TEXT NOT NULL,
                    time_created INTEGER NOT NULL, model TEXT, cost REAL NOT NULL,
                    tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL
                );
                CREATE TABLE message (
                    id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                CREATE TABLE part (
                    id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
                    time_created INTEGER NOT NULL, data TEXT NOT NULL
                );
                INSERT INTO session VALUES
                    ('s1', 'SQLite session', '/tmp/project', 1000, 'test-model', 0.25, 10, 20);
                INSERT INTO message VALUES
                    ('m1', 's1', 1000, '{\"role\":\"user\"}'),
                    ('m2', 's1', 2000, '{\"role\":\"assistant\"}');
                INSERT INTO part VALUES
                    ('p1', 'm1', 1000, '{\"type\":\"text\",\"text\":\"question\"}'),
                    ('p2', 'm2', 2000, '{\"type\":\"text\",\"text\":\"answer\"}');",
            )
            .unwrap();
        drop(connection);

        let sessions = OpenCodeConnector
            .import_sqlite(&database, &ImportOptions::default())
            .await
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "opencode:s1");
        assert_eq!(sessions[0].messages.len(), 2);
        assert_eq!(sessions[0].messages[0].role, MessageRole::User);
        assert_eq!(sessions[0].messages[0].content, "question");
        assert_eq!(sessions[0].messages[1].role, MessageRole::Assistant);
        assert_eq!(sessions[0].messages[1].content, "answer");
    }
}
