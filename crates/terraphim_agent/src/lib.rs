//! Terraphim agent library — TUI, robot mode, and multi-agent coordination.
//!
//! Bundles the interactive REPL, robot-mode JSON output, forgiving CLI parser,
//! MCP tool index, onboarding workflows, and optional shared-learning store.
//! Feature flags gate heavier subsystems: `server`, `repl`, `shared-learning`.
#[cfg(feature = "server")]
pub mod client;
pub mod logging;
pub mod onboarding;
pub mod service;
#[cfg(feature = "shared-learning")]
pub mod shared_learning;
pub mod tui_backend;

// Robot mode - always available for AI agent integration
pub mod robot;

// Forgiving CLI - always available for typo-tolerant parsing
pub mod forgiving;

// MCP Tool Index - re-exported from terraphim_mcp_search for back-compat.
pub use terraphim_mcp_search::McpToolIndex;

// Deprecated shim: terraphim_agent::mcp_tool_index::McpToolIndex still works,
// but emits a deprecation warning. Remove in the next major release.
pub mod mcp_tool_index;

// Command guard patterns - always available for risk classification
pub mod guard_patterns;

// Learning capture system - always available so secret-redaction and hook
// passthrough logic are exercised by `cargo test --lib` and `--doc` gates.
pub mod learnings;

#[cfg(feature = "repl")]
pub mod repl;

#[cfg(feature = "repl-custom")]
pub mod commands;

#[cfg(feature = "server")]
pub use client::*;

// Re-export robot mode types
pub use robot::{
    BudgetEngine, BudgetError, BudgetedResults, ExitCode, FieldMode, OutputFormat, RobotConfig,
    RobotError, RobotFormatter, RobotResponse, SelfDocumentation,
};

// Re-export forgiving CLI types
pub use forgiving::{AliasRegistry, ForgivingParser, ParseResult};

#[cfg(feature = "repl")]
pub use repl::*;

#[cfg(feature = "repl-custom")]
pub use commands::*;

// Test-specific exports - make modules available in tests with required features
#[cfg(test)]
pub mod test_exports {
    #[cfg(feature = "repl")]
    pub use crate::repl::*;

    #[cfg(feature = "repl")]
    pub use std::str::FromStr;

    #[cfg(feature = "repl-custom")]
    pub use crate::commands::*;

    pub use crate::forgiving::*;
    pub use crate::robot::*;
}

/// Regression coverage for the `mcp_tool_index` deprecation shim and for the
/// load-bearing `[patch.terraphim]` block in the workspace `Cargo.toml`.
///
/// These tests fail to *compile* if either contract is broken:
///   1. `terraphim_agent::mcp_tool_index::McpToolIndex` stops being the same
///      type as `terraphim_mcp_search::McpToolIndex` (shim contract).
///   2. `terraphim_mcp_search`'s `terraphim_types::McpToolEntry` stops being the
///      same type as `terraphim_agent`'s own `terraphim_types::McpToolEntry`.
///      This only holds while the patch unifies the two registries; removing it
///      yields two distinct copies of `terraphim_types` and a compile error here.
#[cfg(test)]
mod mcp_shim_identity_tests {
    use crate::McpToolIndex;
    use crate::mcp_tool_index::McpToolIndex as ShimMcpToolIndex;
    use terraphim_mcp_search::McpToolIndex as SearchMcpToolIndex;
    use terraphim_types::McpToolEntry;

    /// Compile-time proof that two type *expressions* denote the same type.
    ///
    /// The single type parameter `T` must be inferred from BOTH arguments, so
    /// this only compiles when `A` and `B` are literally the same type. Two
    /// distinct types (e.g. two copies of `McpToolIndex` from different
    /// `terraphim_types` sources) produce a type-mismatch error at the call site.
    fn assert_same_type<T>(_: &T, _: &T) {}

    #[test]
    fn shim_re_export_is_search_crate_type() {
        // F2: the deprecated module path and the crate-root re-export must both
        // resolve to the *same* type as `terraphim_mcp_search::McpToolIndex`.
        // Each call below fails to compile if either path drifts.
        let shim: fn(std::path::PathBuf) -> ShimMcpToolIndex = ShimMcpToolIndex::new;
        let reexported: fn(std::path::PathBuf) -> McpToolIndex = McpToolIndex::new;
        let search: fn(std::path::PathBuf) -> SearchMcpToolIndex = SearchMcpToolIndex::new;
        assert_same_type(&shim, &search);
        assert_same_type(&reexported, &search);
    }

    #[test]
    fn patch_unifies_terraphim_types() {
        // F1: the `[patch.terraphim]` block is load-bearing. It forces the
        // terraphim-registry `terraphim_types` (depended on by both this crate
        // and `terraphim_mcp_search`) onto the same crates.io source the rest
        // of the workspace uses. Without it, the dual-registry split resurfaces
        // and this test breaks -- either at version resolution (no `^1.20.4`
        // match on the terraphim registry) or, with compatible versions, at
        // type-checking here, because `add_tool`'s `McpToolEntry` would come
        // from a *different* `terraphim_types` than the one named below.
        fn search_entry() -> terraphim_mcp_search::McpToolIndex {
            unreachable!()
        }
        // `McpToolIndex::add_tool` takes `terraphim_types::McpToolEntry`. The
        // closure coerces to `fn(&mut SearchMcpToolIndex, McpToolEntry)` only
        // while the search crate's `McpToolEntry` is the same type as ours.
        let _: fn(&mut SearchMcpToolIndex, McpToolEntry) = |idx, entry| idx.add_tool(entry);
        let _ = search_entry;
    }

    #[test]
    fn shim_path_search_returns_results() {
        // Behavioural smoke: build an index via the deprecated module path and
        // confirm search still returns results. This complements the compile-time
        // identity proofs above -- it does NOT exercise save()/load() persistence
        // (covered in terraphim_mcp_search::tests::test_tool_index_save_and_load).
        let mut index =
            ShimMcpToolIndex::new(std::env::temp_dir().join("ta-mcp-shim-identity.json"));
        index.add_tool(McpToolEntry::new(
            "grep_search",
            "Search text using grep",
            "search",
        ));
        assert_eq!(index.tool_count(), 1);
        assert_eq!(index.search("grep").len(), 1);
        assert_eq!(index.search("nope").len(), 0);
    }

    #[test]
    fn shim_path_save_load_round_trip() {
        // P3 closure: exercise save()/load() persistence through the *deprecated*
        // module path, proving the back-compat surface does real I/O, not just
        // compile. The type-identity proofs above guarantee the same code runs
        // as `terraphim_mcp_search` -- this test pins that contract at runtime.
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let path = std::env::temp_dir().join(format!("ta-mcp-shim-roundtrip-{unique}.json"));

        // Save via the deprecated path.
        {
            let mut index = ShimMcpToolIndex::new(path.clone());
            index.add_tool(McpToolEntry::new(
                "save_load_tool",
                "Persists via deprecated shim",
                "test",
            ));
            index.save().expect("save via shim must succeed");
        }

        // Load via the deprecated path and verify.
        let loaded = ShimMcpToolIndex::load(path.clone()).expect("load via shim must succeed");
        assert_eq!(loaded.tool_count(), 1);
        assert_eq!(loaded.tools()[0].name, "save_load_tool");

        let _ = std::fs::remove_file(&path);
    }
}
