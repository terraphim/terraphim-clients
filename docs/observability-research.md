# Observability research: terraphim-clients

Refs terraphim/terraphim-clients#251. Phase 1 (research) of the disciplined observability workflow. Written 2026-09-11 against commit 1120180 on `main`. Read-only: no source was changed. Detailed per-area evidence lives in the three survey notes this document synthesises (logging surface, hot paths and boundaries, metrics and dependency feasibility); every claim below cites `file:line` from those surveys.

## 1. Summary

The workspace has six binaries and four incompatible logging setups, no spans, no OpenTelemetry and no metrics. The largest binary, terraphim-agent, has no tracing subscriber at all: its 38 tracing events and the roughly 150 events from the libraries it links survive only through tracing's `log` fallback, which flattens structured fields into strings. Two binaries write log lines to stdout where they interleave with machine-readable output. One library installs a global logger. Several long-running modes have no shutdown hook where a tracer could flush.

The single most consequential finding is not an observability gap but something a first trace would expose immediately: the agent runs an update check with an outbound HTTPS request before dispatching every subcommand, including the Claude Code hooks that fire on every tool call (`crates/terraphim_agent/src/main.rs:448-456`).

Priority order for implementation (child issues in section 9):

1. One shared tracing initialiser used by every binary; agent and cli move from `env_logger` to it; `.try_init()` everywhere; stderr everywhere.
2. `#[instrument]` on the eight workflow chains in section 5, with the context-propagation fixes for spawn_blocking, rayon and child processes.
3. `otel` feature on the long-running modes only (agent interactive, repl and listen; MCP stdio and SSE; LSP; `tsa watch`), OTLP over HTTP via the reqwest 0.13 line already in the lock.
4. `prometheus` feature with `/metrics` on the MCP SSE router and a bind address for the listener.
5. CI feature lanes and release-binary feature lists.

## 2. Current state per crate

| Crate | Kind | Facade in use | Subscriber or logger | Verdict |
|---|---|---|---|---|
| terraphim_agent | lib + bin | `log` (54 calls) and `tracing` (38 calls) mixed | `env_logger` via `log::set_boxed_logger` under `Once`, installed from library code at `src/service.rs:53` (`src/logging.rs:117`); `tracing-subscriber` declared at `Cargo.toml:54` but unused | No spans possible today; structured fields lost; library installs a global logger |
| terraphim_cli | bin | `log` (21) | External `terraphim_service::logging::init_logging` (`src/service.rs:34-36`), env_logger, `try_init` | Reads `RUST_LOG` only for module-qualified directives; bare level overridden |
| terraphim_grep | lib + bin | `tracing` (17) and `log` (5), bridged by `LogTracer` | Layered `registry().with(fmt::layer())` at `src/main.rs:143-151`, stderr, `.init()` | Best shape in the workspace; default filter `info,terraphim_grep=debug` prints debug on every run (`main.rs:145`) |
| terraphim_mcp_server | lib + bin | `tracing` (45) | `fmt().init()` at `src/main.rs:155-175`; stderr in stdio mode, stdout in SSE mode | Non-layerable form; `--verbose` doc at `main.rs:62` disagrees with code at `:155-159` |
| terraphim-session-analyzer (`tsa`) | lib + bin | `tracing` (32) | `fmt().init()` at `src/main.rs:190-194`, stdout, level and target stripped, `RUST_LOG` ignored | Log lines corrupt JSON and CSV output on the normal path (`main.rs:326`, `:464`, `parser.rs:55`) |
| terraphim_lsp | lib + bin | `log` (1) | none | The one `log::warn!` at `src/kg_analysis.rs:60` is dropped; LSP `log_message` channel unused |
| terraphim_sessions | lib | `tracing` (40) | none (correct for a library) | Host binary decides |
| terraphim_update | lib | `tracing` (108) | none | Prints to stdout from library code at `src/platform.rs:193-236`, `notification.rs:143`, `downloader.rs:256` |
| terraphim_negative_contribution | lib | `log` (1) | none | |
| terraphim_hooks, terraphim_command_runtime | lib | none | none | No logging at all |

