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
        use std::collections::BTreeMap;

        use rusqlite::Connection;

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
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                    row.get::<_, i64>(12)?,
                    row.get::<_, String>(13)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        type Messages = BTreeMap<String, (MessageRole, Vec<String>)>;
        let mut session_map: BTreeMap<String, (Session, Messages)> = BTreeMap::new();

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

            let timestamp =
                jiff::Timestamp::from_second((message_created / 1000).try_into().unwrap_or(0)).ok();
            let (_, messages) = session_map.entry(session_id.clone()).or_insert_with(|| {
                (
                    Session {
                        id: format!("opencode:{session_id}"),
                        source: "opencode".into(),
                        external_id: session_id,
                        title: Some(title),
                        source_path: db_path.to_path_buf(),
                        started_at: timestamp,
                        ended_at: timestamp,
                        messages: Vec::new(),
                        metadata: SessionMetadata::new(
                            Some(directory),
                            model,
                            vec!["opencode".into()],
                            serde_json::json!({
                                "cost": cost,
                                "tokens_input": tokens_input,
                                "tokens_output": tokens_output,
                            }),
                        ),
                    },
                    BTreeMap::new(),
                )
            });
            messages
                .entry(message_id)
                .or_insert_with(|| (role, Vec::new()))
                .1
                .push(text);
        }

        let mut sessions = Vec::new();
        for (_, (mut session, messages)) in session_map {
            for (idx, (_, (role, text_parts))) in messages.into_iter().enumerate() {
                if options.limit.is_some_and(|limit| limit > 0 && idx >= limit) {
                    break;
                }
                let content = text_parts.join("\n");
                session.messages.push(Message {
                    idx,
                    role,
                    author: None,
                    content: content.clone(),
                    blocks: vec![ContentBlock::Text { text: content }],
                    created_at: None,
                    extra: serde_json::Value::Null,
                });
            }
            if !session.messages.is_empty() {
                sessions.push(session);
                if options
                    .limit
                    .is_some_and(|limit| limit > 0 && sessions.len() >= limit)
                {
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
