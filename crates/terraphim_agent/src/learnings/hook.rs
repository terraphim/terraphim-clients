//! Hook input types and parser for AI agent integration.
//!
//! This module parses JSON hook-event payloads emitted by AI coding agents and
//! normalises them into a single internal representation ([`HookInput`]) so that
//! failed commands can be captured as learnings regardless of which agent
//! produced the event.
//!
//! # Supported agents
//!
//! Different agents emit different hook-event envelopes. [`AgentFormat`] selects
//! the parser; `Auto` (the default) shape-sniffs the JSON.
//!
//! - **Claude Code** ([`AgentFormat::Claude`]): the canonical envelope
//!   `{ tool_name, tool_input.command, tool_result.{exit_code,stdout,stderr} }`.
//! - **opencode** ([`AgentFormat::Opencode`]): the native `tool.execute.after`
//!   envelope `{ tool, args.command, output, metadata.exitCode }` *or* the
//!   Claude-shaped payload its plugin normalises to before invocation.
//! - **Codex** ([`AgentFormat::Codex`]): the Claude-shaped tool event its shell
//!   hook forwards. Codex's turn-level `notify` events (e.g.
//!   `agent-turn-complete`) carry no per-command result and are accepted but
//!   never captured.
//!
//! # Usage
//!
//! ```rust,ignore
//! use terraphim_agent::learnings::{AgentFormat, HookInput};
//!
//! let json = r#"{ "tool_name": "Bash", "tool_input": {"command": "git push"}, "tool_result": {"exit_code": 1, "stdout": "", "stderr": "rejected"} }"#;
//! let input = HookInput::from_json_with_format(json, AgentFormat::Auto)?;
//!
//! if input.should_capture() {
//!     // Capture learning from failed command
//! }
//! ```

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::learnings::{
    LearningCaptureConfig, LearningError, capture_failed_command, redact_secrets,
};

/// Hook type for multi-hook pipeline.
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum LearnHookType {
    /// Pre-tool-use: warn if command matches past failure patterns
    PreToolUse,
    /// Post-tool-use: capture failed commands (existing behavior)
    PostToolUse,
    /// User prompt submit: capture user corrections inline
    UserPromptSubmit,
}

/// Per-agent hook-event format.
///
/// Selects how a raw hook-event payload is parsed before normalisation into a
/// [`HookInput`]. `Auto` shape-sniffs the JSON and is the default for the
/// `learn hook` CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum AgentFormat {
    /// Detect the envelope from the JSON shape.
    #[default]
    Auto,
    /// Claude Code `PostToolUse`/`PreToolUse` envelope.
    Claude,
    /// OpenAI Codex CLI hook/notify envelope.
    Codex,
    /// opencode plugin `tool.execute.*` envelope.
    Opencode,
}

/// Capture learning from hook input.
///
/// Extracts the command, error output, and exit code from the hook input
/// and delegates to `capture_failed_command` for storage.
///
/// # Arguments
///
/// * `input` - The parsed hook input
///
/// # Returns
///
/// Path to the saved learning file, or error if capture failed/ignored.
pub fn capture_from_hook(input: &HookInput) -> Result<PathBuf, LearningError> {
    let command = input
        .command()
        .ok_or_else(|| LearningError::Ignored("No command in input".to_string()))?;

    let error_output = input.error_output();
    let exit_code = input.tool_result.exit_code;

    let config = LearningCaptureConfig::default();
    capture_failed_command(command, &error_output, exit_code, &config)
}

/// Process hook input with an explicit hook type.
///
/// Routes to the appropriate handler based on the hook type:
/// - PreToolUse: checks command against known error patterns, warns if similar to past failure
/// - PostToolUse: captures failed commands (original behavior)
/// - UserPromptSubmit: captures user corrections inline
///
/// The `format` selects the per-agent parser; use [`AgentFormat::Auto`] to
/// shape-sniff the payload.
///
/// All hook types maintain fail-open behavior: errors are logged but
/// never block the pipeline.
pub async fn process_hook_input_with_type(
    hook_type: LearnHookType,
    format: AgentFormat,
) -> Result<(), HookError> {
    process_hook_with_streams(hook_type, format, tokio::io::stdin(), tokio::io::stdout()).await
}