Zero `#[instrument]`, zero spans (`rg "span!|Span::|.instrument("` over `crates/*/src` matches only `jiff::Span` arithmetic). Nine events carry key-value fields, six of them in the agent listener where no subscriber exists.

## 3. Verbosity controls today

| Binary | Flag | Env var |
|---|---|---|
| terraphim-agent | none | `LOG_LEVEL` only (`src/logging.rs:97`); `RUST_LOG` not read, although three integration tests set it |
| terraphim-cli | `--quiet` suppresses one `eprintln!` only | `LOG_LEVEL` wins; `RUST_LOG` partially |
| terraphim-grep | none | `RUST_LOG`, fallback `info,terraphim_grep=debug` |
| terraphim_mcp_server | `-v` (DEBUG vs INFO) | `RUST_LOG` merged |
| tsa | `-v` (debug vs info) | none |
| terraphim-lsp | none | none |

An operator cannot set one variable and get one behaviour. The target is `RUST_LOG` everywhere with `LOG_LEVEL` honoured as a fallback for one release, then removed.

## 4. println and eprintln triage

Counts over `src`, doc comments and inline tests included: terraphim_agent 918 println and 64 eprintln; tsa 103; terraphim_update 60 and 7; terraphim_grep 26 and 1. Classes: (a) user-facing output that stays on stdout, (b) diagnostics that become tracing events, (c) errors that become `tracing::error!` plus a user message on stderr.

| File | (a) keep | (b) to tracing | (c) to error + stderr | Notes |
|---|---|---|---|---|
| terraphim_agent/src/repl/handler.rs | ~295 | ~50 (`1169-1300` unimplemented-command echoes, `1459-1487` code echoes, `1605`) | ~30 (`578`, `821`, `833`, `1340`, `1368`, `1389`, `1481`, `1553`, `1572`, `1798-1888`) | Every REPL error goes to stdout via `println!`; scripted users cannot separate errors from results |
| terraphim_agent/src/main.rs | ~185 | ~15 (`535-542`, `1143`, `1651`) | ~23 (`769`, `785`, `857`, `909`, `1023`, `1097`, `1896-1902`, `2350`, `2374`) | |
| terraphim_agent/src/memory_command.rs | ~77 | ~8 (`110-123`, `166-171`) | 2 (`69`, `684`) | |
| terraphim_agent/src/server_command.rs | ~62 | ~4 (`486`, `502`, `683`) | ~17 (`496`, `512`, `543-570`, `647`, `726`, `967-976`) | |
| terraphim_agent/src/learn_command.rs | ~54 | ~4 (`369-385`, `400`) | 9 (`33`, `45`, `131`, `279`, `331`, `337`, `347`, `496`) | |
| terraphim-session-analyzer/src/reporter.rs | 59 | 0 | 0 | Report renderer; all output is the product |
| terraphim-session-analyzer/src/main.rs | ~38 | ~3 (`565-590`) | 1 (`240`) | |
| terraphim_update/src/platform.rs | 31 (`193-236`) | 0 | 0 | User-facing text emitted from a library function; should be returned as a `String` |

Rule for implementation: class (a) is untouched. Class (b) becomes `tracing::debug!`/`info!`. Class (c) becomes `tracing::error!(error = %e, ...)` plus the existing user message moved to stderr. The `--format json` and robot-mode paths must keep stdout clean of everything but the payload.

## 5. Span hierarchy

Process modes decide where an exporter is worth its cost. Long-running: agent `interactive` (TUI loop `main.rs:2979`), `repl` (`repl/handler.rs:85-118`), `listen` (`listener.rs:1351`); MCP stdio and SSE (`terraphim_mcp_server/src/main.rs:251-306`); LSP (`terraphim_lsp/src/bin/terraphim-lsp.rs:10`); `tsa watch` (`main.rs:574-617`). Everything else is one-shot and gets a stderr `fmt` layer with span timing only.

Root spans and the chains beneath them (function names, file:line, the fields to record; large arguments are skipped):

