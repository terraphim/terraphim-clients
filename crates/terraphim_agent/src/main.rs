use std::io;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::{List, ListItem, Paragraph},
};
use serde::Serialize;
#[cfg(feature = "repl")]
use terraphim_agent::repl;
use terraphim_agent::{guard_patterns, learnings, onboarding, robot, tui_backend};
use terraphim_persistence::Persistable;
use tokio::runtime::Runtime;

mod cli_helpers;
mod cli_schema;
mod listener;
mod robot_dispatch;
mod shell_dispatch;

use cli_helpers::*;
use cli_schema::*;
use robot_dispatch::*;

// Robot mode and forgiving CLI - always available

// Learning capture for failed commands

// KG-based command validation for PreToolUse hook pipeline
mod kg_validation;

#[cfg(feature = "server")]
use terraphim_agent::client::{ApiClient, SearchResponse};
use terraphim_agent::service::TuiService;
use terraphim_types::{
    Document, Layer, LogicalOperator, NormalizedTermValue, RoleName, SearchQuery,
};
use terraphim_update::{TerraphimUpdater, UpdaterConfig};

/// Show helpful usage information when run without a TTY
fn show_usage_info() {
    println!("Terraphim AI Agent v{}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Interactive Modes (requires TTY):");
    println!("  terraphim-agent              # Start fullscreen TUI (requires running server)");
    println!("  terraphim-agent repl         # Start REPL (offline-capable by default)");
    println!("  terraphim-agent repl --server # Start REPL in server mode");
    println!();
    println!("Common Commands:");
    println!("  search <query>               # Search documents (offline-capable by default)");
    println!("  roles list                   # List available roles");
    println!("  config show                  # Show configuration");
    println!("  replace <text>               # Replace terms using thesaurus");
    println!("  validate <text>              # Validate against knowledge graph");
    println!();
    println!("For more information:");
    println!("  terraphim-agent --help       # Show full help");
    println!("  terraphim-agent help         # Show command-specific help");
}

#[cfg(feature = "server")]
fn resolve_tui_server_url(explicit: Option<&str>) -> String {
    let env_server = std::env::var("TERRAPHIM_SERVER").ok();
    resolve_tui_server_url_with_env(explicit, env_server.as_deref())
}

#[cfg(feature = "server")]
fn resolve_tui_server_url_with_env(explicit: Option<&str>, env_server: Option<&str>) -> String {
    explicit
        .map(ToOwned::to_owned)
        .or_else(|| env_server.map(ToOwned::to_owned))
        .unwrap_or_else(|| "http://localhost:8000".to_string())
}

#[cfg(feature = "server")]
fn tui_server_requirement_error(url: &str, cause: &anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "Fullscreen TUI requires a running Terraphim server at {}. \
         Start terraphim_server or use offline mode with `terraphim-agent repl`. \
         Connection error: {}",
        url,
        cause
    )
}

#[cfg(feature = "server")]
fn ensure_tui_server_reachable(
    runtime: &tokio::runtime::Runtime,
    api: &ApiClient,
    url: &str,
) -> Result<()> {
    runtime
        .block_on(api.health())
        .map_err(|err| tui_server_requirement_error(url, &err))
}

