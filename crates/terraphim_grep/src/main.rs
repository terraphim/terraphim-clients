use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use terraphim_automata::AutomataPath;
use terraphim_grep::{
    GrepOptions, GrepResult, Haystack, HybridSearcher, SufficiencyJudge, TerraphimGrep,
};
use terraphim_types::Thesaurus;
use terraphim_update::{TerraphimUpdater, UpdaterConfig};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Parser, Debug)]
#[command(name = "terraphim-grep")]
#[command(
    version,
    about = "Intelligent hybrid grep with RLM fallback and KG curation"
)]
struct Args {
    #[arg(help = "Search query")]
    query: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,

    #[arg(
        short = 'C',
        long,
        default_value = "0",
        help = "Context lines before/after match"
    )]
    context: usize,

    #[arg(
        short = 'n',
        long,
        default_value = "50",
        help = "Maximum number of results"
    )]
    max_results: usize,

    #[arg(
        short = 'H',
        long,
        value_enum,
        default_value = "all",
        help = "Haystack to search"
    )]
    haystack: HaystackArg,

    #[arg(long, help = "Force RLM fallback for all queries")]
    force_rlm: bool,

    #[arg(long, help = "Include LLM-generated answer")]
    answer: bool,

    #[arg(long, help = "Output JSON format")]
    json: bool,

    #[arg(long, help = "Search paths (default: current directory)")]
    paths: Vec<PathBuf>,

    #[arg(long, help = "Role to use for search")]
    role: Option<String>,

    #[arg(long, help = "Thesaurus path")]
    thesaurus: Option<PathBuf>,

    #[arg(
        long,
        help = "Path to a JSON file containing a terraphim_config::Role with LLM/router settings"
    )]
    role_config: Option<PathBuf>,

    #[arg(long, help = "KG directory for persisting learned concepts")]
    kg_path: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Check for updates without installing
    CheckUpdate,
    /// Update to latest version if available
    Update,
}

#[derive(Debug, Clone, ValueEnum)]
enum HaystackArg {
    All,
    Code,
    Docs,
}

#[allow(clippy::derivable_impls)]
impl Default for HaystackArg {
    fn default() -> Self {
        HaystackArg::All
    }
}

impl From<HaystackArg> for Haystack {
    fn from(arg: HaystackArg) -> Self {
        match arg {
            HaystackArg::All => Haystack::All,
            HaystackArg::Code => Haystack::Code,
            HaystackArg::Docs => Haystack::Docs,
        }
    }
}