1. **Agent search, offline.** `main` (`main.rs:442`, fields command, robot, format) -> `run_offline_command` (`:1338`) -> `TuiService::new` (`service.rs:31`, roles) -> `handle_search_command` (`main.rs:1117`, query, role, limit, operator) -> `resolve_or_auto_route` (`service.rs:388`, requested_role, resolved_role, auto_routed) -> `search_with_query` (`service.rs:456`, results recorded after; start the span before `lock().await` so lock wait is visible) -> external `TerraphimService::search`. The robot formatting block (`main.rs:1201-1310`) is a sibling span.
2. **Agent search, server.** `main` -> `run_server_command` (`server_command.rs`, server_url) -> `ApiClient::resolve_role` (`client.rs:50`; note the extra `/config` round trip before every search) -> `ApiClient::search` (`client.rs:100`, http.method, http.url, http.status_code, otel.kind=client; inject `traceparent` here).
3. **REPL command.** Session span for lifecycle only (`handler.rs:2884`); a fresh root per `readline` (`handler.rs:85-118`); `execute_command` (`:263`, command recorded after parse, auto_corrected) -> `handle_search` (`:463`) -> as 1 or 2.
4. **MCP tool call.** `call_tool` (`lib.rs:2359`, tool, mcp.request_id from `RequestContext.id`, otel.kind=server) is the root; the `_meta` object can carry a client `traceparent` (rmcp 0.9.1 `Meta` is a JSON object). Beneath: `McpService::search` (`lib.rs:183`) or `grep_files` (`:1349`) or `spawn_rlm_cli` (`:1573`, rlm.command, session_id, exit_code; set `TRACEPARENT` on the child). `terraphim_service()` (`lib.rs:159`) builds a fresh service from a config clone on every call and deserves its own span before anyone optimises it.
5. **Grep search.** `main` (`grep main.rs:478`) -> `resolve_thesaurus` (entries) -> `HybridSearcher::new` (`hybrid_searcher.rs:229`) -> `TerraphimGrep::search` (`lib.rs:157`, sufficiency, chunks) -> `HybridSearcher::search` (`:255`) -> `search_kg` (`:310`) and `search_code` (`:342`, one span per path, files_scanned) -> `judge_with_metrics` -> `search_with_rlm_fallback` (`lib.rs:310`) -> `OpenRouterClient::chat_completion` (`openrouter_client.rs:73`, model, http.status_code).
6. **Update check.** Child of the process root, never its own trace: `check_update` (`terraphim_update/src/lib.rs:242`) -> `check_update_r2` (`:258`; the body is `spawn_blocking`, so capture and enter the span) -> `fetch_manifest` (`manifest.rs:149`, attempts, http.status_code) -> `ureq::get` (`:156`, `traceparent` via `.set`).
7. **LSP diagnostics.** `did_change` (`server.rs:106`, uri, version) -> `schedule_diagnostics` (`:37`, aborted_previous) -> debounced task (`:53-73`, child span created inside the spawned task with the parent captured before `tokio::spawn`) -> `scan_file` (`negative_contribution/src/scanner.rs:28`, findings) -> `publish_diagnostics`.
8. **Hooks.** Learning capture: `main` -> `run_learn_command` (`learn_command.rs:18`) -> `process_hook_input_with_type` (`learnings/hook.rs:86`, session_id and tool_name recorded after parse) -> `capture_from_hook` (`:65`) -> `capture_failed_command` (`capture.rs:1010`) -> `fs::write` (`:1095`). KG guard: `main` -> `TuiService::new` -> `handle_hook_command` (`main.rs:2157`, hook_type, decision) -> `CommandGuard::check` -> `get_thesaurus` -> `ReplacementService::replace_fail_open` (`terraphim_hooks/src/replacement.rs`; `CommandValidator::validate` already records duration_ms at `validation.rs:34-51`) -> `validate_command_against_kg`. `session_id` from the hook JSON is the root field.
9. **Listener poll.** `run_forever` lifecycle span; `poll_once` (`listener.rs:1361`) root per poll; `process_comment` (`:1448`, issue, author, decision); tracker calls wrapped with otel.kind=client (upstream `terraphim_tracker` cannot inject headers); `execute_dispatch` (`shell_dispatch.rs:186`, exit_code, duration_ms, timed_out; `TRACEPARENT` on the child).

