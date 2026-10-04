//! Secret redaction for session content.
//!
//! Applied to message content during import to prevent secrets found in AI coding
//! sessions (API keys, tokens, connection strings) from being persisted verbatim.

use crate::model::{ContentBlock, Message, Session};
use regex::Regex;

/// Regex patterns: (pattern, replacement).
/// Ordered from most specific to least specific — Bearer is matched before bare sk- tokens
/// so that `Bearer sk-xxx` is replaced as a unit rather than leaving the `Bearer` label behind.
const SECRET_PATTERNS: &[(&str, &str)] = &[
    // HTTP Bearer tokens (must precede bare sk- / xox* patterns)
    (r"Bearer\s+[A-Za-z0-9\-._~+/]+=*", "Bearer [REDACTED]"),
    // AWS Access Key IDs (AKIA prefix)
    (r"AKIA[A-Z0-9]{16}", "[AWS_KEY_REDACTED]"),
    // OpenAI / generic sk- API keys
    (r"sk-[A-Za-z0-9\-_]{20,}", "[OPENAI_KEY_REDACTED]"),
    // Slack tokens
    (r"xox[baprs]-[A-Za-z0-9\-]+", "[SLACK_TOKEN_REDACTED]"),
    // GitHub personal access tokens
    (r"ghp_[A-Za-z0-9]{36}", "[GITHUB_TOKEN_REDACTED]"),
    (r"gho_[A-Za-z0-9]{36}", "[GITHUB_TOKEN_REDACTED]"),
    // Database connection strings with embedded credentials
    (r"postgresql://[^@\s]+:[^@\s]+@", "postgresql://[REDACTED]@"),
    (r"mysql://[^@\s]+:[^@\s]+@", "mysql://[REDACTED]@"),
    (
        r"mongodb(?:\+srv)?://[^@\s]+:[^@\s]+@",
        "mongodb://[REDACTED]@",
    ),
    (r"redis://[^@\s]+:[^@\s]+@", "redis://[REDACTED]@"),
];

/// Redact secrets from a text string.
///
/// Applies regex patterns to replace API keys, tokens, and connection strings
/// with `[REDACTED]` placeholders. Safe to call on arbitrary text — returns the
/// input unchanged when no patterns match.
///
/// # Example
///
/// ```
/// use terraphim_sessions::redaction::redact_session_content;
///
/// let input = "curl -H 'Authorization: Bearer sk-1234567890abcdef1234567890abcdef'";
/// let redacted = redact_session_content(input);
/// assert!(redacted.contains("[REDACTED]"));
/// assert!(!redacted.contains("sk-1234567890abcdef1234567890abcdef"));
/// ```
/// Compiled redaction patterns, built once per process.
///
/// Compiling on every call is pathological: import over a large corpus
/// (hundreds of thousands of messages) would recompile the whole pattern
/// set per message. `OnceLock` keeps the build cost to one pay-up-front.
static COMPILED_PATTERNS: std::sync::OnceLock<Vec<(Regex, &'static str)>> =
    std::sync::OnceLock::new();

fn compiled_patterns() -> &'static [(Regex, &'static str)] {
    COMPILED_PATTERNS.get_or_init(|| {
        SECRET_PATTERNS
            .iter()
            .filter_map(|(pattern, replacement)| {
                Regex::new(pattern)
                    .map(|re| (re, *replacement))
                    .map_err(|e| tracing::warn!("invalid redaction pattern {pattern:?}: {e}"))
                    .ok()
            })
            .collect()
    })
}

pub fn redact_session_content(text: &str) -> String {
    let mut result = text.to_string();
    for (re, replacement) in compiled_patterns() {
        result = re.replace_all(&result, *replacement).to_string();
    }
    result
}

