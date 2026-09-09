//! Robot mode dispatch and forgiving CLI parsing.
//!
//! Originally part of the monolithic `main.rs`; moved here as step 3 of the
//! de-monolithization tracked in terraphim/terraphim-clients#211 (steps 1
//! and 2 extracted `cli_helpers` and `cli_schema` respectively).
//!
//! This module owns the robot/error dispatch surface:
//! - `emit_robot_error_and_exit` / `classify_error` for F1.2 exit-code mapping
//! - `build_cli_forgiving_parser` / `apply_forgiving_parsing` for typo-tolerant
//!   subcommand expansion
//! - `format_robot_output` / `handle_robot_command` for the
//!   `terraphim-agent robot {capabilities,schemas,examples}` self-documentation
//!   commands
//!
//! `RobotFormat` and `OutputFormat` come from `crate` because they are still
//! defined in `main.rs`/`cli_schema.rs` and are reused by the offline/server
//! dispatch paths.

use anyhow::Result;
use serde::Serialize;
use terraphim_agent::{forgiving, robot};

use crate::cli_schema::{OutputFormat, RobotSub};

/// Emit a robot-mode JSON error envelope (when the user opted into robot
/// output) and exit with the given exit code. The stderr message is always
/// printed for humans.
pub(crate) fn emit_robot_error_and_exit(
    err: &anyhow::Error,
    code: robot::exit_codes::ExitCode,
    robot: bool,
    format: &OutputFormat,
) -> ! {
    if robot || !matches!(format, OutputFormat::Human) {
        use robot::schema::{ResponseMeta, RobotError, RobotResponse};
        let meta = ResponseMeta::new("unknown");
        let robot_error = RobotError::new(format!("E{:03}", code.code()), format!("{:#}", err));
        let response = RobotResponse::<()>::error(vec![robot_error], meta);
        if let Ok(json) = serde_json::to_string(&response) {
            println!("{}", json);
        }
    }
    eprintln!("Error: {:#}", err);
    std::process::exit(code.code().into())
}

/// Map an `anyhow::Error` to the F1.2 exit-code contract.
///
/// Prefers typed downcasts (`tokio::time::error::Elapsed`, `reqwest::Error`)
/// and falls back to substring heuristics on the lowercased message. The
/// heuristics are pinned by `classify_error_tests` below; keep both in sync.
pub(crate) fn classify_error(err: &anyhow::Error) -> robot::exit_codes::ExitCode {
    use robot::exit_codes::ExitCode;

    if err.chain().any(|e| e.is::<tokio::time::error::Elapsed>()) {
        return ExitCode::ErrorTimeout;
    }

    #[cfg(feature = "server")]
    if err.chain().any(|e| e.is::<reqwest::Error>()) {
        let is_timeout = err
            .chain()
            .filter_map(|e| e.downcast_ref::<reqwest::Error>())
            .any(|re| re.is_timeout());
        if is_timeout {
            return ExitCode::ErrorTimeout;
        }
        return ExitCode::ErrorNetwork;
    }

    let msg = err.to_string().to_lowercase();

    if msg.contains("timed out") || msg.contains("timeout") || msg.contains("elapsed") {
        ExitCode::ErrorTimeout
    } else if msg.contains("connection refused")
        || msg.contains("connection reset")
        || msg.contains("network")
        || msg.contains("dns")
        || msg.contains("transport")
        || msg.contains("connect error")
    {
        ExitCode::ErrorNetwork
    } else if msg.contains("unauthori")
        || msg.contains("unauthenticated")
        || msg.contains("forbidden")
        || msg.contains("authentication required")
        || msg.contains("authentication failed")
        || msg.contains(" 401 ")
        || msg.contains(" 403 ")
        || msg.ends_with(" 401")
        || msg.ends_with(" 403")
        || msg.contains("http 401")
        || msg.contains("http 403")
    {
        ExitCode::ErrorAuth
    } else if msg.contains("index not found")
        || msg.contains("index missing")
        || msg.contains("not initialised")
        || msg.contains("not initialized")
        || (msg.contains("not found") && msg.contains("index"))
        || msg.contains("knowledge graph not configured")
        || msg.contains("no local knowledge graph")
        || (msg.contains("thesaurus")
            && (msg.contains("not found") || msg.contains("failed to load")))
    {
        ExitCode::ErrorIndexMissing
    } else {
        ExitCode::ErrorGeneral
    }
}

