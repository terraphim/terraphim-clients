//! Server-mode command execution (step 6.1 of #211).
//!
//! Extracted from `main.rs`: `run_server_command` is the server-mode twin of
//! `run_offline_command` -- same `Command` surface, but every arm talks to a
//! running terraphim server over HTTP via `ApiClient` instead of a local
//! `TuiService`. Extracted verbatim; only the imports are new.

use anyhow::Result;
use tokio::runtime::Runtime;

#[cfg(feature = "server")]
use terraphim_agent::client::{ApiClient, SearchResponse};
use terraphim_agent::{guard_patterns, onboarding, robot};
use terraphim_types::{Layer, LogicalOperator, NormalizedTermValue, RoleName, SearchQuery};
use terraphim_update::{TerraphimUpdater, UpdaterConfig};

use crate::cli_schema::{Command, CommandOutputMode, ConfigSub, KgSub, RolesSub, SessionsSub};
use crate::learn_command::run_learn_command;
use crate::memory_command::run_memory_command;
use crate::{CommandOutputConfig, print_json_output, session_output, truncate_snippet};

#[cfg(feature = "server")]
pub(crate) async fn run_server_command(
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
        // Server mode: the server owns the role config, so there is no local
        // `--config` to honour here.
        Command::Memory { sub } => run_memory_command(sub, &output, None).await,
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
