//! CLI schema for the `terraphim-agent` binary.
//!
//! Originally part of the monolithic `main.rs`; moved here as step 2 of the
//! de-monolithization tracked in terraphim/terraphim-clients#211 (the first
//! extraction, `cli_helpers`, was step 1 / PR #212).
//!
//! This module holds the clap-derived schema (`Cli`, `Command`, the per-sub
//! enums, and the small `ValueEnum`/format enums they reference). Dispatch,
//! output formatting, and `RobotFormat`/`CommandOutputConfig` stay in
//! `main.rs` because they have their own coupling to output rendering and
//! the robot layer.
//!
//! All items are `pub(crate)` so `main.rs` can pattern-match on them without
//! leaking the schema outside the binary crate.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use terraphim_agent::{learnings, robot};
use terraphim_types::LogicalOperator;

/// Hook types for Claude Code integration
#[derive(ValueEnum, Debug, Clone)]
pub(crate) enum HookType {
    /// Pre-tool-use hook (intercepts tool calls)
    PreToolUse,
    /// Post-tool-use hook (processes tool results)
    PostToolUse,
    /// Pre-commit hook (validate before commit)
    PreCommit,
    /// Prepare-commit-msg hook (enhance commit message)
    PrepareCommitMsg,
}

/// Boundary mode for text replacement
#[derive(ValueEnum, Debug, Clone, Default)]
pub(crate) enum BoundaryMode {
    /// Match anywhere (default, current behavior)
    #[default]
    None,
    /// Only match at word boundaries
    Word,
}