#[cfg(test)]
mod classify_error_tests {
    use super::*;
    use robot::exit_codes::ExitCode;

    fn err(msg: &str) -> anyhow::Error {
        anyhow::anyhow!("{}", msg)
    }

    #[test]
    fn general_error_maps_to_1() {
        assert_eq!(
            classify_error(&err("something unexpected happened")),
            ExitCode::ErrorGeneral
        );
    }

    #[test]
    fn index_missing_patterns_map_to_3() {
        assert_eq!(
            classify_error(&err("index not found on disk")),
            ExitCode::ErrorIndexMissing
        );
        assert_eq!(
            classify_error(&err("index missing")),
            ExitCode::ErrorIndexMissing
        );
        assert_eq!(
            classify_error(&err("automata index not initialised")),
            ExitCode::ErrorIndexMissing
        );
        assert_eq!(
            classify_error(&err("Config error: knowledge graph not configured")),
            ExitCode::ErrorIndexMissing
        );
        assert_eq!(
            classify_error(&err("no local knowledge graph path available")),
            ExitCode::ErrorIndexMissing
        );
        assert_eq!(
            classify_error(&err("thesaurus not found at path")),
            ExitCode::ErrorIndexMissing
        );
    }

    #[test]
    fn auth_patterns_map_to_5() {
        assert_eq!(
            classify_error(&err("authentication required")),
            ExitCode::ErrorAuth
        );
        assert_eq!(
            classify_error(&err("request forbidden: 403")),
            ExitCode::ErrorAuth
        );
        assert_eq!(
            classify_error(&err("401 Unauthorised")),
            ExitCode::ErrorAuth
        );
        assert_eq!(
            classify_error(&err("server returned 403 Forbidden")),
            ExitCode::ErrorAuth
        );
    }

    #[test]
    fn non_auth_strings_do_not_map_to_5() {
        assert_ne!(
            classify_error(&err("author field missing")),
            ExitCode::ErrorAuth
        );
        assert_ne!(
            classify_error(&err("authority header")),
            ExitCode::ErrorAuth
        );
        assert_ne!(
            classify_error(&err("failed to open auth_tokens.json")),
            ExitCode::ErrorAuth
        );
        assert_ne!(
            classify_error(&err("error code 4010 unknown")),
            ExitCode::ErrorAuth
        );
    }

    #[test]
    fn timeout_patterns_map_to_7() {
        assert_eq!(
            classify_error(&err("operation timed out")),
            ExitCode::ErrorTimeout
        );
        assert_eq!(
            classify_error(&err("deadline elapsed waiting for response")),
            ExitCode::ErrorTimeout
        );
        assert_eq!(
            classify_error(&err("request timeout after 30s")),
            ExitCode::ErrorTimeout
        );
    }

    #[test]
    fn network_patterns_map_to_6() {
        assert_eq!(
            classify_error(&err("connection refused on port 8080")),
            ExitCode::ErrorNetwork
        );
        assert_eq!(
            classify_error(&err("dns resolution failed")),
            ExitCode::ErrorNetwork
        );
        assert_eq!(
            classify_error(&err("network error connecting to host")),
            ExitCode::ErrorNetwork
        );
    }
}

/// Build a ForgivingParser with the actual CLI subcommands.
fn build_cli_forgiving_parser() -> forgiving::ForgivingParser {
    let mut commands = vec![
        "search",
        "roles",
        "config",
        "graph",
        "extract",
        "replace",
        "validate",
        "suggest",
        "hook",
        "guard",
        "interactive",
        "setup",
        "check-update",
        "update",
        "learn",
        "listen",
        "cache",
    ];

    #[cfg(feature = "llm")]
    commands.push("chat");

    #[cfg(feature = "repl")]
    commands.push("repl");

    #[cfg(feature = "repl-sessions")]
    commands.push("sessions");

    let parser = forgiving::ForgivingParser::new(commands.into_iter().map(String::from).collect());

    let mut aliases = forgiving::AliasRegistry::empty();
    aliases.add("q", "search");
    aliases.add("s", "search");
    aliases.add("query", "search");
    aliases.add("find", "search");
    aliases.add("r", "roles");
    aliases.add("role", "roles");
    aliases.add("c", "config");
    aliases.add("cfg", "config");
    aliases.add("g", "graph");
    aliases.add("kg", "graph");
    aliases.add("i", "interactive");

    parser.with_aliases(aliases)
}

