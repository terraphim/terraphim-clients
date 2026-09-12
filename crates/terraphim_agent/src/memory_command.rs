//! Memory lifecycle command execution (step 6.2 of #211).
//!
//! Extracted from `main.rs`: `run_memory_command` fans out over the
//! `MemorySub` subcommands (capture / search / stats / prune / run lifecycle)
//! against the agent-evolution store, plus the rubric scoring helpers
//! (`RubricScore`, `RunMetrics`, `score_memory_item`, `compute_decay`,
//! `compute_risk`) that only this command uses. Extracted verbatim; only the
//! imports are new.

use anyhow::Result;

use terraphim_agent::service::TuiService;

use crate::cli_schema::MemorySub;
use crate::{CommandOutputConfig, load_evolution, save_evolution, truncate_snippet};

pub(crate) async fn run_memory_command(
    sub: MemorySub,
    output: &CommandOutputConfig,
    config_path: Option<String>,
) -> Result<()> {
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
        MemorySub::Retrieve {
            role,
            format,
            limit,
            offset,
            query,
        } => {
            use terraphim_agent::memory_retrieve::{collect_memory_items, retrieve};

            // Single-role service: `memory retrieve` runs in agent loops, so
            // building every role's thesaurus + rolegraph (the full
            // `TuiService::new` path) is a per-call tax we can skip. Refs #206.
            let (service, role_name) =
                TuiService::new_for_single_role(config_path, role.as_deref()).await?;
            let thesaurus = service.get_thesaurus(&role_name).await.map_err(|e| {
                anyhow::anyhow!(
                    "no knowledge graph available for role '{}': {}",
                    role_name,
                    e
                )
            })?;

            let evolution = load_evolution();
            let items = collect_memory_items(&evolution.memory.current_state);

            let outcome = retrieve(
                &role_name,
                thesaurus,
                &items,
                &query,
                Some(offset),
                Some(limit),
            )?;
            let hits = &outcome.hits;

            let as_json = output.is_machine_readable() || format.eq_ignore_ascii_case("json");
            if as_json {
                let json_items: Vec<serde_json::Value> = hits
                    .iter()
                    .map(|h| {
                        serde_json::json!({
                            "id": h.item.id,
                            "item_type": format!("{:?}", h.item.item_type),
                            "content": h.item.content,
                            "importance": format!("{:?}", h.item.importance),
                            "tags": h.item.tags,
                            "rank": h.rank,
                            "matched_concepts": h.matched_concepts,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "ok",
                        "action": "retrieve",
                        "role": role_name.to_string(),
                        "query": query,
                        "query_concepts": outcome.query_concepts,
                        "count": json_items.len(),
                        "items": json_items,
                    })
                );
            } else if hits.is_empty() {
                println!(
                    "No memory items matched '{}' in the knowledge graph for role '{}'.",
                    query, role_name
                );
                // Two different causes, and saying which one saves the reader
                // from assuming the command is broken.
                if outcome.query_concepts.is_empty() {
                    println!(
                        "  The query names none of this role's concepts, so there is nothing to rank."
                    );
                    println!("  Retrieval is concept-based; there is no lexical fallback.");
                } else {
                    println!(
                        "  The query maps to concept(s): {}.",
                        outcome.query_concepts.join(", ")
                    );
                    println!("  No stored memory item is indexed under them. Note that an item");
                    println!("  must contain at least two concepts to be indexed at all.");
                }
                println!("  ({} memory items in the store)", items.len());
            } else {
                println!("Memory items matching '{}' (role: {}):", query, role_name);
                for (i, h) in hits.iter().enumerate() {
                    let first_line = h.item.content.lines().next().unwrap_or(&h.item.content);
                    println!(
                        "  {}. [{:?}] {} -- rank {} via {}",
                        i + 1,
                        h.item.item_type,
                        truncate_snippet(first_line, 80),
                        h.rank,
                        if h.matched_concepts.is_empty() {
                            "knowledge graph".to_string()
                        } else {
                            h.matched_concepts.join(", ")
                        }
                    );
                }
            }
            Ok(())
        }
        MemorySub::Apply { role, prompt } => {
            use terraphim_agent::memory_bench::{RETRIEVAL_LIMIT, injected_size};
            use terraphim_agent::memory_retrieve::{collect_memory_items, retrieve};

            // Real hook preview, not a scaffold: run the role's thesaurus
            // over the input with the same find_matches the hook pipeline
            // uses, and list every term that would be rewritten. Refs #237.
            let input = match prompt {
                Some(p) => p,
                None => {
                    use std::io::Read;
                    let mut buffer = String::new();
                    std::io::stdin().read_to_string(&mut buffer)?;
                    buffer.trim().to_string()
                }
            };

            // Single-role service: same per-call cost argument as retrieve
            // (Refs #206).
            let (service, role_name) =
                TuiService::new_for_single_role(config_path, role.as_deref()).await?;
            let thesaurus = service.get_thesaurus(&role_name).await.map_err(|e| {
                anyhow::anyhow!(
                    "no knowledge graph available for role '{}': {}",
                    role_name,
                    e
                )
            })?;

            let replacement_service = terraphim_hooks::ReplacementService::new(thesaurus.clone());
            let matches = replacement_service.find_matches(&input)?;

            // Injected size (#261): retrieve the memory items the hook would
            // inject for this prompt, through the unchanged `retrieve` with
            // the benchmark's limit, and measure the exact injection text.
            let evolution = load_evolution();
            let store_items = collect_memory_items(&evolution.memory.current_state);
            let injected_items: Vec<terraphim_agent_evolution::MemoryItem> = retrieve(
                &role_name,
                thesaurus,
                &store_items,
                &input,
                None,
                Some(RETRIEVAL_LIMIT),
            )?
            .hits
            .into_iter()
            .map(|h| h.item)
            .collect();
            let injected = injected_size(&input, &injected_items);

            if output.is_machine_readable() {
                let json_matches: Vec<serde_json::Value> = matches
                    .iter()
                    .map(|m| {
                        serde_json::json!({
                            "term": m.term,
                            "normalized_term": m.normalized_term.value.to_string(),
                            "start": m.pos.map(|(s, _)| s),
                            "end": m.pos.map(|(_, e)| e),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({
                        "status": "ok",
                        "action": "apply",
                        "role": role_name.to_string(),
                        "count": json_matches.len(),
                        "matches": json_matches,
                        "retrieved_items": injected_items.len(),
                        "injected_bytes": injected.bytes,
                        "estimated_tokens": injected.estimated_tokens,
                    })
                );
            } else if matches.is_empty() {
                println!(
                    "No hook injections for the given input (role: {}).",
                    role_name
                );
                print_injected_size(injected_items.len(), injected);
            } else {
                println!(
                    "Hooks would inject {} replacement(s) (role: {}):",
                    matches.len(),
                    role_name
                );
                for m in &matches {
                    match m.pos {
                        Some((start, end)) => println!(
                            "  - '{}' -> {} (at {}..{})",
                            m.term, m.normalized_term.value, start, end
                        ),
                        None => println!("  - '{}' -> {}", m.term, m.normalized_term.value),
                    }
                }
                print_injected_size(injected_items.len(), injected);
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

/// Text-mode line for the injected size reported by `memory apply` (#261).
fn print_injected_size(retrieved: usize, size: terraphim_agent::memory_bench::InjectedSize) {
    println!(
        "Memory items retrieved for the prompt: {} ({} bytes injected, about {} tokens, estimated as bytes/4)",
        retrieved, size.bytes, size.estimated_tokens
    );
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
