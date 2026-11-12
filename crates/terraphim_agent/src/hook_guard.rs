//! Asking the guard for a decision, out of process, and failing closed.
//!
//! The PreToolUse hook is the last line of defence: it decides whether a tool
//! call is allowed. The guard that helps it decide must therefore not be able
//! to take the hook down with it - a crash, an abort (a stack overflow is an
//! abort, not a panic, so `catch_unwind` cannot see it), an OOM kill or a
//! signal inside the guard must not turn into an *allow*.
//!
//! So the guard runs as a child process, and the hook reads its decision from
//! the child's output. When the child produced no parseable decision - it
//! crashed, it was killed, it printed nothing, it printed something that is
//! not a decision - the call is **blocked**. Failing open here would turn
//! every internal failure of the guard into a bypass of the guard itself.
//!
//! Refs terraphim-clients#465.

use std::path::Path;
use std::process::Stdio;

use serde::Deserialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// What the guard decided about a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardOutcome {
    /// The command may run.
    Allow,
    /// The guard wants the command sandboxed. The hook allows it, as it did
    /// before the guard moved out of process.
    Sandbox { reason: String },
    /// The command must not run.
    Block { reason: String },
    /// The guard produced no usable decision, so the command must not run.
    Failed { message: String },
}

impl std::fmt::Display for GuardOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GuardOutcome::Allow => write!(f, "allow"),
            GuardOutcome::Sandbox { reason } => write!(f, "sandbox: {reason}"),
            GuardOutcome::Block { reason } => write!(f, "block: {reason}"),
            GuardOutcome::Failed { message } => write!(f, "failed: {message}"),
        }
    }
}

impl GuardOutcome {
    /// Whether the hook has to block the call.
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            GuardOutcome::Block { .. } | GuardOutcome::Failed { .. }
        )
    }
}

/// The guard's answer, as the `guard --json` command writes it.
#[derive(Debug, Deserialize)]
struct GuardAnswer {
    decision: String,
    #[serde(default)]
    reason: Option<String>,
}

/// The message used when the guard did not decide.
fn failure_message(status: Option<i32>, detail: &str) -> String {
    match status {
        Some(code) => format!("{detail} (exit {code})"),
        None => format!("{detail} (the guard was killed)"),
    }
}

/// Interpret the guard's output.
///
/// An empty output, an output that is not JSON, and JSON without a recognised
/// decision all mean the same thing: the guard did not decide. The caller
/// must block the call rather than read it as an allow.
pub fn parse_guard_output(stdout: &str, status: Option<i32>) -> GuardOutcome {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return GuardOutcome::Failed {
            message: failure_message(status, "the guard produced no output"),
        };
    }

    let answer: GuardAnswer = match serde_json::from_str(trimmed) {
        Ok(answer) => answer,
        Err(error) => {
            return GuardOutcome::Failed {
                message: failure_message(
                    status,
                    &format!("the guard output is not a decision ({error})"),
                ),
            };
        }
    };

    match answer.decision.as_str() {
        "allow" => GuardOutcome::Allow,
        "sandbox" => GuardOutcome::Sandbox {
            reason: answer
                .reason
                .unwrap_or_else(|| "the guard asked for a sandbox".to_string()),
        },
        "block" => GuardOutcome::Block {
            reason: answer
                .reason
                .unwrap_or_else(|| "blocked by the guard".to_string()),
        },
        other => GuardOutcome::Failed {
            message: failure_message(
                status,
                &format!("the guard reported an unrecognised decision '{other}'"),
            ),
        },
    }
}

/// Run the guard as a child process and interpret its answer.
///
/// `program` is the agent binary; the guard runs as `<program> guard --json`
/// with the command on its standard input. The child is given no standard
/// error of its own: a failing guard must not write into the hook's output.
pub async fn run_guard(program: &Path, command: &str) -> GuardOutcome {
    let mut child = match Command::new(program)
        .arg("guard")
        .arg("--json")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return GuardOutcome::Failed {
                message: format!("the guard could not be started ({error})"),
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(command.as_bytes()).await {
            return GuardOutcome::Failed {
                message: format!("the guard could not be given the command ({error})"),
            };
        }
        // Dropping the handle closes the pipe, which is how the guard learns
        // that the command is complete.
        drop(stdin);
    }

    let output = match child.wait_with_output().await {
        Ok(output) => output,
        Err(error) => {
            return GuardOutcome::Failed {
                message: format!("the guard could not be waited for ({error})"),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_guard_output(&stdout, output.status.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_allow_decision_is_read() {
        let outcome = parse_guard_output(r#"{"decision":"allow"}"#, Some(0));
        assert_eq!(outcome, GuardOutcome::Allow);
    }

    #[test]
    fn a_block_decision_carries_its_reason() {
        let outcome = parse_guard_output(
            r#"{"decision":"block","reason":"rm -rf is destructive"}"#,
            Some(0),
        );
        assert_eq!(
            outcome,
            GuardOutcome::Block {
                reason: "rm -rf is destructive".to_string()
            }
        );
    }

    #[test]
    fn no_output_is_a_failure_not_an_allow() {
        let outcome = parse_guard_output("", Some(101));
        assert!(
            outcome.is_blocking(),
            "an empty answer must block, got {outcome:?}"
        );
        assert!(
            outcome.to_string().contains("101"),
            "the message must name the exit status: {outcome:?}"
        );
    }

    #[test]
    fn output_that_is_not_a_decision_is_a_failure() {
        let outcome = parse_guard_output("Segmentation fault", Some(139));
        assert!(outcome.is_blocking(), "got {outcome:?}");
    }

    #[test]
    fn an_unrecognised_decision_is_a_failure() {
        let outcome = parse_guard_output(r#"{"decision":"maybe"}"#, Some(0));
        assert!(outcome.is_blocking(), "got {outcome:?}");
    }

    #[test]
    fn a_killed_guard_is_a_failure() {
        let outcome = parse_guard_output("", None);
        assert!(outcome.is_blocking(), "got {outcome:?}");
    }
}