/// Apply forgiving parsing to CLI arguments.
///
/// Intercepts the subcommand argument before clap sees it, applying:
/// - Alias expansion (e.g. `q` -> `search`)
/// - Auto-correction (e.g. `serach` -> `search`)
/// - Case-insensitive matching (e.g. `SEARCH` -> `search`)
///
/// Prints correction notifications to stderr.
pub(crate) fn apply_forgiving_parsing(args: &[String]) -> Vec<String> {
    if args.len() < 2 {
        return args.to_vec();
    }

    let mut subcommand_idx = None;
    let mut skip_next = false;

    for (i, arg) in args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }

        if arg.starts_with('-') {
            match arg.as_str() {
                "--server-url" | "--format" | "--config" => {
                    skip_next = true;
                }
                _ => {}
            }
            continue;
        }

        subcommand_idx = Some(i);
        break;
    }

    let idx = match subcommand_idx {
        Some(i) => i,
        None => return args.to_vec(),
    };

    let input = &args[idx];
    let parser = build_cli_forgiving_parser();
    let result = parser.parse(input);

    let corrected_cmd = match &result {
        forgiving::ParseResult::AliasExpanded {
            command, original, ..
        } => {
            if command != original {
                eprintln!("Note: '{}' expanded to '{}'", original, command);
            }
            Some(command.clone())
        }
        forgiving::ParseResult::AutoCorrected {
            command, original, ..
        } => {
            eprintln!("Note: '{}' auto-corrected to '{}'", original, command);
            Some(command.clone())
        }
        forgiving::ParseResult::Exact {
            command, original, ..
        } => {
            if command != original {
                Some(command.clone())
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(cmd) = corrected_cmd {
        let mut corrected = args.to_vec();
        corrected[idx] = cmd;
        corrected
    } else {
        args.to_vec()
    }
}

/// Format a value using robot mode output formatting.
pub(crate) fn format_robot_output<T: Serialize>(
    value: &T,
    format: crate::RobotFormat,
) -> Result<String> {
    let robot_format = match format {
        crate::RobotFormat::Json => robot::output::OutputFormat::Json,
        crate::RobotFormat::Table => robot::output::OutputFormat::Table,
        crate::RobotFormat::Minimal => robot::output::OutputFormat::Minimal,
    };
    let config = robot::output::RobotConfig::new().with_format(robot_format);
    let formatter = robot::output::RobotFormatter::new(config);
    formatter
        .format(value)
        .map_err(|e| anyhow::anyhow!("Failed to format output: {}", e))
}

/// Handle robot mode self-documentation commands.
pub(crate) fn handle_robot_command(sub: RobotSub) -> Result<()> {
    let docs = robot::SelfDocumentation::new();

    match sub {
        RobotSub::Capabilities { format } => {
            let caps = docs.capabilities_data();
            let output = format_robot_output(&caps, format)?;
            println!("{}", output);
        }
        RobotSub::Schemas { command, format } => {
            if let Some(cmd) = command {
                if let Some(schema) = docs.schema(&cmd) {
                    let output = format_robot_output(&schema, format)?;
                    println!("{}", output);
                } else {
                    return Err(anyhow::anyhow!("Unknown command: {}", cmd));
                }
            } else {
                let schemas = docs.all_schemas();
                let output = format_robot_output(&schemas, format)?;
                println!("{}", output);
            }
        }
        RobotSub::Examples { command, format } => {
            if let Some(cmd) = command {
                if let Some(examples) = docs.examples(&cmd) {
                    let output = format_robot_output(&examples, format)?;
                    println!("{}", output);
                } else {
                    return Err(anyhow::anyhow!("Unknown command: {}", cmd));
                }
            } else {
                let all_examples: Vec<_> = docs
                    .all_schemas()
                    .iter()
                    .flat_map(|s| &s.examples)
                    .collect();
                let output = format_robot_output(&all_examples, format)?;
                println!("{}", output);
            }
        }
    }

    Ok(())
}