impl From<Haystack> for HaystackArg {
    fn from(arg: Haystack) -> Self {
        match arg {
            Haystack::All => HaystackArg::All,
            Haystack::Code => HaystackArg::Code,
            Haystack::Docs => HaystackArg::Docs,
        }
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,terraphim_grep=debug"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(filter)
        .init();
}

fn grep_updater() -> TerraphimUpdater {
    let config = UpdaterConfig::new("terraphim-grep")
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_repo("terraphim", "terraphim-clients");
    TerraphimUpdater::new(config)
}

async fn handle_update_command(command: Command) -> Result<()> {
    let updater = grep_updater();
    let status = match command {
        Command::CheckUpdate => {
            println!("Checking for terraphim-grep updates...");
            updater.check_update().await?
        }
        Command::Update => {
            println!("Updating terraphim-grep...");
            updater.check_and_update().await?
        }
    };
    println!("{status}");
    Ok(())
}

/// Discover project-level config from `.terraphim/` directory.
///
/// Returns the `.terraphim/` path if found, enabling auto-discovery of
/// thesaurus, role config, and KG path without CLI flags.
fn discover_project_dir() -> Option<std::path::PathBuf> {
    terraphim_config::project::discover(None).ok().flatten()
}

fn load_project_config() -> Option<(PathBuf, terraphim_config::project::ProjectConfig)> {
    let dir = discover_project_dir()?;
    let config = terraphim_config::project::ProjectConfig::load_from_dir(&dir).ok()?;
    Some((dir, config))
}

fn resolve_role_name(
    explicit_role: Option<&str>,
    project_config: Option<&terraphim_config::project::ProjectConfig>,
) -> Result<String> {
    if let Some(config) = project_config
        && let Some(role) = config.resolve_role_name(explicit_role)?
    {
        return Ok(role);
    }

    Ok(explicit_role.unwrap_or("default").to_string())
}

fn push_unique_candidate(candidates: &mut Vec<String>, candidate: impl Into<String>) {
    let candidate = candidate.into();
    if !candidate.is_empty() && !candidates.contains(&candidate) {
        candidates.push(candidate);
    }
}

fn thesaurus_role_candidates(
    role_name: &str,
    project_config: Option<&terraphim_config::project::ProjectConfig>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique_candidate(&mut candidates, role_name);

    if let Some(config) = project_config {
        if let Some(role) = config.roles.get(role_name)
            && let Some(shortname) = &role.shortname
        {
            push_unique_candidate(&mut candidates, shortname);
        }

        for (key, role) in &config.roles {
            if role.name.to_string() == role_name {
                push_unique_candidate(&mut candidates, key);
                if let Some(shortname) = &role.shortname {
                    push_unique_candidate(&mut candidates, shortname);
                }
            }
        }
    }

    candidates
}

fn discover_project_thesaurus(dir: &Path, role_name: &str) -> Option<PathBuf> {
    let project_config = terraphim_config::project::ProjectConfig::load_from_dir(dir).ok();
    for candidate in thesaurus_role_candidates(role_name, project_config.as_ref()) {
        if let Some(path) = terraphim_config::project::discover_thesaurus(dir, &candidate) {
            tracing::info!("Using project thesaurus: {:?}", path);
            return Some(path);
        }
    }

    None
}

/// Find thesaurus path with project config priority.
///
/// Resolution order:
///   1. `.terraphim/thesaurus-<role>.json` or the matching role shortname (project config)
///   2. `*_thesaurus.json` in CWD or nearby directories (filesystem heuristic)
fn find_default_thesaurus(role_name: &str) -> Option<PathBuf> {
    if let Some(dir) = discover_project_dir()
        && let Some(path) = discover_project_thesaurus(&dir, role_name)
    {
        return Some(path);
    }

    let possible_paths = vec![
        PathBuf::from("."),
        PathBuf::from("../docs/src"),
        PathBuf::from("../../docs/src"),
    ];

    for base in possible_paths {
        if let Ok(cwd) = std::env::current_dir() {
            let candidate = cwd.join(&base);
            if candidate.exists()
                && let Ok(entries) = std::fs::read_dir(&candidate)
            {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.ends_with("_thesaurus.json") {
                        return Some(candidate.join(&name));
                    }
                }
            }
        }
    }
    None
}

/// Resolve the role name to use for a search, respecting the --thesaurus short-circuit.
///
/// When `has_explicit_thesaurus` is `true` the caller already knows which thesaurus to load;
/// project role discovery must be skipped to avoid "multiple project roles found" errors in
/// directories that contain more than one `role-*.json` file (terraphim/terraphim-ai#2722).
///
/// `load_config_fn` is a closure that loads the project config; it is only called when
/// `has_explicit_thesaurus` is `false`, making the short-circuit branch allocation-free.
fn determine_role_name<F>(
    has_explicit_thesaurus: bool,
    explicit_role: Option<&str>,
    load_config_fn: &F,
) -> Result<String>
where
    F: Fn() -> Option<(std::path::PathBuf, terraphim_config::project::ProjectConfig)>,
{
    if has_explicit_thesaurus {
        return Ok(explicit_role.unwrap_or("default").to_string());
    }

    let project_config = load_config_fn();
    resolve_role_name(
        explicit_role,
        project_config.as_ref().map(|(_, config)| config),
    )
}

/// Build a thesaurus for the requested role.
///
/// Resolution order:
///   1. If `--thesaurus <path>` is provided, load it.
///   2. Otherwise try `find_default_thesaurus` (project config or filesystem heuristic).
///   3. If none of the above succeeds, return an empty thesaurus so the CLI can fall back to
///      `fff-search` enhanced grep without a knowledge graph.
async fn resolve_thesaurus(role_name: &str, explicit: Option<&Path>) -> Result<Thesaurus> {
    if let Some(path) = explicit {
        let automata_path = AutomataPath::from_local(path);
        return terraphim_automata::load_thesaurus(&automata_path)
            .await
            .with_context(|| format!("Failed to load thesaurus from {:?}", path));
    }
    if let Some(path) = find_default_thesaurus(role_name) {
        let automata_path = AutomataPath::from_local(&path);
        return terraphim_automata::load_thesaurus(&automata_path)
            .await
            .with_context(|| format!("Failed to load thesaurus from {:?}", path));
    }
    Ok(Thesaurus::new(role_name.to_string()))
}

