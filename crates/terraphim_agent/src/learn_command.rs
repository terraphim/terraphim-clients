//! Learn command execution (step 6.3 of #211).
//!
//! Extracted from `main.rs`: `run_learn_command` fans out over the
//! `LearnSub` subcommands (capture / compile / export-kg / list / correct /
//! query / suggest / shared-learning). Extracted verbatim; only the imports
//! are new. The `shared-learning`-gated arms keep their gates, so the
//! matching imports are gated too.

use anyhow::Result;

use terraphim_agent::learnings;

use crate::cli_schema::{CorrectionSub, LearnSub, ProcedureSub};
use crate::get_session_cache_path;
#[cfg(feature = "shared-learning")]
use crate::{run_shared_learning_command, run_suggest_command};

pub(crate) async fn run_learn_command(sub: LearnSub) -> Result<()> {
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
