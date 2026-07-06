use crate::models::{
    AgentInvocation, ContentBlock, FileOpType, FileOperation, Message, SessionEntry, ToolCategory,
    ToolInvocation, extract_file_path, parse_timestamp,
};
use crate::patterns::PatternMatcher;
use crate::tool_analyzer;
use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::{debug, info, warn};

/// Claude Code JSONL entry types that carry metadata, not conversational messages.
/// These are skipped during parsing to avoid false deserialization failures and
/// reduce log noise (~2003 of ~2430 WARN lines eliminated).
const SKIP_ENTRY_TYPES: &[&str] = &[
    "last-prompt",
    "mode",
    "permission-mode",
    "ai-title",
    "file-history-snapshot",
    "queue-operation",
    "agent-name",
    "pr-link",
    "tool_reference",
    "text",
    "attachment",
    "system",
];

/// Lightweight struct for peeking at the entry type before full deserialization.
#[derive(Deserialize)]
struct EntryTypePeek {
    #[serde(rename = "type")]
    entry_type: String,
}

pub struct SessionParser {
    entries: Vec<SessionEntry>,
    session_id: String,
    project_path: String,
}

impl SessionParser {
    /// Parse a single JSONL session file
    /// Parse a single JSONL session file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or contains malformed JSON
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        info!("Parsing session file: {}", path.display());

        let file = File::open(path)
            .with_context(|| format!("Failed to open session file: {}", path.display()))?;
        let reader = BufReader::new(file);

        let mut entries = Vec::new();
        let mut session_id = String::new();
        let mut project_path = String::new();

        for (line_num, line) in reader.lines().enumerate() {
            match line {
                Ok(line) if !line.trim().is_empty() => {
                    if let Ok(peek) = serde_json::from_str::<EntryTypePeek>(&line)
                        && SKIP_ENTRY_TYPES.contains(&peek.entry_type.as_str())
                    {
                        debug!("Skipping metadata entry of type: {}", peek.entry_type);
                        continue;
                    }
                    match serde_json::from_str::<SessionEntry>(&line) {
                        Ok(entry) => {
                            // Extract session metadata from first entry
                            if session_id.is_empty() {
                                session_id.clone_from(&entry.session_id);
                            }
                            if project_path.is_empty() {
                                if let Some(cwd) = &entry.cwd {
                                    project_path.clone_from(cwd);
                                }
                            }
                            entries.push(entry);
                        }
                        Err(e) => {
                            warn!(
                                "Failed to parse line {}: {} - Error: {}",
                                line_num + 1,
                                line,
                                e
                            );
                        }
                    }
                }
                Ok(_) => {
                    // Skip empty lines
                }
                Err(e) => {
                    warn!("Failed to read line {}: {}", line_num + 1, e);
                }
            }
        }

        info!(
            "Parsed {} entries from session {}",
            entries.len(),
            session_id
        );