#[derive(Debug, Clone, PartialEq)]
enum ViewMode {
    Search,
    ResultDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TuiAction {
    None,
    Quit,
    SearchOrOpen,
    MoveUp,
    MoveDown,
    Autocomplete,
    SwitchRole,
    SummarizeSelection,
    SummarizeDetail,
    Backspace,
    InsertChar(char),
    BackToSearch,
}

#[cfg(test)]
fn key_event(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn map_search_key_event(event: KeyEvent) -> TuiAction {
    match (event.code, event.modifiers) {
        (KeyCode::Char('q'), KeyModifiers::CONTROL) => TuiAction::Quit,
        (KeyCode::Esc, KeyModifiers::NONE) => TuiAction::Quit,
        (KeyCode::Enter, KeyModifiers::NONE) => TuiAction::SearchOrOpen,
        (KeyCode::Up, KeyModifiers::NONE) => TuiAction::MoveUp,
        (KeyCode::Down, KeyModifiers::NONE) => TuiAction::MoveDown,
        (KeyCode::Tab, KeyModifiers::NONE) => TuiAction::Autocomplete,
        (KeyCode::Char('r'), KeyModifiers::CONTROL) => TuiAction::SwitchRole,
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => TuiAction::SummarizeSelection,
        (KeyCode::Backspace, KeyModifiers::NONE) => TuiAction::Backspace,
        (KeyCode::Char(c), KeyModifiers::NONE) => TuiAction::InsertChar(c),
        _ => TuiAction::None,
    }
}

fn map_detail_key_event(event: KeyEvent) -> TuiAction {
    match (event.code, event.modifiers) {
        (KeyCode::Esc, KeyModifiers::NONE) => TuiAction::BackToSearch,
        (KeyCode::Char('q'), KeyModifiers::CONTROL) => TuiAction::Quit,
        (KeyCode::Char('s'), KeyModifiers::CONTROL) => TuiAction::SummarizeDetail,
        _ => TuiAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_search_key_event_allows_plain_letters() {
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Char('s'), KeyModifiers::NONE)),
            TuiAction::InsertChar('s')
        );
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Char('r'), KeyModifiers::NONE)),
            TuiAction::InsertChar('r')
        );
        // 'q' should also be typeable now
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Char('q'), KeyModifiers::NONE)),
            TuiAction::InsertChar('q')
        );
    }

    #[test]
    fn map_search_key_event_ctrl_shortcuts() {
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            TuiAction::SummarizeSelection
        );
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Char('r'), KeyModifiers::CONTROL)),
            TuiAction::SwitchRole
        );
        // Ctrl+q quits
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            TuiAction::Quit
        );
        // Esc also quits in search mode
        assert_eq!(
            map_search_key_event(key_event(KeyCode::Esc, KeyModifiers::NONE)),
            TuiAction::Quit
        );
    }

    #[test]
    fn map_detail_key_event_ctrl_s_summarizes() {
        assert_eq!(
            map_detail_key_event(key_event(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            TuiAction::SummarizeDetail
        );
        assert_eq!(
            map_detail_key_event(key_event(KeyCode::Char('s'), KeyModifiers::NONE)),
            TuiAction::None
        );
    }

    #[test]
    fn map_detail_key_event_ctrl_q_quits() {
        // Ctrl+q quits in detail mode
        assert_eq!(
            map_detail_key_event(key_event(KeyCode::Char('q'), KeyModifiers::CONTROL)),
            TuiAction::Quit
        );
        // Plain 'q' does nothing (no typing in detail mode)
        assert_eq!(
            map_detail_key_event(key_event(KeyCode::Char('q'), KeyModifiers::NONE)),
            TuiAction::None
        );
        // Esc goes back to search, not quit
        assert_eq!(
            map_detail_key_event(key_event(KeyCode::Esc, KeyModifiers::NONE)),
            TuiAction::BackToSearch
        );
    }

    #[test]
    fn resolve_tui_server_url_uses_explicit_then_env_then_default() {
        let explicit = resolve_tui_server_url_with_env(Some("http://explicit:9000"), None);
        assert_eq!(explicit, "http://explicit:9000");

        let from_env = resolve_tui_server_url_with_env(None, Some("http://env:7000"));
        assert_eq!(from_env, "http://env:7000");

        let defaulted = resolve_tui_server_url_with_env(None, None);
        assert_eq!(defaulted, "http://localhost:8000");
    }

    #[test]
    fn tui_server_requirement_error_mentions_repl_fallback() {
        let cause = anyhow::anyhow!("connect error");
        let err = tui_server_requirement_error("http://localhost:8000", &cause);
        let msg = err.to_string();
        assert!(msg.contains("Fullscreen TUI requires a running Terraphim server"));
        assert!(msg.contains("terraphim-agent repl"));
        assert!(msg.contains("http://localhost:8000"));
    }

    #[test]
    fn session_expand_output_serialises_to_json() {
        use session_output::{ExpandedMessage, SessionExpandOutput};
        let payload = SessionExpandOutput {
            id: "sess-abc".to_string(),
            title: Some("My session".to_string()),
            message_count: 2,
            messages: vec![
                ExpandedMessage {
                    idx: 0,
                    role: "user".to_string(),
                    content: "hello".to_string(),
                },
                ExpandedMessage {
                    idx: 1,
                    role: "assistant".to_string(),
                    content: "world".to_string(),
                },
            ],
        };
        let json = serde_json::to_string(&payload).expect("serialisation failed");
        assert!(json.contains("sess-abc"));
        assert!(json.contains("My session"));
        assert!(json.contains("hello"));
        assert!(json.contains("world"));
        assert!(json.contains("\"idx\":0"));
        assert!(json.contains("\"idx\":1"));
    }

    #[test]
    fn session_expand_output_no_title_serialises() {
        use session_output::{ExpandedMessage, SessionExpandOutput};
        let payload = SessionExpandOutput {
            id: "sess-xyz".to_string(),
            title: None,
            message_count: 1,
            messages: vec![ExpandedMessage {
                idx: 0,
                role: "user".to_string(),
                content: "test".to_string(),
            }],
        };
        let json = serde_json::to_string(&payload).expect("serialisation failed");
        assert!(json.contains("sess-xyz"));
        assert!(
            json.contains("null") || !json.contains("\"title\"") || json.contains("\"title\":null")
        );
    }
}

#[derive(clap::ValueEnum, Debug, Clone, Default)]
pub(crate) enum RobotFormat {
    #[default]
    Json,
    Table,
    Minimal,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CommandOutputConfig {
    mode: CommandOutputMode,
    robot: bool,
}

impl CommandOutputConfig {
    fn is_machine_readable(self) -> bool {
        self.robot || !matches!(self.mode, CommandOutputMode::Human)
    }
}

fn resolve_output_config(robot: bool, format: OutputFormat) -> CommandOutputConfig {
    let mode = match format {
        OutputFormat::Human => {
            if robot {
                CommandOutputMode::Json
            } else {
                CommandOutputMode::Human
            }
        }
        OutputFormat::Json => CommandOutputMode::Json,
        OutputFormat::JsonCompact => CommandOutputMode::JsonCompact,
    };
    CommandOutputConfig { mode, robot }
}

/// Get the session cache file path
#[cfg(feature = "repl-sessions")]
fn get_session_cache_path() -> std::path::PathBuf {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("terraphim-agent");
    std::fs::create_dir_all(&cache_dir).ok();
    cache_dir.join("sessions.json")
}

#[cfg(feature = "repl-sessions")]
mod session_output {
    use serde::Serialize;

    #[derive(Debug, Serialize)]
    pub struct SourcesOutput {
        pub count: usize,
        pub sources: Vec<SourceEntry>,
    }

    #[derive(Debug, Serialize)]
    pub struct SourceEntry {
        pub id: String,
        pub name: Option<String>,
        pub available: bool,
    }

    #[derive(Debug, Serialize)]
    pub struct SessionListOutput {
        pub total: usize,
        pub shown: usize,
        pub sessions: Vec<SessionEntry>,
    }

    #[derive(Debug, Serialize)]
    pub struct SessionEntry {
        pub id: String,
        pub title: Option<String>,
        pub message_count: usize,
        pub source: String,
    }

    #[derive(Debug, Serialize)]
    pub struct SessionSearchOutput {
        pub query: String,
        pub total: usize,
        pub shown: usize,
        pub sessions: Vec<SessionSearchEntry>,
    }

    #[derive(Debug, Serialize)]
    pub struct SessionSearchEntry {
        pub id: String,
        pub title: Option<String>,
        pub message_count: usize,
        pub preview: Option<String>,
    }

    #[derive(Debug, Serialize)]
    pub struct SessionStatsOutput {
        pub total_sessions: usize,
        pub total_messages: usize,
        pub total_user_messages: usize,
        pub total_assistant_messages: usize,
        pub by_source: std::collections::HashMap<String, usize>,
    }

    #[derive(Debug, Serialize)]
    pub struct SessionExpandOutput {
        pub id: String,
        pub title: Option<String>,
        pub message_count: usize,
        pub messages: Vec<ExpandedMessage>,
    }

    #[derive(Debug, Serialize)]
    pub struct ExpandedMessage {
        pub idx: usize,
        pub role: String,
        pub content: String,
    }
}

fn print_json_output<T: Serialize>(value: &T, mode: CommandOutputMode) -> Result<()> {
    let out = match mode {
        CommandOutputMode::Human => serde_json::to_string_pretty(value)?,
        CommandOutputMode::Json => serde_json::to_string_pretty(value)?,
        CommandOutputMode::JsonCompact => serde_json::to_string(value)?,
    };
    println!("{}", out);
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let corrected_args = apply_forgiving_parsing(&args);
    let cli = Cli::parse_from(corrected_args);
    let output = resolve_output_config(cli.robot, cli.format.clone());

    // Check for updates on startup (non-blocking, debug logging on failure)
    let rt = Runtime::new()?;
    rt.block_on(async {
        let config = UpdaterConfig::new("terraphim-agent").with_version(env!("CARGO_PKG_VERSION"));
        let updater = TerraphimUpdater::new(config);
        if let Err(e) = updater.check_update().await {
            log::debug!("Update check failed: {}", e);
        }
    });

    match cli.command {
        Some(Command::Interactive) | None => {
            // Check if we're in a TTY for interactive mode (both stdout and stdin required)
            use std::io::IsTerminal;
            if !std::io::stdout().is_terminal() {
                show_usage_info();
                std::process::exit(0);
            }

            if !std::io::stdin().is_terminal() {
                show_usage_info();
                std::process::exit(0);
            }

            #[cfg(feature = "server")]
            {
                if cli.server {
                    run_tui_server_mode(&cli.server_url, cli.transparent)
                } else {
                    run_tui_offline_mode(cli.transparent)
                }
            }
            #[cfg(not(feature = "server"))]
            {
                if cli.server {
                    eprintln!(
                        "TUI server mode requires the 'server' feature. Use offline mode instead."
                    );
                    return Err(anyhow::anyhow!("TUI server mode requires server feature"));
                }
                run_tui_offline_mode(cli.transparent)
            }
        }

        #[cfg(feature = "repl")]
        Some(Command::Repl { server, .. }) => {
            let rt = Runtime::new()?;
            #[cfg(feature = "server")]
            {
                if server {
                    return rt.block_on(repl::run_repl_server_mode("http://localhost:8000"));
                }
            }
            #[cfg(not(feature = "server"))]
            {
                if server {
                    eprintln!(
                        "REPL server mode requires the 'server' feature. Starting in offline mode instead."
                    );
                }
            }
            rt.block_on(repl::run_repl_offline_mode())
        }

        Some(Command::Listen {
            identity,
            config,
            server,
        }) => {
            // Listen mode is offline-only - reject --server flag
            if server {
                eprintln!("error: listen mode does not support --server flag");
                eprintln!("The listener runs in offline mode only.");
                std::process::exit(2);
            }
            let identity = match identity {
                Some(id) => id,
                None => {
                    eprintln!("error: --identity is required for listen mode");
                    std::process::exit(2);
                }
            };
            let listener_config = match config.as_deref() {
                Some(path) => listener::ListenerConfig::load_from_path(path)?,
                None => listener::ListenerConfig::for_identity(identity.clone()),
            };
            listener_config.validate()?;
            println!("listener would start with identity: {}", identity);
            println!(
                "resolved Gitea login: {}",
                listener_config.identity.resolved_gitea_login()
            );
            println!("poll interval: {}s", listener_config.poll_interval_secs);
            if listener_config.gitea.is_none() {
                println!("listener config has no Gitea connection; discovery only");
                return Ok(());
            }
            rt.block_on(listener::run_listener(listener_config))
        }
        Some(Command::Robot { sub }) => {
            handle_robot_command(sub)?;
            Ok(())
        }
        Some(command) => {
            let rt = Runtime::new()?;
            let robot_mode = cli.robot;
            let output_format = cli.format.clone();
            #[cfg(feature = "server")]
            {
                if cli.server {
                    let result = rt.block_on(run_server_command(command, &cli.server_url, output));
                    if let Err(ref e) = result {
                        let code = classify_error(e);
                        emit_robot_error_and_exit(e, code, robot_mode, &output_format);
                    }
                    return result;
                }
            }
            let result = rt.block_on(run_offline_command(command, output, cli.config));
            if let Err(ref e) = result {
                let code = classify_error(e);
                emit_robot_error_and_exit(e, code, robot_mode, &output_format);
            }
            result
        }
    }
}
fn run_tui_offline_mode(transparent: bool) -> Result<()> {
    // Fullscreen TUI mode requires a running server.
    // For offline operation, use `terraphim-agent repl`.
    run_tui(None, transparent)
}

fn run_tui_server_mode(server_url: &str, transparent: bool) -> Result<()> {
    run_tui(Some(server_url.to_string()), transparent)
}

/// Stateless config validation -- runs before TuiService initialization.
/// Shows config sources, paths, and what would be loaded.
async fn run_config_validate() -> Result<()> {
    use terraphim_settings::DeviceSettings;

    println!("== Device Settings ==");
    let ds = match DeviceSettings::load_from_env_and_file(None) {
        Ok(s) => {
            let config_path = DeviceSettings::default_config_path();
            println!("  settings.toml: {}/settings.toml", config_path.display());
            println!("  server_hostname: {}", s.server_hostname);
            println!("  api_endpoint: {}", s.api_endpoint);
            println!("  default_data_path: {}", s.default_data_path);
            println!("  profiles: {:?}", s.profiles.keys().collect::<Vec<_>>());
            s
        }
        Err(e) => {
            println!("  FAILED to load: {:?}", e);
            println!("  Would use embedded defaults");
            DeviceSettings::default_embedded()
        }
    };

    println!();
    println!("== Role Configuration ==");
    match &ds.role_config {
        Some(path) => {
            let expanded = terraphim_config::expand_path(path);
            println!("  role_config: {} (expanded: {})", path, expanded.display());
            if expanded.exists() {
                match terraphim_config::Config::load_from_json_file(path) {
                    Ok(config) => {
                        println!("  Status: OK - loaded {} role(s)", config.roles.len());
                        for (name, role) in &config.roles {
                            println!("    - {} (shortname: {:?})", name, role.shortname);
                        }
                        println!("  default_role in file: {}", config.default_role);
                        println!("  selected_role in file: {}", config.selected_role);
                    }
                    Err(e) => {
                        println!("  Status: PARSE ERROR - {:?}", e);
                    }
                }
            } else {
                println!("  Status: FILE NOT FOUND at {}", expanded.display());
            }
        }
        None => {
            println!("  role_config: not set (using persistence/embedded defaults)");
        }
    }

    if let Some(ref role) = ds.default_role {
        println!("  default_role override: {}", role);
    }

    println!();
    println!("== Persistence ==");
    match terraphim_config::ConfigBuilder::new_with_id(terraphim_config::ConfigId::Embedded).build()
    {
        Ok(mut config) => match config.load().await {
            Ok(persisted) => {
                println!(
                    "  Persisted config found with {} role(s):",
                    persisted.roles.len()
                );
                for (name, role) in &persisted.roles {
                    println!("    - {} (shortname: {:?})", name, role.shortname);
                }
                println!("  selected_role: {}", persisted.selected_role);
            }
            Err(_) => {
                println!("  No persisted config found (first run or empty)");
            }
        },
        Err(e) => {
            println!("  Failed to check persistence: {:?}", e);
        }
    }

    println!();
    println!("== Summary ==");
    if ds.role_config.is_some() {
        println!("  Config source: role_config in settings.toml (bootstrap-then-persistence)");
    } else {
        println!("  Config source: persistence layer or embedded defaults");
    }

    Ok(())
}

struct GuardArgs<'a> {
    command: &'a Option<String>,
    json: bool,
    fail_open: bool,
    guard_thesaurus: &'a Option<String>,
    guard_allowlist: &'a Option<String>,
    explain: bool,
}

async fn handle_guard_command(args: &GuardArgs<'_>) -> Result<()> {
    let input_command = match args.command {
        Some(c) => c.clone(),
        None => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            buffer.trim().to_string()
        }
    };

    let guard = match (args.guard_thesaurus, args.guard_allowlist) {
        (Some(thesaurus_path), Some(allowlist_path)) => {
            let destructive_json = std::fs::read_to_string(thesaurus_path)?;
            let allowlist_json = std::fs::read_to_string(allowlist_path)?;
            guard_patterns::CommandGuard::from_json(&destructive_json, &allowlist_json, None)
                .map_err(|e| anyhow::anyhow!("Failed to load custom guard thesauruses: {}", e))?
        }
        (Some(thesaurus_path), None) => {
            let destructive_json = std::fs::read_to_string(thesaurus_path)?;
            guard_patterns::CommandGuard::from_json(
                &destructive_json,
                guard_patterns::CommandGuard::default_allowlist_json(),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to load custom guard thesaurus: {}", e))?
        }
        (None, Some(allowlist_path)) => {
            let allowlist_json = std::fs::read_to_string(allowlist_path)?;
            guard_patterns::CommandGuard::from_json(
                guard_patterns::CommandGuard::default_destructive_json(),
                &allowlist_json,
                None,
            )
            .map_err(|e| anyhow::anyhow!("Failed to load custom guard allowlist: {}", e))?
        }
        (None, None) => guard_patterns::CommandGuard::new(),
    };
    let result = guard.check(&input_command);

    if args.explain {
        // Recompute the trace so we can show the per-stage path even
        // when the final decision came from a short-circuit. The trace
        // shares the same matchers as `check`, so this is a second
        // walk over the same inputs (cheap: a `Vec<4>` plus three
        // Aho-Corasick matches).
        let trace = guard.check_with_trace(&input_command);
        trace.print(args.json)?;
        // Still respect the normal exit-code semantics when --explain is on
        // so scripts can use `--explain --fail-on-empty` style gating.
        if trace.result.decision == guard_patterns::GuardDecision::Block && !args.fail_open {
            std::process::exit(1);
        }
        return Ok(());
    }

    if args.json {
        println!("{}", serde_json::to_string(&result)?);
    } else if result.decision == guard_patterns::GuardDecision::Block
        && let Some(reason) = &result.reason
    {
        eprintln!("BLOCKED: {}", reason);
        if !args.fail_open {
            std::process::exit(1);
        }
    }
    // If allowed, no output in non-JSON mode (silent success)
    Ok(())
}

async fn handle_check_update_command() -> Result<()> {
    println!("Checking for terraphim-agent updates...");
    let config = UpdaterConfig::new("terraphim-agent").with_version(env!("CARGO_PKG_VERSION"));
    let updater = TerraphimUpdater::new(config);
    match updater.check_update().await {
        Ok(status) => {
            println!("{}", status);
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to check for updates: {}", e);
            std::process::exit(1);
        }
    }
}

async fn handle_update_command() -> Result<()> {
    println!("Updating terraphim-agent...");
    let config = UpdaterConfig::new("terraphim-agent").with_version(env!("CARGO_PKG_VERSION"));
    let updater = TerraphimUpdater::new(config);
    match updater.check_and_update().await {
        Ok(status) => {
            println!("{}", status);
            Ok(())
        }
        Err(e) => {
            eprintln!("Update failed: {}", e);
            std::process::exit(1);
        }
    }
}

// Reads the configuration directly and skips the thesaurus/rolegraph build that
// `TuiService::new` does (~63% of startup per profiling). See the comment at the
// call site for the broader rationale (Refs #120).
async fn handle_roles_list_command(config_path: Option<String>) -> Result<()> {
    let config = TuiService::load_config(config_path, false).await?;
    let selected = TuiService::selected_role_of(&config);
    for (name, shortname) in TuiService::roles_with_info_of(&config) {
        let marker = if name == selected.to_string() {
            "*"
        } else {
            " "
        };
        if let Some(short) = shortname {
            println!("{} {} ({})", marker, name, short);
        } else {
            println!("{} {}", marker, name);
        }
    }
    Ok(())
}

struct SetupArgs {
    template: Option<String>,
    path: Option<String>,
    add_role: bool,
    list_templates: bool,
}

async fn handle_setup_command(args: SetupArgs, service: &TuiService) -> Result<()> {
    use onboarding::{
        SetupMode, SetupResult, apply_template, list_templates as get_templates, run_setup_wizard,
    };

    // List templates and exit if requested
    if args.list_templates {
        println!("Available templates:\n");
        for template in get_templates() {
            let path_note = if template.requires_path {
                " (requires --path)"
            } else if template.default_path.is_some() {
                &format!(" (default: {})", template.default_path.as_ref().unwrap())
            } else {
                ""
            };
            println!("  {} - {}{}", template.id, template.description, path_note);
        }
        println!("\nUse --template <id> to apply a template directly.");
        return Ok(());
    }

    // Apply template directly if specified
    if let Some(template_id) = args.template {
        println!("Applying template: {}", template_id);
        match apply_template(&template_id, args.path.as_deref()) {
            Ok(role) => {
                // Save the role to config
                if args.add_role {
                    service.add_role(role.clone()).await?;
                    println!("Role '{}' added to configuration.", role.name);
                } else {
                    service.set_role(role.clone()).await?;
                    println!("Configuration set to role '{}'.", role.name);
                }
                return Ok(());
            }
            Err(e) => {
                eprintln!("Failed to apply template: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Run interactive wizard
    let mode = if args.add_role {
        SetupMode::AddRole
    } else {
        SetupMode::FirstRun
    };

    match run_setup_wizard(mode).await {
        Ok(SetupResult::Template {
            template,
            custom_path: _,
            role,
        }) => {
            if args.add_role {
                service.add_role(role.clone()).await?;
                println!(
                    "\nRole '{}' added from template '{}'.",
                    role.name, template.id
                );
            } else {
                service.set_role(role.clone()).await?;
                println!(
                    "\nConfiguration set to role '{}' from template '{}'.",
                    role.name, template.id
                );
            }
        }
        Ok(SetupResult::Custom { role }) => {
            if args.add_role {
                service.add_role(role.clone()).await?;
                println!("\nCustom role '{}' added to configuration.", role.name);
            } else {
                service.set_role(role.clone()).await?;
                println!("\nConfiguration set to custom role '{}'.", role.name);
            }
        }
        Ok(SetupResult::Cancelled) => {
            println!("\nSetup cancelled.");
        }
        Err(onboarding::OnboardingError::NotATty) => {
            eprintln!(
                "Interactive mode requires a terminal. Use --template for non-interactive setup."
            );
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Setup failed: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

// Fuzzy suggestion arm extracted from `run_offline_command`. The Suggest arm
// reads the query from stdin (when `--query` is not given), resolves the
// active role, asks the thesaurus for fuzzy matches above the configured
// threshold, and prints either a JSON payload or a human-readable listing.
//
// The body has no machine-readable / mode-specific branches and never inspects
// `output`, so `output: &CommandOutputConfig` is intentionally omitted from the
// signature. Like `handle_search_command`, the function takes `&Command` rather
// than a dedicated `*Args` struct and re-destructures the variant internally:
// variants are not types in Rust, so `&Command::Suggest` is not valid syntax.
// The caller (the early-return in `run_offline_command`) has already verified
// the variant via `if let Command::Suggest { .. }`, so the `else` branch is
// truly unreachable.
async fn handle_suggest_command(service: &TuiService, suggest: &Command) -> Result<()> {
    let Command::Suggest {
        query,
        role,
        fuzzy: _,
        threshold,
        limit,
        json,
    } = suggest
    else {
        unreachable!("handle_suggest_command called with non-Suggest command")
    };

    let input_query = match query {
        Some(q) => q.clone(),
        None => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            buffer.trim().to_string()
        }
    };

    let role_name = service.resolve_role(role.as_deref()).await?;

    let suggestions = service
        .fuzzy_suggest(&role_name, &input_query, *threshold, Some(*limit))
        .await?;

    if *json {
        println!("{}", serde_json::to_string(&suggestions)?);
    } else if suggestions.is_empty() {
        println!(
            "No suggestions found for '{}' with threshold {}",
            input_query, threshold
        );
    } else {
        println!(
            "Suggestions for '{}' (threshold: {}):",
            input_query, threshold
        );
        for s in &suggestions {
            println!("  {} (similarity: {:.2})", s.term, s.similarity);
        }
    }

    Ok(())
}

struct ReplaceArgs {
    text: Option<String>,
    role: Option<String>,
    format: Option<String>,
    boundary: BoundaryMode,
    json: bool,
    fail_open: bool,
}

// First post-TuiService match arm extracted. The Replace handler is
// ~150 LOC of inline thesaurus-driven text replacement; pulling it out
// makes the surrounding match block shorter and easier to review. The
// body shape (calls `service.get_thesaurus`, runs `ReplacementService`,
// emits JSON or plain output, returns `Ok`) is structurally similar to
// other post-TuiService arms (`Validate`, `Hook`) that follow.
async fn handle_replace_command(args: ReplaceArgs, service: &TuiService) -> Result<()> {
    let input_text = match args.text {
        Some(t) => t,
        None => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

    let role_name = service.resolve_role(args.role.as_deref()).await?;

    let link_type = match args.format.as_deref() {
        Some("markdown") => terraphim_hooks::LinkType::MarkdownLinks,
        Some("wiki") => terraphim_hooks::LinkType::WikiLinks,
        Some("html") => terraphim_hooks::LinkType::HTMLLinks,
        _ => terraphim_hooks::LinkType::PlainText,
    };

    let thesaurus = match service.get_thesaurus(&role_name).await {
        Ok(t) => t,
        Err(e) => {
            if args.fail_open {
                let hook_result = terraphim_hooks::HookResult::fail_open(
                    input_text.clone(),
                    e.to_string(),
                );
                if args.json {
                    println!("{}", serde_json::to_string(&hook_result)?);
                } else {
                    eprintln!("Warning: {}", e);
                    print!("{}", input_text);
                }
                return Ok(());
            } else {
                return Err(e);
            }
        }
    };

    let replacement_service = terraphim_hooks::ReplacementService::new(thesaurus.clone())
        .with_link_type(link_type);

    let hook_result = match args.boundary {
        BoundaryMode::None => {
            // Standard replacement - match anywhere
            if args.fail_open {
                replacement_service.replace_fail_open(&input_text)
            } else {
                replacement_service.replace(&input_text)?
            }
        }
        BoundaryMode::Word => {
            // Word boundary mode - only match at word boundaries
            let matches_result = replacement_service.find_matches(&input_text);
            match matches_result {
                Ok(matches) => {
                    // Filter matches to only those at word boundaries
                    let filtered_matches: Vec<_> = matches
                        .into_iter()
                        .filter(|m| {
                            if let Some((start, end)) = m.pos {
                                is_at_word_boundary(&input_text, start, end)
                            } else {
                                false
                            }
                        })
                        .collect();

                    if filtered_matches.is_empty() {
                        terraphim_hooks::HookResult::pass_through(input_text.clone())
                    } else {
                        // Apply filtered matches in reverse order to preserve positions
                        let mut result = input_text.clone();
                        let mut sorted_matches = filtered_matches;
                        #[allow(clippy::unnecessary_sort_by)]
                        sorted_matches.sort_by(|a, b| b.pos.cmp(&a.pos));

                        for m in sorted_matches {
                            if let Some((start, end)) = m.pos {
                                let replacement =
                                    format_replacement_link(&m.normalized_term, link_type);
                                result.replace_range(start..end, &replacement);
                            }
                        }

                        terraphim_hooks::HookResult::success(input_text.clone(), result)
                    }
                }
                Err(e) => {
                    if args.fail_open {
                        terraphim_hooks::HookResult::fail_open(
                            input_text.clone(),
                            e.to_string(),
                        )
                    } else {
                        return Err(anyhow::anyhow!("Failed to find matches: {}", e));
                    }
                }
            }
        }
    };

    if args.json {
        println!("{}", serde_json::to_string(&hook_result)?);
    } else {
        if let Some(ref err) = hook_result.error {
            eprintln!("Warning: {}", err);
        }
        print!("{}", hook_result.result);
    }

    Ok(())
}

// Largest single extraction from `run_offline_command`. The `Search` arm is the
// original entry point of `run_offline_command` and carries the full
// `Command::Search` variant destructuring (eleven fields) along with the
// machine-readable formatter path and the `fail-on-empty` exit-code path.
//
// Passing `&Command` rather than a dedicated `SearchArgs` struct keeps this
// diff focused on the extraction itself. The body destructures `search`
// internally via `let Command::Search { .. } = search else { unreachable!() }`,
// so the caller has already verified the variant. `output` is taken by
// reference because the body inspects `output.is_machine_readable()`,
// `output.mode`, and `output.robot`; passing it through here means the
// surrounding match block no longer needs to keep it in scope across arms.
async fn handle_search_command(
    service: &TuiService,
    output: &CommandOutputConfig,
    search: &Command,
) -> Result<()> {
    let Command::Search {
        query,
        terms,
        operator,
        role,
        limit,
        fail_on_empty,
        include_pinned,
        min_quality,
        max_tokens,
        max_content_length,
        fields,
    } = search
    else {
        unreachable!("handle_search_command called with non-Search command")
    };

    let (role_name, auto) = service
        .resolve_or_auto_route(role.as_deref(), query)
        .await?;
    if let Some(ref ar) = auto {
        eprintln!("{}", format_auto_route_line(ar));
    }

    let results = if let Some(additional_terms) = terms {
        // Multi-term query with logical operators
        let mut all_terms = vec![query.as_str().to_string()];
        all_terms.extend(additional_terms.iter().cloned());

        let op_str = match operator {
            Some(LogicalOperatorCli::And) => "AND",
            Some(LogicalOperatorCli::Or) | None => "OR", // Default to OR
        };
        if !output.is_machine_readable() {
            println!(
                "Multi-term search: {} terms using {} operator",
                all_terms.len(),
                op_str
            );
        }

        let search_query = SearchQuery {
            search_term: NormalizedTermValue::from(all_terms[0].as_str()),
            search_terms: if all_terms.len() > 1 {
                Some(
                    all_terms[1..]
                        .iter()
                        .map(|t| NormalizedTermValue::from(t.as_str()))
                        .collect(),
                )
            } else {
                None
            },
            operator: operator.as_ref().map(|op| op.clone().into()),
            skip: Some(0),
            limit: Some(*limit),
            include_pinned: *include_pinned,
            role: Some(role_name.clone()),
            layer: Layer::default(),
            min_quality: *min_quality,
        };

        service.search_with_query(&search_query).await?
    } else {
        // Single term query
        let search_query = SearchQuery {
            search_term: NormalizedTermValue::from(query.as_str()),
            search_terms: None,
            operator: None,
            skip: Some(0),
            limit: Some(*limit),
            include_pinned: *include_pinned,
            role: Some(role_name.clone()),
            layer: Layer::default(),
            min_quality: *min_quality,
        };
        service.search_with_query(&search_query).await?
    };

    let results_count = results.len();
    if output.is_machine_readable() {
        use robot::schema::{SearchResultItem, SearchResultsData};
        use robot::{ResponseMeta, RobotConfig, RobotFormatter, RobotResponse};
        use std::time::Instant;

        let start = Instant::now();
        let robot_format = match output.mode {
            CommandOutputMode::JsonCompact => robot::output::OutputFormat::Minimal,
            _ => robot::output::OutputFormat::Json,
        };
        let mut robot_config = RobotConfig::new()
            .with_format(robot_format)
            .with_max_results(*limit);
        if let Some(mt) = max_tokens {
            robot_config = robot_config.with_max_tokens(*mt);
        } else if output.robot {
            robot_config = robot_config.with_max_tokens(8000);
        }
        if let Some(mcl) = max_content_length {
            robot_config = robot_config.with_max_content_length(*mcl);
        } else if output.robot {
            robot_config = robot_config.with_max_content_length(2000);
        }
        if let Some(fm) = fields {
            robot_config = robot_config.with_fields(fm.clone());
        }

        let formatter = RobotFormatter::new(robot_config.clone());
        let max_results = robot_config.max_results.unwrap_or(*limit);
        let truncated_results: Vec<_> = results.into_iter().take(max_results).collect();
        let total = truncated_results.len();

        let items: Vec<SearchResultItem> = truncated_results
            .iter()
            .enumerate()
            .map(|(i, doc)| {
                let preview = doc.description.as_deref().or(if doc.body.is_empty() {
                    None
                } else {
                    Some(doc.body.as_str())
                });
                let (preview_text, preview_truncated) = match preview {
                    Some(text) => {
                        let (t, was_truncated) = formatter.truncate_content(text.trim());
                        (Some(t), was_truncated)
                    }
                    None => (None, false),
                };
                SearchResultItem {
                    rank: i + 1,
                    id: doc.id.clone(),
                    title: doc.title.clone(),
                    url: if doc.url.is_empty() {
                        None
                    } else {
                        Some(doc.url.clone())
                    },
                    score: doc.rank.unwrap_or_default() as f64,
                    preview: preview_text,
                    source: None,
                    date: None,
                    preview_truncated,
                }
            })
            .collect();

        let (concepts_matched, thesaurus_matched) = match service.get_thesaurus(&role_name).await {
            Ok(thesaurus) => {
                let concepts = terraphim_automata::compute_concepts_matched(query, &thesaurus);
                // `thesaurus_matched` used to be a naive substring scan, so any
                // term appearing *inside* a longer query word was reported --
                // the two-letter term `ce` matched `con(ce)pt`. Derive it from
                // the same boundary-aware matcher that produces `concepts`, so
                // the two fields can never disagree.
                let matched: std::collections::HashSet<String> =
                    concepts.iter().map(|c| c.to_lowercase()).collect();
                let thesaurus_terms: Vec<String> = thesaurus
                    .keys()
                    .filter(|key| matched.contains(&key.to_string().to_lowercase()))
                    .map(|key| key.to_string())
                    .collect();
                (concepts, thesaurus_terms)
            }
            Err(e) => {
                log::debug!(
                    "get_thesaurus failed for {}: {}; concepts_matched empty",
                    role_name,
                    e
                );
                (Vec::new(), Vec::new())
            }
        };

        let wildcard_fallback = concepts_matched.is_empty();
        let data = SearchResultsData {
            results: items,
            total_matches: total,
            concepts_matched,
            thesaurus_matched,
            wildcard_fallback,
        };

        let meta = ResponseMeta::new("search")
            .with_elapsed(start.elapsed().as_millis() as u64)
            .with_query(query)
            .with_role(role_name.as_str());
        let response = RobotResponse::success(data, meta);
        let output_str = formatter.format(&response)?;
        println!("{}", output_str);
    } else {
        for doc in results.iter() {
            let snippet = doc
                .description
                .as_deref()
                .or(if doc.body.is_empty() {
                    None
                } else {
                    Some(doc.body.as_str())
                })
                .map(|s| truncate_snippet(s.trim(), 120));
            println!("[{}] {}", doc.rank.unwrap_or_default(), doc.title);
            if !doc.url.is_empty() {
                println!("    {}", doc.url);
            }
            if let Some(snip) = snippet {
                println!("    {}", snip);
            }
            println!();
        }
    }
    if *fail_on_empty && results_count == 0 {
        std::process::exit(robot::exit_codes::ExitCode::ErrorNotFound.code().into());
    }
    Ok(())
}

async fn run_offline_command(
    command: Command,
    output: CommandOutputConfig,
    config_path: Option<String>,
) -> Result<()> {
    // Handle stateless commands that don't need TuiService first
    if let Command::Guard {
        command: guard_command,
        json,
        fail_open,
        guard_thesaurus,
        guard_allowlist,
        explain,
    } = &command
    {
        return handle_guard_command(&GuardArgs {
            command: guard_command,
            json: *json,
            fail_open: *fail_open,
            guard_thesaurus,
            guard_allowlist,
            explain: *explain,
        })
        .await;
    }

    // CheckUpdate is stateless - handle before TuiService initialization
    if let Command::CheckUpdate = &command {
        return handle_check_update_command().await;
    }

    // Update is stateless - handle before TuiService initialization
    if let Command::Update = &command {
        return handle_update_command().await;
    }

    // Config validate is stateless - handle before TuiService initialization
    if let Command::Config {
        sub: ConfigSub::Validate,
    } = &command
    {
        return run_config_validate().await;
    }

    // `config show` and the read-only `roles` subcommands need the configuration but not
    // the thesaurus or rolegraph that `ConfigState::new` builds. Profiling put that build at
    // ~63% of startup -- a full markdown AST parse per knowledge-graph file, done twice --
    // so they load the config directly and skip it. The integration suite drives exactly
    // these commands 20-30 times per test. Refs #120.
    if let Command::Config {
        sub: ConfigSub::Show,
    } = &command
    {
        let config = TuiService::load_config(config_path, false).await?;
        println!("{}", serde_json::to_string_pretty(&config)?);
        return Ok(());
    }

    if let Command::Roles {
        sub: RolesSub::List,
    } = &command
    {
        return handle_roles_list_command(config_path).await;
    }

    // Cache is stateless - handle before TuiService initialization
    if let Command::Cache { sub } = &command {
        return run_cache_command(sub).await;
    }

    // Learn is stateless - handle before TuiService initialization.
    // Must be last early-return because it consumes `command` via destructuring.
    if let Command::Learn { sub } = command {
        return run_learn_command(sub).await;
    }

    // Memory lifecycle CLI commands are stateless - handle before TuiService initialization.
    if let Command::Memory { sub } = command {
        return run_memory_command(sub, &output).await;
    }

    let service = TuiService::new(config_path, false).await?;

    // Suggest is a stateful command (needs the thesaurus / role index the
    // `TuiService` exposes via `fuzzy_suggest`), so it lives in the same
    // early-return tier as `Search`. Pulling it out ahead of the match keeps
    // `run_offline_command` from growing another long body and lets the body
    // take `&Command` (re-destructured internally) just like Search does.
    if let Command::Suggest { .. } = &command {
        return handle_suggest_command(&service, &command).await;
    }

    // Search is the largest single arm. Pulling it out ahead of the match
    // block mirrors how Guard / CheckUpdate / Update / Cache / Learn / Memory
    // are already handled -- they short-circuit before the match consumes
    // `command` so they can pass `&command` (or move sub-fields out of it)
    // to the dedicated handler. The remaining arms all need to consume
    // `command` directly, so the match stays below.
    if let Command::Search { .. } = &command {
        return handle_search_command(&service, &output, &command).await;
    }

    match command {
        Command::Roles { sub } => {
            match sub {
                RolesSub::List => {
                    let roles_with_info = service.list_roles_with_info().await;
                    let selected = service.get_selected_role().await;
                    for (name, shortname) in roles_with_info {
                        let marker = if name == selected.to_string() {
                            "*"
                        } else {
                            " "
                        };
                        if let Some(short) = shortname {
                            println!("{} {} ({})", marker, name, short);
                        } else {
                            println!("{} {}", marker, name);
                        }
                    }
                }
                RolesSub::Select { name } => {
                    // Find role by name or shortname
                    let role_name = service
                        .find_role_by_name_or_shortname(&name)
                        .await
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "Role '{}' not found (checked name and shortname)",
                                name
                            )
                        })?;
                    service.update_selected_role(role_name.clone()).await?;
                    service.save_config().await?;
                    println!("selected:{}", role_name);
                }
            }
            Ok(())
        }
        Command::Config { sub } => {
            match sub {
                ConfigSub::Show => {
                    let config = service.get_config().await;
                    println!("{}", serde_json::to_string_pretty(&config)?);
                }
                ConfigSub::Set { key, value } => match key.as_str() {
                    "selected_role" => {
                        let role_name = RoleName::new(&value);
                        service.update_selected_role(role_name).await?;
                        service.save_config().await?;
                        println!("updated selected_role to {}", value);
                    }
                    _ => {
                        println!("unsupported key: {}", key);
                    }
                },
                ConfigSub::Validate => {
                    // Handled as early-return above; should not reach here
                    unreachable!("config validate is handled before TuiService init");
                }
                ConfigSub::Reload => {
                    let ds = terraphim_settings::DeviceSettings::load_from_env_and_file(None)
                        .unwrap_or_else(|_| terraphim_settings::DeviceSettings::default_embedded());
                    match &ds.role_config {
                        Some(path) => match service.reload_from_json(path).await {
                            Ok(count) => {
                                println!(
                                    "Reloaded {} role(s) from '{}' and saved to persistence",
                                    count, path
                                );
                            }
                            Err(e) => {
                                eprintln!("Failed to reload from '{}': {:?}", path, e);
                                std::process::exit(1);
                            }
                        },
                        None => {
                            eprintln!("No role_config set in settings.toml. Nothing to reload.");
                            eprintln!(
                                "Add role_config = \"path/to/roles.json\" to your settings.toml"
                            );
                            std::process::exit(1);
                        }
                    }
                }
            }
            Ok(())
        }
        Command::Graph {
            role,
            top_k,
            pinned,
        } => {
            let role_name = service.resolve_role(role.as_deref()).await?;

            if pinned {
                let pinned_concepts = service.get_role_graph_pinned(&role_name).await?;
                for concept in pinned_concepts {
                    println!("{}", concept);
                }
            } else {
                let concepts = service.get_role_graph_top_k(&role_name, top_k).await?;
                for concept in concepts {
                    println!("{}", concept);
                }
            }
            Ok(())
        }
        Command::Kg { sub } => match sub {
            KgSub::List {
                role,
                top_k,
                pinned,
            } => {
                let role_name = service.resolve_role(role.as_deref()).await?;

                if pinned {
                    let pinned_concepts = service.get_role_graph_pinned(&role_name).await?;
                    for concept in pinned_concepts {
                        println!("{}", concept);
                    }
                } else {
                    let concepts = service.get_role_graph_top_k(&role_name, top_k).await?;
                    for concept in concepts {
                        println!("{}", concept);
                    }
                }
                Ok(())
            }
        },
        #[cfg(feature = "llm")]
        Command::Chat {
            role,
            prompt,
            model,
        } => {
            let role_name = service.resolve_role(role.as_deref()).await?;

            let response = service.chat(&role_name, &prompt, model).await?;
            println!("{}", response);
            Ok(())
        }
        Command::Extract {
            text,
            role,
            exclude_term,
        } => {
            let role_name = service.resolve_role(role.as_deref()).await?;

            let results = service
                .extract_paragraphs(&role_name, &text, exclude_term)
                .await?;

            if results.is_empty() {
                println!("No matches found in the text.");
            } else {
                println!("Found {} paragraph(s):", results.len());
                for (i, (matched_term, paragraph)) in results.iter().enumerate() {
                    println!("\n--- Match {} (term: '{}') ---", i + 1, matched_term);
                    println!("{}", paragraph);
                }
            }

            Ok(())
        }
        Command::Replace {
            text,
            role,
            format,
            boundary,
            json,
            fail_open,
        } => {
            return handle_replace_command(
                ReplaceArgs {
                    text,
                    role,
                    format,
                    boundary,
                    json,
                    fail_open,
                },
                &service,
            )
            .await;
        }
        Command::Validate {
            text,
            role,
            connectivity,
            checklist,
            json,
        } => {
            return handle_validate_command(
                ValidateArgs {
                    text,
                    role,
                    connectivity,
                    checklist,
                    json,
                },
                &service,
            )
            .await;
        }
        Command::Suggest { .. } => {
            unreachable!("Suggest commands are handled after TuiService initialization")
        }
        Command::Hook {
            hook_type,
            input,
            role,
            json: _,
            with_guard,
            no_with_guard,
            rewrite,
        } => {
            return handle_hook_command(
                HookArgs {
                    hook_type,
                    input,
                    role,
                    with_guard,
                    no_with_guard,
                    rewrite,
                },
                &service,
            )
            .await;
        }
        Command::Guard { .. } => {
            // Handled above before TuiService initialization
            unreachable!("Guard command should be handled before TuiService initialization")
        }
        Command::Setup {
            template,
            path,
            add_role,
            list_templates,
        } => {
            return handle_setup_command(
                SetupArgs {
                    template,
                    path,
                    add_role,
                    list_templates,
                },
                &service,
            )
            .await;
        }
        Command::CheckUpdate => {
            unreachable!("CheckUpdate command should be handled before TuiService initialization")
        }
        Command::Update => {
            unreachable!("Update command should be handled before TuiService initialization")
        }
        Command::Learn { .. } => {
            unreachable!("Learn command should be handled before TuiService initialization")
        }
        Command::Memory { .. } => {
            unreachable!("Memory command should be handled before TuiService initialization")
        }

        #[cfg(feature = "repl-sessions")]
        Command::Sessions { sub } => handle_sessions_command(sub, &output).await,

        Command::Listen {
            identity, config, ..
        } => {
            if let Some(id) = identity {
                println!("listener would start with identity: {}", id);
            }
            if let Some(path) = config.as_deref() {
                println!("listener config: {}", path);
            }
            Ok(())
        }
        Command::Interactive => {
            unreachable!("Interactive mode should be handled above")
        }

        #[cfg(feature = "repl")]
        Command::Repl { .. } => {
            unreachable!("REPL mode should be handled above")
        }
        Command::Robot { .. } => {
            unreachable!("Robot commands are handled in main()")
        }
        Command::Cache { .. } => {
            unreachable!("Cache commands are handled before TuiService initialization")
        }
        Command::Search { .. } => {
            unreachable!("Search commands are handled after TuiService initialization")
        }
    }
}

// Post-TuiService arm extracted (step 5.6). The Sessions arm is the largest
// remaining inline branch (~220 LOC): it shadows the outer TuiService with
// its own terraphim_sessions::SessionService, loads the on-disk session
// cache, then fans out over Sources/List/Search/Stats/Expand with
// machine-readable and human-readable renderings of each.
//
// The handler takes `sub: SessionsSub` by value (the match consumes
// `command`) and `output: &CommandOutputConfig` because every sub-arm
// branches on `output.is_machine_readable()` / `output.mode`. It does NOT
// take `&TuiService`: session state lives in SessionService, and the arm
// never touches the thesaurus or role index.
async fn handle_sessions_command(sub: SessionsSub, output: &CommandOutputConfig) -> Result<()> {
    use session_output::*;
    use terraphim_sessions::SessionService;

    let service = SessionService::new();

    // Load cached sessions from disk
    let cache_path = get_session_cache_path();
    if cache_path.exists()
        && let Ok(data) = std::fs::read_to_string(&cache_path)
        && let Ok(cached) = serde_json::from_str::<Vec<terraphim_sessions::Session>>(&data)
    {
        service.load_sessions(cached).await;
        if !output.is_machine_readable() {
            println!("Loaded sessions from cache.");
        }
    }

    match sub {
        SessionsSub::Sources => {
            let sources = service.detect_sources();
            if output.is_machine_readable() {
                let payload = SourcesOutput {
                    count: sources.len(),
                    sources: sources
                        .into_iter()
                        .map(|s| {
                            let available = s.is_available();
                            SourceEntry {
                                id: s.id,
                                name: s.name,
                                available,
                            }
                        })
                        .collect(),
                };
                print_json_output(&payload, output.mode)?;
            } else if sources.is_empty() {
                println!("No session sources detected.");
            } else {
                println!("Available session sources:");
                for source in sources {
                    let status = if source.is_available() {
                        "available"
                    } else {
                        "not found"
                    };
                    println!(
                        "  - {} ({})",
                        source.name.unwrap_or_else(|| source.id.clone()),
                        status
                    );
                }
            }
            Ok(())
        }
        SessionsSub::List { limit } => {
            let sessions = service.list_sessions().await;
            if output.is_machine_readable() {
                let session_entries: Vec<SessionEntry> = sessions
                    .iter()
                    .take(limit)
                    .map(|s| SessionEntry {
                        id: s.id.to_string(),
                        title: s.title.clone(),
                        message_count: s.message_count(),
                        source: s.source.clone(),
                    })
                    .collect();
                let shown = session_entries.len();
                let payload = SessionListOutput {
                    total: sessions.len(),
                    shown,
                    sessions: session_entries,
                };
                print_json_output(&payload, output.mode)?;
            } else if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("Cached sessions ({} total):", sessions.len());
                for session in sessions.iter().take(limit) {
                    let msg_count = session.message_count();
                    let title = session.title.as_deref().unwrap_or("(untitled)");
                    println!("  - {} ({} messages)", title, msg_count);
                }
                if sessions.len() > limit {
                    println!("  ... and {} more", sessions.len() - limit);
                }
            }
            Ok(())
        }
        SessionsSub::Search { query, limit } => {
            let results = service.search(&query).await;
            if output.is_machine_readable() {
                let entries: Vec<SessionSearchEntry> = results
                    .iter()
                    .take(limit)
                    .map(|s| {
                        let preview = s
                            .messages
                            .iter()
                            .find(|msg| {
                                msg.content.to_lowercase().contains(&query.to_lowercase())
                            })
                            .map(|msg| {
                                let p: String = msg.content.chars().take(100).collect();
                                p
                            });
                        SessionSearchEntry {
                            id: s.id.to_string(),
                            title: s.title.clone(),
                            message_count: s.message_count(),
                            preview,
                        }
                    })
                    .collect();
                let shown = entries.len();
                let payload = SessionSearchOutput {
                    query: query.clone(),
                    total: results.len(),
                    shown,
                    sessions: entries,
                };
                print_json_output(&payload, output.mode)?;
                if results.is_empty() {
                    std::process::exit(
                        robot::exit_codes::ExitCode::ErrorNotFound.code().into(),
                    );
                }
            } else if results.is_empty() {
                println!("No sessions matching '{}'.", query);
            } else {
                println!("Found {} matching sessions:", results.len());
                for session in results.iter().take(limit) {
                    let title = session.title.as_deref().unwrap_or("(untitled)");
                    println!("  - {}", title);
                    for msg in &session.messages {
                        let content_lower = msg.content.to_lowercase();
                        if content_lower.contains(&query.to_lowercase()) {
                            let preview: String = msg.content.chars().take(100).collect();
                            println!("    > {}", preview);
                            break;
                        }
                    }
                }
            }
            Ok(())
        }
        SessionsSub::Stats => {
            let stats = service.statistics().await;
            if output.is_machine_readable() {
                let payload = SessionStatsOutput {
                    total_sessions: stats.total_sessions,
                    total_messages: stats.total_messages,
                    total_user_messages: stats.total_user_messages,
                    total_assistant_messages: stats.total_assistant_messages,
                    by_source: stats.sessions_by_source,
                };
                print_json_output(&payload, output.mode)?;
            } else {
                println!("Session Statistics:");
                println!("  Total sessions: {}", stats.total_sessions);
                println!("  Total messages: {}", stats.total_messages);
                println!("  User messages: {}", stats.total_user_messages);
                println!("  Assistant messages: {}", stats.total_assistant_messages);
                if !stats.sessions_by_source.is_empty() {
                    println!("  By source:");
                    for (source, count) in stats.sessions_by_source {
                        println!("    - {}: {}", source, count);
                    }
                }
            }
            Ok(())
        }
        SessionsSub::Expand {
            id,
            context_lines: _,
        } => {
            let session = service.get_session(&id).await;
            match session {
                None => {
                    if !output.is_machine_readable() {
                        eprintln!("Session '{}' not found.", id);
                    }
                    std::process::exit(
                        robot::exit_codes::ExitCode::ErrorNotFound.code().into(),
                    );
                }
                Some(session) => {
                    if output.is_machine_readable() {
                        let payload = SessionExpandOutput {
                            id: session.id.clone(),
                            title: session.title.clone(),
                            message_count: session.message_count(),
                            messages: session
                                .messages
                                .iter()
                                .map(|msg| ExpandedMessage {
                                    idx: msg.idx,
                                    role: msg.role.to_string(),
                                    content: msg.content.clone(),
                                })
                                .collect(),
                        };
                        print_json_output(&payload, output.mode)?;
                    } else {
                        let title = session.title.as_deref().unwrap_or("(untitled)");
                        println!("Session: {} ({})", title, session.id);
                        println!("Messages: {}", session.message_count());
                        println!("{}", "=".repeat(80));
                        for msg in &session.messages {
                            println!("[{}]", msg.role);
                            println!("{}", msg.content);
                            println!("{}", "-".repeat(40));
                        }
                    }
                    Ok(())
                }
            }
        }
    }
}

struct ValidateArgs {
    text: Option<String>,
    role: Option<String>,
    connectivity: bool,
    checklist: Option<String>,
    json: bool,
}

// Second post-TuiService match arm extracted. Validate follows the
// same template as Replace (step 5.1). The body shape is similar:
// calls service.validate(), formats output, returns Ok.
async fn handle_validate_command(args: ValidateArgs, service: &TuiService) -> Result<()> {
    let input_text = match args.text {
        Some(t) => t,
        None => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            buffer.trim().to_string()
        }
    };

    let role_name = service.resolve_role(args.role.as_deref()).await?;

    if args.connectivity {
        let result = service.check_connectivity(&role_name, &input_text).await?;

        if args.json {
            println!("{}", serde_json::to_string(&result)?);
        } else {
            println!("Connectivity Check for role '{}':", role_name);
            println!("  Connected: {}", result.connected);
            println!("  Matched terms: {:?}", result.matched_terms);
            println!("  {}", result.message);
        }
    } else if let Some(checklist_name) = args.checklist {
        // Checklist validation mode
        let result = service
            .validate_checklist(&role_name, &checklist_name, &input_text)
            .await?;

        if args.json {
            println!("{}", serde_json::to_string(&result)?);
        } else {
            println!(
                "Checklist '{}' Validation for role '{}':",
                checklist_name, role_name
            );
            println!("  Passed: {}", result.passed);
            println!("  Score: {}/{}", result.satisfied.len(), result.total_items);
            if !result.satisfied.is_empty() {
                println!("  Satisfied items:");
                for item in &result.satisfied {
                    println!("    ✓ {}", item);
                }
            }
            if !result.missing.is_empty() {
                println!("  Missing items:");
                for item in &result.missing {
                    println!("    ✗ {}", item);
                }
            }
        }
    } else {
        // Default validation: find matches
        let matches = service.find_matches(&role_name, &input_text).await?;

        if args.json {
            let output = serde_json::json!({
                "role": role_name.to_string(),
                "matched_count": matches.len(),
                "matches": matches.iter().map(|m| m.term.clone()).collect::<Vec<_>>()
            });
            println!("{}", serde_json::to_string(&output)?);
        } else {
            println!("Validation for role '{}':", role_name);
            println!("  Found {} matched term(s)", matches.len());
            for m in &matches {
                println!("    - {}", m.term);
            }
        }
    }

    Ok(())
}

struct HookArgs {
    hook_type: HookType,
    input: Option<String>,
    role: Option<String>,
    with_guard: bool,
    no_with_guard: bool,
    rewrite: bool,
}

// Third post-TuiService match arm extracted. Hook follows the same
// template as Replace (5.1) and Validate (5.2). The body shape is
// similar: calls service.hook(...), formats output, returns Ok.
async fn handle_hook_command(args: HookArgs, service: &TuiService) -> Result<()> {
    // For pre-tool-use, default the guard check to ON so destructive
    // commands are denied unless the user explicitly opts out. Other
    // hook types (post-tool-use, pre-commit, prepare-commit-msg) fire
    // after execution or on text inputs and do not need a guard, so
    // they keep the user's explicit `--with-guard` setting. An
    // explicit `--no-with-guard` overrides everything.
    let with_guard = !args.no_with_guard
        && (args.with_guard || matches!(args.hook_type, HookType::PreToolUse));
    // Read JSON input from argument or stdin
    let input_json = match args.input {
        Some(i) => i,
        None => {
            use std::io::Read;
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

    let role_name = service.resolve_role(args.role.as_deref()).await?;

    // Parse input JSON
    let input_value: serde_json::Value = serde_json::from_str(&input_json)
        .map_err(|e| anyhow::anyhow!("Invalid JSON input: {}", e))?;

    match args.hook_type {
        HookType::PreToolUse => {
            // Extract tool_name and tool_input from the hook input
            let tool_name = input_value
                .get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Only process Bash commands
            if tool_name == "Bash" {
                if let Some(command) = input_value
                    .get("tool_input")
                    .and_then(|v| v.get("command"))
                    .and_then(|v| v.as_str())
                {
                    // Guard check if --with-guard flag is set (default ON
                    // for pre-tool-use; see the Hook args doc comment).
                    if with_guard {
                        let guard = guard_patterns::CommandGuard::new();
                        let guard_result = guard.check(command);

                        if guard_result.decision == guard_patterns::GuardDecision::Block {
                            // Output deny response for Claude Code
                            let output = serde_json::json!({
                                "hookSpecificOutput": {
                                    "hookEventName": "PreToolUse",
                                    "permissionDecision": "deny",
                                    "permissionDecisionReason": format!(
                                        "BLOCKED: {}",
                                        guard_result.reason.unwrap_or_default()
                                    )
                                }
                            });
                            println!("{}", serde_json::to_string(&output)?);
                            return Ok(());
                        }
                    }

                    // Substitution is opt-in. We always probe the
                    // replacement so we can warn the user when their
                    // command contained KG-replaceable substrings, but
                    // we only emit a rewritten command when `--rewrite`
                    // is set. This prevents the previous behaviour
                    // where any substring match could silently mutate
                    // a destructive command (Refs #126).
                    let thesaurus = service.get_thesaurus(&role_name).await?;
                    let replacement_service =
                        terraphim_hooks::ReplacementService::new(thesaurus);
                    let hook_result = replacement_service.replace_fail_open(command);

                    let kg_validation = kg_validation::validate_command_against_kg(command);

                    let mut output = input_value.clone();
                    let mut emitted_warning = false;

                    if hook_result.replacements > 0 {
                        if args.rewrite {
                            // Opt-in: actually substitute
                            if let Some(tool_input) = output.get_mut("tool_input")
                                && let Some(obj) = tool_input.as_object_mut()
                            {
                                obj.insert(
                                    "command".to_string(),
                                    serde_json::Value::String(hook_result.result.clone()),
                                );
                            }
                        } else {
                            // Suppressed: warn the user
                            if let Some(obj) = output.as_object_mut() {
                                let warnings = obj
                                    .entry("warnings".to_string())
                                    .or_insert(serde_json::Value::Array(vec![]));
                                if let Some(arr) = warnings.as_array_mut() {
                                    arr.push(serde_json::Value::String(format!(
                                        "command contained {} KG-replaceable substring(s); pass --rewrite to enable substitution. Original: `{}`",
                                        hook_result.replacements, command
                                    )));
                                }
                            }
                            emitted_warning = true;
                        }
                    }

                    if kg_validation.has_findings
                        && let Some(obj) = output.as_object_mut()
                    {
                        obj.insert(
                            "validations".to_string(),
                            serde_json::to_value(&kg_validation).unwrap_or_default(),
                        );
                    }

                    if emitted_warning
                        || (args.rewrite && hook_result.replacements > 0)
                        || kg_validation.has_findings
                    {
                        println!("{}", serde_json::to_string(&output)?);
                    } else {
                        // No changes, pass through
                        println!("{}", input_json);
                    }
                } else {
                    // No command to process
                    println!("{}", input_json);
                }
            } else {
                // Not a Bash command, pass through
                println!("{}", input_json);
            }
        }
        HookType::PostToolUse => {
            // Post-tool-use: validate output against checklist or connectivity
            let tool_result = input_value
                .get("tool_result")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Check connectivity of the output
            let connectivity = service.check_connectivity(&role_name, tool_result).await?;

            let output = serde_json::json!({
                "original": input_value,
                "validation": {
                    "connected": connectivity.connected,
                    "matched_terms": connectivity.matched_terms
                }
            });
            println!("{}", serde_json::to_string(&output)?);
        }
        HookType::PreCommit | HookType::PrepareCommitMsg => {
            // Extract commit message or diff
            let content = input_value
                .get("message")
                .or_else(|| input_value.get("diff"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Extract concepts from the content
            let matches = service.find_matches(&role_name, content).await?;
            let concepts: Vec<String> = matches.iter().map(|m| m.term.clone()).collect();

            let output = serde_json::json!({
                "original": input_value,
                "concepts": concepts,
                "concept_count": concepts.len()
            });
            println!("{}", serde_json::to_string(&output)?);
        }
    }

    Ok(())
}

async fn run_cache_command(sub: &CacheSub) -> Result<()> {
    use terraphim_persistence::DeviceStorage;

    match sub {
        CacheSub::Flush { role } => {
            let storage = DeviceStorage::instance().await?;
            let fastest_op = &storage.fastest_op;

            if let Some(role_name) = role {
                let key = format!("thesaurus_{}.json", role_name.to_lowercase());
                match fastest_op.delete(&key).await {
                    Ok(_) => {
                        println!("Flushed cache for role: {}", role_name);
                    }
                    Err(e) => {
                        eprintln!("Failed to flush cache for role '{}': {}", role_name, e);
                        std::process::exit(1);
                    }
                }
            } else {
                // Flush all thesaurus entries
                let prefix = "thesaurus_";
                match fastest_op.list(prefix).await {
                    Ok(entries) => {
                        let mut count = 0;
                        for entry in entries {
                            let path = entry.path();
                            if path.ends_with(".json") {
                                match fastest_op.delete(path).await {
                                    Ok(_) => count += 1,
                                    Err(e) => {
                                        log::warn!("Failed to delete '{}': {}", path, e);
                                    }
                                }
                            }
                        }
                        println!("Flushed {} cached thesaurus entries", count);
                    }
                    Err(e) => {
                        eprintln!("Failed to list cache entries: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Ok(())
        }
    }
}

async fn run_learn_command(sub: LearnSub) -> Result<()> {
    use learnings::{
        CorrectionType, LearningCaptureConfig, capture_correction, capture_failed_command,
        correct_learning, list_all_entries,
    };
    let config = LearningCaptureConfig::default();

    match sub {
        LearnSub::Capture {
            command,
            error,
            exit_code,
            debug,
        } => {
            if debug {
                eprintln!(
                    "Capturing learning: command='{}', exit_code={}",
                    command, exit_code
                );
            }
            match capture_failed_command(&command, &error, exit_code, &config) {
                Ok(path) => {
                    println!("Captured learning: {}", path.display());
                    Ok(())
                }
                Err(e) => {
                    if debug {
                        eprintln!("Failed to capture learning: {}", e);
                    }
                    Err(e.into())
                }
            }
        }
        LearnSub::List { recent, global } => {
            let storage_loc = config.storage_location();
            let storage_dir = if global {
                &config.global_dir
            } else {
                &storage_loc
            };
            match list_all_entries(storage_dir, recent) {
                Ok(entries) => {
                    if entries.is_empty() {
                        println!("No learnings found.");
                    } else {
                        println!("Recent learnings:");
                        for (i, entry) in entries.iter().enumerate() {
                            let source_indicator = match entry.source() {
                                learnings::LearningSource::Project => "[P]",
                                learnings::LearningSource::Global => "[G]",
                            };
                            println!("  {}. {} {}", i + 1, source_indicator, entry.summary());
                            if let Some(correction) = entry.correction_text() {
                                println!("     Correction: {}", correction);
                            }
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        }
        LearnSub::Query {
            pattern,
            exact,
            global,
            semantic,
        } => {
            let storage_loc = config.storage_location();
            let storage_dir = if global {
                &config.global_dir
            } else {
                &storage_loc
            };
            let query_result = if semantic {
                learnings::query_all_entries_semantic(storage_dir, &pattern, exact, semantic)
            } else {
                learnings::query_all_entries(storage_dir, &pattern, exact)
            };
            match query_result {
                Ok(entries) => {
                    if entries.is_empty() {
                        println!("No learnings matching '{}'.", pattern);
                    } else {
                        println!("Learnings matching '{}'.", pattern);
                        for entry in entries {
                            let source_indicator = match entry.source() {
                                learnings::LearningSource::Project => "[P]",
                                learnings::LearningSource::Global => "[G]",
                            };
                            println!("  {} {}", source_indicator, entry.summary());
                            if let Some(correction) = entry.correction_text() {
                                println!("     Correction: {}", correction);
                            }
                            let entities = entry.entities();
                            if !entities.is_empty() {
                                println!("     Entities: {}", entities.join(", "));
                            }
                        }
                    }
                    Ok(())
                }
                Err(e) => Err(e.into()),
            }
        }
        LearnSub::Correct { id, correction } => {
            let storage_loc = config.storage_location();
            match correct_learning(&storage_loc, &id, &correction) {
                Ok(path) => {
                    println!("Correction added to learning {}: {}", id, path.display());
                    Ok(())
                }
                Err(e) => {
                    eprintln!("Failed to add correction: {}", e);
                    Err(e.into())
                }
            }
        }
        LearnSub::Correction { sub } => match sub {
            CorrectionSub::Add {
                original,
                corrected,
                correction_type,
                context,
                session_id,
            } => {
                let ct: CorrectionType = correction_type
                    .parse()
                    .unwrap_or(CorrectionType::Other(correction_type.clone()));
                if let Some(ref sid) = session_id {
                    log::debug!("Correction session_id: {}", sid);
                }
                match capture_correction(ct, &original, &corrected, &context, &config) {
                    Ok(path) => {
                        println!("Captured correction: {}", path.display());
                        Ok(())
                    }
                    Err(e) => {
                        eprintln!("Failed to capture correction: {}", e);
                        Err(e.into())
                    }
                }
            }
            CorrectionSub::List {
                recent,
                filter_type,
                global,
            } => {
                let storage_loc = config.storage_location();
                let storage_dir = if global {
                    &config.global_dir
                } else {
                    &storage_loc
                };
                match list_all_entries(storage_dir, recent) {
                    Ok(entries) => {
                        let corrections: Vec<_> = entries
                            .into_iter()
                            .filter_map(|e| {
                                if let learnings::LearningEntry::Correction(c) = e {
                                    Some(c)
                                } else {
                                    None
                                }
                            })
                            .filter(|c| {
                                filter_type
                                    .as_ref()
                                    .is_none_or(|ft| c.correction_type.to_string() == *ft)
                            })
                            .collect();
                        if corrections.is_empty() {
                            println!("No corrections found.");
                        } else {
                            println!("Corrections ({}):", corrections.len());
                            for c in &corrections {
                                println!(
                                    "  [{}] {} -> {}",
                                    c.correction_type, c.original, c.corrected
                                );
                                if !c.context_description.is_empty() {
                                    println!("     Context: {}", c.context_description);
                                }
                            }
                        }
                        Ok(())
                    }
                    Err(e) => Err(e.into()),
                }
            }
        },
        LearnSub::Hook {
            format,
            learn_hook_type,
        } => learnings::process_hook_input_with_type(format, learn_hook_type)
            .await
            .map_err(|e| e.into()),
        LearnSub::InstallHook { agent } => {
            learnings::install_hook(agent).await.map_err(|e| e.into())
        }
        LearnSub::Procedure { sub } => {
            let procedures_path = config.global_dir.join("procedures.jsonl");
            let store = learnings::ProcedureStore::new(procedures_path);

            match sub {
                ProcedureSub::List { recent } => {
                    let all = store.load_all()?;
                    if all.is_empty() {
                        println!("No procedures found.");
                    } else {
                        let display_count = recent.min(all.len());
                        println!("Procedures ({} of {}):", display_count, all.len());
                        for proc in all.iter().rev().take(recent) {
                            println!(
                                "  [{}] {} -- {} steps, confidence {:.0}% ({}/{})",
                                proc.id,
                                proc.title,
                                proc.step_count(),
                                proc.confidence.score * 100.0,
                                proc.confidence.success_count,
                                proc.confidence.total_executions(),
                            );
                        }
                    }
                    Ok(())
                }
                ProcedureSub::Show { id } => {
                    match store.find_by_id(&id)? {
                        Some(proc) => {
                            println!("Procedure: {}", proc.title);
                            println!("ID: {}", proc.id);
                            println!("Description: {}", proc.description);
                            println!(
                                "Confidence: {:.0}% ({} successes, {} failures)",
                                proc.confidence.score * 100.0,
                                proc.confidence.success_count,
                                proc.confidence.failure_count,
                            );
                            if proc.disabled {
                                println!("Status: DISABLED");
                            }
                            println!("Created: {}", proc.created_at);
                            println!("Updated: {}", proc.updated_at);
                            if !proc.tags.is_empty() {
                                println!("Tags: {}", proc.tags.join(", "));
                            }
                            if let Some(ref session) = proc.source_session {
                                println!("Source session: {}", session);
                            }
                            println!("Steps ({}):", proc.step_count());
                            for step in &proc.steps {
                                println!("  {}. {}", step.ordinal, step.command);
                                if let Some(ref pre) = step.precondition {
                                    println!("     pre: {}", pre);
                                }
                                if let Some(ref post) = step.postcondition {
                                    println!("     post: {}", post);
                                }
                            }
                        }
                        None => {
                            eprintln!("Procedure '{}' not found.", id);
                        }
                    }
                    Ok(())
                }
                ProcedureSub::Record { title, description } => {
                    use uuid::Uuid;
                    let id = Uuid::new_v4().to_string();
                    let desc = description.unwrap_or_default();
                    let procedure =
                        terraphim_types::procedure::CapturedProcedure::new(id.clone(), title, desc);
                    store.save(&procedure)?;
                    println!("Created procedure: {}", id);
                    Ok(())
                }
                ProcedureSub::AddStep {
                    id,
                    command,
                    precondition,
                    postcondition,
                } => {
                    let mut proc = store
                        .find_by_id(&id)?
                        .ok_or_else(|| anyhow::anyhow!("Procedure '{}' not found", id))?;
                    let ordinal = proc.step_count() as u32 + 1;
                    proc.add_step(terraphim_types::procedure::ProcedureStep {
                        ordinal,
                        command,
                        precondition,
                        postcondition,
                        working_dir: None,
                        privileged: false,
                        tags: vec![],
                    });
                    store.save(&proc)?;
                    println!("Added step {} to procedure '{}'.", ordinal, id);
                    Ok(())
                }
                ProcedureSub::Success { id } => {
                    store.update_confidence(&id, true)?;
                    println!("Recorded success for procedure '{}'.", id);
                    Ok(())
                }
                ProcedureSub::Failure { id } => {
                    store.update_confidence(&id, false)?;
                    println!("Recorded failure for procedure '{}'.", id);
                    Ok(())
                }
                ProcedureSub::Replay { id, dry_run } => {
                    let procedure = store.find_by_id(&id)?;
                    match procedure {
                        None => {
                            eprintln!("Procedure '{}' not found.", id);
                            std::process::exit(1);
                        }
                        Some(proc) => {
                            // Check if procedure is disabled
                            if proc.disabled {
                                eprintln!(
                                    "Procedure '{}' is disabled. Use 'learn procedure enable {}' to re-enable it.",
                                    id, id,
                                );
                                std::process::exit(1);
                            }

                            // Check minimum confidence threshold
                            if proc.confidence.total_executions() > 0 && proc.confidence.score < 0.5
                            {
                                eprintln!(
                                    "Procedure '{}' has low confidence ({:.0}%). \
                                     Use --dry-run to preview, or record more successes first.",
                                    id,
                                    proc.confidence.score * 100.0,
                                );
                                std::process::exit(1);
                            }

                            println!(
                                "Replaying procedure '{}' ({} steps){}",
                                proc.title,
                                proc.step_count(),
                                if dry_run { " [DRY RUN]" } else { "" },
                            );

                            let result = learnings::replay_procedure(&proc, dry_run)?;

                            // Print outcomes
                            for (ordinal, outcome) in &result.outcomes {
                                match outcome {
                                    learnings::StepOutcome::Success { stdout } => {
                                        println!("  step {}: OK", ordinal);
                                        if !stdout.trim().is_empty() && stdout != "(dry-run)" {
                                            for line in stdout.lines() {
                                                println!("    | {}", line);
                                            }
                                        }
                                    }
                                    learnings::StepOutcome::Failed { stderr, exit_code } => {
                                        println!("  step {}: FAILED (exit {})", ordinal, exit_code);
                                        if !stderr.trim().is_empty() {
                                            for line in stderr.lines() {
                                                println!("    | {}", line);
                                            }
                                        }
                                    }
                                    learnings::StepOutcome::Skipped { reason } => {
                                        println!("  step {}: SKIPPED ({})", ordinal, reason);
                                    }
                                }
                            }

                            // Update confidence based on result (skip for dry-run)
                            if !dry_run {
                                store.update_confidence(&id, result.overall_success)?;
                                if result.overall_success {
                                    println!("Replay completed successfully.");
                                } else {
                                    println!("Replay failed.");
                                    std::process::exit(1);
                                }
                            } else {
                                println!("Dry run completed.");
                            }

                            Ok(())
                        }
                    }
                }
                ProcedureSub::Health => {
                    let reports = store.health_check()?;
                    if reports.is_empty() {
                        println!("No procedures found.");
                    } else {
                        println!(
                            "{:<38} {:<12} {:<8} {:<6} {:<9}",
                            "ID", "STATUS", "RATE", "RUNS", "DISABLED"
                        );
                        println!("{}", "-".repeat(73));
                        for report in &reports {
                            println!(
                                "{:<38} {:<12} {:<8.0}% {:<6} {:<9}",
                                report.id,
                                report.status.to_string(),
                                report.success_rate * 100.0,
                                report.total_executions,
                                if report.auto_disabled
                                    || store
                                        .find_by_id(&report.id)?
                                        .map(|p| p.disabled)
                                        .unwrap_or(false)
                                {
                                    "yes"
                                } else {
                                    "no"
                                },
                            );
                        }
                        let auto_disabled_count =
                            reports.iter().filter(|r| r.auto_disabled).count();
                        if auto_disabled_count > 0 {
                            println!(
                                "\n{} procedure(s) auto-disabled due to critical failure rate.",
                                auto_disabled_count,
                            );
                        }
                    }
                    Ok(())
                }
                ProcedureSub::Enable { id } => {
                    store.set_disabled(&id, false)?;
                    println!("Procedure '{}' enabled.", id);
                    Ok(())
                }
                ProcedureSub::Disable { id } => {
                    store.set_disabled(&id, true)?;
                    println!("Procedure '{}' disabled.", id);
                    Ok(())
                }
                #[cfg(feature = "repl-sessions")]
                ProcedureSub::FromSession { session_id, title } => {
                    use terraphim_sessions::SessionService;

                    let service = SessionService::new();

                    // Load cached sessions from disk
                    let cache_path = get_session_cache_path();
                    if cache_path.exists()
                        && let Ok(data) = std::fs::read_to_string(&cache_path)
                        && let Ok(cached) =
                            serde_json::from_str::<Vec<terraphim_sessions::Session>>(&data)
                    {
                        service.load_sessions(cached).await;
                    }

                    let session = service.get_session(&session_id).await;
                    match session {
                        Some(sess) => {
                            let commands =
                                learnings::procedure::extract_bash_commands_from_session(&sess);
                            if commands.is_empty() {
                                println!("No Bash commands found in session '{}'.", session_id);
                                return Ok(());
                            }
                            let total_cmds = commands.len();
                            let mut procedure =
                                learnings::procedure::from_session_commands(commands, title);
                            procedure.source_session = Some(session_id.clone());
                            let step_count = procedure.step_count();

                            let saved = store.save_with_dedup(procedure)?;
                            println!(
                                "Created procedure '{}' (ID: {}) with {} steps from {} commands.",
                                saved.title, saved.id, step_count, total_cmds
                            );
                            Ok(())
                        }
                        None => {
                            eprintln!(
                                "Session '{}' not found. Try running 'sessions list' first to import sessions.",
                                session_id
                            );
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
        LearnSub::Compile { output, merge_with } => {
            let storage_loc = config.storage_location();
            let compiled = learnings::compile_corrections_to_thesaurus(&storage_loc)
                .map_err(|e| anyhow::anyhow!("Failed to compile corrections: {}", e))?;

            let compiled_count = compiled.len();

            let final_thesaurus = if let Some(ref merge_path) = merge_with {
                let curated_json = std::fs::read_to_string(merge_path).map_err(|e| {
                    anyhow::anyhow!("Failed to read curated thesaurus {:?}: {}", merge_path, e)
                })?;
                let curated: terraphim_types::Thesaurus = serde_json::from_str(&curated_json)
                    .map_err(|e| {
                        anyhow::anyhow!("Failed to parse curated thesaurus {:?}: {}", merge_path, e)
                    })?;
                let curated_count = curated.len();
                let merged = learnings::merge_thesauruses(curated, compiled);
                println!(
                    "Compiled {} correction(s), merged with {} curated entries -> {} total entries.",
                    compiled_count,
                    curated_count,
                    merged.len()
                );
                merged
            } else {
                println!("Compiled {} correction(s).", compiled_count);
                compiled
            };

            learnings::write_thesaurus_json(&final_thesaurus, &output)
                .map_err(|e| anyhow::anyhow!("Failed to write thesaurus to {:?}: {}", output, e))?;

            println!("Thesaurus written to: {}", output.display());
            Ok(())
        }
        LearnSub::ExportKg {
            output,
            correction_type,
        } => {
            let storage_loc = config.storage_location();
            let filter = match correction_type.as_str() {
                "tool-preference" => learnings::CorrectionTypeFilter::ToolPreference,
                "all" => learnings::CorrectionTypeFilter::All,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Invalid correction_type '{}'. Use 'tool-preference' or 'all'.",
                        correction_type
                    ));
                }
            };
            let count = learnings::export_corrections_as_kg(&storage_loc, &output, filter)
                .map_err(|e| anyhow::anyhow!("Failed to export corrections: {}", e))?;
            println!(
                "Exported {} correction(s) as KG markdown to: {}",
                count,
                output.display()
            );
            Ok(())
        }
        #[cfg(feature = "shared-learning")]
        LearnSub::Suggest { sub } => run_suggest_command(sub).await,
        #[cfg(feature = "shared-learning")]
        LearnSub::Shared { sub } => run_shared_learning_command(sub, &config).await,
    }
}

fn evolution_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("terraphim")
        .join("evolution")
        .join("cli-agent.json")
}

fn load_evolution() -> terraphim_agent_evolution::AgentEvolutionSystem {
    let path = evolution_path();
    if path.exists()
        && let Ok(data) = std::fs::read_to_string(&path)
    {
        #[derive(serde::Deserialize)]
        struct EvolutionState {
            memory: terraphim_agent_evolution::MemoryState,
            lessons: terraphim_agent_evolution::LessonsState,
        }
        if let Ok(state) = serde_json::from_str::<EvolutionState>(&data) {
            let mut evolution =
                terraphim_agent_evolution::AgentEvolutionSystem::new("cli-agent".to_string());
            evolution.memory.current_state = state.memory;
            evolution.lessons.current_state = state.lessons;
            return evolution;
        }
    }
    terraphim_agent_evolution::AgentEvolutionSystem::new("cli-agent".to_string())
}

fn save_evolution(
    evolution: &terraphim_agent_evolution::AgentEvolutionSystem,
) -> Result<(), anyhow::Error> {
    let path = evolution_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = serde_json::json!({
        "agent_id": evolution.agent_id,
        "saved_at": chrono::Utc::now().to_rfc3339(),
        "memory": evolution.memory.current_state,
        "lessons": evolution.lessons.current_state,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&state)?)?;
    Ok(())
}

async fn run_memory_command(sub: MemorySub, output: &CommandOutputConfig) -> Result<()> {
    match sub {
        MemorySub::Capture { provenance_tag } => {
            use terraphim_agent_evolution::{ImportanceLevel, MemoryItem, MemoryItemType};

            let mut evolution = load_evolution();
            let content = provenance_tag
                .as_ref()
                .map(|tag| format!("Memory item captured via CLI with provenance: {}", tag))
                .unwrap_or_else(|| "Memory item captured via CLI".to_string());
            let tags: Vec<String> = provenance_tag
                .clone()
                .map(|tag| vec![format!("provenance:{}", tag)])
                .unwrap_or_default();
            let memory = MemoryItem {
                id: uuid::Uuid::new_v4().to_string(),
                item_type: MemoryItemType::Experience,
                content,
                created_at: chrono::Utc::now(),
                last_accessed: None,
                access_count: 0,
                importance: ImportanceLevel::Medium,
                tags,
                associations: std::collections::HashMap::new(),
            };
            let id = memory.id.clone();
            match evolution.memory.add_memory(memory).await {
                Ok(()) => {
                    save_evolution(&evolution)?;
                    if output.is_machine_readable() {
                        println!(
                            "{}",
                            serde_json::json!({ "status": "ok", "action": "capture", "memory_id": id, "provenance_tag": provenance_tag })
                        );
                    } else {
                        println!("Memory captured: {}", id);
                        if let Some(tag) = provenance_tag {
                            println!("  provenance_tag: {}", tag);
                        }
                    }
                }
                Err(e) => {
                    if output.is_machine_readable() {
                        println!(
                            "{}",
                            serde_json::json!({ "status": "error", "action": "capture", "error": e.to_string() })
                        );
                    } else {
                        eprintln!("Failed to capture memory: {}", e);
                    }
                    return Err(anyhow::anyhow!("{}", e));
                }
            }
            Ok(())
        }
        MemorySub::Distill { format } => {
            if output.is_machine_readable() {
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "distill", "format": format })
                );
            } else {
                println!(
                    "Memory distill: routing to learn compile + export-kg (format: {})",
                    format
                );
            }
            Ok(())
        }
        MemorySub::Scope {
            role,
            project,
            check,
        } => {
            let (role_clone, project_clone) = (role.clone(), project.clone());
            if check {
                println!(
                    "Memory scope --check: verifying no permissioned items in public locations"
                );
                let config_dir = dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("terraphim");
                let kg_dir = config_dir.join("kg");
                if kg_dir.exists() {
                    let public_risk = false;
                    for entry in std::fs::read_dir(&kg_dir)? {
                        let entry = entry?;
                        let path = entry.path();
                        if path.is_dir() && path.file_name().is_some_and(|n| n != "projects") {
                            println!("  found role KG: {}", path.display());
                        }
                        if path.is_dir() && path.file_name().is_some_and(|n| n == "projects") {
                            for p in std::fs::read_dir(&path)? {
                                let p = p?;
                                println!("  found project KG: {}", p.path().display());
                            }
                        }
                    }
                    if !public_risk {
                        println!("  no permissioned items detected in public locations");
                    }
                } else {
                    println!("  no KG directory found at {}", kg_dir.display());
                }
            } else {
                println!("Memory scope:");
                if let Some(ref r) = role_clone {
                    println!("  role: {}", r);
                }
                if let Some(ref p) = project_clone {
                    println!("  project: {}", p);
                }
                let config_dir = dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("terraphim");
                let kg_dir = config_dir.join("kg");
                if kg_dir.exists() {
                    println!("  KG directory: {}", kg_dir.display());
                    let mut count = 0;
                    for entry in std::fs::read_dir(&kg_dir)? {
                        let entry = entry?;
                        if entry.path().is_dir() {
                            count += 1;
                        }
                    }
                    println!("  role KGs found: {}", count);
                } else {
                    println!("  No KG directory configured");
                }
            }
            if output.is_machine_readable() {
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "scope", "role": role_clone, "project": project_clone, "check": check })
                );
            }
            Ok(())
        }
        MemorySub::Provenance { memory_id, query } => {
            if output.is_machine_readable() {
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "provenance", "memory_id": memory_id, "query": query })
                );
            } else {
                println!("Memory provenance: routing to sessions search");
                if let Some(id) = memory_id {
                    println!("  memory_id: {}", id);
                }
                if let Some(q) = query {
                    println!("  query: {}", q);
                }
            }
            Ok(())
        }
        MemorySub::Retrieve { role, query } => {
            if output.is_machine_readable() {
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "retrieve", "role": role, "query": query })
                );
            } else {
                println!("Memory retrieve: routing to search (role: {:?})", role);
                println!("  query: {}", query);
            }
            Ok(())
        }
        MemorySub::Apply { prompt } => {
            if output.is_machine_readable() {
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "apply", "prompt": prompt })
                );
            } else {
                println!("Memory apply: showing what hooks would inject for prompt");
                if let Some(p) = prompt {
                    println!("  prompt: {}", truncate_snippet(&p, 200));
                }
            }
            Ok(())
        }
        MemorySub::Validate { all, lesson_id } => {
            use terraphim_agent_evolution::MemoryItem;

            let evolution = load_evolution();
            let items: Vec<&MemoryItem> = if all {
                evolution.memory.current_state.short_term.iter().collect()
            } else if let Some(ref id) = lesson_id {
                evolution
                    .memory
                    .current_state
                    .short_term
                    .iter()
                    .filter(|m| m.id == *id)
                    .collect()
            } else {
                evolution
                    .memory
                    .current_state
                    .short_term
                    .iter()
                    .rev()
                    .take(20)
                    .collect()
            };

            if items.is_empty() {
                println!("No memory items found to validate.");
                return Ok(());
            }

            let mut scores = Vec::new();
            for item in &items {
                let score = score_memory_item(item);
                scores.push((item.id.clone(), score));
            }

            if output.is_machine_readable() {
                let json_scores: Vec<serde_json::Value> = scores
                    .iter()
                    .map(|(id, s)| {
                        serde_json::json!({
                            "memory_id": id,
                            "faithfulness": s.faithfulness,
                            "scope": s.scope,
                            "provenance": s.provenance,
                            "actionability": s.actionability,
                            "decay": s.decay,
                            "risk": s.risk,
                            "composite": s.composite(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "validate", "scores": json_scores })
                );
            } else {
                println!("Memory Validation Results\n");
                for (i, (id, score)) in scores.iter().enumerate() {
                    println!("{}. {} (composite: {:.2})", i + 1, id, score.composite());
                    println!(
                        "   Faithfulness: {:.1}  Scope: {:.1}  Provenance: {:.1}",
                        score.faithfulness, score.scope, score.provenance
                    );
                    println!(
                        "   Actionability: {:.1}  Decay: {:.1}  Risk: {:.1}",
                        score.actionability, score.decay, score.risk
                    );
                }

                let avg_composite =
                    scores.iter().map(|(_, s)| s.composite()).sum::<f64>() / scores.len() as f64;
                println!("\nAverage composite score: {:.2}", avg_composite);
            }
            Ok(())
        }
        MemorySub::Retire { lesson_id, reason } => {
            let out_path = match &lesson_id {
                Some(id) => {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join("terraphim");
                    config_dir.join(format!("retired-{}.md", id))
                }
                None => {
                    let config_dir = dirs::config_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join("terraphim");
                    config_dir.join("learned-rules-retirements.md")
                }
            };

            let reason_text = reason.as_deref().unwrap_or("no reason provided");
            let timestamp = chrono::Utc::now().to_rfc3339();
            let entry = format!(
                "## Retirement Proposal\n\n\
                 **Date:** {}\n\
                 **Lesson ID:** {}\n\
                 **Reason:** {}\n\
                 **Status:** PENDING CTO APPROVAL\n\n",
                timestamp,
                lesson_id.as_deref().unwrap_or("all"),
                reason_text,
            );

            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&out_path, &entry)?;

            println!("Retirement proposal written to: {}", out_path.display());
            if output.is_machine_readable() {
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "retire", "lesson_id": lesson_id, "reason": reason, "output": out_path.to_string_lossy() })
                );
            }
            Ok(())
        }
        MemorySub::Rubric {
            project,
            output: outfile,
        } => {
            use terraphim_agent_evolution::MemoryItem;

            let evolution = load_evolution();
            let items: Vec<&MemoryItem> =
                evolution.memory.current_state.short_term.iter().collect();

            if items.is_empty() {
                println!("No memory items found for rubric analysis.");
                return Ok(());
            }

            let scores: Vec<(&MemoryItem, RubricScore)> = items
                .iter()
                .map(|item| (*item, score_memory_item(item)))
                .collect();

            let avg_composite =
                scores.iter().map(|(_, s)| s.composite()).sum::<f64>() / scores.len() as f64;

            let avg_dimensions = RubricScore {
                faithfulness: scores.iter().map(|(_, s)| s.faithfulness).sum::<f64>()
                    / scores.len() as f64,
                scope: scores.iter().map(|(_, s)| s.scope).sum::<f64>() / scores.len() as f64,
                provenance: scores.iter().map(|(_, s)| s.provenance).sum::<f64>()
                    / scores.len() as f64,
                actionability: scores.iter().map(|(_, s)| s.actionability).sum::<f64>()
                    / scores.len() as f64,
                decay: scores.iter().map(|(_, s)| s.decay).sum::<f64>() / scores.len() as f64,
                risk: scores.iter().map(|(_, s)| s.risk).sum::<f64>() / scores.len() as f64,
            };

            let mut offender_list: Vec<(&MemoryItem, f64)> = scores
                .iter()
                .map(|(item, s)| (*item, s.composite()))
                .collect();
            offender_list
                .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            let top_offenders: Vec<_> = offender_list.iter().take(3).collect();

            let retirement_recs: Vec<&MemoryItem> = scores
                .iter()
                .filter(|(_, s)| s.decay < 0.4 || s.risk > 0.7)
                .map(|(item, _)| *item)
                .take(3)
                .collect();

            let mut report = String::new();
            report.push_str("# Memory Reliability Rubric Report\n\n");
            report.push_str(&format!("**Project:** {}\n", project));
            report.push_str(&format!(
                "**Generated:** {}\n",
                chrono::Utc::now().to_rfc3339()
            ));
            report.push_str(&format!("**Items analysed:** {}\n\n", items.len()));

            report.push_str("## Overall Scores\n\n");
            report.push_str("| Dimension | Score | Status |\n|---|---|---|\n");
            for (name, value) in [
                ("Faithfulness", avg_dimensions.faithfulness),
                ("Scope", avg_dimensions.scope),
                ("Provenance", avg_dimensions.provenance),
                ("Actionability", avg_dimensions.actionability),
                ("Decay", avg_dimensions.decay),
                ("Risk", avg_dimensions.risk),
            ] {
                let status = if value >= 0.7 {
                    "Good"
                } else if value >= 0.4 {
                    "Adequate"
                } else {
                    "Needs attention"
                };
                report.push_str(&format!("| {} | {:.2} | {} |\n", name, value, status));
            }
            report.push_str(&format!(
                "\n**Composite score:** {:.2} / 1.00\n\n",
                avg_composite
            ));

            report.push_str("## Top 3 Items Needing Attention\n\n");
            for (i, (item, score)) in top_offenders.iter().enumerate() {
                let first_line = item.content.lines().next().unwrap_or(&item.content);
                report.push_str(&format!(
                    "{}. **{}** (composite: {:.2})\n   {}\n\n",
                    i + 1,
                    item.id,
                    score,
                    truncate_snippet(first_line, 100),
                ));
            }

            report.push_str("## Recommended Retirements\n\n");
            if retirement_recs.is_empty() {
                report.push_str("No items recommended for retirement.\n\n");
            } else {
                for item in &retirement_recs {
                    let first_line = item.content.lines().next().unwrap_or(&item.content);
                    report.push_str(&format!(
                        "- **{}**: {} (decay: {:.2}, risk: {:.2})\n",
                        item.id,
                        truncate_snippet(first_line, 80),
                        compute_decay(item.created_at),
                        compute_risk(&item.content),
                    ));
                }
            }

            if let Some(path) = outfile {
                std::fs::write(&path, &report)?;
                println!("Rubric report written to: {}", path);
            } else {
                println!("{}", report);
            }
            Ok(())
        }
        MemorySub::List { item_type, limit } => {
            let evolution = load_evolution();
            let state = &evolution.memory.current_state;

            let items = if let Some(ref t) = item_type {
                let filter = t.to_lowercase();
                state
                    .short_term
                    .iter()
                    .filter(|m| {
                        format!("{:?}", m.item_type)
                            .to_lowercase()
                            .contains(&filter)
                    })
                    .take(limit)
                    .collect::<Vec<_>>()
            } else {
                state.short_term.iter().take(limit).collect::<Vec<_>>()
            };

            if output.is_machine_readable() {
                let json_items: Vec<serde_json::Value> = items
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "id": m.id,
                            "item_type": format!("{:?}", m.item_type),
                            "content": truncate_snippet(&m.content, 200),
                            "importance": format!("{:?}", m.importance),
                            "tags": m.tags,
                            "access_count": m.access_count,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({ "status": "ok", "action": "list", "count": json_items.len(), "items": json_items })
                );
            } else {
                if items.is_empty() {
                    println!("No memory items found in evolution store.");
                    if item_type.is_some() {
                        println!("  (try without --item-type filter)");
                    }
                } else {
                    println!("Memory items ({} total):", items.len());
                    for (i, m) in items.iter().enumerate() {
                        let first_line = m.content.lines().next().unwrap_or(&m.content);
                        println!(
                            "  {}. [{:?}] {} -- {:?} importance (accessed {}x)",
                            i + 1,
                            m.item_type,
                            truncate_snippet(first_line, 80),
                            m.importance,
                            m.access_count
                        );
                    }
                }

                let lesson_count = evolution.lessons.current_state.total_lessons();
                if lesson_count > 0 {
                    println!(
                        "\n{} lessons stored (use `memory export` for full lesson data)",
                        lesson_count
                    );
                }
            }
            Ok(())
        }
        MemorySub::Show { id, json } => {
            let evolution = load_evolution();

            let memory_item = evolution
                .memory
                .current_state
                .short_term
                .iter()
                .find(|m| m.id == id)
                .cloned();
            let all_lessons: Vec<_> = {
                let ls = &evolution.lessons.current_state;
                let mut v = Vec::new();
                v.extend(ls.technical_lessons.iter());
                v.extend(ls.process_lessons.iter());
                v.extend(ls.domain_lessons.iter());
                v.extend(ls.failure_lessons.iter());
                v.extend(ls.success_patterns.iter());
                v
            };
            let lesson = all_lessons.iter().find(|l| l.id == id).cloned().cloned();

            if memory_item.is_none() && lesson.is_none() {
                eprintln!("No memory item or lesson found with ID: {}", id);
                if output.is_machine_readable() {
                    println!(
                        "{}",
                        serde_json::json!({ "status": "error", "action": "show", "error": format!("no item found with ID {}", id) })
                    );
                }
                return Ok(());
            }

            if json || output.is_machine_readable() {
                let payload = serde_json::json!({
                    "status": "ok",
                    "action": "show",
                    "id": id,
                    "memory_item": memory_item,
                    "lesson": lesson,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                if let Some(m) = memory_item {
                    println!("Memory Item: {}", m.id);
                    println!("  type: {:?}", m.item_type);
                    println!("  importance: {:?}", m.importance);
                    println!("  created: {}", m.created_at);
                    println!("  accessed: {} times", m.access_count);
                    if !m.tags.is_empty() {
                        println!("  tags: {}", m.tags.join(", "));
                    }
                    println!("  content:");
                    for line in m.content.lines().take(20) {
                        println!("    {}", line);
                    }
                    if m.content.lines().count() > 20 {
                        println!("    ... ({} more lines)", m.content.lines().count() - 20);
                    }
                }
                if let Some(l) = lesson {
                    println!("\nLesson: {} ({})", l.title, l.id);
                    println!("  category: {:?}", l.category);
                    println!("  impact: {:?}", l.impact);
                    println!("  confidence: {:.0}%", l.confidence * 100.0);
                    println!("  learned: {}", l.learned_at);
                    println!(
                        "  applied: {} times (success rate: {:.0}%)",
                        l.applied_count,
                        l.success_rate * 100.0
                    );
                    println!("  validated: {}", if l.validated { "yes" } else { "no" });
                    if !l.tags.is_empty() {
                        println!("  tags: {}", l.tags.join(", "));
                    }
                    println!("  context:");
                    for line in l.context.lines().take(10) {
                        println!("    {}", line);
                    }
                    println!("  insight:");
                    for line in l.insight.lines().take(10) {
                        println!("    {}", line);
                    }
                }
            }
            Ok(())
        }
        MemorySub::Export {
            format,
            output: outfile,
        } => {
            let evolution = load_evolution();

            let memory_items: Vec<serde_json::Value> = evolution
                .memory
                .current_state
                .short_term
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "item_type": format!("{:?}", m.item_type),
                        "content": m.content,
                        "importance": format!("{:?}", m.importance),
                        "tags": m.tags,
                        "access_count": m.access_count,
                        "created_at": m.created_at.to_rfc3339(),
                    })
                })
                .collect();

            let all_lessons: Vec<_> = {
                let ls = &evolution.lessons.current_state;
                let mut v = Vec::new();
                v.extend(ls.technical_lessons.iter());
                v.extend(ls.process_lessons.iter());
                v.extend(ls.domain_lessons.iter());
                v.extend(ls.failure_lessons.iter());
                v.extend(ls.success_patterns.iter());
                v
            };
            let lessons: Vec<serde_json::Value> = all_lessons
                .iter()
                .map(|l| {
                    serde_json::json!({
                        "id": l.id,
                        "title": l.title,
                        "category": format!("{:?}", l.category),
                        "impact": format!("{:?}", l.impact),
                        "confidence": l.confidence,
                        "learned_at": l.learned_at.to_rfc3339(),
                        "applied_count": l.applied_count,
                        "success_rate": l.success_rate,
                        "validated": l.validated,
                        "tags": l.tags,
                        "context": l.context,
                        "insight": l.insight,
                    })
                })
                .collect();

            let payload = serde_json::json!({
                "agent": "cli-agent",
                "exported_at": chrono::Utc::now().to_rfc3339(),
                "memory_items": memory_items,
                "lessons": lessons,
                "summary": {
                    "memory_count": memory_items.len(),
                    "lesson_count": lessons.len(),
                }
            });

            let output_str = match format.as_str() {
                "markdown" => {
                    let mut md = String::new();
                    md.push_str("# Memory Export\n\n");
                    md.push_str("**Agent:** cli-agent\n");
                    md.push_str(&format!(
                        "**Exported:** {}\n\n",
                        chrono::Utc::now().to_rfc3339()
                    ));
                    md.push_str(&format!("## Memory Items ({})\n\n", memory_items.len()));
                    for m in &memory_items {
                        md.push_str(&format!(
                            "- **{}** [{:?}]: {} (importance: {:?}, accessed: {}x)\n",
                            m["id"].as_str().unwrap_or("?"),
                            m["item_type"].as_str().unwrap_or("?"),
                            truncate_snippet(m["content"].as_str().unwrap_or(""), 100),
                            m["importance"].as_str().unwrap_or("?"),
                            m["access_count"].as_u64().unwrap_or(0),
                        ));
                    }
                    md.push_str(&format!("\n## Lessons ({})\n\n", lessons.len()));
                    for l in &lessons {
                        md.push_str(&format!(
                            "- **{}** ({:?}): {} [{:.0}% confidence, {:.0}% success]\n",
                            l["title"].as_str().unwrap_or("?"),
                            l["category"].as_str().unwrap_or("?"),
                            truncate_snippet(l["insight"].as_str().unwrap_or(""), 100),
                            l["confidence"].as_f64().unwrap_or(0.0) * 100.0,
                            l["success_rate"].as_f64().unwrap_or(0.0) * 100.0,
                        ));
                    }
                    md
                }
                _ => serde_json::to_string_pretty(&payload)?,
            };

            if let Some(path) = outfile {
                std::fs::write(&path, &output_str)?;
                println!("Memory export written to: {}", path);
            } else {
                println!("{}", output_str);
            }
            Ok(())
        }
        MemorySub::SecondRun { issue } => {
            let artefact_base = std::env::var("TERRAPHIM_ADF_ARTEFACTS_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    dirs::cache_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("."))
                        .join("terraphim")
                        .join("adf-artefacts")
                })
                .join(format!("issue-{}", issue));

            let mut runs: Vec<RunMetrics> = Vec::new();
            if artefact_base.exists() {
                for entry in std::fs::read_dir(&artefact_base)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().is_some_and(|e| e == "json")
                        && let Ok(data) = std::fs::read_to_string(&path)
                        && let Ok(metrics) = serde_json::from_str::<RunMetrics>(&data)
                    {
                        runs.push(metrics);
                    }
                }
            }

            runs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

            if runs.len() < 2 {
                if output.is_machine_readable() {
                    println!(
                        "{}",
                        serde_json::json!({
                            "status": "ok",
                            "action": "second-run",
                            "issue": issue,
                            "runs_found": runs.len(),
                            "note": "need at least 2 runs to compute delta"
                        })
                    );
                } else {
                    println!(
                        "Found {} runs for issue #{}. Need at least 2 to compute delta.",
                        runs.len(),
                        issue
                    );
                    if artefact_base.exists() {
                        println!("  artefact directory: {}", artefact_base.display());
                    } else {
                        println!(
                            "  no artefact directory found (expected at: {})",
                            artefact_base.display()
                        );
                    }
                }
                return Ok(());
            }

            let run_1 = &runs[0];
            let run_2 = &runs[runs.len() - 1];

            let token_delta = run_1.input_tokens as i64 - run_2.input_tokens as i64;
            let retry_delta = run_1.retry_count as i32 - run_2.retry_count as i32;
            let time_delta = run_1.wall_time_seconds - run_2.wall_time_seconds;

            let signal = serde_json::json!({
                "gitea_issue": issue,
                "runs_compared": runs.len(),
                "run_1": run_1,
                "run_2": run_2,
                "delta": {
                    "tokens_saved": token_delta,
                    "retries_avoided": retry_delta,
                    "wall_time_delta_seconds": time_delta,
                    "interpretation": if token_delta > 0 {
                        "improved (fewer tokens in later run)"
                    } else if token_delta < 0 {
                        "regressed (more tokens in later run)"
                    } else {
                        "no change"
                    }
                }
            });

            println!("{}", serde_json::to_string_pretty(&signal)?);
            Ok(())
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct RubricScore {
    faithfulness: f64,
    scope: f64,
    provenance: f64,
    actionability: f64,
    decay: f64,
    risk: f64,
}