#[derive(ValueEnum, Debug, Clone, Default)]
pub(crate) enum OutputFormat {
    /// Human-readable output (default)
    #[default]
    Human,
    /// Machine-readable JSON output
    Json,
    /// Compact JSON for piping
    JsonCompact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandOutputMode {
    Human,
    Json,
    JsonCompact,
}

#[derive(ValueEnum, Debug, Clone)]
pub(crate) enum LogicalOperatorCli {
    And,
    Or,
}

impl From<LogicalOperatorCli> for LogicalOperator {
    fn from(op: LogicalOperatorCli) -> Self {
        match op {
            LogicalOperatorCli::And => LogicalOperator::And,
            LogicalOperatorCli::Or => LogicalOperator::Or,
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "terraphim-agent",
    version,
    about = "Terraphim Agent: server-backed fullscreen TUI with offline-capable REPL and CLI commands",
    after_long_help = "EXIT CODES (F1.2 contract)\n\
        \n\
        \x20 0  SUCCESS             Operation completed successfully\n\
        \x20 1  ERROR_GENERAL       Unspecified or unexpected error\n\
        \x20 2  ERROR_USAGE         Invalid arguments or unknown command\n\
        \x20 3  ERROR_INDEX_MISSING Required index not initialised\n\
        \x20 4  ERROR_NOT_FOUND     No results (only with --fail-on-empty)\n\
        \x20 5  ERROR_AUTH          Authentication required or failed\n\
        \x20 6  ERROR_NETWORK       Transport-level network error\n\
        \x20 7  ERROR_TIMEOUT       Operation exceeded configured timeout\n"
)]
pub(crate) struct Cli {
    /// Use server API mode instead of self-contained offline mode
    #[arg(long, default_value_t = false)]
    pub(crate) server: bool,
    /// Server URL for API mode
    #[arg(long, default_value = "http://localhost:8000")]
    pub(crate) server_url: String,
    /// Enable transparent background mode
    #[arg(long, default_value_t = false)]
    pub(crate) transparent: bool,
    /// Enable robot mode for AI agent integration (JSON output, exit codes)
    #[arg(long, default_value_t = false)]
    pub(crate) robot: bool,
    /// Output format (human, json, json-compact)
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    pub(crate) format: OutputFormat,
    /// Path to a JSON config file (overrides settings.toml and persistence)
    #[arg(long)]
    pub(crate) config: Option<String>,
    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// Search documents using the knowledge graph
    Search {
        /// Primary search query
        query: String,
        /// Additional search terms for multi-term queries
        #[arg(long, num_args = 1.., value_delimiter = ',')]
        terms: Option<Vec<String>>,
        /// Logical operator for combining multiple search terms (and/or)
        #[arg(long, value_enum)]
        operator: Option<LogicalOperatorCli>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, default_value_t = false)]
        fail_on_empty: bool,
        /// Include pinned KG entries in results
        #[arg(long, default_value_t = false)]
        include_pinned: bool,
        /// Minimum composite quality score (0.0-1.0). Excludes documents below this threshold.
        #[arg(long)]
        min_quality: Option<f64>,
        /// Maximum estimated tokens in robot-mode output (4 chars ≈ 1 token)
        #[arg(long)]
        max_tokens: Option<usize>,
        /// Maximum characters per content/preview field before truncation
        #[arg(long)]
        max_content_length: Option<usize>,
        /// Output field set: full, summary, minimal, or custom:<f1>,<f2>
        #[arg(long)]
        fields: Option<robot::output::FieldMode>,
    },
    /// Manage roles (list, select)
    Roles {
        #[command(subcommand)]
        sub: RolesSub,
    },
    /// Manage configuration (show, set, validate, reload)
    Config {
        #[command(subcommand)]
        sub: ConfigSub,
    },
    /// Display the knowledge graph for a role
    Graph {
        #[arg(long)]
        role: Option<String>,
        #[arg(long, default_value_t = 50)]
        top_k: usize,
        /// Show only pinned entries
        #[arg(long, default_value_t = false)]
        pinned: bool,
    },
    /// Manage knowledge graph entries
    Kg {
        #[command(subcommand)]
        sub: KgSub,
    },
    /// Chat with the AI using a specific role
    #[cfg(feature = "llm")]
    Chat {
        #[arg(long)]
        role: Option<String>,
        prompt: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Extract paragraphs matching knowledge graph terms from text
    Extract {
        text: String,
        #[arg(long)]
        role: Option<String>,
        #[arg(long, default_value_t = false)]
        exclude_term: bool,
    },
    /// Replace terms in text using the knowledge graph thesaurus
    Replace {
        /// Text to replace (reads from stdin if not provided)
        text: Option<String>,
        #[arg(long)]
        role: Option<String>,
        /// Output format: plain (default), markdown, wiki, html
        #[arg(long)]
        format: Option<String>,
        /// Boundary mode: none (match anywhere) or word (only at word boundaries)
        #[arg(long, default_value = "none")]
        boundary: BoundaryMode,
        /// Output as JSON with metadata (for hook integration)
        #[arg(long, default_value_t = false)]
        json: bool,
        /// Suppress errors and pass through unchanged on failure
        #[arg(long, default_value_t = false)]
        fail_open: bool,
    },
    /// Validate text against knowledge graph
    Validate {
        /// Text to validate (reads from stdin if not provided)
        text: Option<String>,
        /// Role to use for validation
        #[arg(long)]
        role: Option<String>,
        /// Check if all matched terms are connected by a single path
        #[arg(long, default_value_t = false)]
        connectivity: bool,
        /// Validate against a named checklist (e.g., "code_review", "security")
        #[arg(long)]
        checklist: Option<String>,
        /// Output as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Suggest similar terms using fuzzy matching
    Suggest {
        /// Query to search for (reads from stdin if not provided)
        query: Option<String>,
        /// Role to use for suggestions
        #[arg(long)]
        role: Option<String>,
        /// Enable fuzzy matching
        #[arg(long, default_value_t = true)]
        fuzzy: bool,
        /// Minimum similarity threshold (0.0-1.0)
        #[arg(long, default_value_t = 0.6)]
        threshold: f64,
        /// Maximum number of suggestions
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Output as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Unified hook handler for Claude Code integration
    Hook {
        /// Hook type (pre-tool-use, post-tool-use, pre-commit, etc.)
        #[arg(long, value_enum)]
        hook_type: HookType,
        /// JSON input from Claude Code (reads from stdin if not provided)
        #[arg(long)]
        input: Option<String>,
        /// Role to use for processing
        #[arg(long)]
        role: Option<String>,
        /// Output as JSON (always true for hooks, but explicit)
        #[arg(long, default_value_t = true)]
        json: bool,
        /// Include guard check for destructive commands (git reset --hard, rm -rf, etc.)
        ///
        /// Defaults to true for pre-tool-use; for other hooks (post-tool-use,
        /// pre-commit, prepare-commit-msg) the default is false because they
        /// fire after execution or on text inputs that do not need a guard.
        #[arg(long, default_value_t = false)]
        with_guard: bool,
        /// Force the guard check off (overrides `--with-guard` and the per-hook-type default).
        ///
        /// Use this escape hatch only when you have already vetted the command and
        /// need to bypass the safety net. Clap does not auto-derive `--no-with-guard`,
        /// hence this explicit negation flag.
        #[arg(long, default_value_t = false, conflicts_with = "with_guard")]
        no_with_guard: bool,
        /// Allow thesaurus-based command rewriting (e.g. `npm install` -> `bun add`)
        ///
        /// Defaults to **false**. Substitution is opt-in so a stray substring
        /// match cannot silently mutate a destructive command. Pass `--rewrite`
        /// to enable KG-driven rewriting.
        #[arg(long, default_value_t = false)]
        rewrite: bool,
    },
    /// Check command against safety guard patterns (blocks destructive git/fs commands)
    Guard {
        /// Command to check (reads from stdin if not provided)
        command: Option<String>,
        /// Output as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
        /// Suppress errors and pass through unchanged on failure
        #[arg(long, default_value_t = false)]
        fail_open: bool,
        /// Path to custom destructive patterns thesaurus JSON file
        #[arg(long)]
        guard_thesaurus: Option<String>,
        /// Path to custom allowlist thesaurus JSON file
        #[arg(long)]
        guard_allowlist: Option<String>,
        /// Print per-stage evaluation trace (allowlist > destructive > suspicious > default)
        /// showing which stage matched and short-circuited. Requires `--json` for structured
        /// output; without `--json` the trace is printed to stderr in a readable form.
        #[arg(long, default_value_t = false)]
        explain: bool,
    },
    /// Start fullscreen interactive TUI mode (requires running server)
    Interactive,

    /// Start REPL (Read-Eval-Print-Loop) interface
    #[cfg(feature = "repl")]
    Repl {
        /// Start in server mode
        #[arg(long)]
        server: bool,
        /// Server URL for API mode
        #[arg(long, default_value = "http://localhost:8000")]
        server_url: String,
    },

    /// Interactive setup wizard for first-time configuration
    Setup {
        /// Apply a specific template directly (skip interactive wizard)
        #[arg(long)]
        template: Option<String>,
        /// Path to use with the template (required for some templates like local-notes)
        #[arg(long)]
        path: Option<String>,
        /// Add a new role to existing configuration (instead of replacing)
        #[arg(long, default_value_t = false)]
        add_role: bool,
        /// List available templates and exit
        #[arg(long, default_value_t = false)]
        list_templates: bool,
    },

    /// Check for updates without installing
    CheckUpdate,

    /// Update to latest version if available
    Update,

    /// Learning capture for failed commands
    Learn {
        #[command(subcommand)]
        sub: LearnSub,
    },

    /// Session management for AI coding assistant history
    #[cfg(feature = "repl-sessions")]
    Sessions {
        #[command(subcommand)]
        sub: SessionsSub,
    },

    /// Start listener mode for AI agent communication (offline-only)
    Listen {
        /// Agent identity/name for this listener instance
        #[arg(long)]
        identity: Option<String>,
        /// Optional listener configuration JSON file
        #[arg(long)]
        config: Option<String>,
        /// Start in server mode (rejected -- listen is offline-only)
        #[arg(long)]
        server: bool,
    },

    /// Manage the compiled thesaurus cache
    Cache {
        #[command(subcommand)]
        sub: CacheSub,
    },

    /// Robot mode self-documentation commands
    Robot {
        #[command(subcommand)]
        sub: RobotSub,
    },

    /// Memory lifecycle management (capture, distill, scope, provenance, retrieve,
    /// apply, validate, retire, rubric, second-run)
    Memory {
        #[command(subcommand)]
        sub: MemorySub,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CacheSub {
    /// Flush (delete) compiled thesaurus cache entries
    Flush {
        /// Specific role to flush (if omitted, flushes all cached thesauri)
        #[arg(long)]
        role: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum LearnSub {
    /// Capture a failed command as a learning
    Capture {
        /// The command that failed
        command: String,
        /// The error output (stderr)
        #[arg(long)]
        error: String,
        /// The exit code
        #[arg(long, default_value_t = 1)]
        exit_code: i32,
        /// Enable debug output
        #[arg(long, default_value_t = false)]
        debug: bool,
    },
    /// List recent learnings
    List {
        /// Number of recent learnings to show
        #[arg(long, default_value_t = 10)]
        recent: usize,
        /// Show global learnings instead of project
        #[arg(long, default_value_t = false)]
        global: bool,
    },
    /// Query learnings by pattern
    Query {
        /// Search pattern
        pattern: String,
        /// Use exact match instead of substring
        #[arg(long, default_value_t = false)]
        exact: bool,
        /// Show global learnings instead of project
        #[arg(long, default_value_t = false)]
        global: bool,
        /// Enable semantic matching via KG entities
        #[arg(long, default_value_t = false)]
        semantic: bool,
    },
    /// Add correction to an existing learning
    Correct {
        /// Learning ID
        id: String,
        /// The correction to add
        #[arg(long)]
        correction: String,
    },
    /// Record and list user corrections (tool preference, naming, workflow, etc.)
    Correction {
        #[command(subcommand)]
        sub: CorrectionSub,
    },
    /// Process hook input from AI agents (reads JSON from stdin)
    Hook {
        /// AI agent format
        #[arg(long, value_enum, default_value = "claude")]
        format: learnings::AgentFormat,
        /// Hook type for multi-hook pipeline
        #[arg(long, value_enum, default_value = "post-tool-use")]
        learn_hook_type: learnings::LearnHookType,
    },
    /// Install hook for AI agent
    InstallHook {
        /// AI agent to install hook for
        #[arg(value_enum)]
        agent: learnings::AgentType,
    },
    /// Manage captured procedures (recorded command sequences)
    Procedure {
        #[command(subcommand)]
        sub: ProcedureSub,
    },
    /// Compile captured corrections into a thesaurus for the replace command
    Compile {
        /// Output path for compiled thesaurus JSON
        #[arg(long, default_value = "compiled-corrections.json")]
        output: PathBuf,
        /// Optional: merge with this curated thesaurus file
        #[arg(long)]
        merge_with: Option<PathBuf>,
    },
    /// Review and approve/reject knowledge suggestions
    #[cfg(feature = "shared-learning")]
    Suggest {
        #[command(subcommand)]
        sub: SuggestSub,
    },
    /// Export captured corrections as reviewable KG markdown artefacts
    ExportKg {
        /// Output directory for KG markdown files
        #[arg(long)]
        output: PathBuf,
        /// Filter by correction type: tool-preference or all (default: all)
        #[arg(long, default_value = "all")]
        correction_type: String,
    },
    /// Manage shared learnings with trust levels (L1/L2/L3)
    #[cfg(feature = "shared-learning")]
    Shared {
        #[command(subcommand)]
        sub: SharedLearningSub,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum CorrectionSub {
    /// Record a new user correction
    Add {
        /// What the agent said/did originally
        #[arg(long)]
        original: String,
        /// What the user said instead
        #[arg(long)]
        corrected: String,
        /// Type of correction: tool-preference, code-pattern, naming, workflow-step, fact-correction, style-preference, other
        #[arg(long, default_value = "other")]
        correction_type: String,
        /// Context description (optional)
        #[arg(long, default_value = "")]
        context: String,
        /// Session ID for traceability
        #[arg(long)]
        session_id: Option<String>,
    },
    /// List stored corrections
    List {
        /// Show at most this many corrections (default: 20)
        #[arg(long, default_value_t = 20)]
        recent: usize,
        /// Filter by correction type (e.g. tool-preference, code-pattern)
        #[arg(long)]
        filter_type: Option<String>,
        /// Show global corrections instead of project-local
        #[arg(long, default_value_t = false)]
        global: bool,
    },
}

#[cfg(feature = "shared-learning")]
#[derive(Subcommand, Debug)]
pub(crate) enum SharedLearningSub {
    /// List shared learnings, optionally filtered by trust level
    List {
        /// Filter by trust level: l1, l2, l3
        #[arg(long)]
        trust_level: Option<String>,
        /// Maximum number of learnings to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Promote a shared learning to a higher trust level
    Promote {
        /// Learning ID
        id: String,
        /// Target trust level: l2 or l3
        #[arg(long)]
        to: String,
    },
    /// Import local captured learnings into the shared learning store at L1
    Import,
    /// Show shared learning statistics by trust level
    Stats,
    /// Sync L2/L3 learnings to Gitea wiki
    Sync,
    /// Inject learnings from shared directory into local store
    #[cfg(feature = "cross-agent-injection")]
    Inject {
        /// Minimum trust level to inject (l1, l2, l3)
        #[arg(long, default_value = "l2")]
        min_trust: String,
        /// Dry run (show what would be injected without injecting)
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[cfg(feature = "shared-learning")]
#[derive(Subcommand, Debug)]
pub(crate) enum SuggestSub {
    /// List pending suggestions, optionally filtered by status
    List {
        /// Filter by status: pending, approved, rejected
        #[arg(long)]
        status: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show full details of a suggestion
    Show { id: String },
    /// Approve a suggestion (promotes to L3 and marks as approved)
    Approve { id: String },
    /// Reject a suggestion
    Reject {
        id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Approve all pending suggestions above a confidence threshold
    ApproveAll {
        #[arg(long, default_value_t = 0.8)]
        min_confidence: f64,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Reject all pending suggestions below a confidence threshold
    RejectAll {
        #[arg(long, default_value_t = 0.3)]
        max_confidence: f64,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Show suggestion approval metrics
    Metrics,
    /// Show session-end suggestion summary
    SessionEnd {
        #[arg(long)]
        context: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ProcedureSub {
    /// List stored procedures (most recent first)
    List {
        /// Number of recent procedures to show
        #[arg(long, default_value_t = 10)]
        recent: usize,
    },
    /// Show full details of a procedure
    Show {
        /// Procedure ID
        id: String,
    },
    /// Create a new empty procedure
    Record {
        /// Procedure title
        title: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Add a step to an existing procedure
    AddStep {
        /// Procedure ID
        id: String,
        /// Command to execute in this step
        command: String,
        /// Precondition that must hold before this step
        #[arg(long)]
        precondition: Option<String>,
        /// Postcondition that should hold after this step
        #[arg(long)]
        postcondition: Option<String>,
    },
    /// Record a successful execution of a procedure
    Success {
        /// Procedure ID
        id: String,
    },
    /// Record a failed execution of a procedure
    Failure {
        /// Procedure ID
        id: String,
    },
    /// Replay a stored procedure (execute its steps in order)
    Replay {
        /// Procedure ID
        id: String,
        /// Print steps without executing them
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Show health status of all procedures (auto-disables critically failing ones)
    Health,
    /// Enable a previously disabled procedure
    Enable {
        /// Procedure ID
        id: String,
    },
    /// Disable a procedure (prevents replay)
    Disable {
        /// Procedure ID
        id: String,
    },
    /// Auto-capture a procedure from a session's Bash commands
    #[cfg(feature = "repl-sessions")]
    FromSession {
        /// Session ID to extract commands from
        session_id: String,
        /// Optional title (auto-generated from first command if not provided)
        #[arg(long)]
        title: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum RolesSub {
    List,
    Select { name: String },
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigSub {
    /// Show current configuration as JSON
    Show,
    /// Set a configuration value
    Set { key: String, value: String },
    /// Validate configuration loading (shows what would be loaded and from where)
    Validate,
    /// Reload roles from JSON file specified in settings.toml role_config
    Reload,
}

#[derive(Subcommand, Debug)]
pub(crate) enum KgSub {
    /// List knowledge graph entries
    List {
        #[arg(long)]
        role: Option<String>,
        #[arg(long, default_value_t = 50)]
        top_k: usize,
        /// Show only pinned entries
        #[arg(long, default_value_t = false)]
        pinned: bool,
    },
}

#[cfg(feature = "repl-sessions")]
#[derive(Subcommand, Debug)]
pub(crate) enum SessionsSub {
    /// Detect available session sources (Claude Code, Cursor, etc.)
    Sources,
    /// List all cached sessions (auto-imports if cache is empty)
    List {
        /// Limit number of sessions to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Search sessions by query string (auto-imports if cache is empty)
    Search {
        /// Search query
        query: String,
        /// Limit number of results
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Show session statistics (auto-imports if cache is empty)
    Stats,
    /// Print the full body of a session by ID
    Expand {
        /// Session ID to expand
        id: String,
        /// Lines of context to show around matched content (reserved for future --query support)
        #[arg(long, default_value_t = 5)]
        context_lines: usize,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum RobotSub {
    /// Show robot capabilities
    Capabilities {
        /// Output format
        #[arg(long, value_enum, default_value_t = super::RobotFormat::Json)]
        format: super::RobotFormat,
    },
    /// Show command schemas
    Schemas {
        /// Command name to get schema for (all commands if omitted)
        command: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t = super::RobotFormat::Json)]
        format: super::RobotFormat,
    },
    /// Show command examples
    Examples {
        /// Command name to get examples for (all commands if omitted)
        command: Option<String>,
        /// Output format
        #[arg(long, value_enum, default_value_t = super::RobotFormat::Table)]
        format: super::RobotFormat,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum MemorySub {
    /// Capture a command or session event as an agentic memory item
    /// (writes to evolution store with provenance metadata)
    Capture {
        /// Provenance tag for traceability (session ID, commit SHA)
        #[arg(long)]
        provenance_tag: Option<String>,
    },
    /// Distill captured learnings into thesaurus and KG entries
    /// (routes to `learn compile` + `learn export-kg`)
    Distill {
        /// Output format: markdown or json
        #[arg(long, default_value = "markdown")]
        format: String,
    },
    /// Show or check role and project memory boundaries
    Scope {
        /// Role name to show scope for
        #[arg(long)]
        role: Option<String>,
        /// Project path to show scope for
        #[arg(long)]
        project: Option<String>,
        /// Check for permissioned items in public locations
        #[arg(long, default_value_t = false)]
        check: bool,
    },
    /// Search session provenance for a memory ID
    /// (routes to `sessions search`)
    Provenance {
        /// Memory ID to search provenance for
        #[arg(long)]
        memory_id: Option<String>,
        /// Search query
        query: Option<String>,
    },
    /// Retrieve memory items by query within role scope
    /// (routes to `search`)
    Retrieve {
        /// Role scope for retrieval
        #[arg(long)]
        role: Option<String>,
        /// Search query
        query: String,
    },
    /// Show what hooks would inject for a given prompt or diff
    /// (routes to `terraphim_hooks` diff)
    Apply {
        /// Prompt text to diff hook application against
        #[arg(long)]
        prompt: Option<String>,
    },
    /// Validate memory items against the reliability rubric
    /// (calls judge pipeline for scoring)
    Validate {
        /// Validate all stored memory items
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Validate a specific lesson by ID
        #[arg(long)]
        lesson_id: Option<String>,
    },
    /// Propose retirement of a memory item
    /// (writes to learned-rules.md with CTO approval flag)
    Retire {
        /// Learning ID to retire
        #[arg(long)]
        lesson_id: Option<String>,
        /// Reason for retirement
        #[arg(long)]
        reason: Option<String>,
    },
    /// Run the full Memory Reliability Rubric diagnostic on a project
    /// (6 dimensions: faithfulness, scope, provenance, actionability, decay, risk)
    Rubric {
        /// Project path to run rubric against
        #[arg(long)]
        project: String,
        /// Output file for markdown readout (stdout if omitted)
        #[arg(long)]
        output: Option<String>,
    },
    /// List memory items from the evolution store
    List {
        /// Filter by type (fact, experience, lesson, etc.)
        #[arg(long)]
        item_type: Option<String>,
        /// Maximum items to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show details of a specific memory item or lesson by ID
    Show {
        /// Memory item or lesson ID
        id: String,
        /// Show raw JSON output
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Export memory items and lessons as JSON or markdown
    Export {
        /// Output format: json or markdown
        #[arg(long, default_value = "json")]
        format: String,
        /// Output file path (stdout if omitted)
        #[arg(long)]
        output: Option<String>,
    },
    /// Compute token delta between two ADF runs of the same Gitea issue
    /// (second-run acceleration signal)
    SecondRun {
        /// Gitea issue number to compare runs for
        #[arg(long)]
        issue: u64,
    },
}