        Ok(Self {
            entries,
            session_id,
            project_path,
        })
    }

    /// Find all session files in the default Claude directory
    ///
    /// # Errors
    ///
    /// Returns an error if the Claude directory doesn't exist or cannot be read
    pub fn from_default_location() -> Result<Vec<Self>> {
        let home = home::home_dir().context("Could not find home directory")?;
        let claude_dir = home.join(".claude").join("projects");

        if !claude_dir.exists() {
            return Err(anyhow::anyhow!(
                "Claude projects directory not found at: {}",
                claude_dir.display()
            ));
        }

        Self::from_directory(claude_dir)
    }

    /// Parse all session files in a directory
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be read or contains invalid session files
    pub fn from_directory<P: AsRef<Path>>(dir: P) -> Result<Vec<Self>> {
        let dir = dir.as_ref();
        info!("Scanning for session files in: {}", dir.display());

        let mut parsers = Vec::new();

        // Walk through all project directories
        for entry in walkdir::WalkDir::new(dir)
            .max_depth(2)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if path.extension() == Some("jsonl".as_ref()) {
                match Self::from_file(path) {
                    Ok(parser) => {
                        debug!("Successfully parsed session: {}", parser.session_id);
                        parsers.push(parser);
                    }
                    Err(e) => {
                        warn!("Failed to parse session file {}: {}", path.display(), e);
                    }
                }
            }
        }

        info!("Found {} valid session files", parsers.len());
        Ok(parsers)
    }

    /// Extract agent invocations from Task tool uses
    #[must_use]
    pub fn extract_agent_invocations(&self) -> Vec<AgentInvocation> {
        self.entries
            .par_iter()
            .filter_map(|entry| {
                if let Message::Assistant { content, .. } = &entry.message {
                    for block in content {
                        if let ContentBlock::ToolUse { name, input, id } = block {
                            if name == "Task" {
                                return self.parse_task_invocation(entry, input, id);
                            }
                        }
                    }
                }
                None
            })
            .collect()
    }

    /// Parse a Task tool invocation into an `AgentInvocation`
    fn parse_task_invocation(
        &self,
        entry: &SessionEntry,
        input: &serde_json::Value,
        _tool_id: &str,
    ) -> Option<AgentInvocation> {
        let agent_type = input
            .get("subagent_type")
            .and_then(|v| v.as_str())?
            .to_string();

        let task_description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let timestamp = match parse_timestamp(&entry.timestamp) {
            Ok(ts) => ts,
            Err(e) => {
                warn!("Failed to parse timestamp '{}': {}", entry.timestamp, e);
                return None;
            }
        };

        Some(AgentInvocation {
            timestamp,
            agent_type,
            task_description,
            prompt,
            files_modified: Vec::new(), // Will be populated later
            tools_used: Vec::new(),     // Will be populated later
            duration_ms: None,          // Will be calculated later
            parent_message_id: entry.uuid.clone(),
            session_id: self.session_id.clone(),
        })
    }

    /// Extract file operations from tool uses
    #[must_use]
    pub fn extract_file_operations(&self) -> Vec<FileOperation> {
        self.entries
            .par_iter()
            .filter_map(|entry| {
                if let Message::Assistant { content, .. } = &entry.message {
                    for block in content {
                        if let ContentBlock::ToolUse { name, input, .. } = block {
                            if let Ok(op_type) = name.parse::<FileOpType>() {
                                if let Some(file_path) = extract_file_path(input) {
                                    let timestamp = match parse_timestamp(&entry.timestamp) {
                                        Ok(ts) => ts,
                                        Err(e) => {
                                            warn!(
                                                "Failed to parse timestamp '{}': {}",
                                                entry.timestamp, e
                                            );
                                            continue;
                                        }
                                    };

                                    return Some(FileOperation {
                                        timestamp,
                                        operation: op_type,
                                        file_path,
                                        agent_context: None, // Will be set during analysis
                                        session_id: self.session_id.clone(),
                                        message_id: entry.uuid.clone(),
                                    });
                                }
                            }
                        }
                    }
                }
                None
            })
            .collect()
    }

    /// Extract tool invocations from Bash commands
    ///
    /// # Arguments
    /// * `matcher` - Pattern matcher for identifying tools in commands
    ///
    /// # Returns
    /// A vector of `ToolInvocation` instances found in Bash tool uses
    #[must_use]
    #[allow(dead_code)] // Will be used in Phase 2
    pub fn extract_tool_invocations(&self, matcher: &dyn PatternMatcher) -> Vec<ToolInvocation> {
        self.entries
            .par_iter()
            .filter_map(|entry| {
                if let Message::Assistant { content, .. } = &entry.message {
                    extract_from_bash_command(entry, content, matcher, &self.session_id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Find the active agent context for a given message
    #[must_use]
    pub fn find_active_agent(&self, message_id: &str) -> Option<String> {
        // Look backwards from the given message to find the most recent Task invocation
        let mut found_message = false;

        for entry in self.entries.iter().rev() {
            if entry.uuid == message_id {
                found_message = true;
                continue;
            }

            if !found_message {
                continue;
            }

            // Look for Task tool invocations
            if let Message::Assistant { content, .. } = &entry.message {
                for block in content {
                    if let ContentBlock::ToolUse { name, input, .. } = block {
                        if name == "Task" {
                            if let Some(agent_type) =
                                input.get("subagent_type").and_then(|v| v.as_str())
                            {
                                return Some(agent_type.to_string());
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Get session metadata
    #[must_use]
    pub fn get_session_info(
        &self,
    ) -> (
        String,
        String,
        Option<jiff::Timestamp>,
        Option<jiff::Timestamp>,
    ) {
        let start_time = self.entries.first().and_then(|e| {
            parse_timestamp(&e.timestamp)
                .map_err(|err| {
                    debug!("Could not parse start timestamp '{}': {}", e.timestamp, err);
                    err
                })
                .ok()
        });
        let end_time = self.entries.last().and_then(|e| {
            parse_timestamp(&e.timestamp)
                .map_err(|err| {
                    debug!("Could not parse end timestamp '{}': {}", e.timestamp, err);
                    err
                })
                .ok()
        });

        (
            self.session_id.clone(),
            self.project_path.clone(),
            start_time,
            end_time,
        )
    }

    /// Get entry count for statistics
    /// Used in integration tests
    #[allow(dead_code)]
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get all entries
    #[must_use]
    pub fn entries(&self) -> &[SessionEntry] {
        &self.entries
    }

    /// Find entries within a time window
    /// Used in integration tests
    #[allow(dead_code)]
    #[must_use]
    pub fn entries_in_window(
        &self,
        start: jiff::Timestamp,
        end: jiff::Timestamp,
    ) -> Vec<&SessionEntry> {
        self.entries
            .iter()
            .filter(|entry| match parse_timestamp(&entry.timestamp) {
                Ok(timestamp) => timestamp >= start && timestamp <= end,
                Err(e) => {
                    debug!(
                        "Skipping entry with invalid timestamp '{}': {}",
                        entry.timestamp, e
                    );
                    false
                }
            })
            .collect()
    }

    /// Find all unique agent types used in this session
    /// Used in integration tests
    #[allow(dead_code)]
    #[must_use]
    pub fn get_agent_types(&self) -> Vec<String> {
        let agents = self.extract_agent_invocations();
        let mut agent_types: Vec<String> = agents
            .into_iter()
            .map(|a| a.agent_type)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        agent_types.sort();
        agent_types
    }

    /// Build a timeline of events for visualization
    /// Used in integration tests
    #[allow(dead_code)]
    #[must_use]
    pub fn build_timeline(&self) -> Vec<TimelineEvent> {
        let mut events = Vec::new();

        // Add agent invocations
        for agent in self.extract_agent_invocations() {
            events.push(TimelineEvent {
                timestamp: agent.timestamp,
                event_type: TimelineEventType::AgentInvocation,
                description: format!("{}: {}", agent.agent_type, agent.task_description),
                agent: Some(agent.agent_type),
                file: None,
            });
        }

        // Add file operations
        for file_op in self.extract_file_operations() {
            events.push(TimelineEvent {
                timestamp: file_op.timestamp,
                event_type: TimelineEventType::FileOperation,
                description: format!("{:?}: {}", file_op.operation, file_op.file_path),
                agent: file_op.agent_context,
                file: Some(file_op.file_path),
            });
        }

        // Sort by timestamp
        events.sort_by_key(|e| e.timestamp);
        events
    }
}

/// Helper function to extract tool invocations from Bash command content
#[allow(dead_code)] // Will be used in Phase 2
fn extract_from_bash_command(
    entry: &SessionEntry,
    content: &[ContentBlock],
    matcher: &dyn PatternMatcher,
    session_id: &str,
) -> Option<ToolInvocation> {
    for block in content {
        if let ContentBlock::ToolUse { name, input, .. } = block {
            if name == "Bash" {
                // Extract the command from the input
                let command = input.get("command").and_then(|v| v.as_str())?;

                // Find tool matches using the pattern matcher
                let matches = matcher.find_matches(command);

                if let Some(tool_match) = matches.first() {
                    // Parse command context to extract arguments and flags
                    if let Some((full_cmd, arguments, flags)) =
                        tool_analyzer::parse_command_context(command, tool_match.start)
                    {
                        // Filter out shell built-ins
                        if !tool_analyzer::is_actual_tool(&tool_match.tool_name) {
                            continue;
                        }

                        let timestamp = match parse_timestamp(&entry.timestamp) {
                            Ok(ts) => ts,
                            Err(e) => {
                                warn!("Failed to parse timestamp '{}': {}", entry.timestamp, e);
                                continue;
                            }
                        };

                        return Some(ToolInvocation {
                            timestamp,
                            tool_name: tool_match.tool_name.clone(),
                            tool_category: ToolCategory::from_string(&tool_match.category),
                            command_line: full_cmd,
                            arguments,
                            flags,
                            exit_code: None,     // Exit code not available from logs
                            agent_context: None, // Will be populated later
                            session_id: session_id.to_string(),
                            message_id: entry.uuid.clone(),
                        });
                    }
                }
            }
        }
    }

    None
}

/// Used in integration tests and public API
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TimelineEvent {
    pub timestamp: jiff::Timestamp,
    pub event_type: TimelineEventType,
    pub description: String,
    pub agent: Option<String>,
    pub file: Option<String>,
}

/// Used in integration tests and public API
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum TimelineEventType {
    AgentInvocation,
    FileOperation,
    UserMessage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_session_entry() {
        let json_line = r#"{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"/home/alex/projects/zestic-at/charm","sessionId":"b325985c-5c1c-48f1-97e2-e3185bb55886","version":"1.0.111","gitBranch":"","type":"user","message":{"role":"user","content":"test message"},"uuid":"ab88a3b0-544a-411a-a8a4-92b142e21472","timestamp":"2025-10-01T09:05:21.902Z"}"#;

        let entry: SessionEntry = serde_json::from_str(json_line).unwrap();
        assert_eq!(entry.session_id, "b325985c-5c1c-48f1-97e2-e3185bb55886");
        assert_eq!(entry.uuid, "ab88a3b0-544a-411a-a8a4-92b142e21472");
    }

    #[test]
    fn test_parse_task_invocation() {
        let json_line = r#"{"parentUuid":"parent-uuid","isSidechain":false,"userType":"external","cwd":"/home/alex/projects","sessionId":"test-session","version":"1.0.111","gitBranch":"","message":{"role":"assistant","content":[{"type":"tool_use","id":"tool-id","name":"Task","input":{"subagent_type":"architect","description":"Design system architecture","prompt":"Please design the architecture"}}]},"requestId":"req-123","type":"assistant","uuid":"msg-uuid","timestamp":"2025-10-01T09:05:21.902Z"}"#;

        let entry: SessionEntry = serde_json::from_str(json_line).unwrap();

        let parser = SessionParser {
            entries: vec![entry.clone()],
            session_id: "test-session".to_string(),
            project_path: "/home/alex/projects".to_string(),
        };

        let agents = parser.extract_agent_invocations();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_type, "architect");
        assert_eq!(agents[0].task_description, "Design system architecture");
    }

    #[test]
    fn test_extract_file_operations() {
        let json_line = r#"{"parentUuid":"parent-uuid","isSidechain":false,"userType":"external","cwd":"/home/alex/projects","sessionId":"test-session","version":"1.0.111","gitBranch":"","message":{"role":"assistant","content":[{"type":"tool_use","id":"tool-id","name":"Write","input":{"file_path":"/path/to/file.rs","content":"test content"}}]},"type":"assistant","uuid":"msg-uuid","timestamp":"2025-10-01T09:05:21.902Z"}"#;

        let entry: SessionEntry = serde_json::from_str(json_line).unwrap();

        let parser = SessionParser {
            entries: vec![entry],
            session_id: "test-session".to_string(),
            project_path: "/home/alex/projects".to_string(),
        };

        let file_ops = parser.extract_file_operations();
        assert_eq!(file_ops.len(), 1);
        assert_eq!(file_ops[0].file_path, "/path/to/file.rs");
        assert!(matches!(file_ops[0].operation, FileOpType::Write));
    }

    #[test]
    fn test_assistant_with_thinking_block_parses() {
        let json_line = r#"{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"/tmp","sessionId":"s1","version":"1.0","gitBranch":"","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Let me analyze this..."},{"type":"text","text":"Here is my answer"}]},"type":"assistant","uuid":"u1","timestamp":"2025-01-01T09:00:00.000Z"}"#;
        let entry: SessionEntry = serde_json::from_str(json_line).unwrap();
        assert_eq!(entry.entry_type, "assistant");
        if let crate::models::Message::Assistant { content, .. } = &entry.message {
            assert_eq!(content.len(), 2);
        } else {
            panic!("Expected Assistant message");
        }
    }

    #[test]
    fn test_assistant_with_unknown_content_block_parses() {
        let json_line = r#"{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"/tmp","sessionId":"s1","version":"1.0","gitBranch":"","message":{"role":"assistant","content":[{"type":"some_future_block_type","data":"whatever"}]},"type":"assistant","uuid":"u1","timestamp":"2025-01-01T09:00:00.000Z"}"#;
        let entry: SessionEntry = serde_json::from_str(json_line).unwrap();
        assert_eq!(entry.entry_type, "assistant");
    }

    #[test]
    fn test_entry_type_peek_extracts_type() {
        let json = r#"{"type":"last-prompt","lastPrompt":"echo hello","sessionId":"abc"}"#;
        let peek: EntryTypePeek = serde_json::from_str(json).unwrap();
        assert_eq!(peek.entry_type, "last-prompt");
    }

    #[test]
    fn test_skip_set_contains_metadata_types() {
        assert!(SKIP_ENTRY_TYPES.contains(&"last-prompt"));
        assert!(SKIP_ENTRY_TYPES.contains(&"mode"));
        assert!(SKIP_ENTRY_TYPES.contains(&"permission-mode"));
        assert!(SKIP_ENTRY_TYPES.contains(&"ai-title"));
        assert!(SKIP_ENTRY_TYPES.contains(&"file-history-snapshot"));
        assert!(SKIP_ENTRY_TYPES.contains(&"queue-operation"));
        assert!(SKIP_ENTRY_TYPES.contains(&"agent-name"));
        assert!(SKIP_ENTRY_TYPES.contains(&"pr-link"));
        assert!(SKIP_ENTRY_TYPES.contains(&"tool_reference"));
        assert!(SKIP_ENTRY_TYPES.contains(&"text"));
        assert!(SKIP_ENTRY_TYPES.contains(&"attachment"));
        assert!(SKIP_ENTRY_TYPES.contains(&"system"));
    }

    #[test]
    fn test_skip_set_excludes_message_types() {
        assert!(!SKIP_ENTRY_TYPES.contains(&"user"));
        assert!(!SKIP_ENTRY_TYPES.contains(&"assistant"));
    }

    #[test]
    fn test_metadata_entries_skipped_in_from_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("metadata-only.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"last-prompt","lastPrompt":"hi","leafUuid":"a","sessionId":"s1"}"#,
                "\n",
                r#"{"type":"mode","mode":"normal","sessionId":"s1"}"#,
                "\n",
                r#"{"type":"permission-mode","permissionMode":"auto","sessionId":"s1"}"#,
                "\n",
                r#"{"type":"ai-title","title":"Test","sessionId":"s1"}"#,
                "\n",
                r#"{"type":"system","content":"system init","sessionId":"s1"}"#,
                "\n",
                r#"{"type":"attachment","attachment":{"type":"deferred_tools_delta"},"uuid":"u1","sessionId":"s1","timestamp":"2025-01-01T00:00:00.000Z"}"#,
                "\n",
            ),
        )
        .unwrap();

        let parser = SessionParser::from_file(&path).unwrap();
        assert_eq!(
            parser.entries.len(),
            0,
            "All metadata entries should be skipped"
        );
    }

    #[test]
    fn test_user_and_assistant_entries_still_parsed() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("messages.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"parentUuid":null,"isSidechain":false,"userType":"external","cwd":"/tmp","sessionId":"s1","version":"1.0","gitBranch":"","type":"user","message":{"role":"user","content":"hello"},"uuid":"u1","timestamp":"2025-01-01T09:00:00.000Z"}"#,
                "\n",
                r#"{"parentUuid":"u1","isSidechain":false,"userType":"external","cwd":"/tmp","sessionId":"s1","version":"1.0","gitBranch":"","message":{"role":"assistant","content":[{"type":"text","text":"hi there"}]},"type":"assistant","uuid":"u2","timestamp":"2025-01-01T09:00:01.000Z"}"#,
                "\n",
                r#"{"type":"mode","mode":"normal","sessionId":"s1"}"#,
                "\n",
            ),
        )
        .unwrap();

        let parser = SessionParser::from_file(&path).unwrap();
        assert_eq!(
            parser.entries.len(),
            2,
            "user + assistant entries should parse, mode should be skipped"
        );
        assert_eq!(parser.entries[0].entry_type, "user");
        assert_eq!(parser.entries[1].entry_type, "assistant");
    }
}