/// Core hook processing logic with injectable I/O streams.
///
/// Reads from `reader`, dispatches to the hook handler, unconditionally redacts
/// any secrets from the input buffer, then writes the redacted output to `writer`.
/// Secrets are never forwarded to `writer`.
pub(crate) async fn process_hook_with_streams<R, W>(
    hook_type: LearnHookType,
    format: AgentFormat,
    mut reader: R,
    mut writer: W,
) -> Result<(), HookError>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Read full input
    let mut buffer = String::new();
    reader
        .read_to_string(&mut buffer)
        .await
        .map_err(HookError::Stdin)?;

    match hook_type {
        LearnHookType::PreToolUse => {
            process_pre_tool_use(&buffer, format);
        }
        LearnHookType::PostToolUse => {
            // Parse JSON and capture failures (existing behavior)
            match HookInput::from_json_with_format(&buffer, format) {
                Ok(input) => {
                    if input.should_capture()
                        && let Err(e) = capture_from_hook(&input)
                    {
                        log::debug!("Hook capture failed: {}", e);
                    }
                }
                Err(e) => {
                    log::debug!("Hook parse failed (fail-open): {}", e);
                }
            }
        }
        LearnHookType::UserPromptSubmit => {
            process_user_prompt_submit(&buffer);
        }
    }

    // Unconditionally redact secrets before passing through to the output stream.
    // The previous contains_secrets() fast-path was removed because its pattern
    // set did not cover GitHub PATs (ghp_*), Slack tokens (xox*), or connection
    // strings, allowing those secrets to bypass redaction entirely.
    let output = redact_secrets(&buffer);

    writer
        .write_all(output.as_bytes())
        .await
        .map_err(HookError::Stdin)?;

    Ok(())
}

/// Pre-tool-use handler: check if the command matches known failure patterns.
///
/// Reads the command from the JSON input and queries past learnings for
/// similar commands. If a match is found (especially one with a correction),
/// emits a warning to stderr. Never blocks execution.
fn process_pre_tool_use(json: &str, format: AgentFormat) {
    let input = match HookInput::from_json_with_format(json, format) {
        Ok(i) => i,
        Err(_) => return, // fail-open
    };

    let command = match input.command() {
        Some(c) => c,
        None => return, // not a Bash tool, nothing to check
    };

    let config = LearningCaptureConfig::default();
    let storage_dir = config.storage_location();

    // Query past learnings for similar commands
    let base_cmd = command.split_whitespace().next().unwrap_or(command);
    let learnings = match crate::learnings::capture::query_learnings(&storage_dir, base_cmd, false)
    {
        Ok(l) => l,
        Err(_) => return,
    };

    if learnings.is_empty() {
        return;
    }

    // Find the best match: prefer one with a correction
    let best = learnings
        .iter()
        .find(|l| l.correction.is_some())
        .or(learnings.first());

    if let Some(learning) = best {
        let mut warning = format!(
            "Warning: similar command failed before (exit {}): {}",
            learning.exit_code, learning.command
        );
        if let Some(ref correction) = learning.correction {
            warning.push_str(&format!("\n  Suggested: {}", correction));
        }
        eprintln!("{}", warning);
    }
}

/// User-prompt-submit handler: capture user corrections inline.
///
/// Expects JSON with "user_prompt" field. Looks for correction patterns
/// like "use X instead of Y" and captures them as correction events.
/// Never blocks execution.
fn process_user_prompt_submit(json: &str) {
    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return, // fail-open
    };

    let prompt = match value.get("user_prompt").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return,
    };

    // Look for correction patterns: "use X instead of Y", "use X not Y", "prefer X over Y"
    if let Some((original, corrected)) = parse_correction_pattern(prompt) {
        let config = LearningCaptureConfig::default();
        if let Err(e) = crate::learnings::capture_correction(
            crate::learnings::CorrectionType::ToolPreference,
            &original,
            &corrected,
            &format!("Auto-captured from user prompt: {}", prompt),
            &config,
        ) {
            log::debug!("User prompt correction capture failed: {}", e);
        }
    }
}

/// Try to parse a correction pattern from user text.
///
/// Supports patterns:
/// - "use X instead of Y"  -> (Y, X)
/// - "use X not Y"         -> (Y, X)
/// - "prefer X over Y"     -> (Y, X)
///
/// Returns None if no pattern matches.
fn parse_correction_pattern(text: &str) -> Option<(String, String)> {
    let lower = text.to_lowercase();
    let trimmed = lower.trim_start();

    // "use X instead of Y" (must start with "use")
    if let Some(use_idx) = trimmed.find("use ")
        && use_idx == 0
    {
        let text_after_use = &text[text.to_lowercase().trim_start().find("use ").unwrap() + 4..];
        let lower_after_use = text_after_use.to_lowercase();
        if let Some(instead_idx) = lower_after_use.find(" instead of ") {
            let corrected = text_after_use[..instead_idx].trim().to_string();
            let original = text_after_use[instead_idx + 12..]
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !corrected.is_empty() && !original.is_empty() {
                return Some((original, corrected));
            }
        }
        // "use X not Y"
        if let Some(not_idx) = lower_after_use.find(" not ") {
            let corrected = text_after_use[..not_idx].trim().to_string();
            let original = text_after_use[not_idx + 5..]
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !corrected.is_empty() && !original.is_empty() {
                return Some((original, corrected));
            }
        }
    }

    // "prefer X over Y" (must start with "prefer")
    if let Some(prefer_idx) = trimmed.find("prefer ")
        && prefer_idx == 0
    {
        let text_after_prefer =
            &text[text.to_lowercase().trim_start().find("prefer ").unwrap() + 7..];
        let lower_after_prefer = text_after_prefer.to_lowercase();
        if let Some(over_idx) = lower_after_prefer.find(" over ") {
            let corrected = text_after_prefer[..over_idx].trim().to_string();
            let original = text_after_prefer[over_idx + 6..]
                .trim()
                .trim_end_matches('.')
                .to_string();
            if !corrected.is_empty() && !original.is_empty() {
                return Some((original, corrected));
            }
        }
    }

    None
}