/// Build an `LlmClient` for the requested role.
///
/// Resolution order:
///   1. If `--role-config <path>` is provided, deserialize a `terraphim_config::Role` from
///      that JSON file and feed it to `terraphim_service::llm::build_llm_from_role`.
///   2. Otherwise construct a minimal in-memory `Role` populated from environment variables
///      (`OPENROUTER_API_KEY`, `OPENROUTER_MODEL`, `OLLAMA_BASE_URL`, `OLLAMA_MODEL`).
///   3. Return `None` if neither source yields a usable LLM client. The grep stays usable in
///      search-only mode -- the LLM is only required when sufficiency falls below threshold.
///
/// `terraphim_service::llm::build_llm_from_role` is the public entry point: it owns the
/// precedence rules and decides internally whether to return a direct provider (Ollama /
/// OpenRouter / GenAi) or a `RouterBridgeLlmClient` based on `role.llm_router_enabled`.
/// Wiring this function rather than the bridge directly keeps grep aligned with how the
/// server, TUI, and RLM consume LLM clients.
#[cfg(feature = "llm")]
fn build_llm_for_role(
    role_name: &str,
    role_config_path: Option<&std::path::Path>,
) -> Option<Arc<dyn terraphim_service::llm::LlmClient>> {
    let role = match role_config_path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(contents) => match serde_json::from_str::<terraphim_config::Role>(&contents) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Failed to parse role config at {:?}: {}", path, e);
                    return None;
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read role config at {:?}: {}", path, e);
                return None;
            }
        },
        None => {
            // Try project config (.terraphim/role-<name>.json) before env vars
            if let Some(dir) = discover_project_dir() {
                let role_file = dir.join(format!("role-{}.json", role_name));
                if role_file.is_file() {
                    tracing::info!("Using project role config: {:?}", role_file);
                    if let Ok(contents) = std::fs::read_to_string(&role_file)
                        && let Ok(r) = serde_json::from_str::<terraphim_config::Role>(&contents)
                    {
                        return terraphim_service::llm::build_llm_from_role(&r);
                    }
                }
            }
            role_from_env(role_name)?
        }
    };

    terraphim_service::llm::build_llm_from_role(&role)
}

#[cfg(not(feature = "llm"))]
#[allow(dead_code)]
fn build_llm_for_role(
    _role_name: &str,
    _role_config_path: Option<&std::path::Path>,
) -> Option<std::sync::Arc<()>> {
    None
}