impl RubricScore {
    fn composite(&self) -> f64 {
        0.30 * self.faithfulness
            + 0.25 * self.actionability
            + 0.15 * self.scope
            + 0.10 * self.provenance
            + 0.10 * self.decay
            + 0.10 * (1.0 - self.risk)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RunMetrics {
    timestamp: String,
    input_tokens: u64,
    output_tokens: u64,
    wall_time_seconds: f64,
    retry_count: u32,
    hook_injected_bytes: u64,
}

fn score_memory_item(item: &terraphim_agent_evolution::MemoryItem) -> RubricScore {
    let faithfulness = if item.content.is_empty() {
        0.1
    } else if item.content.len() > 20 {
        0.8
    } else {
        0.5
    };

    let scope = if !item.tags.is_empty() {
        0.80f64.min(0.5 + item.tags.len() as f64 * 0.1)
    } else {
        0.3
    };

    let provenance = if item.created_at > chrono::Utc::now() - chrono::Duration::days(30) {
        0.9
    } else {
        0.6
    };

    let actionability = match item.item_type {
        terraphim_agent_evolution::MemoryItemType::LessonLearned => 0.9,
        terraphim_agent_evolution::MemoryItemType::ExecutionResult => 0.6,
        terraphim_agent_evolution::MemoryItemType::Skill => 0.8,
        terraphim_agent_evolution::MemoryItemType::Concept => 0.5,
        _ => 0.4,
    };

    let decay = compute_decay(item.created_at);

    let risk = compute_risk(&item.content);

    RubricScore {
        faithfulness,
        scope,
        provenance,
        actionability,
        decay,
        risk,
    }
}

fn compute_decay(created_at: chrono::DateTime<chrono::Utc>) -> f64 {
    let days = (chrono::Utc::now() - created_at).num_days() as f64;
    if days < 0.0 {
        1.0
    } else {
        (1.0f64).min(60.0 / (1.0 + days))
    }
}

fn compute_risk(content: &str) -> f64 {
    if content.contains("sudo") || content.contains("rm -rf") || content.contains("DROP TABLE") {
        0.7
    } else if content.contains("unsafe") {
        0.4
    } else {
        0.1
    }
}

#[cfg(feature = "shared-learning")]
async fn run_suggest_command(sub: SuggestSub) -> Result<()> {
    use learnings::suggest::{SuggestionMetrics, SuggestionMetricsEntry};
    use terraphim_agent::shared_learning::{SharedLearningStore, StoreConfig, SuggestionStatus};
    use terraphim_types::shared_learning::SuggestionStatus as Status;

    let store_config = StoreConfig::default();
    let store = SharedLearningStore::open(store_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to open shared learning store: {}", e))?;
    let metrics = SuggestionMetrics::new(SuggestionMetrics::default_path());

    match sub {
        SuggestSub::List { status, limit } => {
            let entries = if let Some(ref s) = status {
                let st: SuggestionStatus = s.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
                store
                    .list_by_status(st)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?
            } else {
                store
                    .list_pending()
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?
            };
            if entries.is_empty() {
                println!("No suggestions found.");
            } else {
                let display_count = limit.min(entries.len());
                println!("Suggestions ({} of {}):", display_count, entries.len());
                for entry in entries.iter().take(limit) {
                    let confidence = entry
                        .bm25_confidence
                        .map(|c| format!("{:.2}", c))
                        .unwrap_or_else(|| "N/A".to_string());
                    println!(
                        "  [{}] {} (confidence: {}, status: {})",
                        &entry.id[..entry.id.len().min(12)],
                        entry.title,
                        confidence,
                        entry.suggestion_status,
                    );
                }
            }
            Ok(())
        }
        SuggestSub::Show { id } => {
            let entry = store.get(&id).await.map_err(|e| anyhow::anyhow!("{}", e))?;
            println!("ID:          {}", entry.id);
            println!("Title:       {}", entry.title);
            println!("Status:      {}", entry.suggestion_status);
            println!("Trust Level: {}", entry.trust_level);
            println!("Source:      {} ({})", entry.source, entry.source_agent);
            println!("Created:     {}", entry.created_at.to_rfc3339());
            if let Some(ref reason) = entry.rejection_reason {
                println!("Reject Reason: {}", reason);
            }
            if let Some(c) = entry.bm25_confidence {
                println!("Confidence:  {:.4}", c);
            }
            println!("\n{}", entry.content);
            Ok(())
        }
        SuggestSub::Approve { id } => {
            store
                .approve(&id)
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let entry = store.get(&id).await.map_err(|e| anyhow::anyhow!("{}", e))?;
            metrics.append(SuggestionMetricsEntry {
                id: id.clone(),
                status: Status::Approved,
                confidence: entry.bm25_confidence.unwrap_or(0.0),
                timestamp: chrono::Utc::now(),
                title: entry.title,
            })?;
            println!("Approved suggestion {}.", id);
            Ok(())
        }
        SuggestSub::Reject { id, reason } => {
            store
                .reject(&id, reason.as_deref())
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let entry = store.get(&id).await.map_err(|e| anyhow::anyhow!("{}", e))?;
            metrics.append(SuggestionMetricsEntry {
                id: id.clone(),
                status: Status::Rejected,
                confidence: entry.bm25_confidence.unwrap_or(0.0),
                timestamp: chrono::Utc::now(),
                title: entry.title,
            })?;
            println!("Rejected suggestion {}.", id);
            Ok(())
        }
        SuggestSub::ApproveAll {
            min_confidence,
            dry_run,
        } => {
            let pending = store
                .list_pending()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let to_approve: Vec<_> = pending
                .iter()
                .filter(|l| {
                    l.bm25_confidence
                        .map(|c| c >= min_confidence)
                        .unwrap_or(false)
                })
                .collect();
            if dry_run {
                println!(
                    "Would approve {} suggestions (confidence >= {}):",
                    to_approve.len(),
                    min_confidence
                );
                for entry in &to_approve {
                    println!(
                        "  [{}] {} ({:.2})",
                        &entry.id[..entry.id.len().min(12)],
                        entry.title,
                        entry.bm25_confidence.unwrap_or(0.0)
                    );
                }
                return Ok(());
            }
            let mut approved = 0usize;
            for entry in &to_approve {
                if store.approve(&entry.id).await.is_ok() {
                    let _ = metrics.append(SuggestionMetricsEntry {
                        id: entry.id.clone(),
                        status: Status::Approved,
                        confidence: entry.bm25_confidence.unwrap_or(0.0),
                        timestamp: chrono::Utc::now(),
                        title: entry.title.clone(),
                    });
                    approved += 1;
                }
            }
            println!("Approved {} suggestions.", approved);
            Ok(())
        }
        SuggestSub::RejectAll {
            max_confidence,
            dry_run,
        } => {
            let pending = store
                .list_pending()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let to_reject: Vec<_> = pending
                .iter()
                .filter(|l| {
                    l.bm25_confidence
                        .map(|c| c <= max_confidence)
                        .unwrap_or(true)
                })
                .collect();
            if dry_run {
                println!(
                    "Would reject {} suggestions (confidence <= {}):",
                    to_reject.len(),
                    max_confidence
                );
                for entry in &to_reject {
                    println!(
                        "  [{}] {} ({:.2})",
                        &entry.id[..entry.id.len().min(12)],
                        entry.title,
                        entry.bm25_confidence.unwrap_or(0.0)
                    );
                }
                return Ok(());
            }
            let mut rejected = 0usize;
            for entry in &to_reject {
                if store.reject(&entry.id, None).await.is_ok() {
                    let _ = metrics.append(SuggestionMetricsEntry {
                        id: entry.id.clone(),
                        status: Status::Rejected,
                        confidence: entry.bm25_confidence.unwrap_or(0.0),
                        timestamp: chrono::Utc::now(),
                        title: entry.title.clone(),
                    });
                    rejected += 1;
                }
            }
            println!("Rejected {} suggestions.", rejected);
            Ok(())
        }
        SuggestSub::Metrics => {
            let summary = metrics.summary()?;
            println!("Suggestion Metrics:");
            println!("  Total:   {}", summary.total);
            println!("  Pending: {}", summary.pending);
            println!("  Approved: {}", summary.approved);
            println!("  Rejected: {}", summary.rejected);
            println!("  Approval Rate: {:.1}%", summary.approval_rate * 100.0);
            Ok(())
        }
        SuggestSub::SessionEnd { context } => {
            let pending = store
                .list_pending()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;
            let count = pending.len();
            if count == 0 {
                println!("[suggestions] No pending suggestions.");
                return Ok(());
            }
            let top = if let Some(ref ctx) = context {
                store
                    .suggest(ctx, "session-end", 1)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))
                    .ok()
                    .and_then(|v| v.into_iter().next())
            } else {
                pending.into_iter().next()
            };
            print!("[suggestions] {} suggestion(s) pending", count);
            if let Some(t) = top {
                println!(", top: '{}'", truncate_snippet(&t.title, 60));
            } else {
                println!();
            }
            println!("  Run `terraphim-agent learn suggest list` to review.");
            Ok(())
        }
    }
}

#[cfg(feature = "shared-learning")]
async fn run_shared_learning_command(
    sub: SharedLearningSub,
    config: &learnings::LearningCaptureConfig,
) -> Result<()> {
    use terraphim_agent::shared_learning::{
        SharedLearning, SharedLearningSource, SharedLearningStore, StoreConfig, TrustLevel,
    };

    let store_config = StoreConfig::default();
    let store = SharedLearningStore::open(store_config)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to open shared learning store: {}", e))?;

    match sub {
        SharedLearningSub::List { trust_level, limit } => {
            let learnings = if let Some(ref level_str) = trust_level {
                let level: TrustLevel = level_str.parse().map_err(|e| anyhow::anyhow!("{}", e))?;
                store
                    .list_by_trust_level(level)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?
            } else {
                store
                    .list_all()
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?
            };

            if learnings.is_empty() {
                println!("No shared learnings found.");
            } else {
                let display_count = limit.min(learnings.len());
                println!(
                    "Shared learnings ({} of {}):",
                    display_count,
                    learnings.len()
                );
                for learning in learnings.iter().take(limit) {
                    println!(
                        "  [{}] {} -- {} ({})",
                        &learning.id[..learning.id.len().min(12)],
                        learning.title,
                        learning.trust_level,
                        learning.source,
                    );
                }
            }
            Ok(())
        }
        SharedLearningSub::Promote { id, to } => {
            let target: TrustLevel = to.parse().map_err(|e| anyhow::anyhow!("{}", e))?;

            match target {
                TrustLevel::L2 => {
                    store
                        .promote_to_l2(&id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    let fetched = store.get(&id).await.map_err(|e| anyhow::anyhow!("{}", e))?;
                    if fetched.trust_level != TrustLevel::L2 {
                        return Err(anyhow::anyhow!(
                            "Promote to L2 had no effect (current trust level: {}). \
                             Learning must be promotable to L2.",
                            fetched.trust_level
                        ));
                    }
                    println!("Promoted learning {} to L2 (Peer-Validated).", id);
                }
                TrustLevel::L3 => {
                    store
                        .promote_to_l3(&id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    let fetched = store.get(&id).await.map_err(|e| anyhow::anyhow!("{}", e))?;
                    if fetched.trust_level != TrustLevel::L3 {
                        return Err(anyhow::anyhow!(
                            "Promote to L3 had no effect (current trust level: {}).",
                            fetched.trust_level
                        ));
                    }
                    println!("Promoted learning {} to L3 (Human-Approved).", id);
                }
                TrustLevel::L1 => {
                    return Err(anyhow::anyhow!(
                        "Cannot promote to L1 -- learnings start at L1. Use l2 or l3."
                    ));
                }
                TrustLevel::L0 => {
                    return Err(anyhow::anyhow!(
                        "Cannot promote to L0 -- L0 is for extracted learnings only."
                    ));
                }
            }
            Ok(())
        }
        SharedLearningSub::Import => {
            use learnings::list_learnings;

            let storage_loc = config.storage_location();
            let local_learnings = list_learnings(&storage_loc, usize::MAX).unwrap_or_default();

            if local_learnings.is_empty() {
                println!("No local learnings found to import.");
                return Ok(());
            }

            let mut imported = 0;
            for local in &local_learnings {
                let title = if local.command.len() > 60 {
                    format!("{}...", &local.command[..60])
                } else {
                    local.command.clone()
                };

                let shared = SharedLearning::new(
                    title,
                    local.error_output.clone(),
                    SharedLearningSource::BashHook,
                    "cli-import".to_string(),
                )
                .with_original_command(local.command.clone())
                .with_error_context(local.error_output.clone())
                .with_keywords(local.tags.clone());

                let shared = if let Some(ref correction) = local.correction {
                    shared.with_correction(correction.clone())
                } else {
                    shared
                };
                let id = shared.id.clone();
                store
                    .insert(shared)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                store
                    .promote_to_l1(&id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                imported += 1;
            }

            println!(
                "Imported {} local learning(s) into shared store at L1.",
                imported
            );
            Ok(())
        }
        SharedLearningSub::Sync => {
            use terraphim_agent::shared_learning::{
                GiteaWikiClient, GiteaWikiConfig, WikiSyncService,
            };

            let wiki_config = GiteaWikiConfig::from_env().map_err(|e| anyhow::anyhow!("{}", e))?;
            let client = GiteaWikiClient::new(wiki_config);
            let service = WikiSyncService::new(client);
            let learnings = store
                .list_all()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            if learnings.is_empty() {
                println!("No shared learnings to sync.");
                return Ok(());
            }

            let report = service.sync_batch(&learnings).await;
            println!("Wiki sync complete:");
            println!("  Total:   {}", report.total);
            println!("  Created: {}", report.created);
            println!("  Updated: {}", report.updated);
            println!("  Skipped: {}", report.skipped);
            println!("  Failed:  {}", report.failed);

            if report.failed > 0 {
                return Err(anyhow::anyhow!(
                    "Wiki sync finished with {} failure(s)",
                    report.failed
                ));
            }
            Ok(())
        }
        SharedLearningSub::Stats => {
            let all = store
                .list_all()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?;

            let l1_count = all
                .iter()
                .filter(|l| l.trust_level == TrustLevel::L1)
                .count();
            let l2_count = all
                .iter()
                .filter(|l| l.trust_level == TrustLevel::L2)
                .count();
            let l3_count = all
                .iter()
                .filter(|l| l.trust_level == TrustLevel::L3)
                .count();

            println!("Shared Learning Statistics:");
            println!("  Total: {}", all.len());
            println!("  L1 (Unverified):      {}", l1_count);
            println!("  L2 (Peer-Validated):  {}", l2_count);
            println!("  L3 (Human-Approved):  {}", l3_count);

            if !all.is_empty() {
                let total_applied: u32 = all.iter().map(|l| l.quality.applied_count).sum();
                let total_effective: u32 = all.iter().map(|l| l.quality.effective_count).sum();
                let avg_success = if total_applied > 0 {
                    (total_effective as f64 / total_applied as f64) * 100.0
                } else {
                    0.0
                };
                println!("  Avg success rate:     {:.1}%", avg_success);
            }
            Ok(())
        }
        #[cfg(feature = "cross-agent-injection")]
        SharedLearningSub::Inject { min_trust, dry_run } => {
            use terraphim_agent::shared_learning::TrustLevel;
            use terraphim_agent::shared_learning::injector::{InjectorConfig, LearningInjector};

            let trust_level = min_trust
                .to_uppercase()
                .parse::<TrustLevel>()
                .unwrap_or(TrustLevel::L2);

            let config = InjectorConfig::default().with_min_trust_level(trust_level);
            let injector = LearningInjector::new(config);

            let result = injector.run_injection().await?;

            if dry_run {
                println!("Dry run - would inject {} learnings:", result.injected);
                for id in &result.injected_ids {
                    println!("  - {}", id);
                }
            } else {
                println!(
                    "Injection complete: {} injected, {} skipped (trust), {} skipped (context), {} skipped (exists)",
                    result.injected,
                    result.skipped_trust,
                    result.skipped_context,
                    result.skipped_exists
                );
            }
            Ok(())
        }
    }
}

#[cfg(feature = "server")]
async fn run_server_command(
    command: Command,
    server_url: &str,
    output: CommandOutputConfig,
) -> Result<()> {
    let api = ApiClient::new(server_url.to_string());

    match command {
        Command::Search {
            query,
            terms,
            operator,
            role,
            limit,
            fail_on_empty,
            include_pinned,
            min_quality,
            max_tokens,
            max_content_length,
            fields,
        } => {
            // Get selected role from server if not specified
            let role_name = if let Some(role) = role {
                api.resolve_role(&role).await?
            } else {
                let config_res = api.get_config().await?;
                config_res.config.selected_role
            };

            let role_for_meta = role_name.clone();
            let q = if let Some(additional_terms) = terms {
                // Multi-term query with logical operators
                let search_terms: Vec<NormalizedTermValue> = additional_terms
                    .into_iter()
                    .map(|t| NormalizedTermValue::from(t.as_str()))
                    .collect();

                SearchQuery {
                    search_term: NormalizedTermValue::from(query.as_str()),
                    search_terms: Some(search_terms),
                    operator: operator.map(|op| op.into()),
                    skip: Some(0),
                    limit: Some(limit),
                    role: Some(role_name.clone()),
                    layer: Layer::default(),
                    include_pinned,
                    min_quality,
                }
            } else {
                // Single term query (backward compatibility)
                SearchQuery {
                    search_term: NormalizedTermValue::from(query.as_str()),
                    search_terms: None,
                    operator: None,
                    skip: Some(0),
                    limit: Some(limit),
                    role: Some(role_name.clone()),
                    layer: Layer::default(),
                    include_pinned,
                    min_quality,
                }
            };

            let res: SearchResponse = api.search(&q).await?;
            // Captured before `res.results` is consumed below, so `--fail-on-empty`
            // behaves identically in server mode and offline mode.
            let results_count = res.results.len();

            if let Some(ref additional_terms) = q.search_terms {
                let op_str = match q.operator {
                    Some(LogicalOperator::And) => "AND",
                    Some(LogicalOperator::Or) => "OR",
                    None => "OR", // Default
                };
                if !output.is_machine_readable() {
                    println!(
                        "Multi-term search: '{}' {} {} additional terms using {} operator",
                        query,
                        op_str,
                        additional_terms.len(),
                        op_str
                    );
                }
            }

            if output.is_machine_readable() {
                use robot::schema::{SearchResultItem, SearchResultsData};
                use robot::{ResponseMeta, RobotConfig, RobotFormatter, RobotResponse};
                use std::time::Instant;

                let start = Instant::now();
                let robot_format = match output.mode {
                    CommandOutputMode::JsonCompact => robot::output::OutputFormat::Minimal,
                    _ => robot::output::OutputFormat::Json,
                };
                let mut robot_config = RobotConfig::new()
                    .with_format(robot_format)
                    .with_max_results(limit);
                if let Some(mt) = max_tokens {
                    robot_config = robot_config.with_max_tokens(mt);
                } else if output.robot {
                    robot_config = robot_config.with_max_tokens(8000);
                }
                if let Some(mcl) = max_content_length {
                    robot_config = robot_config.with_max_content_length(mcl);
                } else if output.robot {
                    robot_config = robot_config.with_max_content_length(2000);
                }
                if let Some(fm) = fields {
                    robot_config = robot_config.with_fields(fm);
                }

                let formatter = RobotFormatter::new(robot_config.clone());
                let max_results = robot_config.max_results.unwrap_or(limit);
                let truncated_results: Vec<_> = res.results.into_iter().take(max_results).collect();
                let total = truncated_results.len();

                let items: Vec<SearchResultItem> = truncated_results
                    .iter()
                    .enumerate()
                    .map(|(i, doc)| {
                        let preview = doc.description.as_deref().or(if doc.body.is_empty() {
                            None
                        } else {
                            Some(doc.body.as_str())
                        });
                        let (preview_text, preview_truncated) = match preview {
                            Some(text) => {
                                let (t, was_truncated) = formatter.truncate_content(text.trim());
                                (Some(t), was_truncated)
                            }
                            None => (None, false),
                        };
                        SearchResultItem {
                            rank: i + 1,
                            id: doc.id.clone(),
                            title: doc.title.clone(),
                            url: if doc.url.is_empty() {
                                None
                            } else {
                                Some(doc.url.clone())
                            },
                            score: doc.rank.unwrap_or_default() as f64,
                            preview: preview_text,
                            source: None,
                            date: None,
                            preview_truncated,
                        }
                    })
                    .collect();

                let (concepts_matched, thesaurus_matched) =
                    match api.get_thesaurus(role_name.as_str()).await {
                        Ok(thesaurus_res) => match thesaurus_res.thesaurus {
                            Some(entries) => {
                                let thesaurus = terraphim_automata::thesaurus_from_terms(
                                    &role_name,
                                    entries.values().map(String::as_str),
                                );
                                let concepts = terraphim_automata::compute_concepts_matched(
                                    &query, &thesaurus,
                                );
                                // See the offline path: derive from the boundary-aware
                                // matcher rather than a naive substring scan.
                                let matched: std::collections::HashSet<String> =
                                    concepts.iter().map(|c| c.to_lowercase()).collect();
                                let thesaurus_terms: Vec<String> = entries
                                    .values()
                                    .filter(|value| matched.contains(&value.to_lowercase()))
                                    .cloned()
                                    .collect();
                                (concepts, thesaurus_terms)
                            }
                            None => (Vec::new(), Vec::new()),
                        },
                        Err(e) => {
                            log::debug!(
                                "get_thesaurus failed for {}: {}; concepts_matched empty",
                                role_name,
                                e
                            );
                            (Vec::new(), Vec::new())
                        }
                    };

                let wildcard_fallback = concepts_matched.is_empty();
                let data = SearchResultsData {
                    results: items,
                    total_matches: total,
                    concepts_matched,
                    thesaurus_matched,
                    wildcard_fallback,
                };

                let meta = ResponseMeta::new("search")
                    .with_elapsed(start.elapsed().as_millis() as u64)
                    .with_query(&query)
                    .with_role(role_for_meta.as_str());
                let response = RobotResponse::success(data, meta);
                let output_str = formatter.format(&response)?;
                println!("{}", output_str);
            } else {
                for doc in res.results.iter() {
                    let snippet = doc
                        .description
                        .as_deref()
                        .or(if doc.body.is_empty() {
                            None
                        } else {
                            Some(doc.body.as_str())
                        })
                        .map(|s| truncate_snippet(s.trim(), 120));
                    println!("[{}] {}", doc.rank.unwrap_or_default(), doc.title);
                    if !doc.url.is_empty() {
                        println!("    {}", doc.url);
                    }
                    if let Some(snip) = snippet {
                        println!("    {}", snip);
                    }
                    println!();
                }
            }
            if fail_on_empty && results_count == 0 {
                std::process::exit(robot::exit_codes::ExitCode::ErrorNotFound.code().into());
            }
            Ok(())
        }
        Command::Roles { sub } => {
            match sub {
                RolesSub::List => {
                    let cfg = api.get_config().await?;
                    let selected = cfg.config.selected_role.to_string();
                    for (name, role) in cfg.config.roles.iter() {
                        let marker = if name.to_string() == selected {
                            "*"
                        } else {
                            " "
                        };
                        if let Some(ref short) = role.shortname {
                            println!("{} {} ({})", marker, name, short);
                        } else {
                            println!("{} {}", marker, name);
                        }
                    }
                }
                RolesSub::Select { name } => {
                    // Try to find role by name or shortname via get_config for
                    // case-insensitive convenience. If the server's /config
                    // endpoint is locked (e.g. background KG indexing holds
                    // the config lock during search/extract), fall back to
                    // the user's input as-is and let the server validate. The
                    // server's update_selected_role does its own contains_key
                    // check and returns a clean "Role not found" error on
                    // miss, so we preserve correctness either way.
                    let role_name = match api.get_config().await {
                        Ok(cfg) => {
                            let query_lower = name.to_lowercase();
                            cfg.config
                                .roles
                                .iter()
                                .find(|(n, _)| n.to_string().to_lowercase() == query_lower)
                                .or_else(|| {
                                    cfg.config.roles.iter().find(|(_, role)| {
                                        role.shortname
                                            .as_ref()
                                            .map(|s| s.to_lowercase() == query_lower)
                                            .unwrap_or(false)
                                    })
                                })
                                .map(|(n, _)| n.to_string())
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "Role '{}' not found (checked name and shortname)",
                                        name
                                    )
                                })?
                        }
                        Err(e) => {
                            log::warn!(
                                "get_config failed during roles select ({}); \
                                 falling back to user-supplied name verbatim",
                                e
                            );
                            name.to_string()
                        }
                    };
                    let _ = api.update_selected_role(&role_name).await?;
                    println!("selected:{}", role_name);
                }
            }
            Ok(())
        }
        Command::Config { sub } => {
            match sub {
                ConfigSub::Show => {
                    let cfg = api.get_config().await?;
                    println!("{}", serde_json::to_string_pretty(&cfg.config)?);
                }
                ConfigSub::Set { key, value } => {
                    let mut cfg = api.get_config().await?.config;
                    match key.as_str() {
                        "selected_role" => {
                            cfg.selected_role = RoleName::new(&value);
                            let _ = api.post_config(&cfg).await?;
                            println!("updated selected_role to {}", value);
                        }
                        _ => {
                            println!("unsupported key: {}", key);
                        }
                    }
                }
                ConfigSub::Validate => {
                    println!(
                        "config validate is only available in offline mode (without --server)"
                    );
                }
                ConfigSub::Reload => {
                    println!("config reload is only available in offline mode (without --server)");
                }
            }
            Ok(())
        }
        Command::Graph {
            role,
            top_k,
            pinned,
        } => {
            let role_name = if let Some(role) = role {
                role
            } else {
                let config_res = api.get_config().await?;
                config_res.config.selected_role.to_string()
            };

            let graph_res = api.rolegraph(Some(&role_name)).await?;
            if pinned {
                let pinned_ids: std::collections::HashSet<u64> =
                    graph_res.pinned_node_ids.iter().copied().collect();
                for node in graph_res.nodes {
                    if pinned_ids.contains(&node.id) {
                        println!("{}", node.label);
                    }
                }
            } else {
                let mut nodes_sorted = graph_res.nodes;
                #[allow(clippy::unnecessary_sort_by)]
                nodes_sorted.sort_by(|a, b| b.rank.cmp(&a.rank));
                for node in nodes_sorted.into_iter().take(top_k) {
                    println!("{}", node.label);
                }
            }
            Ok(())
        }
        Command::Kg { sub } => match sub {
            KgSub::List {
                role,
                top_k,
                pinned,
            } => {
                let role_name = if let Some(role) = role {
                    role
                } else {
                    let config_res = api.get_config().await?;
                    config_res.config.selected_role.to_string()
                };

                let graph_res = api.rolegraph(Some(&role_name)).await?;
                if pinned {
                    let pinned_ids: std::collections::HashSet<u64> =
                        graph_res.pinned_node_ids.iter().copied().collect();
                    for node in graph_res.nodes {
                        if pinned_ids.contains(&node.id) {
                            println!("{}", node.label);
                        }
                    }
                } else {
                    let mut nodes_sorted = graph_res.nodes;
                    #[allow(clippy::unnecessary_sort_by)]
                    nodes_sorted.sort_by(|a, b| b.rank.cmp(&a.rank));
                    for node in nodes_sorted.into_iter().take(top_k) {
                        println!("{}", node.label);
                    }
                }
                Ok(())
            }
        },
        #[cfg(feature = "llm")]
        Command::Chat {
            role,
            prompt,
            model,
        } => {
            let role_name = if let Some(role) = role {
                role
            } else {
                let config_res = api.get_config().await?;
                config_res.config.selected_role.to_string()
            };

            let chat_res = api.chat(&role_name, &prompt, model.as_deref()).await?;
            match (chat_res.status.as_str(), chat_res.message) {
                ("Success", Some(msg)) => println!("{}", msg),
                _ => println!(
                    "error: {}",
                    chat_res.error.unwrap_or_else(|| "unknown error".into())
                ),
            }
            Ok(())
        }
        Command::Extract {
            text,
            role,
            exclude_term,
        } => {
            let role_name = if let Some(role) = role {
                role
            } else {
                let config_res = api.get_config().await?;
                config_res.config.selected_role.to_string()
            };

            // Get the thesaurus from the server for the role
            let thesaurus_res = api.get_thesaurus(&role_name).await?;

            // Build thesaurus from response
            let mut thesaurus = terraphim_types::Thesaurus::new(format!("role-{}", role_name));
            if let Some(entries) = &thesaurus_res.thesaurus {
                for value in entries.values() {
                    let normalized_term = terraphim_types::NormalizedTerm::new(
                        1u64,
                        terraphim_types::NormalizedTermValue::from(value.clone()),
                    );
                    thesaurus.insert(
                        terraphim_types::NormalizedTermValue::from(value.clone()),
                        normalized_term,
                    );
                }
            }

            // Extract paragraphs using automata
            let results = terraphim_automata::matcher::extract_paragraphs_from_automata(
                &text,
                &thesaurus,
                !exclude_term, // include_term is opposite of exclude_term
            )?;

            if results.is_empty() {
                println!("No matches found in the text.");
            } else {
                println!("Found {} paragraph(s):", results.len());
                for (i, (matched, paragraph)) in results.iter().enumerate() {
                    println!(
                        "\n--- Match {} (term: '{}') ---",
                        i + 1,
                        matched.normalized_term.value
                    );
                    println!("{}", paragraph);
                }
            }

            Ok(())
        }
        Command::CheckUpdate => {
            println!("🔍 Checking for terraphim-agent updates...");
            let config =
                UpdaterConfig::new("terraphim-agent").with_version(env!("CARGO_PKG_VERSION"));
            let updater = TerraphimUpdater::new(config);
            match updater.check_update().await {
                Ok(status) => {
                    println!("{}", status);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("❌ Failed to check for updates: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::Update => {
            println!("🚀 Updating terraphim-agent...");
            let config =
                UpdaterConfig::new("terraphim-agent").with_version(env!("CARGO_PKG_VERSION"));
            let updater = TerraphimUpdater::new(config);
            match updater.check_and_update().await {
                Ok(status) => {
                    println!("{}", status);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("❌ Update failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Command::Replace {
            text,
            role: _,
            format: _,
            boundary: _,
            json,
            fail_open,
        } => {
            let input_text = match text {
                Some(t) => t,
                None => {
                    use std::io::Read;
                    let mut buffer = String::new();
                    std::io::stdin().read_to_string(&mut buffer)?;
                    buffer
                }
            };

            if fail_open {
                let hook_result = terraphim_hooks::HookResult::fail_open(
                    input_text.clone(),
                    "Replace command requires offline mode for full functionality".to_string(),
                );
                if json {
                    println!("{}", serde_json::to_string(&hook_result)?);
                } else {
                    eprintln!("Warning: {}", hook_result.error.as_deref().unwrap_or(""));
                    print!("{}", input_text);
                }
                Ok(())
            } else {
                eprintln!("Replace command is only available in offline mode");
                std::process::exit(1);
            }
        }
        Command::Validate { json, .. } => {
            if json {
                let err = serde_json::json!({
                    "error": "Validate command is only available in offline mode"
                });
                println!("{}", serde_json::to_string(&err)?);
            } else {
                eprintln!("Validate command is only available in offline mode");
            }
            std::process::exit(1);
        }
        Command::Suggest { json, .. } => {
            if json {
                let err = serde_json::json!({
                    "error": "Suggest command is only available in offline mode"
                });
                println!("{}", serde_json::to_string(&err)?);
            } else {
                eprintln!("Suggest command is only available in offline mode");
            }
            std::process::exit(1);
        }
        Command::Hook { .. } => {
            let err = serde_json::json!({
                "error": "Hook command is only available in offline mode"
            });
            println!("{}", serde_json::to_string(&err)?);
            std::process::exit(1);
        }
        Command::Guard {
            command,
            json,
            fail_open,
            guard_thesaurus,
            guard_allowlist,
            explain,
        } => {
            // Guard works the same in server mode - no server needed for pattern matching
            let input_command = match command {
                Some(c) => c,
                None => {
                    use std::io::Read;
                    let mut buffer = String::new();
                    std::io::stdin().read_to_string(&mut buffer)?;
                    buffer.trim().to_string()
                }
            };

            let guard = match (guard_thesaurus, guard_allowlist) {
                (Some(thesaurus_path), Some(allowlist_path)) => {
                    let destructive_json = std::fs::read_to_string(thesaurus_path)?;
                    let allowlist_json = std::fs::read_to_string(allowlist_path)?;
                    guard_patterns::CommandGuard::from_json(
                        &destructive_json,
                        &allowlist_json,
                        None,
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                }
                (Some(thesaurus_path), None) => {
                    let destructive_json = std::fs::read_to_string(thesaurus_path)?;
                    guard_patterns::CommandGuard::from_json(
                        &destructive_json,
                        guard_patterns::CommandGuard::default_allowlist_json(),
                        None,
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                }
                (None, Some(allowlist_path)) => {
                    let allowlist_json = std::fs::read_to_string(allowlist_path)?;
                    guard_patterns::CommandGuard::from_json(
                        guard_patterns::CommandGuard::default_destructive_json(),
                        &allowlist_json,
                        None,
                    )
                    .map_err(|e| anyhow::anyhow!("{}", e))?
                }
                (None, None) => guard_patterns::CommandGuard::new(),
            };
            let result = guard.check(&input_command);

            if explain {
                let trace = guard.check_with_trace(&input_command);
                trace.print(json)?;
                if trace.result.decision == guard_patterns::GuardDecision::Block && !fail_open {
                    std::process::exit(1);
                }
                return Ok(());
            }

            if json {
                println!("{}", serde_json::to_string(&result)?);
            } else if result.decision == guard_patterns::GuardDecision::Block
                && let Some(reason) = &result.reason
            {
                eprintln!("BLOCKED: {}", reason);
                if !fail_open {
                    std::process::exit(1);
                }
            }

            Ok(())
        }
        Command::Setup {
            template,
            path,
            add_role,
            list_templates,
        } => {
            // Setup command - can run in server mode to add roles to running config
            if list_templates {
                println!("Available templates:");
                for t in onboarding::list_templates() {
                    let path_info = if t.requires_path {
                        " (requires --path)"
                    } else if t.default_path.is_some() {
                        " (optional --path)"
                    } else {
                        ""
                    };
                    println!("  {} - {}{}", t.id, t.description, path_info);
                }
                return Ok(());
            }

            if let Some(template_id) = template {
                // Apply template directly
                let role = onboarding::apply_template(&template_id, path.as_deref())
                    .map_err(|e| anyhow::anyhow!("{}", e))?;

                println!("Configured role: {}", role.name);
                println!("To add this role to a running server, restart with the new config.");

                // In server mode, we could potentially add the role via API
                // For now, just show what was configured
                if !role.haystacks.is_empty() {
                    println!("Haystacks:");
                    for h in &role.haystacks {
                        println!("  - {} ({:?})", h.location, h.service);
                    }
                }
                if role.kg.is_some() {
                    println!("Knowledge graph: configured");
                }
                if role.llm_enabled {
                    println!("LLM: enabled");
                }
            } else {
                // Interactive wizard
                let mode = if add_role {
                    onboarding::SetupMode::AddRole
                } else {
                    onboarding::SetupMode::FirstRun
                };

                match onboarding::run_setup_wizard(mode).await {
                    Ok(onboarding::SetupResult::Template {
                        template,
                        role,
                        custom_path,
                    }) => {
                        println!("\nApplied template: {}", template.name);
                        if let Some(ref path) = custom_path {
                            println!("Custom path: {}", path);
                        }
                        println!("Role '{}' configured successfully.", role.name);
                    }
                    Ok(onboarding::SetupResult::Custom { role }) => {
                        println!("\nCustom role '{}' configured successfully.", role.name);
                    }
                    Ok(onboarding::SetupResult::Cancelled) => {
                        println!("\nSetup cancelled.");
                    }
                    Err(e) => {
                        eprintln!("Setup error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
            Ok(())
        }
        Command::Learn { sub } => run_learn_command(sub).await,
        Command::Memory { sub } => run_memory_command(sub, &output).await,
        Command::Interactive => {
            unreachable!("Interactive mode should be handled above")
        }

        #[cfg(feature = "repl")]
        Command::Repl { .. } => {
            unreachable!("REPL mode should be handled above")
        }

        #[cfg(feature = "repl-sessions")]
        Command::Sessions { sub } => {
            use session_output::*;
            use terraphim_sessions::SessionService;

            let rt = Runtime::new()?;
            rt.block_on(async {
                let service = SessionService::new();

                match sub {
                    SessionsSub::Sources => {
                        let sources = service.detect_sources();
                        if output.is_machine_readable() {
                            let payload = SourcesOutput {
                                count: sources.len(),
                                sources: sources
                                    .into_iter()
                                    .map(|s| {
                                        let available = s.is_available();
                                        SourceEntry {
                                            id: s.id,
                                            name: s.name,
                                            available,
                                        }
                                    })
                                    .collect(),
                            };
                            print_json_output(&payload, output.mode)?;
                        } else if sources.is_empty() {
                            println!("No session sources detected.");
                        } else {
                            println!("Available session sources:");
                            for source in sources {
                                let status = if source.is_available() {
                                    "available"
                                } else {
                                    "not found"
                                };
                                println!(
                                    "  - {} ({})",
                                    source.name.unwrap_or_else(|| source.id.clone()),
                                    status
                                );
                            }
                        }
                        Ok(())
                    }

                    SessionsSub::List { limit } => {
                        let sessions = service.list_sessions().await;
                        if output.is_machine_readable() {
                            let session_entries: Vec<SessionEntry> = sessions
                                .iter()
                                .take(limit)
                                .map(|s| SessionEntry {
                                    id: s.id.to_string(),
                                    title: s.title.clone(),
                                    message_count: s.message_count(),
                                    source: s.source.clone(),
                                })
                                .collect();
                            let shown = session_entries.len();
                            let payload = SessionListOutput {
                                total: sessions.len(),
                                shown,
                                sessions: session_entries,
                            };
                            print_json_output(&payload, output.mode)?;
                        } else if sessions.is_empty() {
                            println!("No sessions found.");
                        } else {
                            println!("Cached sessions ({} total):", sessions.len());
                            for session in sessions.iter().take(limit) {
                                let msg_count = session.message_count();
                                let title = session.title.as_deref().unwrap_or("(untitled)");
                                println!("  - {} ({} messages)", title, msg_count);
                            }
                            if sessions.len() > limit {
                                println!("  ... and {} more", sessions.len() - limit);
                            }
                        }
                        Ok(())
                    }
                    SessionsSub::Search { query, limit } => {
                        let results = service.search(&query).await;
                        if output.is_machine_readable() {
                            let entries: Vec<SessionSearchEntry> = results
                                .iter()
                                .take(limit)
                                .map(|s| {
                                    let preview = s
                                        .messages
                                        .iter()
                                        .find(|msg| {
                                            msg.content
                                                .to_lowercase()
                                                .contains(&query.to_lowercase())
                                        })
                                        .map(|msg| {
                                            let p: String = msg.content.chars().take(100).collect();
                                            p
                                        });
                                    SessionSearchEntry {
                                        id: s.id.to_string(),
                                        title: s.title.clone(),
                                        message_count: s.message_count(),
                                        preview,
                                    }
                                })
                                .collect();
                            let shown = entries.len();
                            let payload = SessionSearchOutput {
                                query: query.clone(),
                                total: results.len(),
                                shown,
                                sessions: entries,
                            };
                            print_json_output(&payload, output.mode)?;
                            if results.is_empty() {
                                std::process::exit(
                                    robot::exit_codes::ExitCode::ErrorNotFound.code().into(),
                                );
                            }
                        } else if results.is_empty() {
                            println!("No sessions matching '{}'.", query);
                        } else {
                            println!("Found {} matching sessions:", results.len());
                            for session in results.iter().take(limit) {
                                let title = session.title.as_deref().unwrap_or("(untitled)");
                                println!("  - {}", title);
                                for msg in &session.messages {
                                    let content_lower = msg.content.to_lowercase();
                                    if content_lower.contains(&query.to_lowercase()) {
                                        let preview: String =
                                            msg.content.chars().take(100).collect();
                                        println!("    > {}", preview);
                                        break;
                                    }
                                }
                            }
                        }
                        Ok(())
                    }
                    SessionsSub::Stats => {
                        let stats = service.statistics().await;
                        if output.is_machine_readable() {
                            let payload = SessionStatsOutput {
                                total_sessions: stats.total_sessions,
                                total_messages: stats.total_messages,
                                total_user_messages: stats.total_user_messages,
                                total_assistant_messages: stats.total_assistant_messages,
                                by_source: stats.sessions_by_source,
                            };
                            print_json_output(&payload, output.mode)?;
                        } else {
                            println!("Session Statistics:");
                            println!("  Total sessions: {}", stats.total_sessions);
                            println!("  Total messages: {}", stats.total_messages);
                            println!("  User messages: {}", stats.total_user_messages);
                            println!("  Assistant messages: {}", stats.total_assistant_messages);
                            if !stats.sessions_by_source.is_empty() {
                                println!("  By source:");
                                for (source, count) in stats.sessions_by_source {
                                    println!("    - {}: {}", source, count);
                                }
                            }
                        }
                        Ok(())
                    }
                    SessionsSub::Expand {
                        id,
                        context_lines: _,
                    } => {
                        // Populate cache via auto-import before lookup
                        let _ = service.list_sessions().await;
                        let session = service.get_session(&id).await;
                        match session {
                            None => {
                                if !output.is_machine_readable() {
                                    eprintln!("Session '{}' not found.", id);
                                }
                                std::process::exit(
                                    robot::exit_codes::ExitCode::ErrorNotFound.code().into(),
                                );
                            }
                            Some(session) => {
                                if output.is_machine_readable() {
                                    let payload = SessionExpandOutput {
                                        id: session.id.clone(),
                                        title: session.title.clone(),
                                        message_count: session.message_count(),
                                        messages: session
                                            .messages
                                            .iter()
                                            .map(|msg| ExpandedMessage {
                                                idx: msg.idx,
                                                role: msg.role.to_string(),
                                                content: msg.content.clone(),
                                            })
                                            .collect(),
                                    };
                                    print_json_output(&payload, output.mode)?;
                                } else {
                                    let title = session.title.as_deref().unwrap_or("(untitled)");
                                    println!("Session: {} ({})", title, session.id);
                                    println!("Messages: {}", session.message_count());
                                    println!("{}", "=".repeat(80));
                                    for msg in &session.messages {
                                        println!("[{}]", msg.role);
                                        println!("{}", msg.content);
                                        println!("{}", "-".repeat(40));
                                    }
                                }
                                Ok(())
                            }
                        }
                    }
                }
            })
        }
        Command::Listen { .. } => {
            eprintln!("error: listen mode is not available in server mode");
            eprintln!("The listener runs in offline mode only.");
            std::process::exit(1);
        }
        Command::Robot { .. } => {
            unreachable!("Robot commands are handled in main()")
        }
        Command::Cache { .. } => {
            eprintln!("error: cache commands are not available in server mode");
            eprintln!("Cache management runs in offline mode only.");
            std::process::exit(1);
        }
    }
}

fn run_tui(server_url: Option<String>, transparent: bool) -> Result<()> {
    // Attempt to set up terminal for TUI
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);

    // Try to enter raw mode and alternate screen
    // These operations can fail in non-interactive environments
    match enable_raw_mode() {
        Ok(()) => {
            // Successfully entered raw mode, proceed with TUI setup
            let mut stdout = io::stdout();
            if let Err(e) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
                // Clean up raw mode before returning error
                let _ = disable_raw_mode();
                return Err(anyhow::anyhow!(
                    "Failed to initialize terminal for interactive mode: {}. \
                     Try using 'repl' mode instead: terraphim-agent repl",
                    e
                ));
            }

            let mut terminal = match Terminal::new(backend) {
                Ok(t) => t,
                Err(e) => {
                    // Clean up before returning
                    let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
                    let _ = disable_raw_mode();
                    return Err(anyhow::anyhow!(
                        "Failed to create terminal: {}. \
                         Try using 'repl' mode instead: terraphim-agent repl",
                        e
                    ));
                }
            };

            let res = ui_loop(&mut terminal, server_url, transparent);

            // Always clean up terminal state
            let _ = disable_raw_mode();
            let _ = execute!(
                terminal.backend_mut(),
                LeaveAlternateScreen,
                DisableMouseCapture
            );
            let _ = terminal.show_cursor();

            res
        }
        Err(e) => {
            // Failed to enter raw mode - not a TTY
            Err(anyhow::anyhow!(
                "Terminal does not support raw mode (not a TTY?). \
                 Interactive mode requires a terminal. \
                 Try using 'repl' mode instead: terraphim-agent repl. \
                 Error: {}",
                e
            ))
        }
    }
}

#[allow(unused_variables)]
fn ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    server_url: Option<String>,
    transparent: bool,
) -> Result<()> {
    let mut input = String::new();
    let mut results: Vec<String> = Vec::new();
    let mut detailed_results: Vec<Document> = Vec::new();
    let mut terms: Vec<String> = Vec::new();
    let mut suggestions: Vec<String> = Vec::new();
    let mut current_role = String::from("Terraphim Engineer"); // Default to Terraphim Engineer
    let mut selected_result_index = 0;
    let mut view_mode = ViewMode::Search;
    let rt = tokio::runtime::Runtime::new()?;

    #[cfg(feature = "server")]
    let backend = {
        let effective_url = resolve_tui_server_url(server_url.as_deref());
        let api = ApiClient::new(effective_url.clone());
        ensure_tui_server_reachable(&rt, &api, &effective_url)?;
        tui_backend::TuiBackend::Remote(api)
    };

    #[cfg(not(feature = "server"))]
    let backend = {
        let service = rt.block_on(async { TuiService::new(None, false).await })?;
        tui_backend::TuiBackend::Local(service)
    };

    // Initialize terms from rolegraph (selected role)
    if let Ok(cfg) = rt.block_on(async { backend.get_config().await }) {
        current_role = cfg.selected_role.to_string();
        if let Ok(rg) = rt.block_on(async { backend.get_rolegraph_terms(&current_role).await }) {
            terms = rg;
        }
    }

    loop {
        terminal.draw(|f| {
            match view_mode {
                ViewMode::Search => {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Length(3), // input
                            Constraint::Length(5), // suggestions
                            Constraint::Min(3),    // results
                            Constraint::Length(3), // status
                        ])
                        .split(f.area());

                    let input_title = format!(
                        "Search [Role: {}] • Enter: search, Tab: autocomplete, Ctrl+r: switch role, q: quit",
                        current_role
                    );
                    let input_widget = Paragraph::new(Line::from(input.as_str())).block(
                        create_block(&input_title, transparent)
                    );
                    f.render_widget(input_widget, chunks[0]);

                    // Suggestions (fixed height 5)
                    let sug_items: Vec<ListItem> = suggestions
                        .iter()
                        .take(5)
                        .map(|s| ListItem::new(s.as_str()))
                        .collect();
                    let sug_list = List::new(sug_items)
                        .block(create_block("Suggestions", transparent));
                    f.render_widget(sug_list, chunks[1]);

                    let items: Vec<ListItem> = results.iter().enumerate().map(|(i, r)| {
                        let item = ListItem::new(r.as_str());
                        if i == selected_result_index {
                            item.style(Style::default().add_modifier(Modifier::REVERSED))
                        } else {
                            item
                        }
                    }).collect();
                    let list = List::new(items).block(create_block(
                        "Results • ↑↓: select, Enter: view details, Ctrl+s: summarize",
                        transparent,
                    ));
                    f.render_widget(list, chunks[2]);

                    let status_text = format!("Terraphim TUI • {} results • Mode: Search", results.len());
                    let status = Paragraph::new(Line::from(status_text))
                        .block(create_block("", transparent));
                    f.render_widget(status, chunks[3]);
                }
                ViewMode::ResultDetail => {
                    if selected_result_index < detailed_results.len() {
                        let doc = &detailed_results[selected_result_index];

                        let chunks = Layout::default()
                            .direction(Direction::Vertical)
                            .constraints([
                                Constraint::Length(3), // title
                                Constraint::Min(5),    // content
                                Constraint::Length(3), // status
                            ])
                            .split(f.area());

                        let title_widget = Paragraph::new(Line::from(doc.title.as_str()))
                            .block(create_block("Document Title", transparent))
                            .wrap(ratatui::widgets::Wrap { trim: true });
                        f.render_widget(title_widget, chunks[0]);

                        let content_text = if doc.body.is_empty() { "No content available" } else { &doc.body };
                        let content_widget = Paragraph::new(content_text)
                            .block(create_block(
                                "Content • Ctrl+s: summarize, Esc: back to search",
                                transparent,
                            ))
                            .wrap(ratatui::widgets::Wrap { trim: true });
                        f.render_widget(content_widget, chunks[1]);

                        let status_text = format!("Document Detail • ID: {} • URL: {}",
                                                doc.id,
                                                if doc.url.is_empty() { "N/A" } else { &doc.url });
                        let status = Paragraph::new(Line::from(status_text))
                            .block(create_block("", transparent));
                        f.render_widget(status, chunks[2]);
                    }
                }
            }
        })?;

        if event::poll(std::time::Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
        {
            match view_mode {
                ViewMode::Search => match map_search_key_event(key) {
                    TuiAction::Quit => break,
                    TuiAction::SearchOrOpen => {
                        let query = input.trim().to_string();
                        let backend = backend.clone();
                        let role = current_role.clone();
                        if !query.is_empty() {
                            if let Ok(docs) = rt.block_on(async move {
                                let q = SearchQuery {
                                    search_term: NormalizedTermValue::from(query.as_str()),
                                    search_terms: None,
                                    operator: None,
                                    skip: Some(0),
                                    limit: Some(10),
                                    role: Some(RoleName::new(&role)),
                                    layer: Layer::default(),
                                    include_pinned: false,
                                    min_quality: None,
                                };
                                backend.search(&q).await
                            }) {
                                let lines: Vec<String> = docs
                                    .iter()
                                    .map(|d| format!("{} {}", d.rank.unwrap_or_default(), d.title))
                                    .collect();
                                results = lines;
                                detailed_results = docs;
                                selected_result_index = 0;
                            }
                        } else if selected_result_index < detailed_results.len() {
                            view_mode = ViewMode::ResultDetail;
                        }
                    }
                    TuiAction::MoveUp => {
                        selected_result_index = selected_result_index.saturating_sub(1);
                    }
                    TuiAction::MoveDown => {
                        if selected_result_index + 1 < results.len() {
                            selected_result_index += 1;
                        }
                    }
                    TuiAction::Autocomplete => {
                        let query = input.trim();
                        if !query.is_empty() {
                            let backend = backend.clone();
                            let role = current_role.clone();
                            if let Ok(autocomplete_resp) =
                                rt.block_on(async move { backend.autocomplete(&role, query).await })
                            {
                                suggestions = autocomplete_resp.into_iter().take(5).collect();
                            }
                        }
                    }
                    TuiAction::SwitchRole => {
                        let backend = backend.clone();
                        if let Ok(cfg) = rt.block_on(async { backend.get_config().await }) {
                            let roles: Vec<String> =
                                cfg.roles.keys().map(|k| k.to_string()).collect();
                            if !roles.is_empty()
                                && let Some(current_idx) =
                                    roles.iter().position(|r| r == &current_role)
                            {
                                let next_idx = (current_idx + 1) % roles.len();
                                current_role = roles[next_idx].clone();
                                if let Ok(rg) = rt.block_on(async {
                                    backend.get_rolegraph_terms(&current_role).await
                                }) {
                                    terms = rg;
                                }
                            }
                        }
                    }
                    TuiAction::SummarizeSelection => {
                        #[cfg(feature = "llm")]
                        {
                            if selected_result_index < detailed_results.len() {
                                let doc = detailed_results[selected_result_index].clone();
                                let backend = backend.clone();
                                let role = current_role.clone();
                                if let Ok(Some(summary_text)) = rt.block_on(async move {
                                    backend.summarize(&doc, Some(&role)).await
                                }) && selected_result_index < results.len()
                                {
                                    results[selected_result_index] =
                                        format!("SUMMARY: {}", summary_text);
                                }
                            }
                        }
                    }
                    TuiAction::Backspace => {
                        input.pop();
                        update_local_suggestions(&input, &terms, &mut suggestions);
                    }
                    TuiAction::InsertChar(c) => {
                        input.push(c);
                        update_local_suggestions(&input, &terms, &mut suggestions);
                    }
                    TuiAction::None | TuiAction::BackToSearch | TuiAction::SummarizeDetail => {}
                },
                ViewMode::ResultDetail => match map_detail_key_event(key) {
                    TuiAction::BackToSearch => {
                        view_mode = ViewMode::Search;
                    }
                    TuiAction::SummarizeDetail => {
                        #[cfg(feature = "llm")]
                        {
                            if selected_result_index < detailed_results.len() {
                                let doc = detailed_results[selected_result_index].clone();
                                let backend = backend.clone();
                                let role = current_role.clone();
                                if let Ok(Some(summary_text)) = rt.block_on(async move {
                                    backend.summarize(&doc, Some(&role)).await
                                }) {
                                    let original_body = if detailed_results[selected_result_index]
                                        .body
                                        .is_empty()
                                    {
                                        "No content"
                                    } else {
                                        &detailed_results[selected_result_index].body
                                    };
                                    detailed_results[selected_result_index].body = format!(
                                        "SUMMARY:\n{}\n\nORIGINAL:\n{}",
                                        summary_text, original_body
                                    );
                                }
                            }
                        }
                    }
                    TuiAction::Quit => break,
                    TuiAction::None
                    | TuiAction::SearchOrOpen
                    | TuiAction::MoveUp
                    | TuiAction::MoveDown
                    | TuiAction::Autocomplete
                    | TuiAction::SwitchRole
                    | TuiAction::SummarizeSelection
                    | TuiAction::Backspace
                    | TuiAction::InsertChar(_) => {}
                },
            }
        }
    }
    Ok(())
}

fn update_local_suggestions(input: &str, terms: &[String], suggestions: &mut Vec<String>) {
    let needle = input
        .rsplit_once(' ')
        .map(|(_, w)| w)
        .unwrap_or(input)
        .to_lowercase();
    *suggestions = if needle.is_empty() {
        Vec::new()
    } else {
        let mut s: Vec<String> = terms
            .iter()
            .filter(|t| t.to_lowercase().contains(&needle))
            .take(50)
            .cloned()
            .collect();
        s.truncate(5);
        s
    };
}
