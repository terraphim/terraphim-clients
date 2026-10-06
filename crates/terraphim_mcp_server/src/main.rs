//! ## Logging and OpenDAL Warning Messages
//!
//! This server uses OpenDAL library for storage operations. You may see
//! WARN-level messages about "NotFound" errors when reading configuration files:
//! ```text
//! [WARN  opendal::services] service=memory name=0x... path=embedded_config.json: read failed NotFound (permanent)
//! ```
//!
//! These messages are **expected and harmless** - they occur when OpenDAL attempts
//! to read configuration files that don't exist yet. The system correctly falls back
//! to default values and continues normal operation.
//!
//! ### Why These Warnings Appear
//!
//! OpenDAL has an internal `LoggingLayer` that logs directly to the Rust `log` crate.
//! This logging is independent of application logging configuration and occurs before
//! our tracing setup takes effect.
//!
//! ### Suppressing These Warnings
//!
//! If you want cleaner logs (without these expected warnings), you can set the
//! `RUST_LOG` environment variable:
//!
//! ```bash
//! # Option 1: Suppress all warnings (includes real ones)
//! RUST_LOG=error terraphim-mcp-server
//!
//! # Option 2: Suppress OpenDAL-specific warnings
//! RUST_LOG="opendal=error" terraphim-mcp-server
//!
//! # Option 3: Use quieter mode
//! RUST_LOG=warn terraphim-mcp-server
//! ```

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use rmcp::{
    ServiceExt,
    transport::{
        sse_server::{SseServer, SseServerConfig},
        stdio,
    },
};
use terraphim_automata::builder::{Logseq, ThesaurusBuilder};
use terraphim_config::{Config, ConfigBuilder, ConfigState};
use terraphim_file_search::kg_scorer::KgPathScorer;
use terraphim_mcp_server::McpService;
use terraphim_types::RoleName;
use tracing::{Level, info, warn};

#[derive(Parser, Debug)]
#[command(name = "terraphim_mcp_server")]
#[command(about = "Terraphim MCP server with configurable profile")]
#[command(version)]
struct Args {
    /// Configuration profile to use
    #[arg(short, long, value_enum, default_value_t = ConfigProfile::Desktop)]
    profile: ConfigProfile,

    /// Enable verbose logging (INFO level instead of WARN)
    #[arg(short, long)]
    verbose: bool,

    /// Start SSE server instead of stdio transport
    #[arg(long, default_value_t = false)]
    sse: bool,

    /// SSE bind address (when --sse)
    #[arg(long, default_value = "127.0.0.1:8000")]
    bind: String,