Boundaries where context is lost today and the required handling: `spawn_blocking` at `terraphim_update/src/lib.rs:263,414`, `terraphim_sessions/src/connector/native.rs:161`, `cursor.rs:128` (capture `Span::current()` before, enter inside); `tokio::spawn` after a sleep at `terraphim_lsp/src/server.rs:53` (`.instrument(span)`); rayon `par_iter` at `terraphim-session-analyzer/src/analyzer.rs:70` (enter per closure); the `std::sync::mpsc` bridge from `notify` at `native.rs:199` (create the per-file span at the flush site with the watcher span as explicit parent); child processes (only `TRACEPARENT` in the environment). Three tokio runtimes are created in one agent process lifetime (`main.rs:449, 494, 552`), so the tracer provider must be installed before the first and flushed after the last, tied to none of them.

## 6. Outbound boundaries and error context

| Client | Target | Header injection | Errors logged with context |
|---|---|---|---|
| `terraphim_agent/src/client.rs:19-22` (28 methods) | terraphim server, default `http://localhost:8000` | Yes, one helper on the `RequestBuilder`, or `reqwest-middleware` (already in the lock via terraphim_orchestrator) | No: bare `?` on `send()`; no log call in the file |
| `terraphim_grep/src/openrouter_client.rs:88-92` | OpenRouter | Yes | Partly: status mapped to an error, nothing logged, no latency |
| `terraphim_update/src/manifest.rs:156`, `downloader.rs:207` (`ureq`, blocking inside spawn_blocking) | R2 `downloads.terraphim.ai` | Yes via `.set` | Yes, per attempt with attempt number and elapsed |
| `terraphim_update/src/lib.rs:399-520` (`self_update`) | GitHub Releases | No | Entry `info!` only |
| `terraphim_agent/src/listener.rs:1303` (`terraphim_tracker::GiteaTracker`) | Gitea | No, upstream change needed | Partly; three `post_comment` results discarded with `let _ =` |
| `terraphim_agent/src/service.rs:562-590`, `terraphim_grep/src/lib.rs:114` (`terraphim_service::llm`) | configured LLM provider | No, upstream | Provider name at `info!`, error wrapped |

Subprocess spawns without duration or trace context: `spawn_rlm_cli` (`mcp_server/src/lib.rs:1603-1631`, no timeout), `learnings/replay.rs:95` (`sh -c`, no timeout), `terraphim_hooks/src/discovery.rs:58` (`which`). Dispatch paths already record `duration_ms` (`shell_dispatch.rs:206-315`, `commands/modes/local.rs:145-179`).

## 7. Metrics catalogue and SLIs

Naming `terraphim_<subsystem>_<name>_<unit>`, histograms in `_seconds`, counters `_total`. Forbidden labels (unbounded): query text, file paths, session ids, issue numbers, learning ids, Gitea logins, document ids and role names (user-defined per installation; carry the role as a span field or resource attribute). Bounded label values are enforced through typed enums in one `telemetry` module.

Existing durations to map without adding timers: `GrepStats` (`terraphim_grep/src/lib.rs:66-71`: search_latency_ms, rlm_latency_ms, chunks_returned, kg_hits), `ValidationResult` duration (`terraphim_hooks/src/validation.rs:34-51`), dispatch `duration_ms`, `DownloadResult` (`downloader.rs:139-188`), `EnrichmentResult.duration_ms` (`enricher.rs:93-123`), MCP uptime (`main.rs:255-272`).

A defect to fix by construction: robot-mode `ResponseMeta.elapsed_ms` starts its timer after the search has returned (`main.rs:1198` then `:1207`; `server_command.rs` search then `:113`), so it reports formatting time only. Populate it from the handler span duration.

Metric families, with the emit site in code:

- MCP: `terraphim_mcp_tool_calls_total{tool,outcome}` and `terraphim_mcp_tool_duration_seconds{tool}` around the 26-arm match at `lib.rs:2364`; `terraphim_mcp_service_build_duration_seconds` at `lib.rs:159-165`; `terraphim_mcp_search_documents_returned` at `lib.rs:233`; `terraphim_mcp_grep_files_scanned` at `lib.rs:1349-1380`; `terraphim_mcp_rlm_spawn_total{outcome}` and duration at `lib.rs:1573-1625`; `terraphim_mcp_sse_connections_active`; `terraphim_mcp_uptime_seconds`; `terraphim_mcp_config_updates_total` at `lib.rs:175-180`.
- Agent: `terraphim_agent_command_duration_seconds{command,mode}` and `_total{command,mode,outcome}` around `main.rs:552-570` and `repl/handler.rs:364-366`; `terraphim_agent_search_duration_seconds{mode}` and `_results` at `service.rs:431-458` and `client.rs:100`; `terraphim_agent_http_client_requests_total{endpoint,status_class}` and duration from the constructor at `client.rs:13-26`; `terraphim_agent_llm_requests_total{provider,outcome}` and duration at `service.rs:588-589`, `:674`; listener `polls_total{outcome}`, `poll_duration_seconds`, `comments_processed_total{action}`, `tracker_requests_total{op,outcome}`, `dispatch_total{kind,outcome_code}`, `dispatch_duration_seconds{kind}`, `dispatch_timeouts_total{kind}` at `listener.rs:1361-1742` and `shell_dispatch.rs:226,298`; `terraphim_agent_learning_lookups_total{store,hit}`; `terraphim_agent_update_checks_total{outcome}` at `terraphim_update/src/lib.rs:258-282` and `update_manifest_fetch_attempts_total` at `manifest.rs:155-182`.
- Grep: `terraphim_grep_search_duration_seconds{haystack,sufficiency}`, `rlm_duration_seconds`, `chunks_returned{haystack}`, `kg_hits`, `code_search_duration_seconds` (one observation per path at `hybrid_searcher.rs:278-282`, never per file), `kg_search_duration_seconds`, `openrouter_requests_total{status_class}` and duration. Grep is one-shot; its `--json` `stats` block is the de facto metrics export and the doc should say so rather than promise scraping.
- Sessions: `import_duration_seconds{connector}`, `import_files_total{connector,outcome}`, `imported_total{connector}`, `cache_size`, `autoimport_total{outcome}`, `search_duration_seconds{kind}`, `search_index_documents` (the BM25 index is rebuilt over every cached session on every query at `search.rs:100-103`), `enrich_duration_seconds`.
- LSP: `diagnostics_duration_seconds`, `document_changes_total`, `documents_open`.