/// Construct a minimal `Role` from environment variables. Returns `None` when no LLM
/// credentials are visible -- the CLI then runs in search-only mode.
#[cfg(feature = "llm")]
fn role_from_env(role_name: &str) -> Option<terraphim_config::Role> {
    use serde_json::Value;

    let openrouter_key = std::env::var("OPENROUTER_API_KEY")
        .ok()
        .filter(|s| !s.is_empty());
    let ollama_url = std::env::var("OLLAMA_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty());

    if openrouter_key.is_none() && ollama_url.is_none() {
        return None;
    }

    let mut role = terraphim_config::Role::new(role_name);
    role.llm_enabled = true;

    if let Some(key) = openrouter_key {
        let model = std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "qwen/qwen3-coder:free".to_string());
        role.llm_api_key = Some(key);
        role.llm_model = Some(model.clone());
        role.extra.insert(
            "llm_provider".to_string(),
            Value::String("openrouter".to_string()),
        );
        role.extra
            .insert("llm_model".to_string(), Value::String(model));
    } else if let Some(url) = ollama_url {
        let model = std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3.2:3b".to_string());
        role.llm_model = Some(model.clone());
        role.extra.insert(
            "llm_provider".to_string(),
            Value::String("ollama".to_string()),
        );
        role.extra
            .insert("ollama_base_url".to_string(), Value::String(url));
        role.extra
            .insert("ollama_model".to_string(), Value::String(model));
    }

    Some(role)
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let mut args = Args::parse();

    if let Some(command) = args.command.take() {
        return handle_update_command(command).await;
    }

    let query = args
        .query
        .as_deref()
        .context("missing search query; run `terraphim-grep --help` for usage")?;

    let options = GrepOptions {
        haystack: args.haystack.into(),
        context_lines: args.context,
        max_results: args.max_results,
        force_rlm: args.force_rlm,
        include_answer: args.answer,
    };

    // Determine role and thesaurus.
    // When --thesaurus is explicitly provided, skip project role discovery entirely.
    // Without this short-circuit, a multi-role project directory causes
    // "multiple project roles found" even though the thesaurus is already known.
    // See terraphim/terraphim-ai#2722.
    let role_name = determine_role_name(
        args.thesaurus.is_some(),
        args.role.as_deref(),
        &load_project_config,
    )?;

    // Load thesaurus, falling back to an empty one when no project thesaurus exists.
    // This lets terraphim-grep behave like an enhanced fff-search grep without a KG.
    let thesaurus = resolve_thesaurus(&role_name, args.thesaurus.as_deref()).await?;
    if thesaurus.is_empty() {
        tracing::info!(
            "No thesaurus found for role '{}'; running in fff-search enhanced grep mode",
            role_name
        );
    } else {
        tracing::debug!("Loaded thesaurus with {} entries", thesaurus.len());
    }

    // Determine search path
    let search_path = args
        .paths
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));

    // Create hybrid searcher
    let mut hybrid_searcher = HybridSearcher::new(role_name.clone(), thesaurus)
        .map_err(|e| anyhow::anyhow!("Failed to create hybrid searcher: {}", e))?;
    hybrid_searcher = hybrid_searcher.with_search_path(search_path);
    let hybrid_searcher = Arc::new(hybrid_searcher);

    // Create sufficiency judge
    let sufficiency_judge = SufficiencyJudge::default();
    let sufficiency_judge = Arc::new(sufficiency_judge);

    // Create TerraphimGrep and optionally attach an LLM client
    let terraphim_grep = TerraphimGrep::new(hybrid_searcher, sufficiency_judge);
    #[cfg(feature = "llm")]
    let terraphim_grep = match build_llm_for_role(&role_name, args.role_config.as_deref()) {
        Some(client) => {
            tracing::info!("LLM client wired: {}", client.name());
            let mut grep = terraphim_grep.with_llm_client(client.clone());
            if let Some(ref kg_path) = args.kg_path {
                let curation =
                    terraphim_grep::KgCurationRlm::new(client).with_kg_path(kg_path.clone());
                grep = grep.with_kg_curation(Arc::new(curation));
                tracing::info!("KG curation enabled, writing to {:?}", kg_path);
            }
            grep
        }
        None => {
            tracing::debug!(
                "No LLM client available -- running in search-only mode (set OPENROUTER_API_KEY \
                 or --role-config to enable RLM synthesis)"
            );
            terraphim_grep
        }
    };

    // Perform search
    let result = terraphim_grep
        .search(query, options)
        .await
        .context("Search failed")?;

    // Output results
    if args.json {
        let json =
            serde_json::to_string_pretty(&result).context("Failed to serialize result to JSON")?;
        println!("{}", json);
    } else {
        print_results(&result, args.context);
    }

    Ok(())
}