    /// Directory in which to discover the project `.terraphim/` config
    /// (defaults to the current working directory). Useful for MCP clients,
    /// such as Zed context servers, that cannot pass the project root.
    #[arg(long, value_name = "DIR")]
    config_dir: Option<PathBuf>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum ConfigProfile {
    /// Use desktop configuration (Terraphim Engineer role with local KG)
    Desktop,
    /// Use server configuration (Default role without KG)
    Server,
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl+c");
}

fn build_profile_config(profile: &ConfigProfile) -> terraphim_config::Config {
    match profile {
        ConfigProfile::Desktop => {
            info!("Using desktop configuration (Terraphim Engineer role with local KG)");
            ConfigBuilder::new()
                .build_default_desktop()
                .build()
                .expect("Failed to build default desktop configuration")
        }
        ConfigProfile::Server => {
            info!("Using server configuration (Default role without KG)");
            ConfigBuilder::new()
                .build_default_server()
                .build()
                .expect("Failed to build default server configuration")
        }
    }
}

fn merge_project_into_base(
    mut config: Config,
    project_config: &terraphim_config::project::ProjectConfig,
) -> Config {
    if let Some(ref shortcut) = project_config.global_shortcut {
        config.global_shortcut = Some(shortcut.clone());
    }

    for (name, role) in &project_config.roles {
        config.roles.insert(RoleName::new(name), role.clone());
    }

    if let Ok(Some(role_name)) = project_config.resolve_role_name(None) {
        let role_name = RoleName::new(&role_name);
        if config.roles.contains_key(&role_name) {
            config.selected_role = role_name.clone();
            config.default_role = role_name;
        }
    }

    repair_selected_roles(&mut config);
    config
}

fn repair_selected_roles(config: &mut Config) {
    if config.roles.contains_key(&config.selected_role)
        && config.roles.contains_key(&config.default_role)
    {
        return;
    }

    if let Some(first_role) = config.roles.keys().next().cloned() {
        if !config.roles.contains_key(&config.selected_role) {
            config.selected_role = first_role.clone();
        }
        if !config.roles.contains_key(&config.default_role) {
            config.default_role = first_role;
        }
    }
}

/// Resolve the effective configuration.
///
/// Project `.terraphim/` discovery starts from `config_dir` when given, or from
/// the current working directory otherwise. When no usable project config is
/// found, the hardcoded `profile` configuration is used.
fn resolve_config(profile: &ConfigProfile, config_dir: Option<&Path>) -> Result<Config> {
    let discovered = terraphim_config::project::discover(config_dir);
    let project_dir = match discovered {
        Ok(Some(dir)) => dir,
        Ok(None) => {
            if let Some(dir) = config_dir {
                warn!(
                    "No .terraphim/ directory found from --config-dir '{}', using profile",
                    dir.display()
                );
            }
            return Ok(build_profile_config(profile));
        }
        Err(e) => {
            if let Some(dir) = config_dir {
                warn!(
                    "Project discovery failed for --config-dir '{}': {}; using profile",
                    dir.display(),
                    e
                );
            }
            return Ok(build_profile_config(profile));
        }
    };

    match terraphim_config::project::ProjectConfig::load_from_dir(&project_dir) {
        Ok(project_config) if !project_config.is_empty() => {
            info!(
                "Using project configuration from '{}' ({} role(s))",
                project_dir.display(),
                project_config.roles.len()
            );
            Ok(merge_project_into_base(
                build_profile_config(profile),
                &project_config,
            ))
        }
        Err(e) if config_dir.is_some() => anyhow::bail!(
            "Failed to load project configuration from '{}': {}",
            project_dir.display(),
            e
        ),
        Err(e) => {
            warn!(
                "Failed to load project configuration from '{}': {}; using profile",
                project_dir.display(),
                e
            );
            Ok(build_profile_config(profile))
        }
        Ok(_) => {
            if config_dir.is_some() {
                warn!(
                    "No project roles found in '{}', using profile",
                    project_dir.display()
                );
            } else {
                info!(
                    "No project roles found in '{}', using profile",
                    project_dir.display()
                );
            }
            Ok(build_profile_config(profile))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    let args = Args::parse();

    // Standardized tracing setup
    let level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive(level.into()),
        );

    if args.sse {
        // SSE mode needs timestamps for server logs - write to stdout
        subscriber.init();
    } else {
        // Stdio mode: write logs to stderr to avoid mixing with JSON-RPC responses on stdout
        subscriber
            .without_time()
            .with_writer(std::io::stderr)
            .init();
    }

    info!("Starting Terraphim MCP Server...");
    info!("Args: {:?}", args);

    // Build configuration based on selected profile
    // Priority: project .terraphim/ config > hardcoded profile
    let config = resolve_config(&args.profile, args.config_dir.as_deref())?;

    // Initialize ConfigState from the config
    let mut temp_config = config.clone();
    let config_state = ConfigState::new(&mut temp_config)
        .await
        .expect("Failed to create config state from config");

    // Create the MCP service
    let config_state = Arc::new(config_state);

    // Wire KgPathScorer from the selected role's KG path
    let (selected_role, kg_path) = {
        let cfg = config_state.config.lock().await;
        let selected = cfg.selected_role.clone();
        let path = cfg
            .roles
            .get(&selected)
            .and_then(|r| r.kg.as_ref())
            .and_then(|kg| kg.knowledge_graph_local.as_ref())
            .map(|kgl| kgl.path.clone());
        (selected, path)
    };

    let mut service = McpService::new(Arc::clone(&config_state));

    if let Some(kg_path) = kg_path {
        info!("Building KgPathScorer from KG path: {:?}", kg_path);
        let builder = Logseq::default();
        let role_name = selected_role.as_lowercase().to_string();
        match builder.build(role_name, kg_path).await {
            Ok(thesaurus) => {
                let term_count = thesaurus.len();
                let scorer = Arc::new(KgPathScorer::new(thesaurus));
                service = service.with_kg_scorer(scorer);
                info!(
                    "KgPathScorer wired with {} terms for role '{}'",
                    term_count, selected_role
                );
            }
            Err(e) => {
                warn!(
                    "Failed to build thesaurus for KgPathScorer (role '{}'): {}",
                    selected_role, e
                );
            }
        }
    }

    if args.sse {
        info!("Starting SSE server on {}", args.bind);

        let server_start = std::time::Instant::now();

        // Start SSE server
        let config = SseServerConfig {
            bind: args.bind.parse().expect("Invalid bind address"),
            sse_path: "/sse".to_string(),
            post_path: "/message".to_string(),
            ct: tokio_util::sync::CancellationToken::new(),
            sse_keep_alive: None,
        };

        let (sse_server, router) = SseServer::new(config);

        // Readiness probe: GET /health returns {"status":"ok","transport":"sse","uptime_secs":N}
        let router = router.route(
            "/health",
            axum::routing::get(move || async move {
                let uptime_secs = server_start.elapsed().as_secs();
                axum::Json(serde_json::json!({
                    "status": "ok",
                    "transport": "sse",
                    "uptime_secs": uptime_secs
                }))
            }),
        );

        let listener = tokio::net::TcpListener::bind(sse_server.config.bind).await?;
        let ct = sse_server.config.ct.child_token();

        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            ct.cancelled().await;
            info!("SSE server cancelled");
        });

        tokio::spawn(async move {
            if let Err(e) = server.await {
                tracing::error!(error = %e, "SSE server shutdown with error");
            }
        });

        let _ct = sse_server.with_service(move || service.clone());

        // Wait for shutdown signal
        shutdown_signal().await;
    } else {
        info!("Starting stdio server");

        // Initialize autocomplete index by default
        service.init_autocomplete_default().await;
        info!("Initialized Terraphim MCP service");

        // Start stdio server
        let mcp_service = service.serve(stdio()).await?;
        mcp_service.waiting().await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn repair_selected_roles_sets_missing_selected_to_existing_role() {
        let mut config = Config::default();
        config.roles.clear();
        config.roles.insert(
            RoleName::new("devops"),
            terraphim_config::Role::new("DevOps"),
        );

        repair_selected_roles(&mut config);

        assert_eq!(config.selected_role, RoleName::new("devops"));
        assert_eq!(config.default_role, RoleName::new("devops"));
    }

    #[test]
    fn merge_project_into_base_selects_single_project_role() {
        let base = build_profile_config(&ConfigProfile::Desktop);
        let mut roles = HashMap::new();
        roles.insert("devops".to_string(), terraphim_config::Role::new("DevOps"));
        let project_config = terraphim_config::project::ProjectConfig {
            roles,
            ..Default::default()
        };

        let config = merge_project_into_base(base, &project_config);

        assert!(config.roles.contains_key(&RoleName::new("devops")));
        assert_eq!(config.selected_role, RoleName::new("devops"));
        assert_eq!(config.default_role, RoleName::new("devops"));
    }

    fn write_role(dir: &Path, file: &str, name: &str) {
        std::fs::write(
            dir.join(file),
            format!(
                r#"{{"shortname":"{name}","name":"{name}","relevance_function":"title-scorer","terraphim_it":false,"theme":"default","haystacks":[]}}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn config_dir_flag_is_parsed() {
        let args = Args::try_parse_from(["terraphim_mcp_server", "--config-dir", "/some/project"])
            .unwrap();
        assert_eq!(args.config_dir, Some(PathBuf::from("/some/project")));

        let args = Args::try_parse_from(["terraphim_mcp_server"]).unwrap();
        assert_eq!(args.config_dir, None);
    }

    #[test]
    fn resolve_config_uses_project_config_from_config_dir() {
        let temp = tempfile::tempdir().unwrap();
        let tp = temp.path().join(".terraphim");
        std::fs::create_dir_all(&tp).unwrap();
        write_role(&tp, "role-zedproject.json", "ZedProject");

        let config = resolve_config(&ConfigProfile::Server, Some(temp.path())).unwrap();

        assert!(config.roles.contains_key(&RoleName::new("zedproject")));
        assert_eq!(config.selected_role, RoleName::new("zedproject"));
        assert_eq!(config.default_role, RoleName::new("zedproject"));
    }

    #[test]
    fn resolve_config_discovers_from_subdirectory_of_config_dir() {
        let temp = tempfile::tempdir().unwrap();
        let tp = temp.path().join(".terraphim");
        std::fs::create_dir_all(&tp).unwrap();
        write_role(&tp, "role-zedproject.json", "ZedProject");
        let sub = temp.path().join("src");
        std::fs::create_dir_all(&sub).unwrap();

        let config = resolve_config(&ConfigProfile::Server, Some(&sub)).unwrap();

        assert!(config.roles.contains_key(&RoleName::new("zedproject")));
    }

    #[test]
    fn resolve_config_falls_back_to_profile_when_config_dir_has_no_terraphim() {
        let temp = tempfile::tempdir().unwrap();

        let config = resolve_config(&ConfigProfile::Server, Some(temp.path())).unwrap();
        let expected = build_profile_config(&ConfigProfile::Server);

        let mut got: Vec<_> = config.roles.keys().map(|k| k.to_string()).collect();
        let mut want: Vec<_> = expected.roles.keys().map(|k| k.to_string()).collect();
        got.sort();
        want.sort();
        assert_eq!(got, want);
        assert_eq!(config.selected_role, expected.selected_role);
    }

    #[test]
    fn resolve_config_falls_back_when_terraphim_dir_has_no_roles() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join(".terraphim")).unwrap();

        let config = resolve_config(&ConfigProfile::Server, Some(temp.path())).unwrap();
        let expected = build_profile_config(&ConfigProfile::Server);

        assert_eq!(config.selected_role, expected.selected_role);
        assert_eq!(config.roles.len(), expected.roles.len());
    }

    #[test]
    fn resolve_config_falls_back_when_config_dir_does_not_exist() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("does-not-exist");

        let config = resolve_config(&ConfigProfile::Server, Some(&missing)).unwrap();
        let expected = build_profile_config(&ConfigProfile::Server);

        assert_eq!(config.selected_role, expected.selected_role);
    }

    #[test]
    fn resolve_config_errors_on_malformed_project_config_with_config_dir() {
        let temp = tempfile::tempdir().unwrap();
        let tp = temp.path().join(".terraphim");
        std::fs::create_dir_all(&tp).unwrap();
        std::fs::write(tp.join("role-broken.json"), "{ not json").unwrap();

        let err = resolve_config(&ConfigProfile::Server, Some(temp.path())).unwrap_err();

        assert!(
            err.to_string()
                .contains("Failed to load project configuration")
        );
    }

    #[test]
    fn health_response_body_has_required_fields() {
        let start = std::time::Instant::now();
        let uptime_secs = start.elapsed().as_secs();
        let body = serde_json::json!({
            "status": "ok",
            "transport": "sse",
            "uptime_secs": uptime_secs
        });
        assert_eq!(body["status"], "ok");
        assert_eq!(body["transport"], "sse");
        assert!(body["uptime_secs"].is_number());
    }
}