/// Redact secrets from all text fields in a message in place.
pub(crate) fn redact_message(msg: &mut Message) {
    msg.content = redact_session_content(&msg.content);
    for block in &mut msg.blocks {
        match block {
            ContentBlock::Text { text } => {
                *text = redact_session_content(text);
            }
            ContentBlock::ToolResult { content, .. } => {
                *content = redact_session_content(content);
            }
            // ToolUse.input is serde_json::Value (structured data) — redacting arbitrary
            // JSON values risks corrupting structure. Image blocks are binary. Skip both.
            ContentBlock::ToolUse { .. } | ContentBlock::Image { .. } => {}
        }
    }
}

/// Redact secrets from all messages across a slice of sessions.
pub(crate) fn redact_sessions(sessions: &mut [Session]) {
    for session in sessions {
        for msg in &mut session.messages {
            redact_message(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ContentBlock, Message, MessageRole};

    #[test]
    fn redact_openai_key() {
        let input = "sk-1234567890abcdef1234567890abcdef1234";
        let redacted = redact_session_content(input);
        assert!(
            !redacted.contains("sk-1234567890"),
            "key should be redacted"
        );
        assert!(redacted.contains("[OPENAI_KEY_REDACTED]"));
    }

    #[test]
    fn redact_bearer_token() {
        let input = "Authorization: Bearer sk-1234567890abcdef1234567890abcdef";
        let redacted = redact_session_content(input);
        assert!(
            !redacted.contains("sk-1234567890"),
            "Bearer token should be redacted"
        );
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn redact_aws_key() {
        let input = "AWS key: AKIAIOSFODNN7EXAMPLE connected";
        let redacted = redact_session_content(input);
        assert!(redacted.contains("[AWS_KEY_REDACTED]"));
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn redact_connection_string() {
        let input = "postgresql://admin:s3cr3tpass@localhost:5432/mydb";
        let redacted = redact_session_content(input);
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("s3cr3tpass"));
    }

    #[test]
    fn safe_text_unchanged() {
        let input = "cargo build --release --workspace";
        assert_eq!(redact_session_content(input), input);
    }

    #[test]
    fn redact_github_token() {
        let input = "GITHUB_TOKEN=ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ1234567890";
        let redacted = redact_session_content(input);
        assert!(redacted.contains("[GITHUB_TOKEN_REDACTED]"));
        assert!(!redacted.contains("ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ1234567890"));
    }

    #[test]
    fn redact_message_content_and_text_block() {
        let secret = "sk-abcdefghijklmnopqrst12345678901234";
        let mut msg = Message {
            idx: 0,
            role: MessageRole::User,
            author: None,
            content: format!("API key: {secret}"),
            blocks: vec![ContentBlock::Text {
                text: format!("API key: {secret}"),
            }],
            created_at: None,
            extra: serde_json::Value::Null,
        };
        redact_message(&mut msg);
        assert!(
            !msg.content.contains(secret),
            "message.content should be redacted"
        );
        assert!(msg.content.contains("[OPENAI_KEY_REDACTED]"));
        if let ContentBlock::Text { text } = &msg.blocks[0] {
            assert!(!text.contains(secret), "text block should be redacted");
        } else {
            panic!("expected Text block");
        }
    }

    #[test]
    fn redact_message_tool_result_content() {
        let secret = "redis://user:hunter2@cache.internal:6379";
        let mut msg = Message {
            idx: 1,
            role: MessageRole::Tool,
            author: None,
            content: secret.to_string(),
            blocks: vec![ContentBlock::ToolResult {
                tool_use_id: "t1".to_string(),
                content: secret.to_string(),
                exit_code: 0,
            }],
            created_at: None,
            extra: serde_json::Value::Null,
        };
        redact_message(&mut msg);
        assert!(
            !msg.content.contains("hunter2"),
            "tool result content should be redacted"
        );
        if let ContentBlock::ToolResult { content, .. } = &msg.blocks[0] {
            assert!(
                !content.contains("hunter2"),
                "ToolResult block should be redacted"
            );
            assert!(content.contains("[REDACTED]"));
        } else {
            panic!("expected ToolResult block");
        }
    }
}