SLIs (no documented targets exist in the repo; every SLO is initial and must be calibrated from the #253 baseline):

| SLI | Definition | Initial SLO |
|---|---|---|
| MCP tool call latency | p95 of tool_duration for `tool="search"` over 5 min | p95 under 2 s; the per-call service rebuild at `lib.rs:159` is the suspected floor |
| MCP tool call success | 1 minus internal_error share | 99.5 percent |
| Agent search success | `outcome="ok"` share of `command="search"` | 99 percent |
| LLM and OpenRouter error rate | 5xx plus timeout share; 429 tracked separately | error under 5 percent, 429 under 1 percent |
| Update-check failure rate | `outcome="failed"` share over 24 h | under 2 percent |
| Listener poll health | fatal share, and time since last success | fatal under 0.1 percent; staleness under 90 s |
| Dispatch timeout rate | timeouts over dispatches | under 1 percent at the 300 s default |

Timeout inventory that bounds these: OpenRouter 120 s (`openrouter_client.rs:39`); agent ApiClient 30 s, 60 s if `TERRAPHIM_CLIENT_TIMEOUT` is unparsable (`client.rs:15-20`); listener poll 30 s, dispatch 300 s (`listener.rs:124-162`); manifest fetch 15 s with 3 attempts and backoff (`manifest.rs:20,156,182`); download 30 s (`downloader.rs:52`); LSP debounce 250 ms (`config.rs:21-23`); upstream terraphim_service 30 s default and 10 s API client.

## 8. Dependency feasibility and feature design

Verified offline from the lock file and the local registry cache:

- tokio 1.52.3, hyper 1.10.1 only (no hyper 0.14), axum 0.8.9, tower-http 0.6.11, reqwest 0.12.28 and 0.13.4 (ring and aws-lc-rs providers already coexist), tracing-subscriber 0.3.23 with `registry` and `tracing-log` on, h2 present, tokio-util present. rmcp 0.9.1 does not pull tonic.
- Nothing from OpenTelemetry, `metrics` or `prometheus` is in the lock today.
- Recommended line: opentelemetry 0.32, opentelemetry_sdk 0.32 (needs rand 0.9 and thiserror 2, both locked), opentelemetry-otlp 0.32 with `default-features = false, features = ["http-proto", "reqwest-client", "reqwest-rustls"]` (reqwest 0.13 already locked via genai; avoid the default blocking client inside async binaries), tracing-opentelemetry 0.33. Defer `grpc-tonic`: it adds tonic 0.14 and prost 0.14 as new crates.
- `metrics` and `metrics-exporter-prometheus` are not in the local cache and could not be verified offline; resolve online before committing. The OpenTelemetry metrics API plus the OTLP exporter is fully verified and may be enough. If the `prometheus` crate is used instead, set `default-features = false` to avoid protobuf 3.7 and expose `/metrics` from the existing axum router at `terraphim_mcp_server/src/main.rs:266-270` rather than a second HTTP stack.
- Registry policy (`Cargo.toml:27-85`): telemetry crates go under `[workspace.dependencies]` as plain crates.io entries, never `registry = "terraphim"`. Instrumentation of upstream `terraphim_service` (for example `chat_completion` at `llm.rs:45`) lands in a Gitea-only crate and would not reach the crates.io build; keep the first pass inside this repo.
- Release binaries: `release-binaries.yml:271,295` builds the agent with default features only (so the `server` feature is not in the public binary, contradicting `docs/agent-reference.md:154-157`); grep is built with an explicit list at `:290,297`. Any shipped telemetry feature must be added to those lines explicitly.

Feature shape, mirroring the workspace conventions (kebab-case, `dep:` syntax, heavy deps off by default, umbrella feature):

```toml
[features]
# Telemetry is opt-in: the OpenTelemetry SDK and OTLP exporter add many crates
# and a background export task; one-shot CLI invocations must not pay for it.
otel = ["dep:opentelemetry", "dep:opentelemetry_sdk", "dep:opentelemetry-otlp", "dep:tracing-opentelemetry"]
prometheus = ["dep:metrics", "dep:metrics-exporter-prometheus"]
telemetry-full = ["otel", "prometheus"]
```

Configuration: use the standard `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` environment variables, which the SDK reads natively; add `--metrics-bind` and `--otlp-endpoint` next to `--bind` in the MCP args (`main.rs:55-73`); add an optional `telemetry` block to the listener JSON config (`listener.rs:95-162`). `DeviceSettings` lives upstream and is not a first-pass surface. On exit in one-shot binaries, call provider shutdown with a bounded timeout so a dead collector cannot hang the CLI.

## 9. Shutdown and flush points

| Mode | Exit mechanism | Flush insertion point |
|---|---|---|
| MCP SSE | `tokio::signal::ctrl_c` (`main.rs:83-87`, awaited at `:298`) | After `:298`. The parent `CancellationToken` is never cancelled by the signal handler, so the server task is dropped, not drained; cancel `sse_server.config.ct` here too |
| MCP stdio | `waiting().await` returns on client stdin close (`main.rs:305-306`) | After `:306`; no signal handler exists |
| LSP | tower-lsp `serve` returns; `shutdown` is a no-op (`server.rs:93-95`) | Inside `shutdown` and after `.serve(service).await` |
| Agent REPL | rustyline Interrupted, Eof or `/quit` (`handler.rs:97-118`) | Next to `save_history` at `:124`; SIGINT during a command kills the process |
| Agent TUI | `TuiAction::Quit` (`main.rs:3111, 3239`) | In `run_tui` after `ui_loop` (`:2952-2960`) and the error-cleanup branches at `:2930-2943` |
| Agent listener | none; loops until error or kill (`listener.rs:1351-1359`) | Add `tokio::select!` on `ctrl_c` around `poll_once` |
| `tsa watch` | none; `std::thread::sleep` loop (`main.rs:574-617`) | Needs a `ctrlc` handler |
| One-shot agent and cli | `std::process::exit` at 30 sites (`emit_robot_error_and_exit` at `main.rs:565, 573` is the chokepoint) | Route every exit through one helper that flushes first |

## 10. Sensitive data

- Medium: `log::debug!("Device settings: {:?}", ...)` at `terraphim_agent/src/service.rs:94` and `terraphim_cli/src/service.rs:74` dumps the free-form storage `profiles` map; remote backends would carry credentials there. Redact or log named fields only.
- Medium: REPL web commands echo user-supplied headers and bodies to stdout (`repl/handler.rs:1203-1213`); the commands are stubs today, but the echo will leak Authorization headers once implemented.
- Low: `info!("Args: {:?}", args)` at `terraphim_mcp_server/src/main.rs:178` (no secrets today).
- No log macro prints a token or key value directly. Redaction helpers exist (`terraphim_sessions/src/redaction.rs:10-14`, `learnings/hook.rs:125`) and should be reused by the telemetry module for any field that could carry user content.

## 11. Overhead and cardinality rules

- Never a span per file, per line, per match or per chunk: grep matching (`hybrid_searcher.rs:342-395`, loops inside fff-search), session JSONL parsing (`native.rs:267-271`), enrichment (`enricher.rs:98-105`), concept pair loops (`enricher.rs:205-206`). One span per call with counts as fields.
- Hidden per-call costs that a first trace will show and that must be measured before optimising: the MCP per-call `TerraphimService` rebuild (`lib.rs:159-165`), the full thesaurus clone per grep query (`hybrid_searcher.rs:177`), the BM25 index rebuild per session search (`search.rs:100-103`), and the update check before every agent command (`main.rs:448-456`).
- One-shot CLIs pay only for the `fmt` layer; the OTLP exporter is gated by the `otel` feature and an explicit endpoint.

## 12. CI implications

Both pipelines run clippy with `-D warnings` over `--all-targets`, so feature-gated code must be warning-free with the feature on and off. Add per-crate lanes mirroring the existing `enrichment` pattern: `cargo clippy -p <crate> --features otel,prometheus -- -D warnings` and the matching test lane for terraphim_mcp_server, terraphim_agent and terraphim_grep. Never `--all-features` on terraphim_mcp_server (it enables `zlob`, which needs Zig). `packaged_install_graph_regression` guards the agent dependency graph and will need updating when optional deps are added. `publish-crates.yml` dry runs require every optional dep to resolve on crates.io. These lanes belong in the #254 pipeline once the features exist.

## 13. Child issues

Each is blocked by the UB audit #252 reaching Phase 8, so audit findings keep stable line references, and by #254 for the CI lanes.

1. Shared tracing initialiser: a `terraphim_telemetry` module or small crate with `init(config) -> Result<Guard>` using `registry()`, `EnvFilter` (`RUST_LOG`, `LOG_LEVEL` fallback for one release), stderr writer, `try_init`; adopt in all six binaries; remove the agent's `env_logger` path and the library-side logger install at `service.rs:53`; fix tsa and MCP SSE stdout logging; grep default filter to `info`; fix the MCP `--verbose` doc.
2. Migrate `log` users (terraphim_cli, terraphim_lsp, terraphim_negative_contribution, terraphim_agent's 54 calls) to `tracing`; the LogTracer bridge stays for dependencies.
3. `#[instrument]` on the nine chains in section 5 with the context-propagation fixes and the `ResponseMeta.elapsed_ms` correction; println triage classes (b) and (c).
4. `otel` feature: workspace deps on the 0.32 line, provider install in the long-running modes only, flush at the shutdown points in section 9, `TRACEPARENT` for child processes, `traceparent` injection on the agent and OpenRouter clients.
5. `prometheus` feature: registry in the long-running modes, `/metrics` on the MCP SSE router, listener `telemetry` config block with a bind address, the metric families in section 7 with typed labels; resolve the `metrics` crate constraints online first.
6. Follow-ups outside observability but found by it: gate the pre-dispatch update check for hook subcommands; redact the `DeviceSettings` debug dump; decide whether the `server` feature belongs in the shipped agent binary.