fn print_results(result: &GrepResult, context_lines: usize) {
    println!("=== Terraphim Grep Results ===");
    println!();

    // Print stats
    println!(
        "Search latency: {}ms (RLM: {:?}ms)",
        result.stats.search_latency_ms, result.stats.rlm_latency_ms
    );
    println!("Chunks returned: {}", result.stats.chunks_returned);
    println!("KG hits: {}", result.stats.kg_hits);
    println!("Sufficiency: {:?}", result.sufficiency);
    println!();

    // Print concepts
    if !result.concepts.is_empty() {
        println!("=== Knowledge Graph Concepts ===");
        for concept in &result.concepts {
            println!("  - {} (score: {:.2})", concept.name, concept.score);
        }
        println!();
    }

    // Print chunks
    if !result.chunks.is_empty() {
        println!("=== Retrieved Chunks ===");
        for (i, chunk) in result.chunks.iter().enumerate() {
            println!(
                "{}. {}:{}",
                i + 1,
                chunk.source,
                chunk
                    .line_start
                    .map_or_else(|| "?".to_string(), |l| l.to_string())
            );
            if context_lines > 0 {
                // Simple context display - just show content
                println!(
                    "   {}",
                    chunk
                        .content
                        .lines()
                        .take(context_lines)
                        .collect::<Vec<_>>()
                        .join("\n   ")
                );
            } else {
                println!("   {}", chunk.content);
            }
            println!();
        }
    }

    // Print answer if present
    if let Some(ref answer) = result.answer {
        println!("=== Synthesised Answer ===");
        println!("{}", answer.answer);
        println!();
        if !answer.citations.is_empty() {
            println!("Citations:");
            for citation in &answer.citations {
                println!(
                    "  - {} (line {:?}): {}",
                    citation.source, citation.line, citation.excerpt
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use terraphim_config::project::ProjectConfig;

    fn minimal_role_json(name: &str) -> String {
        format!(
            r#"{{"shortname":"{}","name":"{}","relevance_function":"title-scorer","terraphim_it":false,"theme":"default","haystacks":[]}}"#,
            name, name
        )
    }

    fn multi_role_project_config() -> ProjectConfig {
        let mut config = ProjectConfig::default();
        config.roles.insert(
            "ai-engineer".to_string(),
            serde_json::from_str(&minimal_role_json("AI Engineer")).unwrap(),
        );
        config.roles.insert(
            "devops".to_string(),
            serde_json::from_str(&minimal_role_json("DevOps")).unwrap(),
        );
        config.roles.insert(
            "rust-engineer".to_string(),
            serde_json::from_str(&minimal_role_json("Rust Engineer")).unwrap(),
        );
        config
    }

    // Regression test: terraphim/terraphim-ai#2722
    // When --thesaurus is explicit, "multiple project roles found" must NOT be raised.
    #[test]
    fn explicit_thesaurus_bypasses_ambiguous_role_error() {
        let config = multi_role_project_config();
        // Sanity-check: without --thesaurus the multi-role config DOES error.
        assert!(
            config.resolve_role_name(None).is_err(),
            "prerequisite: multi-role config without explicit role must error"
        );

        // With explicit thesaurus: determine_role_name must succeed.
        let tmp = tempfile::TempDir::new().unwrap();
        let thesaurus_path = tmp.path().join("thesaurus-rust-engineer.json");
        fs::write(&thesaurus_path, "[]").unwrap();

        let result = determine_role_name(
            true, // has_explicit_thesaurus
            None, // no --role flag
            &|| None::<(std::path::PathBuf, ProjectConfig)>,
        );
        assert!(
            result.is_ok(),
            "explicit --thesaurus must bypass ambiguous-role error"
        );
        assert_eq!(result.unwrap(), "default");
    }

    #[test]
    fn explicit_thesaurus_with_role_uses_given_role() {
        let result = determine_role_name(true, Some("rust-engineer"), &|| {
            None::<(std::path::PathBuf, ProjectConfig)>
        });
        assert_eq!(result.unwrap(), "rust-engineer");
    }

    #[test]
    fn no_thesaurus_single_role_project_resolves() {
        let mut config = ProjectConfig::default();
        config.roles.insert(
            "devops".to_string(),
            serde_json::from_str(&minimal_role_json("DevOps")).unwrap(),
        );
        let tmp = tempfile::TempDir::new().unwrap();
        let dummy_path = tmp.path().to_path_buf();

        let result =
            determine_role_name(false, None, &|| Some((dummy_path.clone(), config.clone())));
        assert_eq!(result.unwrap(), "devops");
    }

    #[test]
    fn no_thesaurus_multi_role_project_errors() {
        let config = multi_role_project_config();
        let tmp = tempfile::TempDir::new().unwrap();
        let dummy_path = tmp.path().to_path_buf();

        let result =
            determine_role_name(false, None, &|| Some((dummy_path.clone(), config.clone())));
        assert!(
            result.is_err(),
            "without --thesaurus, ambiguous roles must still error"
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("multiple project roles found")
        );
    }

    #[test]
    fn thesaurus_candidates_include_matching_role_shortname() {
        let mut config = ProjectConfig::default();
        let mut role: terraphim_config::Role =
            serde_json::from_str(&minimal_role_json("Project Developer")).unwrap();
        role.shortname = Some("projdev".to_string());
        config.roles.insert("Project Developer".to_string(), role);

        let candidates = thesaurus_role_candidates("Project Developer", Some(&config));

        assert_eq!(
            candidates,
            vec!["Project Developer".to_string(), "projdev".to_string()]
        );
    }

    #[test]
    fn project_thesaurus_resolves_by_role_shortname() {
        let tmp = tempfile::TempDir::new().unwrap();
        let terraphim_dir = tmp.path().join(".terraphim");
        fs::create_dir(&terraphim_dir).unwrap();
        fs::write(
            terraphim_dir.join("config.json"),
            r#"{
              "roles": {
                "Project Developer": {
                  "shortname": "projdev",
                  "name": "Project Developer",
                  "relevance_function": "title-scorer",
                  "terraphim_it": false,
                  "theme": "default",
                  "haystacks": []
                }
              }
            }"#,
        )
        .unwrap();
        let expected = terraphim_dir.join("thesaurus-projdev.json");
        fs::write(&expected, "{}").unwrap();

        let actual = discover_project_thesaurus(&terraphim_dir, "Project Developer");

        assert_eq!(actual, Some(expected));
    }
}