/// Errors that can occur during hook processing.
///
/// Only `Stdin` is currently produced: JSON-parse and capture failures are
/// handled inline (fail-open, logged) rather than propagated, so no further
/// variants are constructed.
#[derive(Debug, Error)]
pub enum HookError {
    /// Failed to read from stdin
    #[error("failed to read stdin: {0}")]
    Stdin(#[from] std::io::Error),
}

/// Input from AI agent hook.
///
/// This struct represents the JSON payload sent by AI agents
/// when a tool is executed. It contains the tool name, input parameters,
/// and execution result.
#[derive(Debug, Clone, Deserialize)]
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub struct HookInput {
    /// Tool name (e.g., "Bash", "Write", "Edit")
    pub tool_name: String,
    /// Tool input parameters
    pub tool_input: ToolInput,
    /// Tool execution result
    pub tool_result: ToolResult,
}

/// Tool input parameters.
///
/// For Bash tools, this contains the command string.
/// For other tools, additional fields are captured via the `extra` map.
#[derive(Debug, Clone, Deserialize)]
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub struct ToolInput {
    /// Command to execute (for Bash tool)
    pub command: Option<String>,
    /// Additional fields for other tool types
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Tool execution result.
///
/// Contains the exit code and captured output from the tool execution.
#[derive(Debug, Clone, Deserialize)]
// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
pub struct ToolResult {
    /// Exit code (0 = success, non-zero = failure)
    pub exit_code: i32,
    /// Standard output captured from the tool
    #[serde(default)]
    pub stdout: String,
    /// Standard error captured from the tool
    #[serde(default)]
    pub stderr: String,
}

/// opencode native `tool.execute.after` event envelope.
///
/// Captured from the deployed opencode plugin (`terraphim-hooks.js`), which
/// reads `input.tool`, `output.args.command`, `output.output`, and
/// `output.metadata.exitCode` / `output.metadata.exit_code`. This is the shape
/// opencode would emit if wired to forward its native event directly, rather
/// than the Claude-normalised payload the current plugin sends.
#[derive(Debug, Clone, Deserialize)]
struct OpencodeEvent {
    /// Tool name (e.g. "bash"); lower-case in opencode.
    tool: String,
    /// Tool arguments; `command` is present for the bash tool.
    #[serde(default)]
    args: OpencodeArgs,
    /// Combined tool output.
    #[serde(default)]
    output: Option<String>,
    /// Execution metadata carrying the exit code.
    #[serde(default)]
    metadata: OpencodeMetadata,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OpencodeArgs {
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct OpencodeMetadata {
    /// Exit code; opencode emits `exitCode`, some builds emit `exit_code`.
    #[serde(default, rename = "exitCode", alias = "exit_code")]
    exit_code: Option<i32>,
}

impl OpencodeEvent {
    /// Normalise an opencode native event into a [`HookInput`].
    ///
    /// The opencode `bash` tool maps to the Claude `"Bash"` tool name so the
    /// shared [`HookInput::should_capture`] logic applies unchanged. Output is
    /// placed in `stdout` to mirror the deployed plugin, which sends
    /// `{ stdout: rawOutput, stderr: "" }`. When the native event omits the
    /// exit code it defaults to `0` (non-capturing) rather than guessing.
    fn into_hook_input(self) -> HookInput {
        let tool_name = if self.tool.eq_ignore_ascii_case("bash") {
            "Bash".to_string()
        } else {
            self.tool
        };
        HookInput {
            tool_name,
            tool_input: ToolInput {
                command: self.args.command,
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: self.metadata.exit_code.unwrap_or(0),
                stdout: self.output.unwrap_or_default(),
                stderr: String::new(),
            },
        }
    }
}

// Cross-binary test API: consumed by `mod tests` and/or sibling `tests/*.rs` files; the bin build does not call it.
#[allow(dead_code)]
impl HookInput {
    /// Build a non-capturing input for an agent event that carries no
    /// per-command result (e.g. a Codex turn-level `notify` event).
    fn non_capturing(tool: &str) -> Self {
        HookInput {
            tool_name: tool.to_string(),
            tool_input: ToolInput {
                command: None,
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        }
    }

    /// Parse a hook-event payload using the given per-agent [`AgentFormat`].
    ///
    /// Normalises every supported envelope into a [`HookInput`]. Returns an
    /// error only when the payload is not valid JSON for the selected format;
    /// callers fail open on error.
    pub fn from_json_with_format(
        json: &str,
        format: AgentFormat,
    ) -> Result<Self, serde_json::Error> {
        match format {
            AgentFormat::Claude => serde_json::from_str(json),
            AgentFormat::Opencode => Self::from_opencode_json(json),
            AgentFormat::Codex => Self::from_codex_json(json),
            AgentFormat::Auto => Self::from_json_auto(json),
        }
    }

    /// Parse an opencode payload: the Claude-normalised shape its plugin sends
    /// today, falling back to opencode's native `tool.execute.after` envelope.
    fn from_opencode_json(json: &str) -> Result<Self, serde_json::Error> {
        if let Ok(claude) = serde_json::from_str::<HookInput>(json) {
            return Ok(claude);
        }
        let event: OpencodeEvent = serde_json::from_str(json)?;
        Ok(event.into_hook_input())
    }

    /// Parse a Codex payload: the Claude-shaped tool event its shell hook
    /// forwards. Codex turn-level `notify` events carry no per-command result,
    /// so any other (valid JSON) object normalises to a non-capturing input.
    fn from_codex_json(json: &str) -> Result<Self, serde_json::Error> {
        if let Ok(claude) = serde_json::from_str::<HookInput>(json) {
            return Ok(claude);
        }
        // Validate it is at least well-formed JSON, then drop it (non-capturing)
        // rather than fabricating a command from a turn-level notify event.
        let _: serde_json::Value = serde_json::from_str(json)?;
        Ok(Self::non_capturing("codex"))
    }

    /// Shape-sniff the payload across all supported envelopes.
    fn from_json_auto(json: &str) -> Result<Self, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(json)?;

        // Claude / Codex / opencode-normalised: canonical tool event.
        if value.get("tool_name").is_some() && value.get("tool_result").is_some() {
            return serde_json::from_str(json);
        }
        // opencode native: `tool` + (`args` | `output`), no `tool_name`.
        if value.get("tool").is_some()
            && (value.get("args").is_some() || value.get("output").is_some())
        {
            let event: OpencodeEvent = serde_json::from_str(json)?;
            return Ok(event.into_hook_input());
        }
        // Anything else (e.g. a Codex turn-level notify event) is non-capturing.
        Ok(Self::non_capturing("unknown"))
    }

    /// Parse hook input from a JSON string.
    ///
    /// # Arguments
    ///
    /// * `json` - The JSON string to parse
    ///
    /// # Returns
    ///
    /// Ok(HookInput) if parsing succeeds, Err(serde_json::Error) otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use terraphim_agent::learnings::HookInput;
    ///
    /// let json = r#"{
    ///     "tool_name": "Bash",
    ///     "tool_input": {"command": "git status"},
    ///     "tool_result": {"exit_code": 128, "stdout": "", "stderr": "fatal: not a git repository"}
    /// }"#;
    ///
    /// let input = HookInput::from_json(json).unwrap();
    /// assert_eq!(input.tool_name, "Bash");
    /// ```
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Check if this input should be captured as a learning.
    ///
    /// Returns true if:
    /// - The tool is "Bash" (command execution)
    /// - The exit code is non-zero (failure)
    ///
    /// # Returns
    ///
    /// true if the failed command should be captured, false otherwise.
    pub fn should_capture(&self) -> bool {
        self.tool_name == "Bash" && self.tool_result.exit_code != 0
    }

    /// Get combined error output (stdout + stderr).
    ///
    /// Combines stdout and stderr with a newline separator if both are present.
    /// Useful for capturing the full error context for learning.
    ///
    /// # Returns
    ///
    /// Combined output string.
    pub fn error_output(&self) -> String {
        let mut output = String::new();
        if !self.tool_result.stdout.is_empty() {
            output.push_str(&self.tool_result.stdout);
        }
        if !self.tool_result.stderr.is_empty() {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&self.tool_result.stderr);
        }
        output
    }

    /// Get the command string from tool input.
    ///
    /// # Returns
    ///
    /// Some(&str) if a command is present, None otherwise.
    pub fn command(&self) -> Option<&str> {
        self.tool_input.command.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_input_parse() {
        let json = r#"{
            "tool_name": "Bash",
            "tool_input": {"command": "git push -f"},
            "tool_result": {"exit_code": 1, "stdout": "", "stderr": "rejected"}
        }"#;

        let input = HookInput::from_json(json).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.command(), Some("git push -f"));
        assert_eq!(input.tool_result.exit_code, 1);
        assert_eq!(input.tool_result.stdout, "");
        assert_eq!(input.tool_result.stderr, "rejected");
    }

    #[test]
    fn test_should_capture_failed_bash() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("cmd".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert!(input.should_capture());
    }

    #[test]
    fn test_should_not_capture_success() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("cmd".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert!(!input.should_capture());
    }

    #[test]
    fn test_should_not_capture_edit() {
        let input = HookInput {
            tool_name: "Edit".to_string(),
            tool_input: ToolInput {
                command: None,
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert!(!input.should_capture());
    }

    #[test]
    fn test_error_output_combining() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("cmd".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: "output line 1".to_string(),
                stderr: "error line 1".to_string(),
            },
        };
        assert_eq!(input.error_output(), "output line 1\nerror line 1");
    }

    #[test]
    fn test_command_extraction() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("git push origin main".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert_eq!(input.command(), Some("git push origin main"));
    }

    #[test]
    fn test_command_extraction_none() {
        let input = HookInput {
            tool_name: "Edit".to_string(),
            tool_input: ToolInput {
                command: None,
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert_eq!(input.command(), None);
    }

    #[test]
    fn test_parse_with_extra_fields() {
        let json = r#"{
            "tool_name": "Write",
            "tool_input": {
                "path": "/tmp/test.txt",
                "content": "hello world"
            },
            "tool_result": {"exit_code": 0, "stdout": "", "stderr": ""}
        }"#;

        let input = HookInput::from_json(json).unwrap();
        assert_eq!(input.tool_name, "Write");
        assert!(input.tool_input.command.is_none());
        assert!(input.tool_input.extra.contains_key("path"));
        assert!(input.tool_input.extra.contains_key("content"));
    }

    #[test]
    fn test_error_output_stdout_only() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("cmd".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: "some output".to_string(),
                stderr: String::new(),
            },
        };
        assert_eq!(input.error_output(), "some output");
    }

    #[test]
    fn test_error_output_stderr_only() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("cmd".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "some error".to_string(),
            },
        };
        assert_eq!(input.error_output(), "some error");
    }

    #[test]
    fn test_error_output_empty() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("cmd".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: String::new(),
            },
        };
        assert_eq!(input.error_output(), "");
    }

    #[test]
    fn test_parse_invalid_json() {
        let json = "not valid json";
        let result = HookInput::from_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_not_capture_bash_with_exit_zero() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("echo hello".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: "hello".to_string(),
                stderr: String::new(),
            },
        };
        assert!(!input.should_capture());
    }

    #[test]
    fn test_should_capture_bash_with_negative_exit_code() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("kill -9 $$".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: -1,
                stdout: String::new(),
                stderr: "Killed".to_string(),
            },
        };
        assert!(input.should_capture());
    }

    #[test]
    fn test_should_not_capture_non_bash_even_if_failed() {
        let input = HookInput {
            tool_name: "Write".to_string(),
            tool_input: ToolInput {
                command: None,
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "Permission denied".to_string(),
            },
        };
        assert!(!input.should_capture());
    }

    #[test]
    fn test_capture_from_hook_success() {
        let input = HookInput {
            tool_name: "Bash".to_string(),
            tool_input: ToolInput {
                command: Some("git push".to_string()),
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 1,
                stdout: String::new(),
                stderr: "rejected".to_string(),
            },
        };

        // Should succeed and return a path
        let result = capture_from_hook(&input);
        // Note: This may fail if global dir is not writable, so we check it's not Ignored
        // for having no command
        if let Err(LearningError::Ignored(msg)) = &result {
            assert_ne!(msg, "No command in input");
        }
    }

    #[test]
    fn test_capture_from_hook_no_command() {
        let input = HookInput {
            tool_name: "Edit".to_string(),
            tool_input: ToolInput {
                command: None,
                extra: HashMap::new(),
            },
            tool_result: ToolResult {
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        };

        let result = capture_from_hook(&input);
        assert!(result.is_err());
        match result.unwrap_err() {
            LearningError::Ignored(msg) => assert_eq!(msg, "No command in input"),
            _ => panic!("Expected Ignored error"),
        }
    }

    /// Verify secrets are stripped from the full process_hook_with_streams pipeline.
    ///
    /// This is the end-to-end test for AC#3: secrets present in hook input must
    /// not appear in the output written to stdout.
    ///
    /// Uses `&[u8]` (impl AsyncRead) as stdin and `Vec<u8>` (impl AsyncWrite) as stdout
    /// so the full I/O path is exercised without spawning a subprocess.
    #[tokio::test]
    async fn test_process_hook_with_streams_strips_secrets_from_output() {
        use super::process_hook_with_streams;

        // Build a fake AWS key at runtime to avoid tripping the pre-commit secret scanner.
        let aws_key = format!("AKIA{}", "IOSFODNN7EXAMPLE");

        let json = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"aws s3 ls"}},"tool_result":{{"exit_code":1,"stdout":"","stderr":"Unable to locate credentials {aws_key}"}}}}"#,
        );

        // &[u8] implements AsyncRead; Vec<u8> implements AsyncWrite.
        let mut output_buf: Vec<u8> = Vec::new();
        process_hook_with_streams(
            LearnHookType::PostToolUse,
            AgentFormat::Auto,
            json.as_bytes(),
            &mut output_buf,
        )
        .await
        .expect("process_hook_with_streams must not fail");

        let output = String::from_utf8(output_buf).expect("output must be valid UTF-8");

        // The secret must not appear in the output written to stdout.
        assert!(
            !output.contains(&aws_key),
            "AWS key must not appear in stdout output; got: {output}"
        );
        assert!(
            output.contains("[AWS_KEY_REDACTED]"),
            "Redacted placeholder must appear in output; got: {output}"
        );
    }

    /// Verify clean input passes through unchanged (no spurious redaction).
    #[tokio::test]
    async fn test_process_hook_with_streams_clean_input_unchanged() {
        use super::process_hook_with_streams;

        let json = r#"{"tool_name":"Bash","tool_input":{"command":"cargo build"},"tool_result":{"exit_code":0,"stdout":"Compiling","stderr":""}}"#;

        let mut output_buf: Vec<u8> = Vec::new();
        process_hook_with_streams(
            LearnHookType::PostToolUse,
            AgentFormat::Auto,
            json.as_bytes(),
            &mut output_buf,
        )
        .await
        .expect("process_hook_with_streams must not fail");

        let output = String::from_utf8(output_buf).expect("output must be valid UTF-8");
        assert_eq!(output, json, "Clean input must pass through unchanged");
    }

    /// Verify pre-tool-use hook also redacts secrets (not just post-tool-use).
    #[tokio::test]
    async fn test_process_hook_with_streams_pre_tool_use_also_redacts() {
        use super::process_hook_with_streams;

        let aws_key = format!("AKIA{}", "IOSFODNN7EXAMPLE");
        let json = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"export AWS_ACCESS_KEY_ID={aws_key}"}},"tool_result":{{"exit_code":0,"stdout":"","stderr":""}}}}"#,
        );

        let mut output_buf: Vec<u8> = Vec::new();
        process_hook_with_streams(
            LearnHookType::PreToolUse,
            AgentFormat::Auto,
            json.as_bytes(),
            &mut output_buf,
        )
        .await
        .expect("process_hook_with_streams must not fail");

        let output = String::from_utf8(output_buf).expect("output must be valid UTF-8");
        assert!(
            !output.contains(&aws_key),
            "AWS key must not appear in pre-tool-use stdout output; got: {output}"
        );
    }

    #[test]
    fn test_hook_passthrough_redacts_aws_key_in_error() {
        use crate::learnings::redact_secrets;

        // Build a fake AWS key at runtime to avoid tripping the pre-commit secret scanner.
        // The key prefix "AKIA" followed by 16 uppercase alphanumeric chars is the pattern.
        let aws_key = format!("AKIA{}", "IOSFODNN7EXAMPLE");

        let json = format!(
            r#"{{
            "tool_name": "Bash",
            "tool_input": {{"command": "aws s3 ls"}},
            "tool_result": {{
                "exit_code": 1,
                "stdout": "",
                "stderr": "Unable to locate credentials {}"
            }}
        }}"#,
            aws_key
        );

        // Verify redaction removes the AWS key
        let redacted = redact_secrets(&json);
        assert!(!redacted.contains(&aws_key));
        assert!(redacted.contains("[AWS_KEY_REDACTED]"));

        // Verify the redacted output is still valid JSON
        let parsed = HookInput::from_json(&redacted).unwrap();
        assert_eq!(parsed.tool_name, "Bash");
        assert_eq!(parsed.tool_result.exit_code, 1);
        assert!(parsed.tool_result.stderr.contains("[AWS_KEY_REDACTED]"));
    }

    #[test]
    fn test_learn_hook_type_variants() {
        assert_ne!(LearnHookType::PreToolUse, LearnHookType::PostToolUse);
        assert_ne!(LearnHookType::PostToolUse, LearnHookType::UserPromptSubmit);
        assert_ne!(LearnHookType::PreToolUse, LearnHookType::UserPromptSubmit);
    }

    #[test]
    fn test_parse_correction_pattern_use_instead_of() {
        let result = parse_correction_pattern("use bun instead of npm");
        assert_eq!(result, Some(("npm".to_string(), "bun".to_string())));
    }

    #[test]
    fn test_parse_correction_pattern_prefer_over() {
        let result = parse_correction_pattern("prefer cargo over make");
        assert_eq!(result, Some(("make".to_string(), "cargo".to_string())));
    }

    #[test]
    fn test_parse_correction_pattern_with_trailing_period() {
        let result = parse_correction_pattern("use Result<T> instead of unwrap().");
        assert_eq!(
            result,
            Some(("unwrap()".to_string(), "Result<T>".to_string()))
        );
    }

    #[test]
    fn test_parse_correction_pattern_use_not() {
        let result = parse_correction_pattern("use uv not pip");
        assert_eq!(result, Some(("pip".to_string(), "uv".to_string())));
    }

    #[test]
    fn test_parse_correction_pattern_no_match() {
        assert!(parse_correction_pattern("hello world").is_none());
        assert!(parse_correction_pattern("this is fine").is_none());
        // "I prefer tea over coffee" is a preference, not a tool correction
        assert!(parse_correction_pattern("I prefer tea over coffee").is_none());
    }

    #[test]
    fn test_pre_tool_use_no_crash_on_non_bash() {
        // Non-Bash tool should not crash (fail-open)
        let json = r#"{
            "tool_name": "Edit",
            "tool_input": {"path": "/tmp/test.txt"},
            "tool_result": {"exit_code": 0, "stdout": "", "stderr": ""}
        }"#;
        process_pre_tool_use(json, AgentFormat::Auto);
        // No panic = pass
    }

    #[test]
    fn test_pre_tool_use_no_crash_on_invalid_json() {
        // Invalid JSON should not crash (fail-open)
        process_pre_tool_use("not valid json", AgentFormat::Auto);
        // No panic = pass
    }

    #[test]
    fn test_user_prompt_submit_no_crash_on_empty() {
        process_user_prompt_submit("{}");
        // No panic = pass
    }

    #[test]
    fn test_user_prompt_submit_no_crash_on_invalid_json() {
        process_user_prompt_submit("invalid");
        // No panic = pass
    }

    // --- Per-agent format parsing (issue #2) ---------------------------------
    //
    // Fixtures are real captured payloads (see test-fixtures/hooks/README.md),
    // not fabricated mocks.

    const CLAUDE_FIXTURE: &str =
        include_str!("../../test-fixtures/hooks/claude_post_tool_use.json");
    const OPENCODE_NATIVE_FIXTURE: &str =
        include_str!("../../test-fixtures/hooks/opencode_native_tool_execute_after.json");
    const OPENCODE_NORMALISED_FIXTURE: &str =
        include_str!("../../test-fixtures/hooks/opencode_normalised.json");
    const CODEX_NOTIFY_FIXTURE: &str =
        include_str!("../../test-fixtures/hooks/codex_notify_turn_complete.json");

    #[test]
    fn test_agent_format_default_is_auto() {
        assert_eq!(AgentFormat::default(), AgentFormat::Auto);
    }

    #[test]
    fn test_claude_format_parses_canonical_event() {
        let input = HookInput::from_json_with_format(CLAUDE_FIXTURE, AgentFormat::Claude).unwrap();
        assert_eq!(input.tool_name, "Bash");
        assert_eq!(input.command(), Some("git push -f origin main"));
        assert_eq!(input.tool_result.exit_code, 1);
        assert!(input.should_capture());
    }

    #[test]
    fn test_opencode_native_event_normalises_and_captures() {
        // opencode's native tool.execute.after envelope: {tool, args.command,
        // output, metadata.exitCode}.
        let input =
            HookInput::from_json_with_format(OPENCODE_NATIVE_FIXTURE, AgentFormat::Opencode)
                .unwrap();
        assert_eq!(input.tool_name, "Bash"); // "bash" -> "Bash" so should_capture applies
        assert_eq!(input.command(), Some("cargo buidl --workspace"));
        assert_eq!(input.tool_result.exit_code, 101);
        assert!(input.tool_result.stdout.contains("no such command"));
        assert!(input.should_capture());
    }

    #[test]
    fn test_opencode_native_exit_code_snake_case_alias() {
        let json =
            r#"{"tool":"bash","args":{"command":"false"},"output":"","metadata":{"exit_code":1}}"#;
        let input = HookInput::from_json_with_format(json, AgentFormat::Opencode).unwrap();
        assert_eq!(input.tool_result.exit_code, 1);
        assert!(input.should_capture());
    }

    #[test]
    fn test_opencode_native_missing_exit_code_defaults_non_capturing() {
        // Without metadata we do not guess an exit code; default 0 => no capture.
        let json = r#"{"tool":"bash","args":{"command":"ls"},"output":"a\nb"}"#;
        let input = HookInput::from_json_with_format(json, AgentFormat::Opencode).unwrap();
        assert_eq!(input.tool_result.exit_code, 0);
        assert!(!input.should_capture());
    }

    #[test]
    fn test_opencode_accepts_claude_normalised_payload() {
        // The deployed opencode plugin normalises to the Claude shape before
        // invoking the CLI with --format opencode.
        let input =
            HookInput::from_json_with_format(OPENCODE_NORMALISED_FIXTURE, AgentFormat::Opencode)
                .unwrap();
        assert_eq!(input.command(), Some("cargo buidl --workspace"));
        assert_eq!(input.tool_result.exit_code, 101);
        assert!(input.should_capture());
    }

    #[test]
    fn test_codex_claude_shaped_event_captures() {
        // Codex's shell hook forwards Claude-shaped tool events.
        let input = HookInput::from_json_with_format(CLAUDE_FIXTURE, AgentFormat::Codex).unwrap();
        assert_eq!(input.command(), Some("git push -f origin main"));
        assert!(input.should_capture());
    }

    #[test]
    fn test_codex_notify_turn_event_is_non_capturing() {
        // Turn-level notify events carry no per-command result.
        let input =
            HookInput::from_json_with_format(CODEX_NOTIFY_FIXTURE, AgentFormat::Codex).unwrap();
        assert_eq!(input.command(), None);
        assert!(!input.should_capture());
    }

    #[test]
    fn test_codex_format_rejects_invalid_json() {
        assert!(HookInput::from_json_with_format("not json", AgentFormat::Codex).is_err());
    }

    #[test]
    fn test_auto_detects_claude_shape() {
        let input = HookInput::from_json_with_format(CLAUDE_FIXTURE, AgentFormat::Auto).unwrap();
        assert!(input.should_capture());
    }

    #[test]
    fn test_auto_detects_opencode_native_shape() {
        let input =
            HookInput::from_json_with_format(OPENCODE_NATIVE_FIXTURE, AgentFormat::Auto).unwrap();
        assert_eq!(input.command(), Some("cargo buidl --workspace"));
        assert_eq!(input.tool_result.exit_code, 101);
        assert!(input.should_capture());
    }

    #[test]
    fn test_auto_treats_unknown_object_as_non_capturing() {
        let input =
            HookInput::from_json_with_format(CODEX_NOTIFY_FIXTURE, AgentFormat::Auto).unwrap();
        assert!(!input.should_capture());
    }

    #[test]
    fn test_auto_rejects_invalid_json() {
        assert!(HookInput::from_json_with_format("not json", AgentFormat::Auto).is_err());
    }

    #[test]
    fn test_opencode_non_bash_tool_not_captured() {
        let json =
            r#"{"tool":"edit","args":{"path":"/tmp/x"},"output":"","metadata":{"exitCode":0}}"#;
        let input = HookInput::from_json_with_format(json, AgentFormat::Opencode).unwrap();
        assert_eq!(input.tool_name, "edit");
        assert!(!input.should_capture());
    }

    /// GitHub PAT bypass: `ghp_` tokens are not in `contains_secrets()` patterns
    /// but ARE matched by `redact_secrets()`. Unconditional redaction must catch them.
    #[tokio::test]
    async fn test_process_hook_github_pat_is_redacted() {
        use super::process_hook_with_streams;

        // Build token at runtime to avoid the pre-commit secret scanner.
        let pat = format!("ghp_{}", "A".repeat(36));
        let json = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"git push"}},"tool_result":{{"exit_code":1,"stdout":"","stderr":"remote: invalid credentials {pat}"}}}}"#,
        );

        let mut output_buf: Vec<u8> = Vec::new();
        process_hook_with_streams(
            LearnHookType::PostToolUse,
            AgentFormat::Auto,
            json.as_bytes(),
            &mut output_buf,
        )
        .await
        .expect("process_hook_with_streams must not fail");

        let output = String::from_utf8(output_buf).expect("output must be valid UTF-8");
        assert!(
            !output.contains(&pat),
            "GitHub PAT must not appear in stdout output; got: {output}"
        );
        assert!(
            output.contains("[GITHUB_TOKEN_REDACTED]"),
            "Redacted placeholder must appear in output; got: {output}"
        );
    }

    /// Slack token bypass: `xoxb-` tokens are not in `contains_secrets()` patterns
    /// but ARE matched by `redact_secrets()`. Unconditional redaction must catch them.
    #[tokio::test]
    async fn test_process_hook_slack_token_is_redacted() {
        use super::process_hook_with_streams;

        // Construct at runtime so push-protection scanners do not flag a literal token.
        let slack_token = format!(
            "xoxb-{}-{}-{}",
            "FAKE_TEST_ID_A", "FAKE_TEST_ID_B", "FAKE_TEST_SECRET"
        );
        let json = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"curl -H 'Authorization: Bearer {slack_token}' https://slack.com/api/chat.postMessage"}},"tool_result":{{"exit_code":0,"stdout":"","stderr":""}}}}"#,
        );

        let mut output_buf: Vec<u8> = Vec::new();
        process_hook_with_streams(
            LearnHookType::PostToolUse,
            AgentFormat::Auto,
            json.as_bytes(),
            &mut output_buf,
        )
        .await
        .expect("process_hook_with_streams must not fail");

        let output = String::from_utf8(output_buf).expect("output must be valid UTF-8");
        assert!(
            !output.contains(&slack_token),
            "Slack token must not appear in stdout output; got: {output}"
        );
        assert!(
            output.contains("[SLACK_TOKEN_REDACTED]"),
            "Redacted placeholder must appear in output; got: {output}"
        );
    }

    /// Connection string bypass: `postgresql://user:pass@host` is not in
    /// `contains_secrets()` patterns but IS matched by `redact_secrets()`.
    /// Unconditional redaction must catch it.
    #[tokio::test]
    async fn test_process_hook_connection_string_is_redacted() {
        use super::process_hook_with_streams;

        let conn = "postgresql://dbuser:s3cr3tpassword@prod-db.internal:5432/appdb";
        let json = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"psql {conn}"}},"tool_result":{{"exit_code":1,"stdout":"","stderr":"connection refused"}}}}"#,
        );

        let mut output_buf: Vec<u8> = Vec::new();
        process_hook_with_streams(
            LearnHookType::PostToolUse,
            AgentFormat::Auto,
            json.as_bytes(),
            &mut output_buf,
        )
        .await
        .expect("process_hook_with_streams must not fail");

        let output = String::from_utf8(output_buf).expect("output must be valid UTF-8");
        assert!(
            !output.contains("s3cr3tpassword"),
            "Connection string password must not appear in stdout output; got: {output}"
        );
        assert!(
            output.contains("postgresql://[REDACTED]@"),
            "Redacted connection string must appear in output; got: {output}"
        );
    }
}
