//! Tests for `/sessions` REPL command parsing — Task 2.6 acceptance criteria.
//!
//! Adapted (2026-09-07) from stale PR #39's `sessions_commands_tests.rs` per
//! owner decision A: `/sessions import` stays removed (auto-import replaced
//! it — the parser returns an explanatory error), and the REPL has no
//! `expand` alias (expand is a CLI-only subcommand, landed via #165). The
//! remaining parser contracts are pinned here so regressions in aliases
//! (`ls`, `detect`, `/session`), flag parsing (`--source`, `--limit`), and
//! error paths surface in CI. Refs terraphim/terraphim-ai#2435.

use std::str::FromStr;
use terraphim_agent::repl::commands::ReplCommand;

#[cfg(feature = "repl-sessions")]
use terraphim_agent::repl::commands::SessionsSubcommand;

// ── list ────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_list_no_args_parses() {
    let cmd = ReplCommand::from_str("/sessions list").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::List {
                    source: None,
                    limit: None
                }
            }
        ),
        "expected List {{ source: None, limit: None }}"
    );
}

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_list_with_source_filter_parses() {
    let cmd = ReplCommand::from_str("/sessions list --source cursor").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::List {
                    source: Some(ref s),
                    limit: None
                }
            }
            if s == "cursor"
        ),
        "expected List {{ source: Some(\"cursor\"), limit: None }}"
    );
}

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_ls_alias_parses() {
    let cmd = ReplCommand::from_str("/sessions ls").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::List { .. }
            }
        ),
        "expected ls to map to List"
    );
}

// ── search ──────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_search_parses_query() {
    let cmd = ReplCommand::from_str("/sessions search rust async tokio").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::Search { ref query }
            }
            if query == "rust async tokio"
        ),
        "expected Search {{ query: \"rust async tokio\" }}"
    );
}

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_search_missing_query_errors() {
    let result = ReplCommand::from_str("/sessions search");
    assert!(
        result.is_err(),
        "search without query should return an error"
    );
}

// ── show ────────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_show_parses_session_id() {
    let cmd = ReplCommand::from_str("/sessions show abc12345").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::Show { ref session_id }
            }
            if session_id == "abc12345"
        ),
        "expected Show {{ session_id: \"abc12345\" }}"
    );
}

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_get_alias_parses() {
    let cmd = ReplCommand::from_str("/sessions get abc12345").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::Show { .. }
            }
        ),
        "expected get to map to Show"
    );
}

// ── sources ──────────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_sources_parses() {
    let cmd = ReplCommand::from_str("/sessions sources").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::Sources
            }
        ),
        "expected Sources"
    );
}

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_detect_alias_parses() {
    let cmd = ReplCommand::from_str("/sessions detect").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::Sources
            }
        ),
        "expected detect to map to Sources"
    );
}

// ── session (singular) alias ──────────────────────────────────────────────────

#[test]
#[cfg(feature = "repl-sessions")]
fn session_singular_alias_works() {
    let cmd = ReplCommand::from_str("/session list").unwrap();
    assert!(
        matches!(
            cmd,
            ReplCommand::Sessions {
                subcommand: SessionsSubcommand::List { .. }
            }
        ),
        "expected /session (singular) to also parse"
    );
}

// ── removed-command contract (owner decision A: import stays removed) ──────

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_import_removed_explains_auto_import() {
    let result = ReplCommand::from_str("/sessions import");
    let err = result.expect_err("import should error (removed command)");
    let msg = err.to_string();
    assert!(
        msg.contains("has been removed") && msg.contains("automatically imported"),
        "expected the removal explanation, got: {msg}"
    );
}

// ── error cases ──────────────────────────────────────────────────────────────

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_missing_subcommand_errors() {
    let result = ReplCommand::from_str("/sessions");
    assert!(result.is_err(), "sessions without subcommand should error");
}

#[test]
#[cfg(feature = "repl-sessions")]
fn sessions_unknown_subcommand_errors() {
    let result = ReplCommand::from_str("/sessions foobar");
    assert!(result.is_err(), "unknown sessions subcommand should error");
}
