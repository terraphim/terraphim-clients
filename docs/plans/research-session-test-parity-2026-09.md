# Terraphim-Agent Session-Search Test Plan

**Full functional coverage benchmarked against cass v0.6.11** · Compiled 2026-09-03 · Workspace: `.cluster/cass-terraphim-testplan/` (evidence base: subagent_01…08, review.md, brief.md)

---

## Chapter 1 — Scope, Goals, Benchmark Method, Assumptions

- **Status:** Draft for review (flagged decisions in §4 await Alex's veto window)
- **Audience:** Rust engineers on the terraphim team
- **Deliverable class:** Test *plan* — this document defines WHAT to test and HOW to verify full functional coverage of everything cass already does. It does not implement the tests.

---

### 1. Purpose and Scope

The goal of this plan is **parity**: terraphim-agent's session-search feature must functionally cover everything the mature **cass** CLI (live `v0.6.11`) already does, and the coverage must be *verifiable*, not asserted. Cass is treated as the frozen functional reference; terraphim is the system under test.

Surfaces in scope:

1. **REPL** — the `/sessions` command family, all 14 subcommands.
2. **CLI** — the `sessions` subcommands of the terraphim-agent binary (non-interactive use).
3. **Robot mode** — machine-readable output surface (payload shape, exit semantics, parseability).
4. **`terraphim_sessions` crate** — the library API the agent builds against, including its existing 99-test suite.

Out of scope for the *plan* itself (see §5): implementing missing terraphim features, modifying cass, rewriting skill docs, and performance work beyond the existing NFR bench (#3014).

### 2. Benchmark Method

The method is a **capability-contract diff** followed by assertion-level traceability:

1. **Cass capability catalog.** Cass's live capability surface was cataloged as a stable contract of **100 capabilities, C01–C100**, grouped into: Search, Indexing, Health/Diagnostics, Sources/Fleet, Models/Semantic, Analytics, Export/Share, Resume, Integration/Robot, and Config/Exclusions. This catalog is the plan's spine.
2. **Parity verdicts.** Each capability received a parity verdict against terraphim's implemented features: **FULL / PARTIAL / MISSING / N-A / UNCLEAR**. Verdicts are **code-verified** (against terraphim source, not docs) and **adversarially reviewed** (a second pass hunting for verdict inflation).
3. **Disposition rules.** Every FULL and PARTIAL row must map to **≥1 test case**. Every MISSING row must receive either a **GAP-tagged proposed test** (proposing what terraphim *would* need to build) or an **explicit deferral** with a reason. Every N-A row carries a **written justification** — N-A is a decision, not an omission. **UNCLEAR** rows (verdict not resolvable from code) get **runtime-probe tests** that settle the question empirically.
4. **Semantic-parity assertions.** The cass skill supplies **92 documented behavior assertions, E01–E93** (the SELF-TEST suite plus 16 reference docs). These supply the expected *semantics* — what output a query should return, how a resume behaves, what an export contains — beyond mere "command exists" parity. Assertions are adopted where terraphim has a counterpart mechanism.
5. **No duplication of existing coverage.** Terraphim's existing tests — **99 in `terraphim_sessions`** plus **3 session-touching agent integration tests** — were audited. The plan **extends** this suite; where an existing test already covers a capability row, the matrix links to it rather than proposing a clone.

### 3. Definitions

For THIS plan, **"fully covering all existing cass functionality"** is met when **all four** hold:

- **(a) Full disposition.** 100% of C01–C100 rows are dispositioned as one of: test assigned / GAP-deferred with reason / N-A with written justification.
- **(b) Per-row executable verification.** Every FULL and PARTIAL row has ≥1 automated test **or** a documented manual probe (for rows that cannot be automated on CI, e.g. interactive-only behavior).
- **(c) Semantic parity.** All applicable E-assertions (E01–E93) are adopted as test expectations wherever terraphim has a counterpart mechanism; each non-adopted assertion is explicitly tied to a MISSING/N-A disposition.
- **(d) CI blind spots closed.** Two known blind spots are fixed: (1) feature-gated tests that are invisible to default-features CI (they compile/pass only under non-default feature flags), and (2) agent integration tests that CI does not currently run. Parity claimed by tests CI never executes does not count.

### 4. Assumptions and Decisions

The following are **decided** for this plan and flagged for Alex to veto during review.

- **(i) Registry vs. local crate version.** The agent binary builds against **registry `terraphim_sessions` 1.20.4**, while the local terraphim-ai checkout is **1.21.3**. **DECISION:** CI tests the **registry build** as production truth, plus a **nightly `[patch]`-style canary lane** that rebuilds the agent against the local 1.21.3 crate to catch drift between what is tested and what is developed. Veto point: if the team prefers the reverse (test local, canary the registry), the harness in Ch4 changes but the test cases do not.
- **(ii) Exit-code collision.** Terraphim CLI exit **4** (empty search result, machine mode) numerically collides with cass exit **4** (network error). **DECISION:** parity assertions target **payload and behavior** (stdout shape, error text, state effects); **never bare exit codes alone**. Exit codes may appear as secondary assertions only, paired with payload checks.
- **(iii) N-A policy.** Capabilities that are cass-infrastructure-specific — TUI macros, self-upgrade, shell completions, pages hosting, fleet wizardry — are recorded as **justified N-A rows** with a one-line reason each, not silently dropped. This keeps the 100-row contract honest and auditable.
- **(iv) Doc-drift policy.** Known divergences between terraphim's session-search skill docs and the code — the removed `/sessions import`, the documented-but-nonexistent `CLAUDE_SESSIONS_DIR`, the stale enricher API, and the phantom `claude-log-analyzer` — become explicit **DOCS-DRIFT test cases**, each with a **fix-or-implement decision**: either the doc is corrected or the feature is implemented and tested. Drift is treated as a defect either way.
- **(v) Platform scope.** CI is Linux-only. macOS dev-machine behavior (notably `dirs` crate path resolution) is covered by **HOME-only isolation** in the harness plus **platform-mirrored fixtures**, per the runnability review. Tests must not assume macOS paths, and CI results must not be read as macOS proof.

### 5. Out of Scope

This plan explicitly does **not** cover:

- **Implementing missing terraphim features** (GAP rows propose tests; implementation is separate engineering work).
- **Modifying cass** in any way — it is the reference, not the SUT.
- **Rewriting skill docs**, beyond flagging drift per §4(iv).
- **Performance tuning**, beyond the existing NFR bench (**#3014**); this plan is functional parity only.

### 6. How to Read This Plan

Chapter order and dependencies:

- **Ch2 — Cass capability catalog:** the C01–C100 contract, grouped, one row per capability.
- **Ch3 — Parity matrix:** C-rows × terraphim T-rows, with verdicts and dispositions.
- **Ch4 — Harness and fixtures:** how tests run (HOME-only isolation, platform-mirrored fixtures, canary lane, CI blind-spot fixes).
- **Ch5–Ch6 — Test cases:** the actual TC definitions per surface and group.
- **Ch7 — Traceability and acceptance:** the closure report proving §3(a)–(d).

Artifact ID conventions used throughout:

| Prefix | Meaning |
|---|---|
| `C-xx` | Cass capability (C01–C100, the frozen reference contract) |
| `T-xx` | Terraphim feature (the implemented counterpart) |
| `E-xx` | Cass documented behavior assertion (E01–E93, semantic expectation) |
| `TC-xx` | Test case (automated test or documented manual probe) |

Every acceptance claim in Ch7 must be traceable as `C-row → verdict → disposition → TC-row(s) → E-assertion(s)` where applicable. A row without a traceable chain fails acceptance.


---

# Chapter 2 — cass Capability Catalog (Condensed Reference)

## 2.1 Orientation: what cass is and why it is the benchmark

**cass** (Rust crate *coding-agent-search*, homebrew-installed, live binary **v0.6.11**, `api_version=1`, `contract_version=1`) is a coding-agent session-search CLI: it ingests session transcripts from AI coding harnesses into a local index (lexical Tantivy + optional semantic/HNSW) and serves search, drill-down, answer-packing, analytics, health/recovery, and resume workflows over them. Its live machine-readable surface comprises **34 top-level commands**, **20 connectors** (incl. `claude_code`, `codex`, `gemini`, `opencode`, `cline`, `aider`, `cursor`, `openclaw`, `kimi`), **21 exit codes**, **40 introspect response schemas**, **47 argv-recovery normalizations**, and **7 documented workflows** (`cold-start`, `api-discovery`, `health-preflight`, `bounded-search`, `answer-pack`, `session-drilldown`, `semantic-models`). It is the benchmark for this test plan because it is the mature, production-hardened implementation of exactly the feature terraphim-agent is building — its skill docs (`~/.claude/skills/cass/`) codify behaviors and failure modes mined from real usage (pitfalls, recovery recipes, observability rules), and its `capabilities`/`introspect` output provides a machine-checkable contract rather than prose. One correction inherited from the evidence phase and propagated here: the local clone `cass_memory_system` is **not** the cass source — it is `cass-memory` (Bun/TS), an upstream *consumer* of the CLI that shells out to `cass` and maps its exit codes; all capability claims below rest on live binary introspection plus skill docs (full evidence tags in `subagent_01.md`).

## 2.2 Capability catalog (C01–C100)

Groups follow `subagent_01.md` §6, which carries the per-row evidence tags (`[LIVE]`, `[CAP]`, `[INTRO]`, `[HELP:cmd]`, `[SKILL:name]`). Descriptions are condensed to one line; the "why it matters" column is filled only where the test relevance is non-obvious. Per-command flag tables are **not** duplicated here — see `subagent_01.md` §2.

### Search (C01–C20)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C01 | `search` core: positional query; repeatable `--agent`/`--workspace`; `--source local\|remote\|all\|<host>`; `--limit` (0 = unbounded w/ RAM-proportional cap + `CASS_SEARCH_NO_LIMIT_CAP/BYTES` overrides); `--offset` | Limit-0 unbounded path is a known sharp edge — pin cap behavior explicitly |
| C02 | Time filters `--days/--today/--yesterday/--week/--since/--until`; ISO date/datetime, keywords `today\|yesterday\|now`, relative `-7d\|-24h\|-30m\|-1w` | Multiple accepted date grammars are a dense parity-test surface |
| C03 | Output control: `--json`/`--robot`, `--robot-format json\|jsonl\|compact\|sessions\|toon`, `--robot-meta` (elapsed_ms, wildcard_fallback, cache_stats), `--fields minimal\|summary\|custom`, `--max-content-length` (+`_truncated`), `--max-tokens`, `--request-id`, `--display table\|lines\|markdown`, `--highlight` | Token-budget shaping for agent consumption; `_truncated` is a parse contract |
| C04 | Cursor pagination: base64 `--cursor` + `hits_clamped` in response | Deterministic paging contract for looped agent queries |
| C05 | Server-side aggregations `--aggregate agent,workspace,date,match_type` → `aggregations.<f>.buckets[]{key,count}`; hard cap `max_agg_buckets=10`; recipe: combine with `--limit 1` | Bucket cap is a behavioral limit, not documentation |
| C06 | Query diagnostics: `--explain` (parsed query/strategy/cost), `--dry-run`, `--timeout ms` partial results, `suggestions`, `wildcard_fallback` | Timeout must degrade to partial results, not fail the query |
| C07 | Search modes `--mode lexical\|semantic\|hybrid`; default `hybrid_preferred`; `_meta.realized_mode`/`fallback_mode` parse contract | Declared vs actually-realized mode must be machine-observable |
| C08 | ANN/HNSW semantic search `--approximate` (requires prior `index --semantic --approximate`) | Index/search flag dependency ordering |
| C09 | Embedder/rerank selection `--model`, `--rerank`, `--reranker` (help's `cass models --list` pointer is stale; use `models status`) | Stale help pointer — version-dependent pin (§2.4.5) |
| C10 | Daemon/latency tiers `--daemon/--no-daemon/--two-tier/--fast-only/--quality-only` (fast ~1 ms, quality ~130 ms, max_refinement_docs 100) | Tier semantics give measurable performance-tier behavior |
| C11 | Chained searches: `--robot-format sessions` emits source_path-per-line; `--sessions-from <file\|->` consumes it (stdin supported) | Multi-hop pipelining primitive for agent workflows |
| C12 | `search --refresh`: incremental index pass before query; errors non-fatal | Refresh failure must not block search (fail-open) |
| C13 | `view` drill-down: `-n/--line`, `-C` default 5, `--source`; argv recovery accepts `path:line`, `line_number` aliases, field bundles (`source_path=… line_number=…`) | Typo-tolerant drill-down entry points |
| C14 | `expand` context window: `--line` required, `-C` default 3 | — |
| C15 | `context` related-session clustering, `--limit` default 5 | — |
| C16 | `sessions` listing: `--workspace`, `--current` (auto-resolve), `--limit` default 10 (1 with `--current`); `current` positional shorthand accepted | Current-session auto-resolution is environment-sensitive |
| C17 | `timeline`: `--since/--until/--today`, `--group-by hour\|day\|none`, repeatable `--agent` | — |
| C18 | `pack` answer-pack: `--max-tokens 12000`, `--max-sessions 8`, `--max-evidence 24`, `--context-lines 3`, `--max-excerpt-chars 1600`, `--require-evidence`, `--explain-selection`, freshness policy/window; schema `pack/evidence/warnings/privacy/freshness/health/omitted/limits/realized/_meta` | The rich pack schema is a parity target in itself, not just a command |
| C19 | Pack-intent argv routing: `answer/why/handoff/bundle` aliases and pack-only flags route to `pack` instead of `search` | Intent routing is behavioral, beyond pure parsing |
| C20 | Wildcard fallback: `search "*"` terrain-scan pattern; `wildcard_fallback` surfaced in `_meta` | Degraded-query behavior must be observable |

### Indexing (C21–C29)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C21 | Incremental `index` with `--json` result schema (`success, conversations, messages, elapsed_ms, indexing_stats, entrypoint, quarantined_conversations, lexical_update_deferred`) | Schema fields enumerate testable index postconditions |
| C22 | `index --full` full rebuild | — |
| C23 | `index --force-rebuild` | — |
| C24 | Watch mode: `--watch`, `--watch-once` (repeatable), `--watch-interval` default 30 | — |
| C25 | Semantic indexing: `--semantic` (fast+quality tiers), `--build-hnsw`, `--embedder` default fastembed; `--approximate` builds HNSW (feeds C08) | — |
| C26 | Idempotent indexing: `--idempotency-key`; consumer maps mismatch to exit 5 | Exit-5 meaning diverges between binary and consumer (§2.4.5) |
| C27 | NDJSON progress events: `--progress-interval-ms` 2000, `--no-progress-events`, env `CASS_INDEX_NO_PROGRESS_EVENTS` | Machine-parseable progress stream for long index runs |
| C28 | `--robot-trace-ingest`: ingest robot trace files during index | — |
| C29 | `import chatgpt`: split web-export conversations.json into connector-indexable files; `--output-dir`; encrypted import via `CHATGPT_ENCRYPTION_KEY` | — |

### Health / Diagnostics (C30–C46)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C30 | `health` fast preflight: <50 ms, exit 0/1, `--stale-threshold` 300 s default (live latency 9 ms) | The gate for every other operation (§2.4.1) |
| C31 | `status` full state surface (stale threshold 1800 s): database/index/semantic/pending/rebuild(+pipeline)/ingest_quarantine/policy_registry/coverage_risk/topology_budget/doctor_summary/recommended_action(+commands) | `recommended_action` drives agent self-recovery |
| C32 | `state` = `status` alias | — |
| C33 | Three-state decision model: healthy / stale-but-usable / broken-uninitialized, with distinct stale vs missing index outcomes | Core operational model — see §2.4.1 |
| C34 | `doctor --check` bounded read-only truth surface (checks, coverage_delta, repair_readiness, safe_auto_eligibility, plan_fingerprint) | Plan fingerprint is the precondition for C36 |
| C35 | `doctor --fix` legacy safe-auto-run: contract-declared safe repairs only; archive-first; never deletes source sessions; failure-marker gating | Non-destructiveness ("never delete source sessions") is the invariant to test |
| C36 | Fingerprinted repair flow: `--dry-run` → plan fingerprint → `--yes --plan-fingerprint <FP>`; `--allow-repeated-repair` override | Replay/double-repair protection |
| C37 | `doctor --force-rebuild` (alias `--force`): derived rebuild without bypassing coverage gates/fingerprints | "Force" must stay inside safety gates |
| C38 | 26 doctor response schemas (check, archive-scan/normalize, backups-list/verify, baseline-save/diff/update, cleanup, reconstruct, repair-dry-run/receipt, restore-rehearsal, sync-gaps, health/status-summary, failure-context, error-envelope, semantic-model-fallback, support-bundle, safe-auto-run) | Typed recovery surface for schema-level parity |
| C39 | `diag`: connectors/database/index/paths/platform/version; `--quarantine`, `-v` | — |
| C40 | `stats`: conversations/messages/by_agent/top_workspaces/date_range/raw_mirror; `--by-source` | — |
| C41 | `triage` one-shot readiness (aliases `ready`, `preflight`): readiness, next_command, recommended_commands, starter_workflows, discovery; top-level `cass --json` defaults to triage | Cold-start contract for first-touch agents |
| C42 | `capabilities` self-description: 34 commands, 20 connectors, 30+ env vars, 21 exit codes, 30 features, limits, 47 mistake recoveries, 7 workflows | The machine-readable parity source of truth |
| C43 | `introspect`: full arguments + 40 response schemas for typed clients | Schema-level parity target |
| C44 | `api-version`: crate/api/contract version triple | — |
| C45 | Argv mistake-recovery engine: 47 documented normalizations (typos, aliases, k=v promotion, query folding/repositioning, output-format aliases, field-bundle paste, leading-flag moves) | Countable, enumerable usability contract |
| C46 | Ingest quarantine + circuit breaker: `quarantined_conversations`, `circuit_breaker_limit` 25/1 h window, `diag --quarantine` | Poison-session containment semantics |

### Sources / Fleet (C47–C56)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C47 | `sources list/add/remove`: named source_id, platform presets, repeatable `-p/--path`, `--no-test`; remove `--purge` + `-y` | — |
| C48 | `sources sync`: `-s` source selection, `--no-index`, `--dry-run`, verbose transfer info | — |
| C49 | `sources doctor`: connectivity/config diagnostics per source | — |
| C50 | `sources discover`: SSH host discovery from ~/.ssh/config, presets, `--skip-existing` | — |
| C51 | `sources setup` wizard: 7 phases, resumable state (~/.cache/cass/setup_state.json), `--hosts/--non-interactive/--skip-install/--skip-index/--skip-sync/--timeout` | Resumability across interruptions is the testable property |
| C52 | `sources mappings {list,add,remove,test}` (P6.3): remote→local prefix rewrite, per-agent rules, test-rewrite simulation | Path-rewrite correctness (remote paths → local view) |
| C53 | `sources agents {list,exclude,include}`: persistent connector exclusions in sources.toml `disabled_agents`; default purge+rebuild of excluded agent data; `--keep-indexed-data` future-only blocking | Default is destructive (purge) — needs explicit safety tests |
| C54 | `sources artifact-manifest`: lexical artifact evidence manifest (`--write`, `--verify-existing`, `--expected-manifest`) for federated installs | Integrity evidence for multi-machine setups |
| C55 | Remote-source search semantics: `--source` filter incl. hostname; hits expose `workspace_original` pre-mapping; source_id (`local`, names) on view/expand/context | Pre/post-mapping provenance must survive into hits |
| C56 | Fleet ops patterns (skill-documented, not binary commands): source sync scheduling, one-shot SSH query, parallel fan-out | Documentation-level parity, not CLI parity |

### Models / Semantic (C57–C63)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C57 | `models status`: state machine not_installed/partial/installed (+ installed/models/files/revision/cache_lifecycle/lexical_fail_open/license/policy embedder) | State machine drives all fallback expectations (C62) |
| C58 | `models install`: default all-minilm-l6-v2, `--mirror`, `--from-file` (air-gapped), `-y` | Offline install path testable without network |
| C59 | `models verify`: SHA256 integrity, `--repair` | — |
| C60 | `models backfill`: bounded batch (`--tier fast\|quality`, `--embedder hash\|fastembed`, `--batch-conversations 64`, `--scheduled`), message/byte checkpoint caps (10k msgs / 8 MiB) | Boundedness prevents runaway backfill — caps are behavioral |
| C61 | `models remove` / `models check-update` (revision tracking) | — |
| C62 | Semantic fallback chain: model missing → hash embedder; unavailable → lexical fail-open; policy `semantic.hybrid_preferred.v1` conservative fallback; hybrid RRF Σ1/(60+rank) | Graceful degradation is core — see §2.4.3 |
| C63 | Semantic daemon (Unix): warm inference socket, `--socket/--idle-timeout/--max-connections`, `CASS_DAEMON_SOCKET` | — |

### Analytics (C64–C70)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C64 | `analytics status`: row counts, freshness, coverage, drift warnings | — |
| C65 | `analytics tokens`: time-bucketed token usage (`--group-by hour\|day\|week`), dimensional filters, cost-estimation pattern | — |
| C66 | `analytics tools`: per-tool counts + derived metrics | — |
| C67 | `analytics models`: top models + `.data.by_api_tokens.rows[].derived{api_coverage_pct, tool_calls_per_1k_api_tokens, plan_message_pct}` + timeseries buckets | — |
| C68 | `analytics rebuild`: rollup backfill with progress, `--force` | — |
| C69 | `analytics validate`: invariant + drift check, `--fix` safe Track A repair | — |
| C70 | Coverage/health metrics: `api_token_coverage_pct`, `estimate_only_pct` (<10 % healthy), `message_metrics_coverage_pct`, `track_a_fresh`; rebuild trigger rule | Threshold-encoded health semantics ("<10 % healthy") |

### Export / Share (C71–C75)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C71 | `export`: markdown/text/json/html, `-o`, `--clipboard`, `--include-tools`, `--include-skills` | — |
| C72 | `export-html`: self-contained HTML, `--encrypt` (Web Crypto), `--password-stdin`, `--no-cdns`, `--theme`, `--dry-run/--explain`, `--open` | — |
| C73 | `pages` encrypted static archive: targets local/github/cloudflare, `--path-mode`, secret scanning (`--scan-secrets/--fail-on-secrets/--secrets-allow/--secrets-deny`), `--no-encryption` consent flag, `--export-only`, `--verify` CI, `--preview/--port`, config surface | Secret scanning is a privacy gate — publication must not leak credentials |
| C74 | Pages multi-slot key management + recovery (key list/add-password/add-recovery/revoke/rotate/show-recovery --qr, Argon2id/HKDF-SHA256, `pages decrypt --recovery`) — **documented surface, NOT present in live 0.6.11** | Version pin: treat as newer-HEAD, never as guaranteed capability (§2.4.5) |
| C75 | `mirror prune`: operator-controlled raw-mirror retention (`--older-than`, `--max-size`, `--keep-tag`, `--safety-hold-down` 7 d, dry-run default + `--apply`) | Dry-run-by-default is the safety invariant |

### Resume (C76–C79)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C76 | `resume` resolution: native harness command; default argv-per-line, `--shell`, `--exec` (process replace), `--json`; `--agent` overrides (claude/codex/opencode/pi/omp/gemini) | — |
| C77 | Per-harness path detection incl. Antigravity (`agy`) and pi vs oh-my-pi disambiguation | Ambiguous-harness resolution needs fixtures |
| C78 | Subagent non-resumability ("subagent trap") handling | Known failure mode from real usage — explicit test case |
| C79 | Resume output contract: emitted command is the native CLI's own form (e.g. `claude resume <uuid>`); do not hand-construct | Contract prohibition — testable as exact output form |

### Integration / Robot (C80–C88)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C80 | Robot output conventions: `_meta` block, request-id echo, `*_truncated` flags, JSON error envelope (`err.kind/message/hint`) | The machine-first contract — see §2.4.2 |
| C81 | `robot-docs` 12 topics incl. `contracts`, `wrap`, `sources`, `analytics`, `doctor` | — |
| C82 | Global `--robot-help` machine-first help | — |
| C83 | JSONL execution tracing: `--trace-file` / `CASS_TRACE_FILE` spans | — |
| C84 | Shell completions (5 shells) + man page generation | — |
| C85 | Scriptable TUI: `--once`, `--asciicast`, `--inline`, macros (`--record-macro/--play-macro`), `--anchor`, `TUI_HEADLESS` | Headless TUI is an automation seam |
| C86 | Self-upgrade: `--check` exit semantics (0 current / 1 update available), cadence `--force`, `-y` install (execs over process), update-prompt suppression env | Exit-code overload (0/1) is behavioral, not textual |
| C87 | API/contract versioning: api v1 + contract v1 + crate version in machine output; exit 6 on incompatibility | Version-negotiation contract |
| C88 | Upstream consumer contract (cass-memory/cm): exit-code subset {0,2,3,4,5,9,10}, availability fallback modes, hit-content coercion, sanitization | Consumer/binary divergence is a documented seam (§2.4.5) |

### Config / Exclusions (C89–C100)

| ID | Capability (one line) | Why it matters for session search |
|---|---|---|
| C89 | Data location overrides: global `--db`, per-command `--data-dir`, `CASS_DATA_DIR`, `CASS_DB_PATH` | The sandbox-isolation lever all tests rely on |
| C90 | Harness exclusion config: sources.toml `disabled_agents`, manual-edit fallback | — |
| C91 | Per-harness discovery root envs: `CODEX_HOME`, `GEMINI_HOME`, `OPENCODE_STORAGE_ROOT`, `PI_CODING_AGENT_DIR`, `CASS_AIDER_DATA_ROOT` | Fixture-redirection levers for connector tests |
| C92 | Semantic tuning env: embedder selection, batch size 128, batch warn/fail watchdogs (30 s/5 min), backfill checkpoint caps | — |
| C93 | Indexer responsiveness governor: `CASS_RESPONSIVENESS_DISABLE`, `CASS_RESPONSIVENESS_CALIBRATION conformal\|static`; live pipeline knobs in health output | — |
| C94 | Streaming consumer tuning: `CASS_STREAMING_CONSUMER_COMMIT_SECS` (5), `CASS_STREAMING_CONSUMER_COMBINE` (1) | — |
| C95 | Output format/color env: `CASS_OUTPUT_FORMAT`, `TOON_*`, `CASS_NO_COLOR`, `CASS_RESPECT_NO_COLOR`, `NO_COLOR` | — |
| C96 | Global UX flags: `--color`, `--progress`, `--wrap/--nowrap`, `-q/-v` | — |
| C97 | Connector support set: 20 connectors (codex, claude_code, gemini, clawdbot, vibe, opencode, amp, cline, aider, cursor, chatgpt, pi_agent, factory, openclaw, kimi, copilot, copilot_cli, qwen, crush, hermes) | Count drifts across versions — assert against live `capabilities`, not constants |
| C98 | Concurrency/lock semantics: exit 7 lock/busy + bounded-backoff guidance; observed multi-second/minute status latency under contention | Known hang-under-contention (issue #196) — tests need timeouts (§2.4.5) |
| C99 | Session format coverage & line-number semantics: claude_code/codex/gemini/antigravity formats; `line_number` anchors into session files enabling view/expand and user-prompt-at-top heuristic | Line anchors are the drill-down backbone — see §2.4.4 |
| C100 | Workspace matching semantics: repeatable workspace filters, `workspace_original` vs mapped workspace, `sessions --current` resolution, context workspace clustering | Case-sensitivity pitfalls known — see §2.4.4 |

## 2.3 Cross-cutting behaviors

**Three-state health model (C30, C31, C33).** Operators must never conflate three states: (1) *healthy* — `health` exit 0 (sub-50 ms preflight; 9 ms live) → search immediately; (2) *stale-but-usable* — `health` exit 1 with `index.stale=true` → search now, refresh in background under a wall-clock cap (e.g. `timeout 600 cass index …`); (3) *broken/uninitialized* — `status.database.exists=false` or `documents=0` → `doctor --fix` then `index --full`. Stale thresholds differ per surface (health 300 s, status 1800 s, triage 300 s), and `index.stale` is distinct from `index.status=missing` (live DB showed `missing` with reason "lexical Tantivy metadata missing"). Every test that touches search/index ops must gate on `health` and wrap in a timeout.

**Robot output conventions (C80, C03, C04, C87).** Machine output carries a `_meta` block; `--request-id` values are echoed back for correlation; trimmed content is flagged with `*_truncated` (paired with `--max-content-length`/`--max-tokens`); errors use a JSON envelope (`err.kind/message/hint`); cursor pagination reports `hits_clamped`; `--robot-meta` opt-in adds `elapsed_ms`, `wildcard_fallback`, `cache_stats`. Exit codes 0–24 (21 documented codes) carry retryable/not-retryable semantics — full table in `subagent_01.md` §3. Any terraphim parity surface should be checked for the same parseable conventions, not just human-readable output.

**Semantic fallback chain (C57, C62).** Degradation is designed, not exceptional: model missing → hash embedder (degraded but functional); embedder unavailable → lexical fail-open (search still returns results); policy `semantic.hybrid_preferred.v1` makes conservative fallback decisions; hybrid ranking uses RRF Σ1/(60+rank). `models status`'s not_installed/partial/installed state machine predicts which leg of the chain applies, and `_meta.realized_mode`/`fallback_mode` expose what actually happened. Tests must assert search succeeds with no model installed and that realized/fallback modes reflect reality.

**Workspace matching & line-number heuristics (C13, C16, C55, C99, C100).** Workspace filters are repeatable; remote hits expose `workspace_original` before mapping rewrite; `sessions --current` auto-resolves the active workspace/session; `context` clusters by workspace. `line_number` anchors into the raw session file, which is what makes the view→expand→context→resume drill-down chain possible — including the user-prompt-at-top heuristic (a session's user prompt sits at a known anchor). Known pitfalls: workspace case sensitivity and `path:line` / `line_number` alias recovery in argv.

**Version-dependent items to pin in tests (from `subagent_01.md` §7).**
- **Pages key management (C74) is doc-only in 0.6.11** — `pages key …`, `pages decrypt`, `export-html --with-recovery` do not exist in the installed binary; parity tests must mark them "version-dependent / newer-HEAD", not guaranteed.
- **Stale help pointer:** `search --model` help says "Use `cass models --list`" but no `models list` subcommand exists (`models status` is the real surface) — confirmed live; help-text drift is itself testable.
- **Connector-count drift:** skill docs list 19 connectors; live binary lists 20 (adds `hermes`). Live `capabilities` output is authoritative per install — never hard-code counts in tests.
- **Exit-5 divergence:** live `capabilities` says 5 = data corruption; consumer `cass-memory` maps 5 = IDEMPOTENCY_MISMATCH. Use live capabilities as truth for the binary and note consumer divergence.
- **Skill baseline vs binary:** skill docs describe v0.3.6-era behavior plus HEAD notes; installed binary 0.6.11 contains all named HEAD features (`sources agents`, `artifact-manifest`). On any parity mismatch, check `cass --version` first.
- **Coverage limits of the evidence base:** `mirror`/`swarm` subcommand args were captured at help level only; index run semantics, doctor repair flows, pages encryption round-trip, remote sync, and model install/backfill were never verified live (read-only constraint). Related live observations: index was `missing`, semantic `needs_consent`, and one `search` hung >25 s under lock contention (known issue #196) — hence the health-gate + timeout-wrapper rule above.

## 2.4 Note for readers

The C-IDs in this chapter are the **stable contract** for the rest of the plan: Chapter 3 assigns each C-ID a parity verdict (covered / partial / gap / N/A) for terraphim-agent, and Chapter 7 traces every test case back to one or more C-IDs. This chapter is deliberately a condensed reference — the **full-detail evidence base is `subagent_01.md`** (§2 per-command flag tables, §3 exit-code contract, §6 catalog with per-row evidence tags); per-command flags are not duplicated here. When a test case cites a C-ID, verify details against `subagent_01.md` §2/§6 rather than this summary, and re-check `cass --version` if behavior appears to contradict the catalog.


---

# Chapter 3 — Parity Matrix & Gap Analysis

Status: FILLED (Round 4, chapter writer) · Inputs: subagent_05a–05d (C01–C100, review fixes applied on disk), review.md, subagent_07.md (coverage-adversary)
Cass reference: cass 0.6.11. Terraphim evidence: implementation-map T-IDs (subagent_02) + file:line where verified. Terraphim_sessions pin: registry 1.20.4 in CI; local 1.21.3 = nightly canary (Decision R1, §3.6).

## 3.1 Reading guide

**Verdict vocabulary** (exactly five tokens; qualifiers live in the source chunk's notes, not in the verdict cell):

- **FULL** — cass capability reproduced by terraphim with equivalent semantics. *No row in this matrix reaches FULL.*
- **PARTIAL** — a real terraphim counterpart exists but is missing named parts, reshapes the mechanism, or is thinner than cass's surface. Every PARTIAL row names its gaps; the counterpart cell is the assertion anchor.
- **MISSING** — no terraphim counterpart found for a capability that *would* be meaningful in terraphim's design. These become GAP rows (deferred tests, §3.4) or roadmap items.
- **N-A** — cass-infrastructure-specific capability with no terraphim counterpart *by design* (e.g., persistent-index flags with no persistent index). Every N-A carries a justification in its source chunk row; testing it would test vaporware. N-A rows may still carry a terraphim-native regression assertion (see C22/C75/C98).
- **UNCLEAR** — verdict cannot be settled from static evidence; needs a runtime probe or a registry-1.20.4-vs-local-1.21.3 check. Exactly 2 rows (C24, C44); dispositions in §3.4.

**Priorities:** P0 core search correctness · P1 robustness · P2 operational · P3 out-of-scope. Priority is test-effort ranking, not verdict severity (C98 is N-A yet P1 — its regression traps T39/T43/T42 still need tests).

**Traceability rule:** every row's disposition — GAP-deferred test, N-A justification, runtime probe, or merged test cluster — is traced in Chapter 7 (E-ID mapping attaches there; per Round-3 review decision, chunk rows reference T-IDs only). Full "what a parity test would assert" text stays in the chunk files (subagent_05a–05d) and lands in Ch5/Ch6; **this chapter intentionally omits the assert column** — for any row, consult `subagent_05{a,b,c,d}.md` row C-NN.

**Drift tag:** rows noted `[DRIFT]` (C02, C16, C29, C44, C48, C87, C91, C92, C97, C99) rest on agent-level verified facts but touch surfaces that may differ between registry 1.20.4 and local 1.21.3 — testable in CI, re-verified by the nightly canary (§3.6).

## 3.2 The parity matrix (100 rows, 10 area groups)

Columns: C-ID | cass capability (short) | verdict | terraphim counterpart (T-ID / file:line / none) | priority. One line per row; assert-level detail lives in the chunk files and Ch5/Ch6 (see §3.1 pointer note).

### G1 — Search core & output control (C01–C06)

<!-- ROWS:G1 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C01 | search core: positional query, repeatable --agent/--workspace, --source, --limit (0=unbounded+RAM cap+env), --offset | PARTIAL | T15 commands.rs:1111-1121 REPL search; T19 CLI search --limit default 10; T16 BM25 ≤50-result cap; T20 --source on list only | P0 |
| C02 | time filters --days/--today/--yesterday/--week/--since/--until; ISO + relative -7d/-24h | MISSING | none for search; import-time only T13 (ImportOptions since/until) [DRIFT] | P1 |
| C03 | output control: --json/--robot, format variants, --robot-meta, --fields, --max-content-length/--max-tokens, --request-id, --display, --highlight | PARTIAL | T02 main.rs:2970-3006 (--robot/--format JSON); T15 top-10 table + total; T19 100-char preview; T33 export json/markdown | P2 |
| C04 | cursor pagination: base64 --cursor + hits_clamped | MISSING | no cursor/offset query params; robot Pagination{total,returned,offset,has_more} exists (schema.rs:134-157) | P2 |
| C05 | server-side aggregations --aggregate agent/workspace/date/match_type (max 10 buckets) | MISSING | none; nearest T30 /sessions stats (corpus totals + per-source, not query-scoped) | P2 |
| C06 | query diagnostics: --explain, --dry-run, --timeout partial results, suggestions, wildcard_fallback | MISSING | none; only diagnostic-ish signal is machine-mode empty query → exit 4 (T19) | P2 |

### G2 — Search modes, chaining & freshness (C07–C12)

<!-- ROWS:G2 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C07 | search modes --mode lexical/semantic/hybrid; _meta.realized_mode/fallback_mode contract | PARTIAL | T15 commands.rs:1160-1168 (search parse arm) + handler.rs:2014-2115 (enrichment branch 2015-2046, plain 2048-2115); T18 KG boost count×10000 | P1 |
| C08 | ANN/HNSW semantic search --approximate | MISSING | none; T18 KG boost is lexical-thesaurus, not embeddings | P3 |
| C09 | embedder/rerank selection --model/--rerank/--reranker | MISSING | none; single fixed scorer BM25 Okapi (T16; T36 prints "Scorer: BM25 (Okapi)") | P3 |
| C10 | daemon/latency tiers --daemon/--two-tier/--fast-only/--quality-only | MISSING | none; in-process singleton service T39 | P3 |
| C11 | chained searches: --robot-format sessions emits source_path lines; --sessions-from consumes | MISSING | none; nearest T33 export (full dumps, not hit lines) + T02 machine JSON | P2 |
| C12 | search --refresh: incremental index pass before query, non-fatal errors | PARTIAL | T06 auto-import on first cache-touching call when cache empty; T16 per-call BM25 rebuild; T07 import_all skips failing connectors; T14 watcher unexposed | P2 |

### G3 — Viewing, listing & timeline (C13–C17)

<!-- ROWS:G3 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C13 | view drill-down: -n/--line, -C default 5, argv recovery (path:line, field bundles) | PARTIAL | T32 /sessions show (5-message, 80-char preview); T19 CLI preview = first matching message (100 chars) | P1 |
| C14 | expand window: --line required, -C default 3 | MISSING | none; T32 fixed 5-message preview is the only drill-down | P2 |
| C15 | related-session context: --limit default 5 | PARTIAL | T26 /sessions related [id] [--min n] — top 5, excludes self; --min accepted but ignored | P1 |
| C16 | sessions listing: --workspace, --current auto-resolve, --limit default 10 | PARTIAL | T20 sessions_by_source → list --source (REPL-only handler.rs:1959-1968; CLI list limit-only main.rs:1271-1277); T04 aider roots at CWD [DRIFT] | P1 |
| C17 | timeline: --since/--until/--today, --group-by hour/day/none, repeatable --agent | PARTIAL | T29 /sessions timeline [--group-by day/week/month] [--limit], groups by started_at date | P1 |

### G4 — Answer packs & wildcard (C18–C20)

<!-- ROWS:G4 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C18 | pack answer-pack: token/session/evidence budgets, --require-evidence, freshness policies, rich schema | MISSING | none; nearest T33 export (raw session dump, no budgets/evidence) | P3 |
| C19 | pack-intent argv routing (answer/why/handoff/bundle aliases) | MISSING | none | P3 |
| C20 | wildcard fallback: search "*" terrain-scan, wildcard_fallback in _meta | PARTIAL | robot document-search sets wildcard_fallback = concepts_matched.is_empty() (main.rs:2231,4383; schema.rs:299); no sessions-search equivalent | P2 |

### G5 — Index lifecycle & import (C21–C29)

<!-- ROWS:G5 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C21 | index incremental refresh (--json schema incl. quarantined_conversations) | MISSING | none; T36 /sessions index is STATUS-ONLY (counts, scorer line; builds nothing); T06 auto-import; T07 skip+truncate | P2 |
| C22 | index --full full rebuild | N-A | T16 in-memory BM25 rebuilt per call — "full" is the only mode; nothing durable exists | P3 |
| C23 | index --force-rebuild | N-A | same as C22 (T16); nothing durable to discard | P3 |
| C24 | watch mode --watch/--watch-once/--watch-interval default 30 | UNCLEAR | T14 native file watcher, 200ms debounce + dedup — public API only, NOT exposed via REPL/CLI; 1.20.4-vs-1.21.3 probe pending | P2 |
| C25 | semantic indexing --semantic tiers, --build-hnsw, --embedder fastembed | MISSING | none; nearest T21/T23 enrichment concepts (in-memory, REPL-bound, offline TUI thesaurus) | P3 |
| C26 | idempotent indexing --idempotency-key, exit-code mapping | MISSING | none; T36 status-only; T38 disk cache read-but-never-written; T43 clone() resets cache | P2 |
| C27 | NDJSON progress events --progress-interval-ms + env override | MISSING | none | P3 |
| C28 | --robot-trace-ingest during index | MISSING | none | P3 |
| C29 | import chatgpt: split web-export conversations.json; --output-dir; encrypted import | MISSING | none; connectors T04 (claude/codex/aider/cline/opencode/TSA) but agent build registers ONLY claude-code-native, claude-code, cursor, aider (T05) [DRIFT] | P2 |

### G6 — Health, status & self-description (C30–C45)

<!-- ROWS:G6 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C30 | health fast preflight (<50ms, exit 0/1, --stale-threshold 300) | MISSING | none; no health→exit-code mapping (terraphim exit 4 = empty search, not health) | P2 |
| C31 | status full state surface (db/index/semantic/pending/rebuild/ingest_quarantine/policy_registry/coverage_risk/doctor_summary/recommended_action) | PARTIAL | T36 /sessions index --verbose — status-only counts + "Scorer: BM25"; other ~9 state families absent by design | P2 |
| C32 | `state` = status alias | MISSING | none | P3 |
| C33 | three-state decision model (healthy / stale-but-usable / broken-uninitialized) | PARTIAL | T01/T03 SourceInfo per-connector status (+estimate); T06 auto-import when cache empty | P2 |
| C34 | doctor --check bounded read-only truth surface (checks, coverage_delta, repair_readiness, plan_fingerprint) | MISSING | none | P2 |
| C35 | doctor --fix safe-auto-run (archive-first, never deletes source sessions, failure-marker gating) | MISSING | none | P2 |
| C36 | fingerprinted repair flow (--dry-run → fingerprint → --yes --plan-fingerprint; --allow-repeated-repair) | MISSING | none | P3 |
| C37 | doctor --force-rebuild | MISSING | T06 auto-import when cache empty; T42 server-mode always cold-imports; T43 clone() resets cache | P2 |
| C38 | 26 doctor response schemas | MISSING | none | P3 |
| C39 | diag (connectors/database/index/paths/platform/version; --quarantine, -v) | PARTIAL | T01–T03 sources/connectors + status; T36 index counts | P2 |
| C40 | stats (conversations/messages/by_agent/top_workspaces/date_range/raw_mirror; --by-source) | PARTIAL | T30 /sessions stats totals + per-source counts; total_messages + user/assistant splits present (service.rs:329-361) | P2 |
| C41 | triage readiness (aliases ready/preflight; top-level --json defaults to triage) | MISSING | none (T02 --robot JSON is per-command output, not a readiness verdict) | P2 |
| C42 | capabilities self-description (commands/connectors/env vars/exit codes/features/limits/mistake recoveries/workflows) | PARTIAL | robot subcommand capabilities/schemas/examples (verified); breadth of advertised fields unprobed | P2 |
| C43 | introspect (full arguments + 40 response schemas) | PARTIAL | robot capabilities/schemas/examples (verified); schema-count assertions range-based until probed | P2 |
| C44 | api-version (crate/api/contract triple) | UNCLEAR | none in implementation map; registry 1.20.4 vs local 1.21.3 drift; --version + robot output probe pending | P2 |
| C45 | argv mistake-recovery engine (47 normalizations) | PARTIAL | AutoCorrection{original,corrected,distance} (schema.rs:124-131); unknown-command "Did you mean" (schema.rs:213-218); ForgivingParser aliases q/s/query/find→search (main.rs:1500-1545) | P3 |

### G7 — Ingest robustness, sources & fleet (C46–C56)

<!-- ROWS:G7 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C46 | ingest quarantine + circuit breaker (25 failures/1h) | N-A | none — no persistent ingest pipeline; closest is T07 import_all skipping failing connectors (stateless, per-run) | P3 |
| C47 | sources list/add/remove (named source_id, platform presets, repeatable --path, --no-test; remove --purge + -y) | PARTIAL | T01/T02 /sessions sources list (aliases detect; offline + server variants; JSON via --robot/--format); detection hardcoded T04, registry fixed at build T05 | P1 |
| C48 | sources sync (-s selection, --no-index, --dry-run) | PARTIAL | T06 auto-import when cache empty (single attempt); T07 skips failing connectors, global limit truncates; ImportOptions since/until/limit (T09–T13) [DRIFT: CLI exposure unconfirmed] | P1 |
| C49 | sources doctor (connectivity/config diagnostics per source) | PARTIAL | T01/T03 per-connector status (+estimate); degraded-source probe is cheapest high-value assertion | P2 |
| C50 | sources discover (SSH host discovery, presets, --skip-existing) | N-A | none — no remote-source model by design | P3 |
| C51 | sources setup wizard (7 phases, resumable state) | N-A | none — zero-config auto-detection is the design | P3 |
| C52 | sources mappings {list,add,remove,test} (remote→local prefix rewrite) | N-A | none — no remote→local mapping model | P3 |
| C53 | sources agents {list,exclude,include} (persistent disabled_agents in sources.toml) | MISSING | none at runtime; compile-time analog is T05 feature-gated registry (claude-code-native, claude-code, cursor, aider only) | P2 |
| C54 | sources artifact-manifest (--write / --verify-existing) | N-A | none — no persisted mirror to manifest | P3 |
| C55 | remote-source search semantics (--source hostname; workspace_original pre-mapping; origin_host metadata) | N-A | none — remote sources out of scope by design; local source-attribution unverified (candidate future row) | P3 |
| C56 | fleet ops patterns (skill-documented, not binary commands) | N-A | none — no fleet surface; single-process agent | P3 |

### G8 — Semantic models & analytics (C57–C70)

<!-- ROWS:G8 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C57 | models status: state machine not_installed/partial/installed + revision/cache lifecycle | PARTIAL | T15 (enrichment build gates hybrid path) + T21 (rebuild advice + dry-run counts on non-enrichment build) + T25 (metadata.enrichment skipped when None) — no revision/cache lifecycle by design | P2 |
| C58 | models install: default all-minilm-l6-v2, --mirror, --from-file, -y | N-A | none — thesaurus provisioned offline via TUI compilation, outside the sessions CLI | P3 |
| C59 | models verify: SHA256 integrity + --repair | N-A | none — no binary artifacts to corrupt; nearest analog T21 dry-run counts (usability, not integrity) | P3 |
| C60 | models backfill: bounded batch, tiers, checkpoint caps | PARTIAL | T21 (/sessions enrich [id] → SessionConcepts into in-memory cache) + T23 (SessionEnricher/EnrichmentConfig: dominant topics + co-occurrences + optional RoleGraph) | P2 |
| C61 | models remove / check-update (revision tracking) | N-A | none — T21 rebuild advice implicitly replaces stale enrichment state | P3 |
| C62 | semantic fallback chain: model missing → hash embedder; unavailable → lexical fail-open; hybrid RRF fusion | PARTIAL | T15 (enrichment → hybrid search_with_thesaurus; else plain search) + T16 (BM25 Okapi, body ≤50k chars, ≤50 results, ≥10% cutoff) + T17 (substring fallback) + T18 (KG boost count×10000, NOT RRF) + T22 (concepts text-search fallback) + T21 (rebuild advice) | P0 |
| C63 | semantic daemon: warm inference socket, --socket/--idle-timeout | MISSING | none; T21 in-memory enrichment cache is the only warm state (scoped to REPL process) | P2 |
| C64 | analytics status: row counts, freshness, coverage, drift warnings | MISSING | T30 (stats: totals + per-source) is the only fragment | P2 |
| C65 | analytics tokens: time-bucketed usage, --group-by, cost estimation | MISSING | none | P3 |
| C66 | analytics tools: per-tool counts + derived metrics | MISSING | T34 (/sessions files: tool→FileAccess read/write mapping) + T35 (by-file substring) are adjacent, not analytics | P2 |
| C67 | analytics models: top models + derived metrics + timeseries | MISSING | none | P3 |
| C68 | analytics rebuild: rollup backfill, --force | MISSING | none | P3 |
| C69 | analytics validate: invariant + drift check, --fix | MISSING | none; weak analog T21 dry-run counts (coverage estimate, not validation) | P3 |
| C70 | coverage/health metrics: api_token_coverage_pct, estimate_only_pct, rebuild trigger | MISSING | none; weak analog T21 dry-run counts | P3 |

### G9 — Export, publishing & resume (C71–C79)

<!-- ROWS:G9 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C71 | export: markdown/text/json/html, -o, --clipboard, --include-tools, --include-skills | PARTIAL | T33 (/sessions export [--format json\|markdown\|md] [-o path] [--session id]; unknown format rejected) + T37 (serde model) + T34/T35 (tool calls → FileAccess as separate surface) | P1 |
| C72 | export-html: self-contained HTML, --encrypt Web Crypto, --password-stdin, --no-cdns, --theme, --dry-run | MISSING | none (verified: no HTML export, no encryption anywhere) | P3 |
| C73 | pages encrypted static archive: targets, secret scanning, --verify CI, config surface | MISSING | none | P3 |
| C74 | pages key management + recovery (doc-only surface, NOT in live cass 0.6.11) | N-A | none — reference side is docs-only; no testable behavior on either side | P3 |
| C75 | mirror prune: raw-mirror retention, safety hold, dry-run default | N-A | none — reads harness stores in place; enrichment cache in-memory only (T21); no mirror exists to prune; keep read-only-invariant assertion | P3 |
| C76 | resume resolution: native harness command, argv-per-line, --shell, --exec, --json, --agent overrides | MISSING | none (verified: no resume functionality, no cross-harness resume commands); nearest surfaces T32 show + T40 learn-from-session | P1 |
| C77 | per-harness path detection incl. Antigravity, pi vs oh-my-pi disambiguation | PARTIAL | T30 (stats per-source — proves multi-harness ingestion awareness); detection depth beyond source enumeration unverified | P2 |
| C78 | subagent non-resumability handling ("subagent trap") | MISSING | T37 (MessageRole in serde model) is a weak structural hook only; latent until resume exists | P2 |
| C79 | resume output contract: emitted command is the native CLI's own form | MISSING | none (dependent on C76) | P2 |

### G10 — Machine contract, configuration & connectors (C80–C100)

<!-- ROWS:G10 -->
| C-ID | cass capability (short) | verdict | terraphim counterpart | priority |
|---|---|---|---|---|
| C80 | robot output conventions: _meta block, request-id echo, *_truncated flags, JSON error envelope | PARTIAL | ResponseMeta (version, elapsed_ms, timestamp) + preview_truncated (schema.rs:317-323; populated main.rs:2180-2200) + TokenBudget.truncated + RobotError{code,message,details,suggestion} (main.rs:1315-1331); absent: request-id echo, literal _meta shape, full *_truncated breadth | P1 |
| C81 | robot-docs: 12 topics incl. contracts/wrap/sources/analytics/doctor | PARTIAL | robot capabilities/schemas/examples (robot/schema.rs); T02 sessions sources ≈ sources topic; no topic docs (negative-assert) | P2 |
| C82 | global --robot-help machine-first help | PARTIAL | none — standard human --help only; no machine-first help variant | P2 |
| C83 | JSONL execution tracing: --trace-file / CASS_TRACE_FILE spans | MISSING | none (TERRAPHIM_VERBOSE logging only) | P2 |
| C84 | shell completions (5 shells) + man page generation | N-A | none — infrastructure absent by design; out of session-search parity scope | P3 |
| C85 | TUI + scripting surface: --once, --asciicast, --inline, macros, --anchor, TUI_HEADLESS | N-A | none — no TUI; terraphim REPL is conversational, not a scriptable TUI (repl-sessions feature flag only) | P3 |
| C86 | self-upgrade: --check exit semantics, cadence, -y install | N-A | none — no self-upgrade mechanism (--version exists for agent binary) | P3 |
| C87 | API/contract versioning: api v1 + contract v1 + crate version in machine output; exit 6 on incompatibility | PARTIAL | ResponseMeta.version = CARGO_PKG_VERSION (schema.rs:56-59); api/contract triple + exit 6 absent [DRIFT] | P2 |
| C88 | upstream consumer contract (cass-memory/cm): exit-code subset, availability fallback modes, hit-content coercion | MISSING | none published — internal consumption only; internal analogs: T40 learn from-session, T19 exit 4 empty machine-mode (see C80 cluster), T38 cache read (main.rs:1257-1264, 2955-2964) | P2 |
| C89 | data location overrides: global --db, per-command --data-dir, CASS_DATA_DIR, CASS_DB_PATH | PARTIAL | CLAUDE_SESSIONS_DIR only (documented in skill); fixed cache read <cache_dir>/terraphim-agent/sessions.json (main.rs:1257-1264, 2955-2964); dirs:: home/data/data_local resolution | P1 |
| C90 | harness exclusion config: sources.toml disabled_agents, manual-edit fallback | MISSING | none — connector set fixed at compile time (T05 feature-gated registration) | P2 |
| C91 | per-harness discovery root envs: CODEX_HOME, GEMINI_HOME, OPENCODE_STORAGE_ROOT, PI_CODING_AGENT_DIR, CASS_AIDER_DATA_ROOT | PARTIAL | CLAUDE_SESSIONS_DIR only (documented in skill); no cursor/aider root envs — resolved via dirs:: [DRIFT] | P2 |
| C92 | semantic tuning env: embedder selection, batch size, watchdogs, backfill checkpoint caps | MISSING | none — no semantic tuning surface; enrichment feature flag exists (enrichment = [repl-sessions, terraphim_sessions/enrichment]) [DRIFT] | P2 |
| C93 | indexer responsiveness governor: env knobs, live pipeline in health output | PARTIAL | T41 IndexStatus sessions fields (robot/schema.rs:370, 381-382); T42 server-mode cold auto-import (main.rs:4906-4913); no governor envs, no persistent pipeline | P2 |
| C94 | streaming consumer tuning env vars | MISSING | none — no streaming consumer surface at all | P3 |
| C95 | output format/color env: CASS_OUTPUT_FORMAT, TOON_*, NO_COLOR family | PARTIAL | flag-based only: T02 --robot/--format JSON; TERRAPHIM_VERBOSE for logging; no env-based format or color controls | P2 |
| C96 | global UX flags: --color, --progress, --wrap/--nowrap, -q/-v | MISSING | none; verbosity via TERRAPHIM_VERBOSE env only | P3 |
| C97 | connector support set: 20 connectors (codex…hermes) | PARTIAL | T05 feature-gated registration — 4 compiled in (claude-code-native, claude-code, cursor, aider); codex/cline/opencode parsers in crate, build features OFF; dep floor terraphim_sessions 1.20.2 (AGENT Cargo.toml:91), resolved 1.20.4 [DRIFT] | P1 |
| C98 | concurrency/lock semantics: exit 7 lock/busy + bounded-backoff guidance | N-A | none — in-memory search, no locks to contend; concurrency-adjacent traps: T39 singleton, T43 clone-reset, T42 cold import (P1 regression tests regardless) | P1 |
| C99 | session format coverage and line-number semantics (claude_code/codex/gemini/antigravity; line_number anchoring) | PARTIAL | T05 build features; claude JSONL native ON, aider ON, cursor registered; codex/cline/opencode parsers in crate but OFF in agent build; no line_number field (robot/schema.rs:317-321) [DRIFT] | P0 |
| C100 | workspace matching semantics: repeatable workspace filters, workspace_original vs mapped, --current resolution, clustering | MISSING | none; nearest analog T09 title=project-path | P1 |

## 3.3 Verdict summary

Counts below are **recounted from the rows above** (not copied from chunk summaries), after reviewer normalizations (05c vocabulary normalized; C20/C45/C87 re-triaged MISSING→PARTIAL).

| Group | Rows | FULL | PARTIAL | MISSING | N-A | UNCLEAR |
|---|---|---|---|---|---|---|
| G1 Search core & output (C01–C06) | 6 | 0 | 2 | 4 | 0 | 0 |
| G2 Modes, chaining & freshness (C07–C12) | 6 | 0 | 2 | 4 | 0 | 0 |
| G3 Viewing, listing & timeline (C13–C17) | 5 | 0 | 4 | 1 | 0 | 0 |
| G4 Packs & wildcard (C18–C20) | 3 | 0 | 1 | 2 | 0 | 0 |
| G5 Index lifecycle & import (C21–C29) | 9 | 0 | 0 | 6 | 2 | 1 |
| G6 Health, status & self-description (C30–C45) | 16 | 0 | 7 | 8 | 0 | 1 |
| G7 Ingest, sources & fleet (C46–C56) | 11 | 0 | 3 | 1 | 7 | 0 |
| G8 Semantic models & analytics (C57–C70) | 14 | 0 | 3 | 8 | 3 | 0 |
| G9 Export, publishing & resume (C71–C79) | 9 | 0 | 2 | 5 | 2 | 0 |
| G10 Machine contract, config & connectors (C80–C100) | 21 | 0 | 10 | 7 | 4 | 0 |
| **Total** | **100** | **0** | **34** | **46** | **18** | **2** |

Priorities: P0 ×3 (C01, C62, C99) · P1 ×15 · P2 ×44 · P3 ×38.

**Headline (honest reading).** Terraphim's session search is a solid **in-memory BM25 + KG-enrichment core with per-connector parsers**: the query path works (C01/C07/C62 PARTIAL), formats parse (C99), sources are detected and listed (C47/C48), and export/self-description exist in usable slices (C71/C42/C43). What is largely absent is cass's **operational shell**: persistent index lifecycle (C21–C28), health/doctor/repair (C30–C38), fleet and remote sources (C50–C56), analytics (C64–C70), semantic-model infrastructure (C58/C59/C61/C63), and much of the robot/consumer contract (C80–C88 partial-or-missing slices). That absence is deliberate design (no persistent index, no embeddings, no daemon), not rot — and it is exactly what this test plan formalizes: 46 MISSING rows become GAP-deferred tests or roadmap items, 18 N-A rows become documented justifications (some with terraphim-native regression assertions), and the 34 PARTIAL rows become the actual parity test surface, asserted terraphim-natively rather than cass-shaped.

## 3.4 Gap analysis — top 10 highest-risk coverage gaps

Ranked by test-plan impact (would the plan silently pass a broken terraphim, or fail a correct one?). Dispositions: **GAP test** = deferred test row in Ch5/Ch6 · **N-A** = documented justification, no test · **probe** = runtime probe first.

1. **C99 — session-format coverage split (P0, GAP test).** Claude-family is effectively the only live source: claude JSONL + aider ON, cursor registered, codex/cline/opencode parsers sit dormant behind OFF build features; no `line_number` anchoring counterpart (robot/schema.rs:317-321). Multi-harness users silently get Claude-only coverage. Drives the whole fixture matrix; `[DRIFT]` → pin crate version, canary re-check.
2. **C62 — semantic fallback / degrade chain (P0, GAP test).** The parity-relevant heart of the models block: enrichment-build → hybrid thesaurus, else plain BM25, else substring (T15/T16/T17/T18/T22/T21). The ×10000 KG boost has no RRF normalization and can dominate rankings — assert graceful degradation + result presence, **never** cross-system score/order parity (fusion math differs by design).
3. **C01 — search core surface (P0, GAP test).** Core query works but repeatable `--agent/--workspace` filters, `--source` on search (list-only via T20), `--offset`, and unbounded-limit/RAM-cap semantics are absent; results hard-capped ≤50 (T16). Every downstream test inherits these caps.
4. **Freshness cluster (C12/C24 — true P0-level risk per chunk A, GAP test + probe).** Corpus goes stale after first import: auto-import fires once on empty cache (T06), BM25 rebuilds per call over the cached corpus (T16), the T14 watcher is public-API-only and unexposed. C24 probe decides the branch (§3.4 UNCLEAR below).
5. **C89/C91 — data-location isolation (P1, GAP test + probe).** `CLAUDE_SESSIONS_DIR` is the only location override; agent cache path and cursor/aider roots are fixed. Fixture isolation is limited and parallel runs contend on the real `sessions.json` read path — a test-infrastructure gap before it is a parity gap. Compounded on macOS by dirs-5.0.1 ignoring XDG vars (feasibility Fix 1: HOME-only isolation + mirroring fixture generator).
6. **C88/T38 — stale-fixture trap (P2, GAP test with seeding fixture).** `sessions.json` is read-but-never-written while T40 `learn from-session` consumes it: tests must seed the cache externally or the T40 path is untestable, and unmanaged fixtures risk false passes against stale data. Cross-chunk fixture (coordinate with T38/T40 owners in Ch4).
7. **C80+C87 — machine contract slices (P1/P2, GAP test, merged cluster).** Truncation flags and the RobotError envelope exist (schema.rs:317-323, main.rs:1315-1331), `ResponseMeta.version` exists (schema.rs:56-59) — but request-id echo, literal `_meta` shape, api/contract version triple, and exit-6 incompatibility signaling are absent. Snapshot `schema.rs` as the machine contract; see merge framing in §3.5.
8. **C100 — workspace scoping (P1, GAP test).** No workspace filters, no `--current` resolution, no clustering; only incidental title=project-path token matching (T09). Cross-project result noise is the user-visible symptom; negative-assert the absent filters, probe the noise.
9. **C76 (+C78/C79) — find→resume journey (P1, GAP-deferred / roadmap).** No resume verb exists (verified). Today: negative-assert absence only (T32 show must not imply resumability). C78's subagent trap is latent — harmless until resume exists, at which point role/parentage checks become a build-time requirement (C79 native-command contract rides along). One decision, three rows.
10. **C64–C70 — analytics block (P2/P3, N-A formalization + one fragment test).** Analytics is wholesale absent; T30 stats (totals + per-source, service.rs:329-361) is the single fragment. Formalize as N-A-style justified absence except one T30 test (per-source counts sum to totals; unknown source does not break stats).

**Exit-code collision caveat (normative for all exit-code tests).** Terraphim's CLI exit 4 = *empty search results in machine mode* (T19, main.rs:3076) numerically collides with cass's exit 4 = *network error class* (cass enum 0–15 + 20–24; consumer subset {0,2,3,4,5,9,10}). Terraphim's enum is 0–7. Parity tests must **assert the accompanying payload/behavior, never the bare code** — and C88's consumer-contract row must not equate the two codes (see §3.5 merge framing).

**The two UNCLEAR dispositions (resolution paths):**

- **C24 (watch mode):** T14 native watcher (200 ms debounce + dedup) is public-API-only, not exposed via REPL/CLI. Runtime probe of the registry 1.20.4 binary: watcher present → re-verdict PARTIAL (engine exists, unexposed; test the T14 API boundary only); watcher is 1.21.x-only → re-verdict MISSING. REPL/CLI watch surface is absent either way, so the negative-assert test is stable under both branches. Probe scheduled as pre-flight task before plan finalization.
- **C44 (api-version triple):** no implementation-map counterpart; drift-dependent (registry 1.20.4 vs local 1.21.3). Runtime probe via `--version` + robot output before merge: any crate/api/contract triple → re-verdict PARTIAL; none → MISSING (no test beyond version-string presence).

## 3.5 Disagreement log (reviewer corrections applied on top of chunk verdicts)

Credit: **verdict-fact-checker** (subagent_07b) caught 2 load-bearing errors (C40, C80 would have produced false-failing tests) and re-triaged 3 rows; **coverage-adversary** (subagent_07) caught the vocabulary/table structural breaks. All fixes are already in the on-disk chunk files; the matrix above reflects them.

- **C20 MISSING→PARTIAL** (fact-checker): `wildcard_fallback` exists in robot *document*-search output (`concepts_matched.is_empty()`, main.rs:2231,4383; schema.rs:299) — not in sessions search; assertions re-aimed at flag presence + doc-vs-session difference.
- **C45 MISSING→PARTIAL** (fact-checker): AutoCorrection in ResponseMeta (schema.rs:124-131), unknown-command "Did you mean" (schema.rs:213-218), ForgivingParser aliases q/s/query/find→search (main.rs:1500-1545).
- **C87 MISSING→PARTIAL** (fact-checker): `ResponseMeta.version` = CARGO_PKG_VERSION (schema.rs:56-59) — crate version IS in machine output; api/contract triple + exit 6 still absent.
- **C40 evidence fixed** (fact-checker): stats JSON **has** `total_messages` + user/assistant message splits (service.rs:329-361) — "counts only" reading corrected before it produced a false-negative test.
- **C80 evidence rewritten** (fact-checker): `preview_truncated` (schema.rs:317-323; populated main.rs:2180-2200), TokenBudget.truncated, and the RobotError{code,message,details,suggestion} envelope (main.rs:1315-1331) **exist** — the prior negative-assert guidance would have false-failed; row now positive-asserts existing fields and negative-asserts only request-id echo + literal `_meta` shape.
- **C07 line refs fixed** (fact-checker): enrichment/plain branch range corrected to handler.rs:2014-2115 (2015-2046 / 2048-2115) + commands.rs:1160-1168.
- **Precision fixes** (fact-checker): C04 — do not assert "no pagination metadata" (Pagination struct exists, schema.rs:134-157); C16 — CLI `sessions list` has no `--source` (REPL-only, handler.rs:1959-1968); C97 — dependency floor pinned terraphim_sessions 1.20.2 (AGENT Cargo.toml:91), resolved 1.20.4.
- **Vocabulary/structure normalization** (coverage-adversary, applied on mainline): 05c's `PARTIAL-by-design` (C57/C60/C62) and `PARTIAL (precise)` (C71/C77) → canonical PARTIAL with qualifiers in notes; C71's unescaped `json|markdown|md` pipes escaped (table integrity restored); 05c summary restated under the standard legend.
- **C80/C88 overlapping assertions — merge framing (this chapter, per review hand-off):** the shared "empty machine-mode search exits 4" assertion is owned by **C80** (robot output-conventions contract) and cross-referenced by **C88**; the two rows form **one merged test cluster with two IDs** — C80 = output conventions contract (schema snapshot, truncation flags, error envelope, exit-4-on-empty *with payload*), C88 = consumer-port compatibility (what an upstream consumer may rely on: exit-code subset, availability fallback, hit-content coercion). When writing Ch5/Ch6, generate one test cluster per assertion and tag it with both C-IDs; never equate terraphim exit 4 (empty results) with cass exit 4 (network) inside it (§3.4 caveat).
- **Defensible non-changes noted for the record:** C12 PARTIAL-vs-MISSING judgment call (chunk A chose PARTIAL; per-call rebuild + one-shot auto-import satisfy "freshness exists"); C22/C23 N-A-by-design; C08 MISSING (user-visible search capability, not infrastructure); C82 PARTIAL per brief though a human `--help` is arguably not a partial `--robot-help`; C93 PARTIAL is thin (downgrade to MISSING if IndexStatus fields are static counts); C98 N-A verdict with deliberate P1 priority (verdict/priority split).

## 3.6 Decision R1 restated — registry 1.20.4 in CI + nightly local-crate canary

**Decision (from the architect's open question, adopted in Round 3):** CI runs the **registry build of terraphim_sessions 1.20.4** — matching production — while a **nightly `[patch]`-style canary lane** builds against the **local crate 1.21.3** to catch drift. Stated as an assumption at delivery; pin the resolved terraphim_sessions version in the test-runner environment.

**Effect on which rows are testable where:**

- **CI-primary (registry 1.20.4):** all rows whose evidence is agent-level verified (T02/T05/T41, robot/schema.rs, main.rs line refs) — the bulk of G1–G10, including all P0 GAP tests (C01, C62, C99) and the machine-contract snapshots (C80/C87).
- **CI + nightly canary re-run (`[DRIFT]` rows):** C02, C16, C29 (chunk A drift notes), C48 (ImportOptions CLI exposure unconfirmed), C87, C91, C92, C97, C99 (`[DRIFT]` notes in chunk D). Primary assertions run in CI against 1.20.4; the canary re-runs the same tests against 1.21.3 and reports deltas instead of failing the mainline suite.
- **Probe-gated (before finalization):** C24 and C44 (§3.4) — settled by runtime probes against the 1.20.4 binary, then re-verdicted and moved into the normal lanes.
- **Canary-only value:** rows where 1.21.x may *add* surface (watcher exposure, import options, parser features) — the canary detects newly testable rows; Ch7 tracks re-verdicts as canary findings, not CI failures.


---

# Chapter 4 — Test Harness, Fixtures & Environment

> **Sources & authority.** This chapter expands subagent_06 §1–§2 into the harness/fixture/environment
> plan. The runnability review (**subagent_08**) is **normative**: wherever this text restates one of its
> corrections, the corrected form is binding. Infrastructure baseline comes from subagent_03 §3; the
> regression context for the nightly/CI lanes comes from subagent_03 §5.
>
> **Lane renumbering.** subagent_06 defined four lanes; this chapter splits crate-level testing into two
> (sessions crate vs agent crate) for five lanes total. Cross-reference map: subagent_06 "Lane A" →
> Lanes **A + B** here; "Lane B" (REPL) → **C**; "Lane C" (CLI/robot) → **D**; "Lane D" (cass
> differential) → **E**. TC IDs in the catalog are unaffected.

| Lane | Kind | Exercises | CI exposure |
|---|---|---|---|
| **A** | `cargo test` / `cargo nextest` in `terraphim-ai` | `terraphim_sessions` unit + integration tests, feature matrix | PR (two jobs) + nightly extras |
| **B** | `cargo test` in `terraphim-agents` | agent-crate `tests/` integration + binary e2e via `CARGO_BIN_EXE_terraphim-agent` | PR (new job) |
| **C** | REPL-scripted | the 14 `/sessions` subcommands through the real handler | via Lane B e2e |
| **D** | CLI / robot contract | exit codes 0–7, JSON envelopes, flag-order contract | via Lane B e2e |
| **E** | cass-differential | read-only **cass 0.6.11** spot-checks, semantic calibration | opt-in `RUN_CASS_DIFF=1` only (manual/weekly) |

---

## 4.1 Harness lanes

### Lane A — crate tests for `terraphim_sessions` (feature matrix)

All existing sessions tests are `#[cfg(test)]` modules in `src/` (there is no `tests/` dir); new
unit/integration tests extend those modules, and a sessions-crate `tests/` dir stays optional and is
only for multi-crate flows (none are needed today). Tests are feature-gated exactly as the existing
patterns dictate: enrichment tests under `#[cfg(all(test, feature = "enrichment"))]` (the
`service.rs` `cluster_tests` pattern), each connector under its own feature.

**Real feature names** (verified against `terraphim_sessions/Cargo.toml`): `default = []`;
`terraphim-session-analyzer`, `tsa-full`, `aider-connector`, `cline-connector`,
`opencode-connector` (= `dep:rusqlite`), `codex-connector`, `extra-connectors`, `enrichment`,
`search-index`, and the aggregate `full = [tsa-full, extra-connectors, enrichment, search-index]`.

**Lane A build matrix** (all commands real):

| Job | Command | Covers |
|---|---|---|
| default lane | `cargo test -p terraphim_sessions` | 56/99 tests — model 21, service 16, native 17, connector/mod 2. This is the only lane that compiles the **substring-search fallback** path (T17). |
| all-features lane | `cargo nextest run -p terraphim_sessions --all-features` | all 99: search (BM25), cluster, enrichment, cline, aider, opencode (incl. SQLite), codex, TSA cla/cursor |
| per-connector bisect | `cargo nextest run -p terraphim_sessions --features opencode-connector` (etc.) | one connector at a time — cheap, self-hosted, aids triage |
| nightly ignored | `cargo test -p terraphim_sessions --all-features -- --ignored` | the `#[ignore]`d inotify watcher test (#814/#815; needs Linux inotify) |
| nightly bench | `cargo bench -p terraphim_sessions --features search-index` | `search_nfr` (has `required-features = ["search-index"]`); NFR G1 <100 ms @10k sessions, F4 <10 ms BM25 |

**The default-features CI blind spot and its fix.** terraphim-ai CI
(`.github/workflows/rust-build.yml`, self-hosted linux x64) runs
`cargo nextest run --workspace --exclude terraphim_agent --profile ci` with default features — so the
43 feature-gated tests (search 10, cluster 7, enrichment 5, cline 5, aider 2, opencode 5, codex 7,
cla 2) **never compile in CI today** (~43/99 invisible; subagent_03 §3). The fix is the
`--all-features` job above, added alongside — not instead of — the default lane, because the
no-features substring fallback is itself a contract (TC-SR-10). nextest is already installed in that
workflow (lines 201–206), so this is a one-job addition.

**Registry caveat.** These tests validate the *local* 1.21.3 sources; the agent binary links registry
`terraphim_sessions` 1.20.4. Lane A results are labeled "local-crate lane" and never transferred to
binary behavior claims — see §4.5.

### Lane B — agent-crate integration tests (`tests/`)

`terraphim-agents/crates/terraphim_agent/tests/` holds ~40 integration files
(`phase1_robot_mode_tests.rs`, `cross_mode_consistency_test.rs`, insta snapshots, …), but agents CI
runs **`cargo test --workspace --lib --no-fail-fast`** — lib tests only. The entire `tests/` tree is
**not run by CI** (subagent_03 §3). Lane B is the new job that runs it
(`cargo nextest run --workspace --no-fail-fast`, or minimally
`cargo test --workspace --no-fail-fast`), plus all new CLI/REPL e2e tests (Lanes C/D) authored here.

E2E tests spawn the real binary via `env!("CARGO_BIN_EXE_terraphim-agent")` — this works because the
`[[bin]]` target lives in the same package and `repl-sessions` is a default feature, so the binary
always carries sessions.

**Feature-unification trap (risk D-13).** The dev-dependencies contain a self-referencing
`terraphim_agent = { path = ".", features = ["repl-full"] }`. Under `cargo test`, the bin compiled for
`CARGO_BIN_EXE_terraphim-agent` gets the **unified** feature set (incl. `repl-full`: server,
repl-chat, repl-mcp, repl-web…), whereas `cargo build -p terraphim-agent` yields the true default
binary. Consequences and mitigation in §4.3 (membership asserts; optional standalone-binary build
before the e2e lane). None of `repl-full`'s features add sessions connectors, so absence probes
(TC-SO-01/SO-08) remain valid under either build.

### Lane C — REPL-scripted tests (piped stdin)

**Mechanism (verified).** The explicit `terraphim-agent repl` subcommand routes to
`run_repl_offline_mode()` (main.rs:1753–1770). The `is_terminal()` checks at main.rs:1723/1728 belong
to the `Interactive`/TUI arm — **there is no TTY gate on the `repl` arm**; the rustyline loop
(handler.rs:135–190) reads piped stdin and `ReadlineError::Eof → break`, so **EOF terminates the
loop**. `/quit` also exists (commands.rs:1122 area); TC-RB-00 pins it, but the harness default is
EOF. REPL history load/save goes through `dirs::home_dir()` (handler.rs:139–143,190) and therefore
stays inside the temp HOME.

**Canonical invocation** (note: **no XDG vars** — see §4.2.3):

```bash
printf '/sessions sources\n/sessions search rust\n' \
  | "$TIMEOUT_BIN" 30 env HOME="$TMP/home" TZ=UTC \
      ./target/debug/terraphim-agent repl
```

**Script conventions.**
- One process per test. Multi-command **stateful flows run inside ONE piped session**, because the
  service is a process-global `OnceLock<Arc<Mutex<SessionService>>>` (handler.rs:1907–1915): state
  (imported sessions, cache, the once-only auto-import attempt) lives and dies with the process
  (e.g. TC-IX-01b — two commands in one session share the single auto-import attempt).
- Every invocation is wrapped in `$TIMEOUT_BIN 30`; exit 124 = hang = test failure.
- `TZ=UTC`; fixtures carry fixed 2026-XX-XX timestamps. The REPL process's own exit status is **not**
  a contract — pass/fail is by output matching.

**Output parsing caveats.** REPL output is comfy-table rendering, which is layout-volatile: assert on
**substrings/regex of content** (session IDs, counts, status words), never on exact table layout.
Anything that needs machine-parseable structure belongs in Lane D (`--robot`/`--format json`), not in
scraping pretty tables. Pin-points that live only in REPL strings (e.g. the exact
"The 'import' command has been removed…" text at commands.rs:1122–1123) are asserted as exact
substring matches.

### Lane D — CLI / robot contract tests

**Surface.** Offline sessions commands: `terraphim-agent sessions {sources,list,search,stats}` with
`--limit` (list default 20, search default 10); robot: `terraphim-agent robot
{capabilities,schemas,examples}`. JSON envelope shapes come from `mod session_output`
(main.rs:590–651) and are asserted key-for-key: `sources → {count, sources:[{id,name,available}]}`;
`list → {total, shown, sessions:[{id,title,message_count,source}]}`;
`search → {query, total, shown, sessions:[{id,title,message_count,preview}]}`;
`stats → totals + by_source`.

**Exit codes 0–7** (`robot/exit_codes.rs`, Success=0 … ErrorTimeout=7). Contract pins:
- **empty search in machine mode → exit 4** (ERROR_NOT_FOUND; main.rs:3076–3077 — unconditional in
  machine mode; human mode prints "No sessions matching…" and exits 0);
- bad args → 2 (clap); the full command × scenario → code table is probed once (TC-RB-01).

**⚠️ Flag order: `--robot`/`--format` MUST precede the subcommand.** They are plain top-level `Cli`
fields with **no `global = true`** (main.rs `struct Cli`), and `apply_forgiving_parsing` only
alias-expands the command token — it does not reorder args. A trailing flag is rejected by clap
("unexpected argument", exit 2). Corrected command templates (subagent_08 §2, normative):

```bash
**Correct — flags BEFORE the subcommand:**
terraphim-agent --robot --format json sessions search "query" --limit 10
terraphim-agent --robot sessions sources
terraphim-agent --format json-compact sessions list

**Wrong — rejected with exit 2 (this rejection is itself pinned as a TC):**
terraphim-agent sessions search "query" --robot        # ✗ clap "unexpected argument" → exit 2
```

**Flag-surface scope.** Only `--robot`, `--format human|json|json-compact`, and `--limit` are pinned.
`--verbose/-v` **does not exist** (0 hits for `verbose` in main.rs) and is excluded from the
flag-surface test (correction #4); the `TERRAPHIM_VERBOSE`-has-no-effect negative pin (TC-SO-09)
remains valid and unaffected. stdout must stay pure JSON (parses cleanly) with diagnostics on stderr
(TC-RB-05). The server-mode variant (`run_server_command` path, skips the disk cache) is probe-only,
excluded from fast CI (subagent_06 open decision D-4).

### Lane E — cass-differential spot-checks (guarded, opt-in)

**Purpose:** semantic calibration against **cass 0.6.11** (version pinned), not equality testing.
Where cass docs (E-IDs) define a behavior terraphim also claims (ranking sanity, empty-query safety,
unicode handling, JSON envelopes), the same fixture-shaped corpus is run through both tools and
divergences are recorded into the parity matrix. Outputs are divergence reports, never pass/fail
gates.

**Read-only allowlist (hard):** only `search | status | health | capabilities | introspect | stats |
sessions | view`, each with `--json`. Everything else — `index`, `doctor --fix`, `models install`,
`sources sync/add`, `pages`, `import`, and any non-allowlisted verb — is forbidden (§4.4).

**Corrected sandbox invocation** (subagent_08 §2, normative — bare `env -i` drops `PATH`, leaving
execvp's default `/bin:/usr/bin`, so a cargo/homebrew-installed `cass` is unresolvable):

```bash
"$TIMEOUT_BIN" 60 env -i HOME="$TMP/cass-home" CASS_DATA_DIR="$TMP/cass-data" PATH="$PATH" \
  cass search "query" --json
```

(Alternative: the absolute binary path instead of the `PATH="$PATH"` re-export.)

**Guard behavior.** Opt-in via `RUN_CASS_DIFF=1`; never runs in PR CI (manual/weekly). The preflight
verifies `$TMP/cass-home` / `$TMP/cass-data` are fresh temp dirs (marker + file-size sanity) and
aborts the lane if a pre-existing real DB is detected; it also probes `command -v cass` and pins
`cass --version` = 0.6.11. **Never touch the real `~/.cass` or `~/.claude`**: fixtures are generated,
never copied or symlinked from real user dirs. Every cass call is capped (`$TIMEOUT_BIN 60`) because
cass can hang under lock contention (>25 s observed). **macOS timeout caveat:** stock macOS has no
GNU `timeout`; the harness preflight resolves the wrapper once —
`TIMEOUT_BIN="$(command -v timeout || command -v gtimeout)"` with a documented perl-alarm fallback —
and every documented `timeout 30/60` in this chapter means `$TIMEOUT_BIN 30/60` (correction #6/#15).
CI (Linux) is unaffected.

---

## 4.2 Fixture strategy

### 4.2.1 Location, generation, and the dirs-mirroring rule

Canonical checked-in tree: `terraphim-agents/test-fixtures/sessions/` (today **all** session fixtures
are inline JSON strings — subagent_03 §3; checked-in files + a generator make the corpus reviewable
and deterministic). A generator (`test-fixtures/sessions/generate.sh` or a Rust helper
`tests/common/fixtures.rs`) materializes the tree into a **per-run temp dir**; checked-in copies are
golden references, runtime always uses the temp copy (no accidental writes into the repo).
Parse-level unit tests in the sessions crate may keep inline strings (existing pattern); corpus-level
tests (ranking, `import_all`, e2e) consume the generated tree.

**The generator MUST mirror `dirs`' platform resolution** — computing destination paths via the same
`dirs` calls the connectors use (tiny Rust helper linking `dirs`, or a per-OS path table). This is
what guarantees fixtures land where connectors actually look on each platform; a fixed POSIX-y tree
silently false-passes on macOS (§4.2.3).

### 4.2.2 Per-connector corpora

| Connector | Fixture contract exercised | Feature gate |
|---|---|---|
| claude-code-native | camelCase LogEntry; content as string OR block array; `tool_use`/`tool_result`; depth-3 walk over `~/.claude/projects/<escaped-cwd>/*.jsonl` (`/`→`-` escaping); id `claude-code-native:{sid}`; title = project cwd | always |
| codex | `session_meta` (id/timestamp/cwd) + `response_item` lines; depth-4 `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`; no-meta → None; meta-only → None | always via agent dep; `codex-connector` in local crate |
| aider | `.aider.chat.history.md` with header block (`Model:`/`Git repo:`); fenced code preserved; **CWD-rooted BFS** discovery incl. nested depth | `aider-connector` |
| cline | `state/taskHistory.json` (shape pinned below) + `tasks/<id>/api_conversation_history.json` + `tasks/<id>/ui_messages.json` | `cline-connector` (absent from binary-under-test → local-crate lane; binary probe asserts absence) |
| opencode | legacy JSONL `prompt-history.jsonl` **and** the SQLite path (pinned DDL below) — the SQLite import path has zero tests today | `opencode-connector` |
| cursor / TSA | no local source (D-3): black-box probe only (TC-SO-07); fixture = a standard `.claude` tree; assert membership of `claude-code`/`cursor` source ids appearing or erroring; record as UNCLEAR | `tsa-full` |

**Native JSONL specifics.** Happy-path file: user(text) / assistant(text+tool_use) / user(tool_result)
lines. Malformed variant: good line, garbage line, JSON with an unknown block kind (hard deserialize
error path, model.rs:147–151 — per-connector skip pinned), empty lines. Empty file (0 bytes) → parse
None. Timestamp spellings: `"…Z"` and millis `"…999Z"`. Subagent file (E11 shape: line 1 metadata,
line 2 prompt) — indexed as an ordinary session (divergence note). **#815 growing-file scenario:**
the fixture is a JSONL file the harness appends to between watch ticks; the watcher must dedup on the
(path, msg_count) key and never re-import unchanged content (#814 error propagation kept green) —
nightly lane only (inotify).

**cline `taskHistory.json` — exact pinned shape** (`Vec<HistoryItem>`, field names from
cline.rs:22–36; generator must emit exactly these):

```json
[{"id": "...", "ulid": "...", "ts": 1750000000000, "task": "...", "tokensIn": 0,
  "tokensOut": 0, "cacheWrites": 0, "cacheReads": 0, "totalCost": 0.0}]
```

**opencode SQLite fixture — pinned DDL and query semantics** (from opencode.rs:89–200; the db is
opened READ_ONLY via URI; rusqlite `bundled` ships JSON1, so `json_extract` is guaranteed):

```sql
CREATE TABLE session (
  id            TEXT PRIMARY KEY,
  title         TEXT,
  directory     TEXT,
  time_created  INTEGER,   -- MILLISECONDS (decoded via from_second(ms/1000))
  model         TEXT,
  cost          REAL,
  tokens_input  INTEGER,
  tokens_output INTEGER
);
CREATE TABLE message (
  id           TEXT PRIMARY KEY,
  session_id   TEXT NOT NULL,
  time_created INTEGER,   -- milliseconds
  data         TEXT       -- JSON with "$.role" ("user" | "assistant")
);
CREATE TABLE part (
  id           TEXT PRIMARY KEY,
  message_id   TEXT NOT NULL,
  time_created INTEGER,   -- milliseconds
  data         TEXT       -- JSON with "$.type" = "text" and "$.text"
);
```

The connector runs one query with **inner joins** and a type filter:
`FROM session s JOIN message m ON m.session_id = s.id JOIN part p ON p.message_id = m.id
WHERE json_extract(p.data,'$.type')='text' ORDER BY s.id, m.time_created, p.id`. Consequences the
generator must honor: (a) sessions without a text part yield **nothing** — plant a deliberate
"textless" session to pin that; (b) every imported session needs ≥1 message with ≥1 `type:"text"`
part; (c) `time_created` is **milliseconds**; (d) there is **no path override for the SQLite lane**
(`options.path` forces JSONL only), so the db must sit at the exact dirs-resolved location — the
dirs-mirroring generator (§4.2.1) is what makes that possible.

### 4.2.3 Isolation: HOME-override ONLY (blocking false-pass fix)

**`dirs` 5.0.1 on macOS reads NO XDG env vars** (vendored `dirs-5.0.1/src/mac.rs`:
`cache_dir()` = `$HOME/Library/Caches`; `data_dir()`/`data_local_dir()` =
`$HOME/Library/Application Support`; only `lin.rs` reads `XDG_*`). Setting
`XDG_CACHE_HOME`/`XDG_DATA_HOME` on macOS is a **silent no-op**. Taken verbatim, subagent_06 §2.5's
"dirs honors XDG on macOS/Linux" premise produced a blocking false-pass risk: the opencode SQLite
lane would silently fall back to legacy JSONL on macOS (TC-IX-19 tests nothing and looks green), the
planted-cache test (TC-IX-06) would look in the wrong directory, and CF-02's XDG assertion would
fail.

**Binding harness env is therefore `HOME=$TMP/home` + `TZ=UTC` only.** `XDG_*` variables are stripped
from every child env; any XDG assertion is a **Linux-only lane** (the one platform where `dirs` reads
them). Because dirs resolution differs per platform, the fixture generator emits the
platform-correct tree:

```
macOS (dirs 5.0.1, no XDG set):
$TMP/home/
  .claude/projects/-data-projects-alpha/<uuid>.jsonl
  .claude/projects/-data-projects-alpha/malformed.jsonl
  .claude/projects/-data-projects-alpha/subagents/agent-01.jsonl
  .codex/sessions/2026/08/30/rollout-abc.jsonl
  .cline/state/taskHistory.json
  .cline/tasks/<id>/api_conversation_history.json
  .cline/tasks/<id>/ui_messages.json
  Library/Caches/terraphim-agent/sessions.json        # CLI cache (dirs::cache_dir)
  Library/Application Support/opencode/opencode.db    # opencode SQLite (data_local_dir)
  .local/state/opencode/prompt-history.jsonl          # opencode legacy (home_dir())
$TMP/cwd-aider/.aider.chat.history.md
$TMP/cwd-aider/nested/deep/.aider.chat.history.md

Linux (no XDG set):
$TMP/home/
  .claude/projects/-data-projects-alpha/<uuid>.jsonl
  .claude/projects/-data-projects-alpha/malformed.jsonl
  .claude/projects/-data-projects-alpha/subagents/agent-01.jsonl
  .codex/sessions/2026/08/30/rollout-abc.jsonl
  .cline/state/taskHistory.json
  .cline/tasks/<id>/api_conversation_history.json
  .cline/tasks/<id>/ui_messages.json
  .local/share/opencode/opencode.db                   # opencode SQLite (data_local_dir)
  .local/state/opencode/prompt-history.jsonl          # opencode legacy (home_dir())
  .cache/terraphim-agent/sessions.json                # CLI cache (dirs::cache_dir)
$TMP/cwd-aider/.aider.chat.history.md
$TMP/cwd-aider/nested/deep/.aider.chat.history.md
```

Notes on the trees:
- **cline** is pinned to the HOME-fallback root `$TMP/home/.cline/…` (resolution order cline.rs:230–250).
  The subagent_06 tree's nesting of `Code/User/globalStorage/…` **under** `.cline` was wrong — the
  connector expects `state/taskHistory.json` directly under the resolved root, so that layout yields
  NotFound and a false-pass. The generator must also ensure **no** `data_dir()` cline candidates exist
  under the temp home (`Library/Application Support/…saoudrizwan…` on macOS), so the fallback fires
  deterministically on both platforms.
- **opencode dispatch** prefers SQLite when `data_local_dir()/opencode/opencode.db` exists, else
  legacy JSONL; legacy-only tests simply don't create the db.
- **CLI cache** (planted fixture for TC-IX-06): macOS
  `$TMP/home/Library/Caches/terraphim-agent/sessions.json`; Linux
  `$TMP/home/.cache/terraphim-agent/sessions.json` (T38 read path).

**Hermetic guarantees.** No test may read real user session data; everything is tempfile-based; the
harness preflight fails fast if `HOME` is not under the test temp root, and the post-run zero-write
assert (TC-CF-06) proves nothing outside the temp roots changed. Aider tests additionally set
`cwd=$TMP/cwd-aider` (CWD-rooted BFS); all other tests use a neutral cwd. User env that could leak
(`CLAUDE_*`, `CASS_*`) is not implemented in terraphim — negative tests pin their non-effect
(TC-SO-09 / TC-CF-03).

**Single-process state model (documented for all lanes).** The REPL/CLI service is a process-global
`OnceLock<Arc<Mutex<SessionService>>>` (handler.rs:1907–1915): imported sessions, cache, and the
once-only auto-import attempt live and die with the process. Each test spawns a fresh process; tests
must never assume cross-process state, and stateful flows must stay inside one piped REPL session
(Lane C) or one CLI invocation.

### 4.2.4 Edge fixtures (checklist)

- **Empty file** (0 bytes → parse None), **empty corpus**, **empty query** — no panic (E03 analog).
- **>50 000-char message** — `MAX_BODY_LENGTH = 50_000` truncation at a char boundary; the prefix
  content stays findable (TC-SR-04).
- **Multibyte UTF-8 spanning the 50k boundary** — CJK + emoji + combining marks; corpus-level twin of
  the existing unit test.
- **Duplicate IDs** — the same session id in two files / across connectors; dedup/collision behavior
  pinned (supports the membership-assert policy of §4.3).
- **Tool-result-only unique token** — terraphim indexes `tool_result` content where cass deliberately
  doesn't (E12 divergence; TC-SR-17).
- **Timestamp forms** — `"…Z"` and millis `"…999Z"` spellings; fixed 2026 timestamps across day
  boundaries for timeline grouping (day/month/"Week of" labels).
- **Planted `sessions.json`** — valid array of `Session` (read path, "Loaded sessions from cache.",
  main.rs:2963) plus a corrupt variant (load-failure degradation pinned).
- **Huge corpus for bench** — 10 000 deterministic synthetic sessions (no RNG), mirroring
  `search_nfr` seeding, for local perf sanity and Lane E parity spot-checks.

---

## 4.3 Environment & CI integration

**terraphim-ai** (`.github/workflows/rust-build.yml`; self-hosted linux x64; nextest pre-installed):
add a **`sessions-all-features` job** running `cargo nextest run -p terraphim_sessions --all-features
--profile ci` next to the existing default-features run — this closes the ~43/99 blind spot while
keeping the default lane for the substring-fallback contract (T17). Optional per-connector matrix
jobs (`--features opencode-connector`, …) for bisectability. **Nightly:** `cargo test
-p terraphim_sessions --all-features -- --ignored` (the `#[ignore]`d inotify watcher test; Linux
required) and `cargo bench -p terraphim_sessions --features search-index` (`search_nfr`, encoding NFR
G1/F4 from #3014; `performance-benchmarking.yml` is a generic stub today — wire the bench there).
Note: **CI has no macOS runner**, so macOS-only path bugs will never surface in CI — the
dirs-mirroring generator (§4.2.1–4.2.3) plus a documented local macOS preflight target are the actual
defense; do not leave platform correctness to CI.

**terraphim-agents** (`.github/workflows/ci.yml`; fmt + `clippy --workspace --all-targets -D
warnings` + build + `cargo test --workspace --lib --no-fail-fast`): add an **integration-tests job
beyond `--lib`** that runs the `tests/` tree (`cargo nextest run --workspace --no-fail-fast`), which
brings Lanes B/C/D e2e into PR CI.

**Lint-clean harness code.** Agents CI clippy runs with `-D warnings` over **all targets**, so every
harness, fixture generator, and test helper must be clippy-clean — no leftover debug `println!`s
(subagent_03 §1.11 found some in enricher tests; inherit-and-fix). This applies to Lane B–D harness
code and to the Rust fixture helper in `tests/common/fixtures.rs`.

**Membership asserts, not exact-set asserts (risk D-13).** Because of the dev-dep feature unification
(§4.1 Lane B), `robot capabilities` feature lists and any "default-build purity" assertion differ
between the cargo-test-spawned binary and a standalone `cargo build` binary. All
capability/connector assertions are therefore written as **membership** assertions
(`contains("claude-code-native")`, registered set ⊇ expected subset, TSA id-prefix membership in
TC-IX-10), never exact-set equality. Where a test genuinely needs the default binary, Lane B builds
it first (`cargo build -p terraphim-agent`) and points the e2e at that artifact.

**Flake budget.** Fixed 2026 timestamps, `TZ=UTC`, `$TIMEOUT_BIN` wrappers everywhere (exit 124 =
hang = failure), no wall-clock asserts outside the bench lane; the fast suite targets ≤10 min in CI
excluding nightly.

---

## 4.4 Guardrails & destructive-op policy

**Forbidden commands (suite-enforced, all lanes).**
- Lane E: any cass verb outside the read-only allowlist `search | status | health | capabilities |
  introspect | stats | sessions | view`. Explicitly forbidden: `index`, `doctor` (esp. `--fix`),
  `models install`, `sources sync/add`, `pages`, `import`, and any non-allowlisted verb.
- All lanes: any invocation that would write outside the run's temp roots; any command run with the
  real `$HOME` in its environment.

**Forbidden paths.** The real `~/.claude`, `~/.cass`, `~/.codex`, `~/.cline`, the real cass store
(`~/Library/Application Support/com.coding-agent-search…`), and generally any path outside `$TMP`.
No fixture symlink may point at real user dirs; fixtures are generated, never copied from `~/.claude`
or real cass stores; no test reads real user session data.

**Sandbox-verification preflight (harness step 0, all lanes).**
1. Compute `$TMP` (tempfile); export `HOME=$TMP/home`; **abort unless `$HOME` is under `$TMP`**.
2. Strip `XDG_*` from the child env (§4.2.3).
3. Resolve `TIMEOUT_BIN` once: `command -v timeout || command -v gtimeout` (perl-alarm fallback);
   abort the lane if none resolves.
4. Lane E only: verify `$TMP/cass-home` / `$TMP/cass-data` are fresh empties (marker + size sanity;
   abort on a pre-existing real DB); verify `command -v cass` resolves and reports 0.6.11.
5. Post-run: zero-write verification across the temp-tree boundary (TC-CF-06), plus the
   no-lock/quarantine-artifact assert over the whole temp tree (TC-HD-08 — structurally proves
   terraphim's no-destructive-ops claim, E46).

Every spawned process (REPL, CLI, cass) is wrapped in `$TIMEOUT_BIN` (30 s for terraphim, 60 s for
cass); exit 124 = hang = test failure.

---

## 4.5 Decision R1 (closed): registry in CI, nightly local-canary

R1 asked whether the agent crate should keep its registry dependency (`terraphim_sessions` 1.20.4 via
Cargo.lock) or `[patch]` it to the local 1.21.3 checkout. **Decision: registry in CI, nightly
canary.** PR CI keeps the registry dependency unchanged — the binary under test links what actually
ships, so Lane B–D golden tests (exit codes, JSON envelopes, flag-order rejections) pin shipped
behavior, while Lane A crate tests are explicitly labeled the "local-crate lane" for the 1.21.3
sources. Drift between 1.20.x and 1.21.x is caught by a scheduled nightly job that applies the patch
transiently — `cargo --config 'patch.terraphim.terraphim_sessions.path="…"'` (no committed edit; the
`[patch.terraphim]` block pattern is already proven in `terraphim-agents/Cargo.toml`) — and re-runs
the golden contract set (TC-RG-05). Binary-lane tests never assert file:line-exact behavior, so
version skew degrades gracefully.

**What breaks if reversed** (i.e. `[patch]` applied in PR CI): the binary under test would link
1.21.3 local sources, so golden tests would silently start pinning **unreleased** behavior — green
tests against a binary no user can install, and the first registry bump after the patch is dropped
would flip them red, destroying the suite's "test what ships" property exactly where it matters (the
contract surface). It would also couple the two repos' HEADs: a terraphim-ai regression would break
terraphim-agents CI with no agents-side change. And since `terraphim-session-analyzer` (1.20.3) has
**no local checkout at all**, `tsa-full` lanes would exercise a hybrid registry-TSA × local-sessions
graph that exists in no shipped configuration. Finally, the drift canary would disappear — with the
patch always on, there is no 1.20.4 baseline left to diff against.


---

# Chapter 5 — Test Cases: Search & Retrieval Core (TC-SEARCH · TC-IMPORT · TC-ENRICH)

- **Status:** Draft for assembly (Round 4 writer `ch5-cases-core-writer`)
- **Inputs consumed:** subagent_05a (C01–C29), subagent_05c (C57–C63), subagent_06 §1/§2/§4 + REVIEW CORRECTIONS (normative), subagent_03 §1.1/§1.3/§4 (extend-don't-duplicate audit), T-IDs from subagent_02
- **Rule:** one line per TC. Types: `unit` | `unit-f` (feature-gated) | `integration` (= skeleton `int`) | `repl` | `cli` | `semantic-parity` (= skeleton `parity`) | `gap-deferred` | `xref` (reference-only to an existing test). Priorities inherit the C-row unless raised here. Where behavior is not certain from the audit, the row says **probe first** and gives the probe command — no invented behavior anywhere in this chapter.

---

## 5.1 Scope and row ownership

This chapter owns the **search & retrieval core**: parity rows **C01–C29** (search + import) **minus the index-lifecycle rows**, plus the **semantic/enrichment rows C57–C63**.

**Routed OUT to Chapter 6 (WATCH-INDEX / ops chapters):**

| Rows | Destination | Reason |
|---|---|---|
| C21, C22, C23, C26, C27, C28 | Ch6 TC-IX (index lifecycle) | N-A-by-design / index telemetry: no persistent index exists; per-call BM25 rebuild (T16) makes "full rebuild" the only mode. C24 (watcher, UNCLEAR) → Ch6 runtime probe: registry 1.20.4 build may lack it (local crate 1.21.3 has it). |
| C17 (timeline) | Ch6 TC-AS | Grouping/labels are analytics surface (AS-03/04). |
| C05 (aggregation analog) | Ch6 TC-AS | Corpus-level `stats` (T30) partial-analog note; query-time aggregation is GAP-spec'd here (TC-SEARCH-23). |
| C13 (show drill-down half) | Ch6 TC-EX | `/sessions show` fixed-window preview = EX-01; the **CLI search-preview half stays here** (TC-SEARCH-11). |
| C29 (chatgpt importer absence) | Ch6 TC-SO probe | Absence probe in sources chapter; the **T05-vs-T04 registry drift it flags is asserted HERE** (TC-IMPORT-05). |
| C03 remainder (robot schema, `--format` variants) | Ch6 TC-RB | This chapter keeps only the search-envelope and preview facets (TC-SEARCH-10/11). |

**GAP-tagged TCs** (type `gap-deferred`): implementation-ready specs for features terraphim lacks — **cursor pagination (C04), aggregations (C05), `--explain` (C06), ANN (C08), pack (C18+ C19)** — written so they can be built later without re-deriving requirements, explicitly deferred with reasons (Ch1 §3-a: a deferral is a decision, not an omission).

**Extend, don't duplicate (subagent_03 audit).** Existing tests already cover BM25 doc building, ranking order, 50k truncation, UTF-8 boundary, empty query/corpus, case-insensitivity, and the 7-test cluster suite. Those are `xref` rows below. Coverage this chapter ADDS (zero tests today): `search_sessions_hybrid` / `search_with_thesaurus` KG-boost ordering (the spec's headline criterion), `MAX_SEARCH_RESULTS=50`, `MIN_SCORE_FRACTION=0.1`, scorer-error fallback, `import_all` fan-out, auto-import, `related`, `by-concept`.

**Normative conventions (subagent_06 §1 + REVIEW CORRECTIONS):**
1. **Flag order:** `--robot`/`--format` PRECEDE the subcommand — `terraphim-agent --robot sessions search "q"`. Never post-subcommand.
2. **Isolation is HOME-only:** dirs-5.0.1 on macOS reads NO XDG env vars. `HOME=$TMP/home` everywhere; XDG lanes are Linux-only extras. Fixture trees MIRROR dirs' platform paths (e.g. cline under `$HOME/Library/Application Support/Code/User/globalStorage/...` on macOS) — the Ch4 dirs-mirroring fixture generator is a **hard dependency of every TC-IMPORT row**; a non-mirroring tree false-passes (correction #1).
3. **Timeout shim:** `TIMEOUT_BIN=$(command -v timeout || command -v gtimeout)` (stock macOS has no GNU `timeout`); exit 124 = hang-fail.
4. **Lane B template:** `printf '/sessions search q\n' | "$TIMEOUT_BIN" 30 env HOME="$TMP/home" ./target/debug/terraphim-agent repl`; assert substrings/regex of content, never comfy-table layout; stateful flows run in ONE piped session (process-global `OnceLock` service, handler.rs:1907-1915).
5. **Exit codes never asserted alone** (Ch1 §4-ii): exit 4 on machine-mode empty search is always paired with a payload assert.
6. **Membership, not exact-set** for any binary capability assert (correction #7, D-13).

---

## 5.2 TC-SEARCH — search & retrieval behavior

Headline of the whole suite is TC-SEARCH-01: **thesaurus-matching sessions must rank above pure-BM25 hits** (`search_sessions_hybrid`, boost = thesaurus match_count × 10000 — `KG_BOOST_MULTIPLIER`, search.rs:20). It is the spec's acceptance criterion and has **zero tests today** (subagent_03 §1.1, §4).

| TC-ID | Title | Given / When / Then asserts | Maps to (C, T, E) | Type | Lane + fixture | Pri |
|---|---|---|---|---|---|---|
| TC-SEARCH-01 | **Hybrid KG-boost ordering — THE headline** (absorbs TC-SR-12) | Given enrichment build + compiled thesaurus + 2 sessions where S2 outscores S1 on raw BM25 for query q, but S1's enrichment concepts contain a thesaurus term matching q / When `search_with_thesaurus(q, Some(&thesaurus))` / Then **S1 ranks above S2**; boost direction = count×10000 dominates raw score; assert ORDERING + boost direction, never score equality (fusion math ≠ cass RRF — no normalization, no hash embedder) | C07(p), C62, T18, E17a | unit-f (search-index+enrichment) | Lane A · `thesaurus+enriched` | P0 |
| TC-SEARCH-02 | Boost scales with thesaurus match count | Given session A matching 2 thesaurus terms, session B matching 1, comparable raw scores / When hybrid search / Then A outranks B; boost monotone in match count | C62, T18 | unit-f | Lane A · `thesaurus+enriched` | P1 |
| TC-SEARCH-03 | `search_with_thesaurus(None)` degrades to plain BM25 (absorbs TC-SR-13) | Given enrichment build, thesaurus = None (server-mode analog) / When `search_with_thesaurus(q, None)` / Then result set == plain `search(q)`; no panic | C62, T15, E64a | unit | Lane A · `corpus-3` | P0 |
| TC-SEARCH-04 | Enrichment vs plain build split (REPL handler) | Given default (non-enrichment) binary / When `/sessions search q` via Lane B / Then plain-search branch (handler.rs:2048-2115): top-10 table + total, no boost; enrichment binary routes hybrid branch (handler.rs:2015-2046); the behavior split is pinned per build, per correction "verify in shipped binary, not just local crate" | C07, C62, T15 | repl | Lane B (default bin) · `corpus-3` | P1 |
| TC-SEARCH-05 | `MAX_SEARCH_RESULTS=50` cap | Given 60 sessions all matching q / When search / Then ≤50 results returned; cap surfaced (total > shown observable at service layer) | C01, T16 | unit | Lane A · `corpus-60` | P1 |
| TC-SEARCH-06 | `MIN_SCORE_FRACTION=0.1` cutoff (absorbs TC-SR-14) | Given corpus with a weak match scoring <10% of top score and a borderline match ≥10% / When search / Then weak match excluded, borderline retained | C01, T16 | unit | Lane A · `corpus-3` | P1 |
| TC-SEARCH-07 | BM25 scorer-error fallback | Given a document that faults the Okapi scorer (fault-injected doc in the index build) / When search / Then warn + empty result, **no panic** (fallback path search.rs:95-157; subagent_03 §1.1 names it uncovered) | C01, T16 | unit | Lane A · `corpus-1` | P2 |
| TC-SEARCH-08 | Substring fallback build, search-index off (absorbs TC-SR-10) | Given build with search-index feature OFF / When service search / Then case-insensitive contains-match over title / project_path / message content; no BM25 | C62, T17, E16a | unit (feature-off lane) | Lane A · `corpus-3` | P1 |
| TC-SEARCH-09 | CLI search default limit 10; total > shown (absorbs TC-SR-08) | Given `corpus-12` all matching / When `terraphim-agent sessions search "q"` / Then 10 rows shown, total=12 surfaced in human output and JSON | C01, T19, E14a | cli | Lane C · `corpus-12` | P1 |
| TC-SEARCH-10 | CLI search JSON envelope exact keys (absorbs TC-SR-06) | When `terraphim-agent --format json sessions search "q"` / Then exact top-level keys `{query, total, shown, sessions:[{id,title,message_count,preview}]}`; no extras | C01, C03(p), T19, E01 | cli | Lane C · `corpus-3` | P1 |
| TC-SEARCH-11 | Preview = first MATCHING message, ≤100 chars (cass user-prompt-at-top analogue) | Given session whose first user message is not the matching one / When CLI search / Then preview shows the **matching** message truncated ≤100 chars — divergence pinned: cass surfaces first user prompt; terraphim surfaces first match | C13(p), C03(p), T19 | semantic-parity | Lane C+D · `corpus-3` | P1 |
| TC-SEARCH-12 | Empty result: machine exit 4 + payload; human exit 0 (absorbs TC-SR-07) | Given corpus with no match / When `terraphim-agent --robot sessions search "zzz"` and human variant / Then machine mode: exit 4 AND error payload shape (RobotError envelope — never bare exit code); human mode: exit 0 + no-results text (pin main.rs:3076/3090-3094) | T19, E22a, C87(p) | cli | Lane C · `empty-corpus` | P1 |
| TC-SEARCH-13 | Empty query, both surfaces (absorbs TC-SR-02) | When REPL `/sessions search` (no arg) and CLI `sessions search ""` / Then empty result, no panic (xref `test_search_sessions_empty_query`); CLI exit + payload pinned | C01, E03a | repl+cli | Lane B/C · `corpus-1` | P1 |
| TC-SEARCH-14 | `--limit 0` defined behavior (absorbs TC-SR-03) | When `terraphim-agent sessions search "q" --limit 0` / Then no crash, 0 hits (or pinned default), defined exit — cass's limit-0 panic (#196 family) must not reproduce here | C01, E03 | cli | Lane C · `corpus-3` | P1 |
| TC-SEARCH-15 | Workspace parity via title = project-path (cass `--workspace` equivalent) | Given native sessions from 2 project dirs / When search on a workspace-name token / Then session found because native connector sets title = project path (T09) and project_path is in the indexed body (xref `test_build_body_includes_metadata`); Then-negative: NO `--workspace` flag on search — workspace filtering is metadata-only (CF-05 divergence) | C01, C100(d), E07a, E08a, T09 | semantic-parity | Lane C · `native-tree` (multi-project) | P1 |
| TC-SEARCH-16 | tool_result-only token IS indexed (cass E12 divergence; absorbs TC-SR-17) | Given token unique to a tool_result block / When search that token / Then session returned (cass deliberately excludes tool output) — divergence pinned | E12, C99 | semantic-parity | Lane C+D · `tool-output-only` | P2 |
| TC-SEARCH-17 | Unicode round-trip on both search paths (absorbs TC-SR-05) | Given CJK + emoji session / When search CJK token via BM25 and via substring fallback / Then found on both; 50k truncation stays char-boundary-safe (xref `test_build_body_truncation_multibyte_utf8`) | T16 | unit | Lane A · `unicode` | P2 |
| TC-SEARCH-18 | Case-insensitivity, both paths | xref `test_search_case_insensitive` (service) + one CLI-level repeat (`"SESSION S1"`-style query) | C01, T17 | xref | Lane A/C · `corpus-3` | P2 |
| TC-SEARCH-19 | Search flag surface pinned: no `--agent`/`--workspace`/`--source`/`--offset`/`--days`/`--since`/`--until` | When `terraphim-agent sessions search "q" --source local` / Then clap rejects unknown flag (exit 2); repeatable agent/workspace filters, source-on-search, and time filters are cass-only (C01/C02 missing facets); `--source` filtering exists on `sessions list` REPL-only (C16) | C01(d), C02(d), C16(p), C95a | cli | Lane C · `corpus-3` | P2 |
| TC-SEARCH-20 | Freshness: first-call visibility, then frozen corpus (C12) | Given fixture session written after service start / When first cache-touching search in a FRESH REPL / Then session findable without restart (auto-import → TC-IMPORT-01); When a second write + another search in the SAME process / Then NOT visible (frozen-corpus pin; watcher exists but unexposed, T14) | C12, T06, T39 | repl | Lane B · `native-tree` | P1 |
| TC-SEARCH-21 | Wildcard `*` pinned (**probe first**) | Probe: `terraphim-agent --robot sessions search "*"` + human variant / Then pin actual behavior (likely literal token, NOT cass universe-scan); sessions search carries **no** `wildcard_fallback` flag — only robot DOCUMENT search sets it (`concepts_matched.is_empty()`, main.rs:2231, schema.rs:299); doc-vs-session difference recorded | C20 | cli+probe | Lane C · `corpus-3` | P2 |
| TC-SEARCH-22 | GAP: cursor pagination | Spec (implementation-ready, DEFERRED): `search --cursor <base64>` pages a frozen result set losslessly; `hits_clamped:true` when cap truncates; robot `Pagination{total,returned,offset,has_more}` (schema.rs:134-157) gains a cursor field / Deferred: ≤50 cap + no persistent result set make a cursor moot; per review correction C04, do NOT assert "no pagination metadata" — the struct exists | C04 | gap-deferred | — | P3 |
| TC-SEARCH-23 | GAP: query-time aggregations | Spec: `--aggregate agent\|workspace\|date` returns ≤10 buckets whose counts equal a manual tally of the query's hits / Deferred: stats is corpus-level only (T30); `match_type` bucket has NO terraphim dimension (no hit-type is exposed) | C05 | gap-deferred | — | P3 |
| TC-SEARCH-24 | GAP: `--explain` / `--dry-run` diagnostics | Spec: explain prints per-hit scorer terms/weights (technically feasible: per-call BM25 rebuild, T16); dry-run plans without executing / Deferred: nothing exposes scorer internals today | C06 | gap-deferred | — | P3 |
| TC-SEARCH-25 | GAP: ANN `--approximate` | Spec: approximate mode returns semantically similar hits with a recall check vs the exact baseline on a 100-session corpus / Deferred BY DESIGN: no embeddings anywhere in terraphim; nearest alternative is enrichment concepts (TC-ENRICH-01) — lexical-thesaurus, not ANN | C08 | gap-deferred | — | P3 |
| TC-SEARCH-26 | GAP: pack answer-pack + intent aliases | Spec: pack honors token/session/evidence budgets, `--require-evidence` fails/retries on missing evidence, schema validates; intent aliases (answer/why/handoff/bundle) route to pack with preset budgets / Deferred: cass-specific deliverable format = new feature; C19 depends on C18 | C18, C19 | gap-deferred | — | P3 |

---

## 5.3 TC-IMPORT — import, connectors, cache

Import is terraphim's only index-build path (no persistent index exists), so auto-import semantics and the cache traps here are correctness-critical, not plumbing. Connector parse contracts already covered by existing tests are `xref` rows; only the audit's named holes (aider discovery/import, cline import, registry fan-out) get new tests.

| TC-ID | Title | Given / When / Then asserts | Maps to (C, T, E) | Type | Lane + fixture | Pri |
|---|---|---|---|---|---|---|
| TC-IMPORT-01 | Auto-import: single attempt per service instance (absorbs TC-IX-01) | Given empty cache + fixture tree / When first cache-touching service call / Then connectors import automatically ONCE; a FAILED attempt is NOT retried in-process (attempted-flag, service.rs:96-101); assert imported counts + flag state | C12, T06, E30a | unit | Lane A · `empty-home` | P0 |
| TC-IMPORT-02 | REPL singleton shares the one attempt (absorbs TC-IX-01b) | Given two commands in ONE piped REPL session (process-global `OnceLock<Arc<Mutex<SessionService>>>`, handler.rs:1907-1915) / When both execute / Then second command does NOT re-trigger import; a NEW process re-attempts (per-instance semantics) | T06, T39 | repl | Lane B · `empty-home` | P0 |
| TC-IMPORT-03 | `import_all` skip-failures fan-out (absorbs TC-IX-02) | Given one broken connector tree + one good tree / When `import_all` / Then good tree imported, failing connector skipped WITHOUT aborting the loop (connector/mod.rs:221-263); per-source outcome observable | T07 | integration | Lane A · `broken+good trees` | P0 |
| TC-IMPORT-04 | Global `ImportOptions.limit` truncates across fan-out (absorbs TC-IX-03) | Given limit=N over a multi-connector corpus exceeding N / When `import_all` / Then total imports ≤N across all sources combined | T07 | unit | Lane A · `native-tree` | P1 |
| TC-IMPORT-05 | Registered-connector membership: binary vs crate drift (absorbs TC-IX-10) | Probe: `terraphim-agent --robot sessions sources` under temp HOME / Then source set is a MEMBERSHIP assert (correction D-13 — never exact-set): binary registers {claude-code-native, claude-code, cursor, aider} (T05); opencode/cline exist in the crate only (T04↔T05 drift flagged here); no chatgpt importer in any build (C29 disposition → Ch6 SO probe); detection must not crash on broken trees | T05, T04, C29 | cli+probe | Lane C · `all-fixtures` | P1 |
| TC-IMPORT-06 | Native JSONL contract: parse xref + timestamp forms (absorbs TC-IX-15) | xref 17 native.rs tests (roles user/assistant/tool_result, tool_use/tool_result blocks, malformed skipped, empty→None, id `claude-code-native:{sid}`, title=project path); NEW: `timestamp-forms.jsonl` ("…Z" and millis "…999Z") both parse; depth-3 walk incl. `subagents/` (E11 shape: indexed as ordinary session) | T09, C99 | xref+integration | Lane A · `native-tree` | P1 |
| TC-IMPORT-07 | Codex parse golden (xref; absorbs TC-IX-16) | xref 7 codex.rs tests: `session_meta`+`response_item`; no-meta→None; meta-only→None; `rollout-*` naming; depth-4 walk | T10, C99 | xref | Lane A · `codex-tree` | P1 |
| TC-IMPORT-08 | Aider discovery + import (NEW — no import test exists today) | Given cwd=`$TMP/cwd-aider` containing root AND `nested/deep/.aider.chat.history.md` / When discovery+import / Then unbounded BFS from CWD finds both, header block (Model:/Git repo:) parsed, fenced code preserved (xref 2 aider parse tests; metadata assertion tightened); aider NEVER reads HOME — cwd is the discovery root, so the lane pins cwd | T11, C99 | integration | Lane A · `aider-tree` (cwd pinned) | P1 |
| TC-IMPORT-09 | Cline import (NEW; local-crate lane) | Given dirs-mirrored `taskHistory.json` + per-task `api_conversation_history.json`/`ui_messages.json` (exact field names pinned in subagent_08, correction #8) / When cline-connector import in the LOCAL-CRATE lane / Then sessions built; the binary-under-test asserts cline ABSENT (membership probe via TC-IMPORT-05) | T12, C99 | unit-f (cline)+probe | Lane A (binary: Lane C) · `cline-tree` | P2 |
| TC-IMPORT-10 | Malformed-line tolerance across connectors (absorbs TC-IX-15 core) | Given good line + garbage line + unknown-ContentBlock line + empty lines / When import / Then good sessions imported; bad lines skipped with per-connector behavior PINNED (native skip vs model.rs:147-151 deserialize-error path) | T09, T37, C99 | integration | Lane A · `malformed.jsonl` | P1 |
| TC-IMPORT-11 | `ImportOptions.since/until` honored (native import) | Given sessions spanning dates / When native import with since/until / Then only in-range sessions imported (native.rs:79-105); note: this is the ONLY time-filter surface in terraphim (search has none — TC-SEARCH-19); confirm in registry 1.20.4 build before asserting (drift-sensitive, 05a) | T13, C02(p) | unit | Lane A · `native-tree` (dated) | P1 |
| TC-IMPORT-12 | `incremental` flag is a dead knob — pinned no-op (absorbs TC-IX-09) | When import with `incremental=true` vs false / Then identical result set (no-op pinned, so a future real implementation flips the test deliberately) | T07q | unit | Lane A · `native-tree` | P2 |
| TC-IMPORT-13 | Cache read-never-written trap (absorbs TC-IX-05/06) | (a) fresh HOME CLI run: cache DIR may be created but NO `sessions.json` is written (no writer exists, main.rs:1257-1264/2955-2964/3605-3614); (b) planted valid cache IS loaded offline — "Loaded sessions from cache." substring; (c) planted corrupt cache: degradation pinned (no panic; observed behavior recorded). All under `HOME=$TMP/home` — a stale/planted fixture influencing results is the false-pass risk this test makes visible | T38, C21(d), C89a | cli | Lane C · `empty-home` + `planted-cache`(+corrupt) | P0 |
| TC-IMPORT-14 | Server-mode skips disk cache (**probe first**; OPEN D-4) | Given planted cache / When server-mode sessions path (Ch4 §D-4 probe recipe — server process, excluded from fast CI) / Then cache ignored, cold auto-import each run (main.rs:4906-4913) | T42 | probe | Lane D · `planted-cache` | P2 |
| TC-IMPORT-15 | `SessionService::clone()` reset trap (test-hygiene requirement) | Given service with imported cache + attempted-flag set / When `clone()` / Then clone has EMPTY cache, fresh registry, reset attempted-flag (service.rs:397-407) — therefore any idempotency-adjacent or stateful test must be per-instance; holding clones silently re-imports | T43 | unit | Lane A · `corpus-1` | P1 |
| TC-IMPORT-16 | E2E: write → auto-import → search finds unique term (absorbs TC-IX-12) | Write fixture file under temp HOME / fresh service (or fresh REPL process) / When search for the fixture-unique token / Then found (cass E30 analog; import IS the index build — no persistent index step exists) | E30, T06, C12 | integration | Lane A/B · `native-tree` | P0 |
| TC-IMPORT-17 | `/sessions import` removed — exact error pin (absorbs TC-IX-14) | When `/sessions import` in REPL / Then exact "has been removed" error (auto-import replaced it); skill still documents the command → DOCS-DRIFT ledger entry (Ch7) | T08, C45a | repl | Lane B · — | P2 |

---

## 5.4 TC-ENRICH — enrichment, concepts, related, cluster

Enrichment is the terraphim-native substitute for cass's entire semantic stack (embedders, ANN, backfill). Parity here means **graceful degradation + result presence**, never score equality (C62 fusion math differs: count×10000 boost, no RRF). All enrichment state lives in the in-memory cache and dies with the REPL process — every test that needs enriched state builds it inside the same piped session.

| TC-ID | Title | Given / When / Then asserts | Maps to (C, T, E) | Type | Lane + fixture | Pri |
|---|---|---|---|---|---|---|
| TC-ENRICH-01 | Enrich computes; persists to IN-MEMORY cache ONLY (absorbs TC-MS-01) | Given enrichment build + thesaurus + imported corpus, ONE piped REPL session / When `/sessions enrich <id>` / Then SessionConcepts computed and written to the in-memory cache via `load_sessions` (handler.rs:2524-2536), counts printed; subsequent cluster/search in the SAME process sees them; a NEW process → state GONE; re-running enrich re-computes from scratch (C60: no checkpoint/tiers/durable persistence — restart pays full cost, C63) | C57, C60, T21 | repl (enrichment binary) | Lane B · `thesaurus+enriched` | P0 |
| TC-ENRICH-02 | Enrich on non-enrichment build (absorbs TC-MS-02) | When `/sessions enrich <id>` on the DEFAULT binary / Then rebuild advice + dry-run counts printed, NO mutation, no panic | C57, T21 | repl | Lane B · `corpus-3` | P1 |
| TC-ENRICH-03 | Dry-run counts stable across repeated runs (C59 surrogate) | When enrich dry-run executed twice / Then identical counts — the only "verify" semantics available: no artifact to checksum, no `--repair` (compiled thesaurus has no corruption surface) | C59, T21 | repl | Lane B · `thesaurus+enriched` | P3 |
| TC-ENRICH-04 | Concepts text-fallback with per-session Matches counts (absorbs TC-MS-03) | When `/sessions concepts <concept>` on BOTH builds / Then per-session substring "Matches" counts rendered EVEN on enrichment builds — fallback always runs (handler.rs:2200, counting 2224-2233); this is the reachable `by-concept` surface | C62, T22 | repl | Lane B (both bins) · `corpus-3` | P1 |
| TC-ENRICH-05 | Related: the ACTUAL contract pinned, gap flagged (absorbs TC-MS-08) | When `/sessions related <id>` / Then relatedness = first 3 tokens of the first user message (handler.rs:2268-2277); self excluded; top 5 fixed (no `--limit` flag); `--min` ACCEPTED BUT IGNORED (`_min`, handler.rs:2261) — pin the silent ignore AND flag the gap (silent flag-acceptance breaks CLI contracts → ch1 §4-iv fix-or-document decision logged); `--min abc` → silent None (coercion xref TC-RB-08) | C15, T26 | repl | Lane B · `corpus-3` | P1 |
| TC-ENRICH-06 | `find_related_sessions` API-level unit (NEW — zero tests today) | Given enriched corpus / When `find_related_sessions(id, …)` / Then first-3-tokens heuristic reproduced at API level; self excluded; ≤5 results ordered | C15, T24 | unit-f (enrichment) | Lane A · `enriched` | P1 |
| TC-ENRICH-07 | `search_by_concept` / `find_related` unreachable from REPL/CLI (absorbs TC-MS-06) | Pin: lib.rs exports (lib.rs:50-54) have NO agent call sites — probe the subcommand list + `terraphim-agent robot capabilities` shows no concept-search surface beyond the concepts fallback; divergence note; API behavior tested in Lane A only (TC-ENRICH-06) | T24, C15(p) | unit-f+probe | Lane A/B · `enriched` | P2 |
| TC-ENRICH-08 | Cluster handler-level suite (extends the 7 existing cluster tests; absorbs TC-MS-10) | xref `cluster_tests` (empty / similar-grouped / k-cap / unenriched-separate / dominant-concepts / min-sessions / sequential-ids); NEW handler-level: `/sessions cluster --format json` → `{cluster_id, session_count, dominant_concepts, sessions[]}`; `--k` merge honored; `--min-sessions` filter; trailing "(no enrichment data)" cluster present when unenriched sessions exist | C60, T27 | repl-f (enrichment) | Lane B (enrichment bin) · `enriched+unenriched` | P1 |
| TC-ENRICH-09 | Jaccard ≥0.1 threshold boundary (NEW handler-level) | Given session pairs at Jaccard just-below / at / above 0.1 (THRESHOLD, service.rs:513) / When cluster / Then only ≥0.1 pairs co-cluster (average-linkage); boundary behavior pinned — the threshold itself is only indirectly exercised by existing tests | T27 | unit-f+repl | Lane A/B · `enriched` | P2 |
| TC-ENRICH-10 | Cluster on non-enrichment build (absorbs TC-MS-11) | When `/sessions cluster` on default binary / Then rebuild hint + available-session count, no clusters | T28, C57 | repl | Lane B · `corpus-3` | P2 |
| TC-ENRICH-11 | `metadata.enrichment` serialization gate (absorbs TC-MS-07) | Given enriched + unenriched sessions / When serialize / Then `enrichment` key present only under the enrichment feature AND skipped when None (model.rs:233-235) | T25, C57 | unit-f | Lane A · `enriched` | P2 |
| TC-ENRICH-12 | SessionEnricher API shape + DOCS-DRIFT (absorbs TC-MS-09) | Real API: `SessionEnricher::new(thesaurus)` + `enrich_session(&session).await` — NOT the skill's documented `SessionEnricher::new(config)?` / `enrich(&session)` shape; the test asserts the REAL shape compiles and runs (and strengthens the conditional `test_dominant_topics` assert); drift is a defect either way: fix docs or implement the documented API (ch1 §4-iv) → Ch7 ledger | T23, DOCS-DRIFT | unit-f (enrichment) | Lane A · `thesaurus` | P2 |
| TC-ENRICH-13 | Composite 3-tier degrade chain (C62 — P0 parity row of this chunk) | Assert ALL: (1) enrichment build → hybrid path boosts enriched sessions without error; (2) thesaurus absent → plain BM25 still returns top-10 table + total; (3) search-index feature off → substring results; (4) `/sessions concepts` returns per-session Matches counts in EVERY build; (5) enrich without thesaurus → rebuild advice + dry-run counts, no crash — graceful degradation + result presence, NEVER score equality | C62, T15, T16, T17, T21, T22 | semantic-parity (composite) | Lane A feature matrix + Lane B (default + enrichment bins) · `thesaurus+enriched`, `corpus-3` | P0 |
| TC-ENRICH-14 | Determinism (absorbs TC-MS-05) | When the same query runs twice / Then identical ranking + scores (BM25 deterministic; cass hash-embedder determinism analog) | C62a, E16a | unit | Lane A · `corpus-3` | P2 |
| TC-ENRICH-15 | `models *` / daemon surfaces ABSENT (absorbs TC-MS-04) | Probe: `terraphim-agent models status` → clap unknown-subcommand error (exit 2); no `--socket`/`--idle-timeout` flags exist anywhere (C63: no warm daemon — absence-only assert); C58/C61: no install/verify/remove/update verbs — N-A, mechanism swap is offline TUI thesaurus compilation | C58, C59, C61, C63 | cli+probe | Lane C · — | P2 |
| TC-ENRICH-16 | Re-enrich after thesaurus change updates concepts (C61 surrogate) | Given enriched session + modified thesaurus / When re-enrich in the same process / Then concepts updated (no revision tracking — staleness handled by advice messaging, pinned) | C61, T21 | repl (enrichment) | Lane B · `thesaurus+enriched` | P3 |

---

## 5.5 Cross-chapter pointers and remaining dispositions

**→ Chapter 6 (routed, not owned here):** C21, C22, C23, C24 (UNCLEAR → runtime probe: does the registry 1.20.4 binary contain the watcher? T14 is local-crate evidence), C26, C27, C28 → Ch6 WATCH-INDEX/TC-IX; C17 → TC-AS; C05 corpus-stats analog → TC-AS; C13 show-drill-down → TC-EX; C29 importer absence → TC-SO; C03 robot-schema remainder, C45 (aliases/typos), C87 (exit enum) → TC-RB.

**Deferral dispositions recorded here (no TC; reason = by-design divergence):** C09 (single fixed Okapi BM25 scorer, no model registry/rerank stage — pin `"Scorer: BM25 (Okapi)"` line via T36 in Ch6), C10 (no daemon; every call rebuilds in memory — asserted negatively by TC-SEARCH-04/TC-IMPORT-14), C11 (no line-oriented session format for chaining; nearest is export, Ch6), C14 (no window/offset controls; fixed 5-message/80-char show preview is Ch6 EX-01), C25 (enrichment concepts are the design alternative — TC-ENRICH-01/13).

**→ Chapter 4 (harness dependencies of every table above):** HOME-only isolation + dirs-mirroring fixture trees (cline path is the hard case), fixture names (`corpus-3/12/60`, `empty-home`, `empty-corpus`, `native-tree`, `codex-tree`, `aider-tree`, `cline-tree`, `broken+good`, `planted-cache`, `thesaurus+enriched`, `enriched`, `unicode`, `big-body`, `tool-output-only`, `multi-source`), gtimeout shim, flag-order rule, D-13 membership asserts, Lane D sandbox with PATH preserved (correction #3), `TZ=UTC` + fixed 2026-XX-XX fixture timestamps.

**→ Chapter 7 (traceability):** every `gap-deferred` row (TC-SEARCH-22…26) and every deferral/DOCS-DRIFT item above lands in the disposition ledger with its reason; TC-ID ↔ C-ID/T-ID/E-ID join via the maps-to column; absorbed skeleton IDs (TC-SR-xx / TC-IX-xx / TC-MS-xx) are noted per-row so the Ch7 matrix can reconcile against subagent_06 §4 without a separate mapping file.


---


# Chapter 6 — Test Cases: Lifecycle & Operations

## 6.1 Scope & routing note

**Parity rows owned here:** C30–C46 (Health/Diagnostics) and C47–C56 (Sources/Fleet) → §6.2; C40, C41, C64, C65, C67, C68, C69, C70 (Analytics fragments) → §6.3; C71–C76, C78, C79 (Export/Share/Resume) → §6.4; C66 → §6.5.

**N-A routing table** (rows with no test constructible; justification refs):

| Row | cass capability | Why N-A in terraphim | Justification ref |
|---|---|---|---|
| C50 | sources discover (SSH) | No remote-source model; connector registry fixed at build | T05 |
| C51 | sources setup wizard | Zero-config auto-detection is the design; nothing to configure | T06 |
| C52 | sources mappings | No mapping surface of any kind | T05 |
| C54 | sources artifact-manifest | No persistent artifact store; only read-only disk cache | T38 |
| C56 | fleet ops patterns | No fleet/remote concept anywhere in CLI or robot schemas | T05 |
| C74 | pages key management | Doc-only in cass too; no test constructible | — |
| C75 | mirror prune | No persisted mirror. Safety remnant (sessions commands never mutate source session stores) folded into TC-SOURCES-06 | T09–T13 |

**GAP-deferred policy:** every MISSING row gets either (a) a real terraphim-native assert — an absence probe, a read-only invariant, or an analog diff — written as a normal TC, or (b) a `gap-deferred` TC line recording the gap for the roadmap chapter. GAP rows routed here: C65/C67/C68/C69/C70 → §6.3; C72/C73/C79 → §6.4. All gap-deferred lines are single TC rows so the assembler can count them mechanically.

**Standing conventions (apply to every TC below):**
- Flag order (NORMATIVE): `--robot`/`--format` PRECEDE the subcommand (e.g. `terraphim-agent --robot sessions sources`); they are not clap-global. Exit codes come from the agent CLI enum 0–7; empty machine-mode search exits 4 (main.rs:3076) — exit 4 is a search result, never a health verdict (C30 note).
- Isolation (NORMATIVE): HOME-override ONLY. dirs-5.0.1 on macOS ignores XDG vars; XDG assertions run in Linux-only lanes. Fixtures mirror dirs' platform paths (dirs-mirroring fixture generator).
- macOS has no GNU `timeout`: use `gtimeout` (coreutils) behind a preflight probe; skip the lane if missing.
- D-13 membership asserts: connector/capability sets are MEMBERSHIP asserts, never exact-set (crate drift changes sets between builds).
- D-5 flakiness rule: FIXED timestamps in fixtures, `TZ=UTC` exported for every run, no wall-clock or sleep-dependent asserts, `gtimeout` on every CLI invocation.
- REPL `/sessions` has 14 subcommands (sources, list, search, stats, show, concepts, related, timeline, export, enrich, cluster, files, by-file, index) and handler.rs has ZERO tests — every REPL TC below is net-new. `/sessions import` was REMOVED (parser returns an explanatory error; auto-import replaced it).
- Extend existing suites (model 21, service 16 + 7 cluster, native 17 incl. #814/#815 with 1 #[ignore]d inotify → nightly lane, codex 7, opencode 5 legacy-JSONL-only [SQLite import_sqlite UNTESTED], cline 5 pure-helpers [no import test], aider 2 parse-only [no discovery/import], connector/mod 2, cla 2, enricher 3, concept 2, search 10 BM25 units); never duplicate them.
- "probe-first" marks behavior that is UNVERIFIED: the TC asserts only what the probe observes and documents the finding. No invented behavior is ever asserted as expected.

## 6.2 TC-SOURCES (rows C30–C56; C40/C41 routed to §6.3)

- TC-SOURCES-01 | health preflight analog (C30) | Given dirs-mirrored HOME with a large aider CWD subtree (T04 large-tree) and TZ=UTC exported | When `gtimeout 10 terraphim-agent --robot sessions sources` | Then exit 0; JSON lists connectors (membership assert) each carrying a status field; detection completes despite large-tree recursion; wall-time recorded in report (no <50ms guarantee asserted; exit 4 is search-empty, never a health code) | C30 | cli | macOS+Linux; fixture T04-large-tree; gtimeout preflight | P2
- TC-SOURCES-02 | index status surface (C31) | Given indexed fixture with known session/message counts | When `terraphim-agent --robot sessions index --verbose` | Then status reports session+message counts and "Scorer: BM25"; negative-assert absence of the ~9 other state families (db/semantic/pending/quarantine/policy/coverage/doctor/recommended_action) | C31 | cli | T36 | P2
- TC-SOURCES-03 | `state` alias probe (C32) | Given stock CLI | When `terraphim-agent --robot sessions state` | Then unknown-command error within exit-code enum 0–7 (probe exact code; a "Did you mean" suggestion may appear per C45 and is recorded, not asserted) | C32 | cli | — | P3
- TC-SOURCES-04 | two-state availability, no staleness (C33) | Given fixture with one healthy connector root and one root dir missing before first detection | When `terraphim-agent --robot sessions sources` | Then missing-root connector reported unavailable, healthy one available; negative-assert: no staleness dimension reported (healthy/stale/broken collapse to available/unavailable; T38 cache staleness untracked) | C33 | cli | T01/T03; dirs-mirror | P2
- TC-SOURCES-05 | stats-vs-disk truth diff (C34) | Given fixture with exactly N on-disk session files per source | When run REPL `/sessions stats` and count session files on disk | Then per-source stats counts are diffed against the on-disk count and the delta is surfaced in the test report or explicitly marked uncomputable (no doctor verb exists to do it; report-only assert) | C34 | repl | T30 + T04 on-disk fixture | P2
- TC-SOURCES-06 | read-only source invariant (C35 + C75 remnant) | Given snapshot hashes of all fixture session stores | When auto-import (T06) and `terraphim-agent --robot sessions list` complete | Then hashes unchanged — parse/import never mutates or deletes source session files (T09–T13 read-only); probe `sessions fix` → unknown-command | C35 | cli | T06+T09–T13; pre/post hashing | P2
- TC-SOURCES-07 | repair verb probe (C36) | Given stock CLI | When `terraphim-agent --robot sessions repair` | Then unknown-command within 0–7 enum (only persistent state is the read-only disk cache, T38; nothing to repair) | C36 | cli | — | P3
- TC-SOURCES-08 | no force-refresh; stale cache served as-is (C37) | Given disk cache (T38) written before the fixture grows newer content | When `terraphim-agent --robot sessions list` in CLI mode | Then stale cache served as-is (newer content absent); probe `sessions refresh` / `reindex` verbs → unknown-command; freshness asymmetry vs server-mode cold-import (T42) and clone() reset (T43) recorded in report | C37 | cli | T38; refs T42/T43 | P2
- TC-SOURCES-09 | zero doctor schemas (C38) | Given robot capabilities/schemas output | When enumerate all advertised schemas | Then no doctor-family schema present (negative count asserted as 0) | C38 | cli | — | P3
- TC-SOURCES-10 | diag slice coverage (C39) | Given fixture and robot mode | When `terraphim-agent --robot sessions sources` then `terraphim-agent --robot sessions index --verbose` | Then connector and index diagnostic slices covered; probe-first for paths/platform/version/quarantine fields → asserted absent (no database to report) | C39 | cli | T01–T03+T36 | P2
- TC-SOURCES-11 | capabilities self-description (C42) | Given robot mode | When capabilities/schemas/examples subcommand | Then commands + schemas + examples listed (membership asserts, D-13); probe env-var/exit-code/limits/recovery/workflow sections and record presence/absence — no breadth asserted beyond what is observed | C42 | cli | — | P2
- TC-SOURCES-12 | introspect coverage (C43) | Given robot capabilities | When enumerate per-command schemas and arguments | Then schema count and per-command argument coverage recorded and compared against cass's 40 (drift documented, no hard equality — sets may drift between builds) | C43 | cli | — | P2
- TC-SOURCES-13 | api-version probe (C44) | Given the built binary | When `terraphim-agent --version` and robot-mode output | Then version triple asserted if present, absence documented otherwise; crate drift 1.20.4 vs 1.21.3 recorded in the report (settles the UNCLEAR verdict) | C44 | cli | — | P2
- TC-SOURCES-14 | typo recovery (C45) | Given robot mode | When `terraphim-agent --robot sessions soources` (typo) | Then error carries AutoCorrection{original,corrected,distance} or "Did you mean" suggestion metadata; separately assert aliases q/s/query/find→search resolve to search | C45 | cli | schema.rs:124-131, 213-218; main.rs:1500-1545 | P3
- TC-SOURCES-15 | broken-connector circuit analog (C46) | Given one connector root unreadable among healthy ones | When auto-import (import_all) runs | Then import continues to completion, the failing connector is skipped and reported, and healthy sources are fully ingested (stateless analog of quarantine/circuit-breaker) | C46 | integration | T07 fixture | P3
- TC-SOURCES-16 | sources list + absent write-side (C47) | Given fixture and both output modes | When `terraphim-agent --robot sessions sources` and text-mode `sessions sources` | Then per-connector status + estimate in both modes; probe `sessions sources add|remove <path>` and custom-path flags → rejected; assert no config file can enable/disable connectors (registry fixed at build, T05) | C47 | cli | T01/T02/T05; dirs-mirror | P1
- TC-SOURCES-17 | implicit sync semantics (C48) | Given cleared cache and populated fixture | When auto-import runs then `terraphim-agent --robot sessions list` | Then sessions present from every registered connector (membership); probe `-s`/`--dry-run`/`--no-index` flags → asserted absent; since/until/limit CLI exposure verified probe-first before any assert (unverified) | C48 | cli | T06/T07/T13 | P1
- TC-SOURCES-18 | removed import verb (C48) | Given CLI and REPL | When `terraphim-agent --robot sessions import` and REPL `/sessions import` | Then parser returns the explanatory error pointing to auto-import (no silent failure); capabilities list contains no import verb | C48 | cli | — | P2
- TC-SOURCES-19 | degraded-source visibility (C49) | Given a warm cache from a healthy detection, then the connector root dir renamed away | When `terraphim-agent --robot sessions sources` | Then the degraded connector is still LISTED with unavailable status — never silently omitted | C49 | cli | T01/T03; rename-after-warm fixture | P2
- TC-SOURCES-20 | no runtime agent exclusion (C53) | Given dirs-mirrored HOME with a config file attempting to disable agents | When `terraphim-agent --robot sessions sources` | Then connector set unchanged (membership assert); no CLI exclude/include flag exists; only cargo features alter the set (T05 compile-time analog) | C53 | cli | T05 | P2
- TC-SOURCES-21 | source attribution side-probe (C55) | Given multi-source fixture | When `terraphim-agent --robot sessions search <term>` | Then probe-first: record whether each hit identifies its (local) source; assert only what is observed and flag the source-attribution gap either way | C55 | cli | T10 fixture | P3

## 6.3 TC-TIMELINE-STATS (rows C40, C41, C64 + analytics GAP rows; D-5 applies: FIXED timestamps, TZ=UTC)

- TC-TIMELINE-STATS-01 | stats totals + role splits (C40) | Given fixed-timestamp fixture (TZ=UTC) with known per-role message counts | When `terraphim-agent --robot sessions stats` | Then total_messages, total_user_messages, total_assistant_messages match the fixture and user+assistant sums to total (service.rs:329-361) | C40 | cli | T30 fixture; TZ=UTC; fixed timestamps | P2
- TC-TIMELINE-STATS-02 | per-source sums + unknown-source resilience (C40/C77) | Given multi-source fixture | When `terraphim-agent --robot sessions stats` | Then per-source counts enumerate every ingested harness and sum to the total; a session from an unknown source does not break stats | C40+C77 | cli | T30 multi-source | P2
- TC-TIMELINE-STATS-03 | stats negative families (C40) | Given robot stats JSON from TC-TIMELINE-STATS-01 | When schema-inspect the output | Then no by_agent, top_workspaces, date_range, or raw_mirror fields present | C40 | cli | — | P2
- TC-TIMELINE-STATS-04 | stats ≡ index consistency (C64) | Given indexed fixture | When run `terraphim-agent --robot sessions stats` and `terraphim-agent --robot sessions index --verbose` | Then stats totals equal the indexed count from index status; negative-assert: no freshness/coverage/drift surface in either output | C64 | cli | T30+T36 | P2
- TC-TIMELINE-STATS-05 | triage readiness composition (C41) | Given fixture and robot mode | When sequential robot runs `sessions sources` → `sessions stats` → `sessions index` | Then all three exit 0 and parse as JSON; documented as caller-composed readiness (exit codes are not health-wired) | C41 | cli | — | P2
- TC-TIMELINE-STATS-06 | timeline REPL smoke (net-new: handler.rs has zero tests) | Given fixed-timestamp fixture (TZ=UTC) | When REPL `/sessions timeline` | Then chronological distribution renders without error; probe-first: output shape recorded as observed, never asserted against an invented schema | — (REPL area coverage) | repl | timeline fixture; TZ=UTC | P2
- TC-TIMELINE-STATS-07 | analytics tokens gap (C65) | GAP row: no token/cost analytics exists; only constructible assert is the negative | When robot stats output is schema-inspected | Then no token/cost fields present; analytics capability deferred to roadmap | C65 | gap-deferred | — | P3
- TC-TIMELINE-STATS-08 | analytics models gap (C67) | GAP row: no model-usage analytics anywhere | When probe stats output and robot schemas for model fields | Then none found; deferred to roadmap | C67 | gap-deferred | — | P3
- TC-TIMELINE-STATS-09 | analytics rebuild gap (C68) | GAP row: stats recomputed per call; no rebuild verb | When probe `sessions analytics` and `sessions rebuild` verbs | Then unknown-command within 0–7 enum; deferred to roadmap | C68 | gap-deferred | — | P3
- TC-TIMELINE-STATS-10 | enrich dry-run invariant, optional (C69) | Given cached fixture sessions | When probe `/sessions enrich` for a dry-run/count mode (unverified — probe first) | Then if the mode exists: enrich counts never exceed the cached session count; if absent: record finding and defer | C69 | gap-deferred | enricher suite refs | P3
- TC-TIMELINE-STATS-11 | coverage/health metrics gap (C70) | GAP row: no coverage/health metrics surface | When probe stats + sources output for coverage/health fields | Then none found; nearest observable is T30 totals; deferred to roadmap | C70 | gap-deferred | — | P3

## 6.4 TC-EXPORT-SHOW (rows C71–C76, C78, C79; T32/T33/T37)

- TC-EXPORT-SHOW-01 | json export round-trip (C71) | Given indexed fixture | When `terraphim-agent --robot sessions export json` | Then output is a pretty-printed JSON array of Session objects that deserializes back equal to the source sessions (T37 serde round-trip) | C71 | cli | T33+T37 | P1
- TC-EXPORT-SHOW-02 | markdown export + md alias (C71) | Given indexed fixture | When `terraphim-agent --robot sessions export markdown` and `terraphim-agent --robot sessions export md` | Then both render all sessions; `md` alias accepted; flag order rule respected throughout | C71 | cli | T33 | P1
- TC-EXPORT-SHOW-03 | -o path write (C71) | Given a writable temp dir | When `terraphim-agent --robot sessions export json -o $TMP/out.json` | Then the file is written and its content is identical to the stdout export | C71 | cli | T33 | P1
- TC-EXPORT-SHOW-04 | --session single-session filter (C71) | Given fixture with ≥2 sessions | When `terraphim-agent --robot sessions export json --session <id>` | Then exactly one session is exported and its id matches | C71 | cli | T33 | P1
- TC-EXPORT-SHOW-05 | unknown formats rejected (C71) | Given CLI | When `terraphim-agent --robot sessions export html`, then `export text`, then `export clipboard` | Then each is rejected with an explicit unknown-format error — never silently ignored; exit within the 0–7 enum | C71 | cli | — | P1
- TC-EXPORT-SHOW-06 | export-html gap (C72) | GAP row: no HTML exporter exists | When covered by TC-EXPORT-SHOW-05's html rejection | Then capability recorded as deferred to roadmap | C72 | gap-deferred | — | P3
- TC-EXPORT-SHOW-07 | encrypted-archive gap (C73) | GAP row: no pages/encrypted-archive concept | When probe export format list for archive formats | Then none present; deferred to roadmap | C73 | gap-deferred | — | P3
- TC-EXPORT-SHOW-08 | show single session (T32) | Given fixture with a known session id | When REPL `/sessions show <id>` and robot-mode show | Then the full transcript for that id is rendered; unknown id → clean error (probe exact shape) | T32/C71-adjacent | repl | T32 | P1
- TC-EXPORT-SHOW-09 | resume absence (C76) | Given capabilities output and CLI | When probe `sessions resume` and `sessions continue` verbs and inspect show/export output | Then both verbs are unknown-command; show output contains no resume/continue affordance (must not imply resumability); flagged as a headline roadmap decision | C76 | cli | T32 | P1
- TC-EXPORT-SHOW-10 | MessageRole surface in export (C78) | Given the exported JSON from TC-EXPORT-SHOW-01 | When inspect message role values | Then roles are drawn from the MessageRole enum (membership assert); report notes the latent hazard: roles are a weak hook if resume is ever built | C78 | cli | T37 | P2
- TC-EXPORT-SHOW-11 | resume output contract (C79) | GAP row: depends on C76, which is absent | When resume verb probed (TC-EXPORT-SHOW-09) | Then no parity test constructible while the verb is absent; recorded as a roadmap checklist item | C79 | gap-deferred | — | P2

## 6.5 TC-FILES (row C66 fragment + T34/T35; mapping: Read/Glob/Grep=read; Edit/Write/MultiEdit/NotebookEdit=write incl. notebook_path; unknown tools skipped; by-file case-insensitive)

- TC-FILES-01 | read-tool mapping (C66) | Given a fixture session containing Read/Glob/Grep tool calls | When REPL `/sessions files <id>` | Then every touched path is listed with access=read and nothing else is | C66 | repl | T34 fixture | P2
- TC-FILES-02 | write-tool mapping incl. notebook_path (C66) | Given a session with Edit/Write/MultiEdit/NotebookEdit calls | When REPL `/sessions files <id>` | Then all written paths are listed with access=write; NotebookEdit contributes its notebook_path | C66 | repl | T34/T35 | P2
- TC-FILES-03 | unknown tools skipped (C66) | Given a session containing unknown/aliased tool names | When run files <id> extraction | Then unknown tools produce no row and no error (skipped, not a crash) | C66 | unit | T34 helpers | P2
- TC-FILES-04 | by-file case-insensitive (C66) | Given fixture files differing only in path case | When REPL `/sessions by-file <path>` using mixed-case input | Then the match succeeds case-insensitively and rows map back to their sessions | C66 | repl | T35 | P2
- TC-FILES-05 | exact mapping, no usage counts (C66) | Given robot mode | When `terraphim-agent --robot sessions files <id>` | Then rows match the tool→access mapping exactly; negative-assert: no per-tool usage-count fields anywhere in the output | C66 | cli | T34/T35 | P2


**TC line format:** `TC-ID | title | Given/When/Then | maps-to (C/T/E) | type (unit/integration/repl/cli/gap-deferred/docs-drift) | lane+fixture | priority`

**Global invariants (apply to every TC below; do not repeat per line):**
- Flag order (NORMATIVE): `--robot` / `--format` **precede** the subcommand — `terraphim-agent --robot sessions search "q"`.
- Isolation (NORMATIVE): HOME-override ONLY (dirs-5.0.1 on macOS ignores XDG); use `gtimeout` on macOS; **membership asserts only** (D-13) — never exact-set equality against upstream cass behavior beyond the compiled registry.
- Exit codes 0–7; empty machine-mode search exits **4** (main.rs:3076).
- Lanes: agents CI runs `cargo test --workspace --lib` only (integration/CLI lanes are opt-in; all harness code must be clippy `-D warnings` clean); terraphim-ai CI runs nextest with default features (~43/99 tests invisible — nightly lane required for full coverage).

## 6.6 TC-ROBOT-CLI

Scope: robot/machine-mode surface + config/env parity rows C80–C100. Robot JSON contract components (verified): ResponseMeta{version=CARGO_PKG_VERSION, elapsed_ms, timestamp} (schema.rs:56-59), AutoCorrection (schema.rs:124-131), Pagination{total,returned,offset,has_more} (schema.rs:134-157), preview_truncated (schema.rs:317-323, populated main.rs:2180-2200), TokenBudget.truncated, RobotError{code,message,details,suggestion} (main.rs:1315-1331), capability flag `session_search` (robot/schema.rs:317-321). `wildcard_fallback` is document-search output only (main.rs:2231,4383; schema.rs:299) — NOT in sessions search.

```
TC-ROBOT-CLI-01 | robot machine JSON schema contract | Given session fixtures seeded under HOME override; When `terraphim-agent --robot sessions search "q"`; Then output parses with ResponseMeta{version,elapsed_ms,timestamp}+Pagination{total,returned,offset,has_more}+TokenBudget.truncated and per-hit preview_truncated=true whenever preview clipped; negative-assert: no request-id field, no literal `_meta` key | C80/T-robot-schema E:schema.rs:56-59,134-157,317-323 E:main.rs:2180-2200 | integration | cli+fx/robot/schema-snapshot | P1
TC-ROBOT-CLI-02 | robot error envelope shape | Given no fixtures; When `terraphim-agent --robot sessions no-such-cmd`; Then exit≠0 and JSON is RobotError{code,message,details,suggestion}; negative-assert: request-id absent | C80 E:main.rs:1315-1331 | cli | cli+fx/robot/error-envelope | P1
TC-ROBOT-CLI-03 | AutoCorrection shape guard | When `terraphim-agent --robot sessions search "q"` returns an auto-correct payload; Then shape matches schema.rs:124-131; else field absent (snapshot-gated, no invented population rule) | C80 E:schema.rs:124-131 | cli | cli+fx/robot/schema-snapshot | P2
TC-ROBOT-CLI-04 | no wildcard_fallback in sessions search | When `terraphim-agent --robot sessions search "q"`; Then output has NO `wildcard_fallback` key (document-search only: main.rs:2231,4383; schema.rs:299) | C80 E:main.rs:2231,4383 E:schema.rs:299 | cli | cli+fx/robot/schema-snapshot | P2
TC-ROBOT-CLI-05 | robot docs surface (capabilities/schemas/examples) | When robot capabilities/schemas/examples documents fetched via `terraphim-agent robot ...`; Then all parse as well-formed JSON incl. capability flag `session_search`; negative-assert: no 12-topic doc-topics surface | C81/T02 E:robot/schema.rs:317-321 | cli | cli+fx/robot/capabilities-golden | P2
TC-ROBOT-CLI-06 | --help parity | When `terraphim-agent --help`; Then exit 0 and lists all 14 `sessions` subcommands (sources,list,search,stats,show,concepts,related,timeline,export,enrich,cluster,files,by-file,index); negative-assert: `--robot-help` is not a recognized flag | C82 | cli | cli+fx/robot/help-golden | P2
TC-ROBOT-CLI-07 | no JSONL trace-file surface | When any sessions invocation with `--trace-file` flag or TERRAPHIM_TRACE_FILE env; Then flag rejected as unknown / env is no-op; TERRAPHIM_VERBOSE verbosity is probe-first (unverified in code — see TC-DOCS-DRIFT-03) | C83 | cli | cli+fx/homes/probe | P2
TC-ROBOT-CLI-08 | no completions/man subcommands | When `terraphim-agent completions` / `terraphim-agent man`; Then unknown-subcommand error (optional coverage) | C84 | cli | cli+fx/robot/help-golden | P3
TC-ROBOT-CLI-09 | no TUI scripting surface | When sessions help scanned for TUI/scripting flags; Then none exist; REPL smoke coverage lives in harness lanes (see ch6a REPL sections) | C85 | repl | repl+repl-harness | P3
TC-ROBOT-CLI-10 | --version + no self-upgrade | When `terraphim-agent --version`; Then prints semver, exit 0; negative-assert: no `upgrade`/`check` subcommand | C86 | cli | cli+fx/robot/help-golden | P3
TC-ROBOT-CLI-11 | API version = CLI version | When `terraphim-agent --version` output compared to ResponseMeta.version from `terraphim-agent --robot sessions search "q"`; Then strings equal; negative-assert: no api/contract version triple, no terraphim_sessions version field, no exit-6 path | C87 E:schema.rs:56-59 | cli | cli+fx/robot/version-pair | P2
TC-ROBOT-CLI-12 | exit-4 empty machine-mode search | Given HOME-override with zero matching fixtures; When `terraphim-agent --robot sessions search "zzz-no-such-token"`; Then exit code exactly 4 (main.rs:3076) | C88/T19 E:main.rs:3076 | cli | cli+fx/homes/empty | P2
TC-ROBOT-CLI-13 | learn from-session resolves via disk cache | Given `<cache_dir>/terraphim-agent/sessions.json` PRE-SEEDED externally (FIXTURE NOTE: CLI disk cache is READ but NEVER written — seed by artifact copy of a known-good sessions.json, never by running the agent first; hash-compare after run to prove read-only) ; When learn from-session executes; Then target session resolves from cache file and cache bytes unchanged | C88/T40/T38 E:main.rs:1257-1264,2955-2964 | integration | cli+fx/cache/seeded-sessions.json | P2
TC-ROBOT-CLI-14 | cache path follows dirs resolution; CLAUDE_SESSIONS_DIR probe | Given HOME override; When CLI session run; Then cache read path resolves under overridden HOME as `<cache_dir>/terraphim-agent/sessions.json` (dirs-5.0.1, XDG ignored on macOS); probe: set CLAUDE_SESSIONS_DIR=<tmp> → expect NO redirect (unimplemented; audit+fact-check confirmed); negative-assert: no --db/--data-dir flags; FIXTURE NOTE: parallel runs share the fixed cache path → isolate HOME per worker or serialize | C89/T38 E:main.rs:1257-1264,2955-2964 | cli | cli+fx/homes/worker-N | P1
TC-ROBOT-CLI-15 | C91 RESOLVED CONFLICT: per-harness discovery root envs | Resolution recorded: audit + fact-check prove CLAUDE_SESSIONS_DIR is NOT implemented, so the assertion IS the probe — with CLAUDE_SESSIONS_DIR=<tmp> set, `terraphim-agent --robot sessions sources` discovery output unchanged; hand result to DOCS-DRIFT lane (TC-DOCS-DRIFT-02); negative-assert: CODEX_HOME / GEMINI_HOME / aider root envs are no-ops (not implemented) | C91 E:audit+fact-check | cli | cli+fx/homes/probe | P2
TC-ROBOT-CLI-16 | no runtime harness exclusion | When `terraphim-agent --robot sessions sources` re-run after any config edit; Then connector set unchanged (compile-time registry T05); membership assert only (D-13) | C90/T05 | cli | cli+fx/robot/sources-golden | P2
TC-ROBOT-CLI-17 | no semantic tuning envs | When embedder/batch/watchdog env names set; Then all no-ops; same query run twice → identical result ordering (BM25 deterministic) | C92 | cli | cli+fx/homes/probe | P2
TC-ROBOT-CLI-18 | IndexStatus populated post-search; no governor envs | Given fixtures; When one `terraphim-agent --robot sessions search "q"` then REPL `/sessions index --verbose`; Then counts non-zero and "Scorer: BM25 (Okapi)" line present, builds nothing (status-only, handler.rs:2840-2874); server-mode: cold start auto-imports every run (no disk cache); negative-assert: governor envs no-ops; measure cold-import latency informationally — do not tune | C93/T41/T42/T36 E:handler.rs:2840-2874 E:main.rs:4906-4913 | repl | repl+repl-harness | P2
TC-ROBOT-CLI-19 | no streaming consumer env | Negative only: streaming-consumer env names are no-ops | C94 | cli | cli+fx/homes/probe | P3
TC-ROBOT-CLI-20 | --format/--robot switching; env no-ops | When `terraphim-agent --format json sessions sources` vs default human output; Then JSON vs plain switch works; CASS_OUTPUT_FORMAT / NO_COLOR set → no effect; plain text byte-stable across two runs | C95 E:main.rs:538-546 | cli | cli+fx/robot/sources-golden | P2
TC-ROBOT-CLI-21 | no global UX flags | When `terraphim-agent --color/--progress/--wrap/-q/-v sessions sources`; Then all rejected as unknown flags; verbosity only via TERRAPHIM_VERBOSE (probe-first, see TC-DOCS-DRIFT-03) | C96 | cli | cli+fx/homes/probe | P3
TC-ROBOT-CLI-22 | sources membership = compiled set | When `terraphim-agent --robot sessions sources`; Then JSON lists exactly the compiled set {claude-code-native, claude-code, cursor, aider} (membership-only, D-13); by_source counts match per-format fixtures; cursor stub note (cla/connector.rs:154-160 when tsa-full off) | C97/T05 E:cla/connector.rs:154-160 | cli | cli+fx/robot/sources-golden | P1
TC-ROBOT-CLI-23 | trap regression: parallel invocations | Given fixtures; When 4 parallel `terraphim-agent --robot sessions search "q"` processes; Then all exit 0 with identical totals (BM25 deterministic), no lock/busy errors; exit 7 never returned for locks (OnceLock singleton T39; clone() resets cache T43) | C98/T39/T43 | integration | cli+fx/homes/worker-N | P1
TC-ROBOT-CLI-24 | trap regression: parallel cold imports | Given two server-mode starts (T42 cold auto-import, disk cache skipped); When started concurrently; Then no collision/corruption; both serve identical session totals | C98/T42 E:main.rs:4906-4913 | integration | cli+fx/homes/worker-N | P1
TC-ROBOT-CLI-25 | P0 matrix: claude-code-native fixture | Given fx/sessions/claude-code-native/; When `terraphim-agent --robot sessions list`; Then session count + parsed fields match fixture exactly (native claude ON) | C99/T05 | integration | cli+fx/sessions/claude-code-native/ | P0
TC-ROBOT-CLI-26 | P0 matrix: claude-code JSONL fixture | Given fx/sessions/claude-code-jsonl/; When `terraphim-agent --robot sessions list` + search; Then counts/fields match fixture (claude-code ON) | C99/T05 | integration | cli+fx/sessions/claude-code-jsonl/ | P0
TC-ROBOT-CLI-27 | P0 matrix: aider fixture | Given fx/sessions/aider/; When `terraphim-agent --robot sessions list` + search; Then counts/fields match fixture (aider ON) | C99/T05 | integration | cli+fx/sessions/aider/ | P0
TC-ROBOT-CLI-28 | P0 matrix: cursor stub fixture | Given fx/sessions/cursor/; When `terraphim-agent --robot sessions list`; Then parses as stub; counts match stub expectations (cla/connector.rs:154-160 when tsa-full off) | C99/T05 E:cla/connector.rs:154-160 | integration | cli+fx/sessions/cursor/ | P0
TC-ROBOT-CLI-29 | negative: codex/cline/opencode absent | When `terraphim-agent --robot sessions sources`; Then codex, cline, opencode ABSENT from source list (parsers exist in crate, features OFF in agent build); negative-assert codex source count = absent, not zero | C99/T05 | cli | cli+fx/robot/sources-golden | P0
TC-ROBOT-CLI-30 | line-number semantics unassertable | Negative: session hit schema has NO line_number field (schema.rs:317-321) → line-anchoring cannot be asserted; record as structural gap | C99 E:schema.rs:317-321 | gap-deferred | — | P0
TC-ROBOT-CLI-31 | workspace matching = title-path coincidence | Given fixtures from two projects; When `terraphim-agent --robot sessions search "<project-path-token>"`; Then top hits carry token in title (title=project-path, nearest T09); negative-assert: no --workspace/--current flags on sessions search; caveat documented: title-as-path only, no real workspace scoping | C100/T09 | cli | cli+fx/sessions/multi-project/ | P1
```

**Section notes (6.6):**
- **C91 conflict resolution (explicit):** earlier sources conflicted on whether CLAUDE_SESSIONS_DIR works. Resolution: audit + fact-check both prove it is NOT implemented → the test is a probe showing no effect on discovery, plus a DOCS-DRIFT artifact (TC-DOCS-DRIFT-02). No positive-redirect assert is ever written.
- **C88 cache-seeding fixture note:** TC-ROBOT-CLI-13 requires the cache file seeded externally; agent never writes it. Fixture = versioned artifact copy + post-run hash equality.
- **C98 trap regressions (P1 despite N-A verdict):** exit-7/lock semantics are N-A upstream, but the T39/T42/T43 singleton + cold-import paths are real concurrency traps; both regression TCs are P1.
- **C99 P0 per-format fixture matrix (TC-ROBOT-CLI-25…30):** highest-risk parity row — multi-harness users silently get Claude-family-only coverage (codex/cline/opencode OFF). [DRIFT] pin crate version in CI (dep floor 1.20.2, resolved 1.20.4); feature set may differ by build.
- Snapshot-test `robot/schema.rs` as the machine contract (C80 note): ResponseMeta / Pagination / RobotError / preview_truncated golden files under fx/robot/.
- [DRIFT] C87: 1.20.4 vs 1.21.3 — CI pin R1 applies to TC-ROBOT-CLI-11.

## 6.7 TC-WATCH-INDEX

Scope: index lifecycle C21–C29 (routed from the Search chunk). Core contract: `/sessions index [--verbose]` is STATUS-ONLY — prints counts + "Scorer: BM25 (Okapi)", builds nothing (handler.rs:2840-2874). Native watcher T14 is public-API-only (200ms debounce + dedup); regression tests #814/#815 exist, one #[ignore]d inotify test → nightly lane.

```
TC-WATCH-INDEX-01 | /sessions index is status-only | Given fixtures imported via ch5 lanes; When REPL `/sessions index` then `/sessions index --verbose`; Then prints counts + "Scorer: BM25 (Okapi)" and nothing else changes; negative-assert: no rebuild side-effect — cache dir mtime/content-hash unchanged, no artifact created | C21/T36 E:handler.rs:2840-2874 | repl | repl+repl-harness | P2
TC-WATCH-INDEX-02 | auto-import trigger + skip/truncate (cross-ref) | Native asserts: T06 auto-import trigger and T07 skip-on-failure + truncation; import tests live in ch5 (TC-SOURCES / TC-FILES) — cross-ref only, no duplicate here | C21/T06/T07 | integration | cli+crossref-ch5 | P2
TC-WATCH-INDEX-03 | --full / --force-rebuild N-A | Every query is already an in-memory full BM25 rebuild (T16) — no such flags exist; record n/a verdict, no runtime assert | C22/C23/T16 | gap-deferred | — | P3
TC-WATCH-INDEX-04 | C24 probe: watcher presence in pinned build | Probe-first: does the pinned registry build (1.20.4) contain the T14 watcher engine? If yes → PARTIAL (engine present, unexposed); if no → MISSING; REPL/CLI watch surface absent either way; record verdict in ch7 + traceability.csv | C24/T14 | integration | nightly+fx/watcher-probe | P2
TC-WATCH-INDEX-05 | watcher regression tests #814/#815 | Tests #814/#815 (200ms debounce + dedup) stay green; one #[ignore]d inotify test runs ONLY in nightly lane (terraphim-ai nextest); default-features CI subset must not silently shrink this set | C24/T14 | unit | nightly+terraphim-ai-nextest | P2
TC-WATCH-INDEX-06 | GAP: semantic indexing | No semantic index exists; enrichment (T21/T23) concepts are in-memory only; enrichment is the design alternative → defer, no test | C25/T21/T23 | gap-deferred | — | P3
TC-WATCH-INDEX-07 | GAP: idempotency-key | No idempotency keys; T36 is status-only; T43 clone() resets cache → dedup is per-instance only, no cross-invocation guarantee → defer | C26/T36/T43 | gap-deferred | — | P2
TC-WATCH-INDEX-08 | GAP: NDJSON progress events | No progress-event stream; defer | C27 | gap-deferred | — | P3
TC-WATCH-INDEX-09 | GAP: robot-trace-ingest | No trace-ingest path; defer | C28 | gap-deferred | — | P3
TC-WATCH-INDEX-10 | GAP: import chatgpt | No chatgpt import; flag T04-vs-T05 registry drift in traceability; defer | C29/T04/T05 | gap-deferred | — | P2
```

**Section notes (6.7):** C21 keeps only terraphim-native asserts (T06/T07) — ch5 owns import mechanics. C22/C23 get no TC action (N-A by design). C24 is probe-first; its verdict (PARTIAL vs MISSING) must land in ch7 and traceability.csv, not be guessed here.

## 6.8 TC-DOCS-DRIFT

Scope: documented-but-unimplemented / phantom surface. Each TC ends in a **doc decision** (fix docs or file implementation issue), recorded in ch7.

```
TC-DOCS-DRIFT-01 | /sessions import removal message | Given REPL; When `/sessions import <args>`; Then parser returns an explanatory error (removal notice, not a crash/unknown-cmd), and no import occurs; skill docs still documenting import are corrected in the same PR | T07-removed E:handler.rs parser | docs-drift | repl+repl-harness | P2
TC-DOCS-DRIFT-02 | CLAUDE_SESSIONS_DIR documented but unimplemented | Given docs claim env redirect; When `CLAUDE_SESSIONS_DIR=<tmp> terraphim-agent --robot sessions sources`; Then discovery unchanged (audit + fact-check: NOT implemented) → doc decision: remove env from docs or file implementation issue; consumes probe result from TC-ROBOT-CLI-15 | C89/C91 | docs-drift | cli+fx/homes/probe | P1
TC-DOCS-DRIFT-03 | TERRAPHIM_VERBOSE documented, unverified in code | Probe-first: `TERRAPHIM_VERBOSE=1 terraphim-agent --robot sessions search "q"` vs unset → diff stderr verbosity; if no delta → doc audit (remove or implement); record verdict in ch7 | C83/C96 | docs-drift | cli+fx/homes/probe | P2
TC-DOCS-DRIFT-04 | claude-log-analyzer phantom crate | Skill docs reference claude-log-analyzer crate which exists in NEITHER workspace; doc audit: grep skill docs, strike the reference, point readers at built-in sessions commands | C88-adjacent | docs-drift | unit+docs-grep | P2
TC-DOCS-DRIFT-05 | supported_formats advertisement vs OutputFormat enum | Robot capabilities advertise supported_formats ["json","jsonl","minimal","table"] but CLI OutputFormat enum is Human|Json|JsonCompact only (main.rs:538-546); probe: `terraphim-agent --format jsonl sessions sources` → expect unknown-format rejection; then doc decision: correct capabilities advertisement or add formats | C81/C95 E:main.rs:538-546 | docs-drift | cli+fx/robot/capabilities-golden | P2
```

**Section notes (6.8):** every docs-drift TC produces a concrete artifact (doc edit or filed issue) — a green test alone is not the deliverable. Probes must run against the CI-pinned version (R1) so drift verdicts are reproducible.

## 6.9 TC-PERF

Scope: benches/search_nfr.rs (criterion, required-features `search-index`); NFR #3014 thresholds: cold search over 10K sessions < 100ms, BM25 scoring op < 10ms. Bench is NOT wired into CI today.

```
TC-PERF-01 | bench compiles and runs | When `cargo bench --features search-index` on pinned toolchain; Then benches/search_nfr.rs (criterion) runs green | NFR-3014 | integration | perf+criterion | P2
TC-PERF-02 | NFR #3014 thresholds on deterministic corpus | Given fx/perf/10k-corpus (10K deterministic sessions); When bench executes; Then cold search p50 < 100ms and BM25 op < 10ms; criterion reports saved as CI artifacts | NFR-3014 | integration | perf+fx/perf/10k-corpus | P2
TC-PERF-03 | scheduled CI perf job (currently a gap) | Bench not wired into CI → add nightly scheduled job (NOT a PR gate) running criterion against a saved baseline; fail only on threshold breach | NFR-3014 | gap-deferred | ci+scheduled | P2
TC-PERF-04 | deterministic corpus generator | Fixed-seed generator produces 10K synthetic sessions with stable IDs/timestamps; regeneration is byte-identical (hash pinned in fixture metadata) | NFR-3014 | unit | perf+fx/perf/10k-corpus-gen | P1
TC-PERF-05 | no wall-clock asserts outside bench | Lint rule for all harness lanes: functional tests must never assert durations; C93 cold-import latency is measured-informational only (optional `gtimeout` guard) | C93 | integration | all-lanes | P3
```

**Section notes (6.9):** deterministic corpus (TC-PERF-04) is the prerequisite for meaningful CI deltas — build it first. Scheduled job output goes to artifacts; never block merges on machine-noise. No wall-clock asserts outside the bench harness, ever.

---

Traceability: full risk→test mapping and lane plan live in **ch7.md**; machine-readable C/T/E↔TC mapping in **traceability.csv** (rows TC-ROBOT-CLI-01…TC-PERF-05, this file).


---

# Chapter 7 — Coverage Traceability, Acceptance Criteria & Risks

- **Status:** RECONCILED (2026-09-03) — TC references mapped to the authoritative chapter catalogs (ch5: TC-SEARCH/IMPORT/ENRICH; ch6a: TC-SOURCES/TIMELINE-STATS/EXPORT-SHOW/FILES; ch6b: TC-ROBOT-CLI/WATCH-INDEX/DOCS-DRIFT/PERF). Machine-readable source of truth: `traceability.csv` (100 rows; 82 actionable rows all mapped — 81 via C-ID join + C17 routed).
- **Machine-readable matrix:** `traceability.csv` (same directory) — 100 rows, header `c_id,verdict,priority,disposition,tc_ids,notes`.

---

## 7.1 Traceability Matrix (C01–C100)

**Disposition vocabulary** (one per row, in CSV `disposition`):

| Disposition | Meaning | Rule |
|---|---|---|
| `TEST` | ≥1 candidate TC exists (incl. absence/negative probes) | FULL/PARTIAL rows and MISSING rows with a terraphim-native analog or probe |
| `GAP-DEFERRED` | No test; capability absent in terraphim and deferred with reason | MISSING rows with no assertable surface; gap is *documented*, not silently dropped |
| `N-A` | Justified non-applicability (mechanism designed away / out of scope) | Written justification mandatory (subagent_07 check 4: PASS, 18/18); absence-probe attached where the capability is command-visible |
| `UNCLEAR` | Verdict not resolvable from code; settled by runtime probe | Exactly 2 rows (C24, C44), each with a probe TC |

**Counts:** verdicts — PARTIAL 34, MISSING 46, N-A 18, UNCLEAR 2, FULL 0 (all four chunk summaries re-verified by subagent_07 check 8). Dispositions — **TEST 66, GAP-DEFERRED 14, N-A 18, UNCLEAR 2**. Priorities — P0×3 (C01, C62, C99 — all TEST), P1×15 (14 TEST + C98 N-A-with-trap-regressions), P2×44, P3×38.

**Coverage-adversary verdict (subagent_07):** FIX-FIRST, 8 fixes (4 blocking) → **ALL RESOLVED on mainline** (verdict vocabulary normalized, C71 pipe-escaping fixed, exit-4 collision caveat added, E-ID strategy decided: E-mapping attaches here, not in matrix rows). Verdict-fact-checker (subagent_07b): 0 WRONG verdicts; C07/C40/C80 evidence corrected; C20/C45/C87 re-triaged MISSING→PARTIAL. Feasibility (subagent_08): **IMPLEMENTABLE WITH FIXES (7)**, no redesign — all 7 applied (see §7.4).

**Owner legend:** ch5 = Search / Import / Enrich chapters; ch6 = Sources / Timeline-Stats / Export-Show / Files / Robot-CLI / Watch-Index / Docs-Drift / Perf. TC refs below use subagent_06 §4 area codes (SR/IX/MS→ch5; SO/AS/EX/RF/RB/CF/HD/RG→ch6); matrix rows below carry the **authoritative TC IDs**; the E-adoption table keeps skeleton area codes (legend at §E-table) because only 5 E-IDs are referenced verbatim on TC lines — the full E→C→TC chain runs through traceability.csv.

| C | Verdict | P | Disposition | Authoritative TC IDs (reconciled 2026-09-03) | Owner § |
|---|---|---|---|---|---|
| C01 | PARTIAL | P0 | TEST | TC-SEARCH-05+TC-SEARCH-06+TC-SEARCH-07+TC-SEARCH-09+TC-SEARCH-10+TC-SEARCH-13+TC-SEARCH-14+TC-SEARCH-15+TC-SEARCH-18+TC-SEARCH-19 | ch5·search |
| C02 | MISSING | P1 | TEST | TC-IMPORT-11+TC-SEARCH-19 | ch5·search |
| C03 | PARTIAL | P2 | TEST | TC-SEARCH-10+TC-SEARCH-11 | ch5·search |
| C04 | MISSING | P2 | GAP-DEFERRED | TC-SEARCH-22 | ch5·search |
| C05 | MISSING | P2 | TEST | TC-SEARCH-23 | ch6·timeline-stats |
| C06 | MISSING | P2 | GAP-DEFERRED | TC-SEARCH-24 | ch5·search |
| C07 | PARTIAL | P1 | TEST | TC-SEARCH-01+TC-SEARCH-04 | ch5·search |
| C08 | MISSING | P3 | GAP-DEFERRED | TC-ENRICH-01+TC-SEARCH-25 | ch5·search |
| C09 | MISSING | P3 | GAP-DEFERRED | TC-ENRICH-01+TC-IMPORT-14+TC-SEARCH-04 | ch5·search |
| C10 | MISSING | P3 | GAP-DEFERRED | TC-ENRICH-01+TC-IMPORT-14+TC-SEARCH-04 | ch5·search |
| C11 | MISSING | P2 | GAP-DEFERRED | TC-ENRICH-01+TC-IMPORT-14+TC-SEARCH-04 | ch5·search |
| C12 | PARTIAL | P2 | TEST | TC-IMPORT-01+TC-IMPORT-16+TC-SEARCH-20 | ch5·import |
| C13 | PARTIAL | P1 | TEST | TC-SEARCH-11 | ch6·export-show |
| C14 | MISSING | P2 | TEST | TC-ENRICH-01+TC-IMPORT-14+TC-SEARCH-04 | ch6·export-show |
| C15 | PARTIAL | P1 | TEST | TC-ENRICH-05+TC-ENRICH-06+TC-ENRICH-07 | ch5·enrich |
| C16 | PARTIAL | P1 | TEST | TC-SEARCH-19 | ch5·search |
| C17 | PARTIAL | P1 | TEST | TC-TIMELINE-STATS-06 | ch6·timeline-stats |
| C18 | MISSING | P3 | GAP-DEFERRED | TC-SEARCH-26 | ch5·search |
| C19 | MISSING | P3 | GAP-DEFERRED | TC-SEARCH-26 | ch5·search |
| C20 | PARTIAL | P2 | TEST | TC-SEARCH-21 | ch5·search |
| C21 | MISSING | P2 | TEST | TC-IMPORT-13+TC-WATCH-INDEX-01+TC-WATCH-INDEX-02 | ch5·import |
| C22 | N-A | P3 | N-A | — | ch5·import |
| C23 | N-A | P3 | N-A | — | ch5·import |
| C24 | UNCLEAR | P2 | UNCLEAR | TC-WATCH-INDEX-04+TC-WATCH-INDEX-05 | ch6·watch-index |
| C25 | MISSING | P3 | GAP-DEFERRED | TC-ENRICH-01+TC-IMPORT-14+TC-SEARCH-04+TC-WATCH-INDEX-06 | ch5·enrich |
| C26 | MISSING | P2 | GAP-DEFERRED | TC-WATCH-INDEX-07 | ch5·import |
| C27 | MISSING | P3 | GAP-DEFERRED | TC-WATCH-INDEX-08 | ch5·import |
| C28 | MISSING | P3 | GAP-DEFERRED | TC-WATCH-INDEX-09 | ch5·import |
| C29 | MISSING | P2 | GAP-DEFERRED | TC-IMPORT-05+TC-WATCH-INDEX-10 | ch5·import |
| C30 | MISSING | P2 | TEST | TC-SOURCES-01 | ch6·robot-cli |
| C31 | PARTIAL | P2 | TEST | TC-SOURCES-02 | ch6·robot-cli |
| C32 | MISSING | P3 | TEST | TC-SOURCES-03 | ch6·robot-cli |
| C33 | PARTIAL | P2 | TEST | TC-SOURCES-04 | ch6·sources |
| C34 | MISSING | P2 | TEST | TC-SOURCES-05 | ch6·robot-cli |
| C35 | MISSING | P2 | TEST | TC-SOURCES-06 | ch6·robot-cli |
| C36 | MISSING | P3 | TEST | TC-SOURCES-07 | ch6·robot-cli |
| C37 | MISSING | P2 | TEST | TC-SOURCES-08 | ch6·watch-index |
| C38 | MISSING | P3 | TEST | TC-SOURCES-09 | ch6·robot-cli |
| C39 | PARTIAL | P2 | TEST | TC-SOURCES-10 | ch6·robot-cli |
| C40 | PARTIAL | P2 | TEST | TC-TIMELINE-STATS-01+TC-TIMELINE-STATS-02+TC-TIMELINE-STATS-03 | ch6·timeline-stats |
| C41 | MISSING | P2 | TEST | TC-TIMELINE-STATS-05 | ch6·robot-cli |
| C42 | PARTIAL | P2 | TEST | TC-SOURCES-11 | ch6·robot-cli |
| C43 | PARTIAL | P2 | TEST | TC-SOURCES-12 | ch6·robot-cli |
| C44 | UNCLEAR | P2 | UNCLEAR | TC-SOURCES-13 | ch6·robot-cli |
| C45 | PARTIAL | P3 | TEST | TC-SOURCES-03+TC-SOURCES-14 | ch6·robot-cli |
| C46 | N-A | P3 | N-A | — | ch6·robot-cli |
| C47 | PARTIAL | P1 | TEST | TC-SOURCES-16 | ch6·sources |
| C48 | PARTIAL | P1 | TEST | TC-SOURCES-17+TC-SOURCES-18 | ch6·sources |
| C49 | PARTIAL | P2 | TEST | TC-SOURCES-19 | ch6·sources |
| C50 | N-A | P3 | N-A | — | ch6·sources |
| C51 | N-A | P3 | N-A | — | ch6·sources |
| C52 | N-A | P3 | N-A | — | ch6·sources |
| C53 | MISSING | P2 | TEST | TC-SOURCES-20 | ch5·import |
| C54 | N-A | P3 | N-A | — | ch6·sources |
| C55 | N-A | P3 | N-A | — | ch6·sources |
| C56 | N-A | P3 | N-A | — | ch6·sources |
| C57 | PARTIAL | P2 | TEST | TC-ENRICH-01+TC-ENRICH-02+TC-ENRICH-10+TC-ENRICH-11 | ch5·enrich |
| C58 | N-A | P3 | N-A | — | ch5·enrich |
| C59 | N-A | P3 | N-A | — | ch5·enrich |
| C60 | PARTIAL | P2 | TEST | TC-ENRICH-01+TC-ENRICH-08 | ch5·enrich |
| C61 | N-A | P3 | N-A | — | ch5·enrich |
| C62 | PARTIAL | P0 | TEST | TC-ENRICH-04+TC-ENRICH-13+TC-SEARCH-01+TC-SEARCH-02+TC-SEARCH-03+TC-SEARCH-04+TC-SEARCH-08 | ch5·search |
| C63 | MISSING | P2 | TEST | TC-ENRICH-01+TC-ENRICH-15 | ch5·enrich |
| C64 | MISSING | P2 | TEST | TC-TIMELINE-STATS-04 | ch6·timeline-stats |
| C65 | MISSING | P3 | TEST | TC-TIMELINE-STATS-07 | ch6·timeline-stats |
| C66 | MISSING | P2 | TEST | TC-FILES-01+TC-FILES-02+TC-FILES-03+TC-FILES-04+TC-FILES-05 | ch6·files |
| C67 | MISSING | P3 | TEST | TC-TIMELINE-STATS-08 | ch6·timeline-stats |
| C68 | MISSING | P3 | TEST | TC-TIMELINE-STATS-09 | ch6·timeline-stats |
| C69 | MISSING | P3 | TEST | TC-TIMELINE-STATS-10 | ch6·timeline-stats |
| C70 | MISSING | P3 | TEST | TC-TIMELINE-STATS-11 | ch6·timeline-stats |
| C71 | PARTIAL | P1 | TEST | TC-EXPORT-SHOW-01+TC-EXPORT-SHOW-02+TC-EXPORT-SHOW-03+TC-EXPORT-SHOW-04+TC-EXPORT-SHOW-05+TC-EXPORT-SHOW-08 | ch6·export-show |
| C72 | MISSING | P3 | TEST | TC-EXPORT-SHOW-05+TC-EXPORT-SHOW-06 | ch6·export-show |
| C73 | MISSING | P3 | GAP-DEFERRED | TC-EXPORT-SHOW-07 | ch6·export-show |
| C74 | N-A | P3 | N-A | — | ch6·export-show |
| C75 | N-A | P3 | N-A | — | ch6·robot-cli |
| C76 | MISSING | P1 | TEST | TC-EXPORT-SHOW-09+TC-EXPORT-SHOW-11 | ch6·files |
| C77 | PARTIAL | P2 | TEST | TC-TIMELINE-STATS-02 | ch6·timeline-stats |
| C78 | MISSING | P2 | TEST | TC-EXPORT-SHOW-01+TC-EXPORT-SHOW-10 | ch6·files |
| C79 | MISSING | P2 | TEST | TC-EXPORT-SHOW-09+TC-EXPORT-SHOW-11 | ch6·files |
| C80 | PARTIAL | P1 | TEST | TC-ROBOT-CLI-01+TC-ROBOT-CLI-02+TC-ROBOT-CLI-03+TC-ROBOT-CLI-04 | ch6·robot-cli |
| C81 | PARTIAL | P2 | TEST | TC-DOCS-DRIFT-05+TC-ROBOT-CLI-05 | ch6·robot-cli |
| C82 | PARTIAL | P2 | TEST | TC-ROBOT-CLI-06 | ch6·robot-cli |
| C83 | MISSING | P2 | TEST | TC-DOCS-DRIFT-03+TC-ROBOT-CLI-07 | ch6·robot-cli |
| C84 | N-A | P3 | N-A | — | ch6·robot-cli |
| C85 | N-A | P3 | N-A | — | ch6·robot-cli |
| C86 | N-A | P3 | N-A | — | ch6·robot-cli |
| C87 | PARTIAL | P2 | TEST | TC-ROBOT-CLI-11+TC-SEARCH-12 | ch6·robot-cli |
| C88 | MISSING | P2 | TEST | TC-DOCS-DRIFT-04+TC-ROBOT-CLI-12+TC-ROBOT-CLI-13 | ch6·robot-cli |
| C89 | PARTIAL | P1 | TEST | TC-DOCS-DRIFT-02+TC-ROBOT-CLI-14+TC-ROBOT-CLI-15 | ch6·robot-cli |
| C90 | MISSING | P2 | TEST | TC-ROBOT-CLI-16 | ch6·robot-cli |
| C91 | PARTIAL | P2 | TEST | TC-ROBOT-CLI-15+TC-DOCS-DRIFT-02 | ch6·sources |
| C92 | MISSING | P2 | TEST | TC-ROBOT-CLI-17 | ch6·robot-cli |
| C93 | PARTIAL | P2 | TEST | TC-PERF-05+TC-ROBOT-CLI-18 | ch6·watch-index |
| C94 | MISSING | P3 | TEST | TC-ROBOT-CLI-19 | ch6·robot-cli |
| C95 | PARTIAL | P2 | TEST | TC-DOCS-DRIFT-05+TC-ROBOT-CLI-20 | ch6·robot-cli |
| C96 | MISSING | P3 | TEST | TC-DOCS-DRIFT-03+TC-ROBOT-CLI-21 | ch6·robot-cli |
| C97 | PARTIAL | P1 | TEST | TC-ROBOT-CLI-22 | ch6·sources |
| C98 | N-A | P1 | N-A | — | ch6·robot-cli |
| C99 | PARTIAL | P0 | TEST | TC-IMPORT-05+TC-IMPORT-06+TC-IMPORT-07+TC-IMPORT-08+TC-IMPORT-09+TC-IMPORT-10+TC-ROBOT-CLI-25+TC-ROBOT-CLI-26+TC-ROBOT-CLI-27+TC-ROBOT-CLI-28+TC-ROBOT-CLI-29+TC-ROBOT-CLI-30+TC-SEARCH-16 | ch5·import |
| C100 | MISSING | P1 | TEST | TC-ROBOT-CLI-31+TC-SEARCH-15 | ch6·files |

**GAP-DEFERRED register (14):** C04 cursor pagination; C06 query diagnostics; C08 ANN/HNSW; C09 embedder/rerank selection; C10 daemon/latency tiers; C11 chained searches; C18/C19 pack + pack-intent; C25 semantic indexing; C26 idempotent indexing; C27 NDJSON progress; C28 ingest tracing; C29 chatgpt import; C73 pages publishing. Each carries its reason in the CSV notes; none is command-visible in terraphim, so no absence-probe is owed beyond the family probes already listed.

---

## 7.2 E-Assertion Adoption (E01–E93, E58 unused → 92 assertions)

Source: subagent_04 §1 (nine assertion groups) — adoption per subagent_06 §4.13. Three states: **ACTIVE** (assertion adopted against a counterpart mechanism), **DIVERGENCE-PINNED** (active TC asserting terraphim's deliberately different behavior), **RECORD-ONLY / N-A** (no counterpart; recorded with parity-row reference).

| Group | E-range | ACTIVE | DIVERGENCE-PINNED | RECORD-ONLY / N-A (row refs) |
|---|---|---|---|---|
| 1a Search semantics | E01–E26 | E01→SR-06; E03→SR-02/03; E14→SR-08; E18→SR-20; E21→RB-05; E22→RB-01 | E10/E11→SR-06 (no line numbers); E12→SR-17 (tool_result IS indexed); E04→SR-15 (literal `*`); E07/E08→CF-05 (no workspace filter); E16→MS-05 (determinism; no mode flag); E20→RB-02 (format subset) | E02, E05/E06→AS-03/04 analog; E09, E13 (Pagination struct exists — no false negative-assert), E19, E23–E26 (C03/C04/C11) |
| 1b Index lifecycle | E27–E40 | E30→IX-12 (write→auto-import→searchable) | E39→IX-04 (status-only quirk pin) | E27–E29→HD-07 (C30/C33); E31 (open#196 N-A); E32–E38→IX-05/07/11 adjacent pins (C26/C36/C37) |
| 1c Recovery (doctor) | E41–E48 | E41/E46 safety property→CF-06 zero-write guardrail (vacuously satisfied + actively asserted) | — | E42–E45, E47, E48→HD-07 probes (C34–C38) |
| 1d Sources / fleet | E49–E57 | — | E53→CF-03/SO-01 (exclusion = compile-time rebuild); E54→SO-01 (slug set membership) | E49–E52 (C50/C52 remote N-A); E55–E57 (C55/C56) |
| 1e Semantic / hybrid | E59–E65 | E64→SR-13 (silent degrade to plain BM25); E17→SR-13/MS-05 | — | E59–E63, E65→MS-04 probes + MS-02 rebuild-advice analog (C57–C63) |
| 1f Analytics | E66–E74 | — | — | E66–E74→AS-05 absence probe (C64–C70); E67/E68/E73 "MUST if implemented" → moot until feature lands |
| 1g Export | E75–E77 | — | E75→RF-01/RF-03 (tool content via files/by-file, not `--include-tools`) | E76/E77→EX-07 (C72/C73) |
| 1h Resume / context | E78–E84 | — | — | E78–E84→RF-04 probe (C76–C79); E80 subagent-trap = latent guard (EX-03 MessageRole) |
| 1i Robot / integration | E85–E93 | E85→RB-04/05 (stream contract); E91→RB-09 (bare invocation) | E87→RB-03 (crate version present; api/contract triple absent); E89→RB-04 (ResponseMeta ≠ `_meta` shape) | E86, E88 (suite IS the consumer contract), E90, E92, E93 |

**MUST-tier sample — 18 concrete E-ID → TC mappings** (adapted variants marked `a` per catalog convention):

| E-ID (tier) | Contract (cass) | Terraphim counterpart | TC (skeleton area code — legend below) | Mode |
|---|---|---|---|---|

> **Skeleton-code legend (E-table only):** SR→TC-SEARCH/IMPORT · IX→TC-WATCH-INDEX · MS→TC-ENRICH · SO→TC-SOURCES · AS→TC-TIMELINE-STATS · EX→TC-EXPORT-SHOW · RB/CF→TC-ROBOT-CLI · HD→TC-SOURCES · RG→TC-PERF. Per-C-ID authoritative mappings: `traceability.csv`.

| E01 (MUST) | Search envelope `hits[]` keys | T19 CLI JSON envelope | SR-06 | ACTIVE |
| E03 (MUST) | `--limit 0` never panics | T19 | SR-02+SR-03 | ACTIVE |
| E10 (MUST) | `line_number` = raw JSONL line | no line model in output | SR-06 schema pin | DIVERGENCE |
| E12 (MUST) | tool stdout/stderr NOT indexed | tool_result IS indexed | SR-17 | DIVERGENCE |
| E14 (MUST) | `total_matches` = corpus count, hits = page | T19 total>shown | SR-08 | ACTIVE |
| E16 (MUST) | default lexical == explicit lexical | no mode flag; fixed BM25 | MS-05 determinism | ADAPTED |
| E17/E64 (MUST) | silent lexical fallback when no model | thesaurus-absent degrade | SR-13 | ACTIVE |
| E18 (MUST) | query special chars safe | REPL joined / CLI positional | SR-20 | ACTIVE |
| E21 (MUST) | stdout data only; stderr diagnostics | T19/T02 | RB-05 | ACTIVE |
| E22 (MUST) | exit-code contract | 0–7 enum (not cass 0–15+20–24) | RB-01 | ACTIVE (partial) |
| E30 (MUST) | new session searchable after index pass | auto-import analog | IX-12 | ACTIVE |
| E39 (MUST) | `.rebuild` always present in status | status-only quirk | IX-04 | DIVERGENCE |
| E41+E46 (MUST) | doctor never deletes source files | no write path at all | CF-06 zero-write guardrail | ACTIVE (vacuous+asserted) |
| E53 (MUST) | harness exclusion semantics | compile-time feature registry | CF-03+SO-01 | DIVERGENCE |
| E75 (MUST) | export tool-call visibility | files/by-file surface | RF-01+RF-03 | DIVERGENCE |
| E79 (MUST) | per-harness path detection | T04 detectors, compiled set only | SO-01..SO-06 | ADAPTED (partial) |
| E85 (MUST) | robot stream contract | ResponseMeta + stderr split | RB-04+RB-05 | ACTIVE |
| E87 (MUST) | introspect self-description | robot capabilities/schemas/examples | RB-03 | ACTIVE (partial) |

E91 (MUST)→RB-09 and E80 (MUST)→RF-04 latent-guard complete the MUST set; every MUST-tier E-ID is dispositioned (§7.3 criterion 8).

---

## 7.3 Acceptance Criteria — Definition of Done

From subagent_06 §5 + applied review fixes. Checklist state at ch7 writing time:

- [x] **1. Every FULL/PARTIAL row → ≥1 automated TC or documented manual probe.** 34/34 PARTIAL rows mapped (FULL = 0); reconciled to authoritative TC IDs 2026-09-03. Probes count where automation is impossible (IX-07, SO-07).
- [x] **2. Every N-A row: written justification (+ absence-probe where command-visible).** 18/18 justified (subagent_07 check 4 PASS); family probes HD-07/HD-08/SO-08/MS-04/AS-05/EX-07/RF-04/CF-03 attached per §7.1.
- [x] **3. Every UNCLEAR row: runtime-probe TC or OPEN+owner.** C24→IX-08 (nightly watcher lane), C44→RB-03 version probe. [DRIFT]-tagged 05d rows (C87/C91/C92/C97/C99) stay verdict-final with CI crate pin.
- [x] **4. 100% C-ID disposition.** Ledger above + CSV = 100/100 (subagent_07 check 1 PASS: no dupes/missing/out-of-range). T-ID resolution: 36/36 referenced T-IDs resolve; T08/T10/T11/T12/T24/T27/T28 enter via ch5/ch6 xref TCs (IX-14, IX-16..18, MS-06, MS-10) — assembler verifies each is referenced ≥1×.
- [x] **5. E-adoption complete for FULL/PARTIAL rows.** §7.2: all MUST-tier adopted or divergence-pinned; SHOULD/NICE record-only with row refs (subagent_06 §4.13).
- [x] **6. ≥1 test per connector format + one negative per format.** IX-15..19 golden + malformed/empty negatives; codex/cline/opencode absence negatives in binary lane; SR-17 tool-output divergence pin.
- [ ] **7. CI lanes added** (pending repo PRs, owners: terraphim team):
  - `cargo nextest run -p terraphim_sessions --all-features` (fixes 43/99 invisible tests) **+** default-features lane (substring-fallback path).
  - terraphim-agents CI runs `cargo test --workspace` **including `tests/`** integration (Lane B/C e2e).
  - Nightly: `-- --ignored` watcher test; `cargo bench -p terraphim_sessions --features search-index` (RG-04); optional `[patch]`-canary drift job (R1 Option C).
  - Lane D (cass differential): manual/weekly only, `RUN_CASS_DIFF=1`, read-only allowlist, PATH-preserving invocation (review fix 3).
- [x] **8. Quality gates specified:** no test touches paths outside temp roots (CF-01/CF-06 preflight); flake budget <1% (fixed 2026 timestamps, TZ=UTC, `gtimeout` + fallback probe — review fix 6); full suite ≤10 min excluding nightly.
- [x] **9. No test touches real user data (guardrail preflight).** HOME-only isolation (review fix 1: dirs 5.0.1 on macOS reads NO XDG vars); dirs-mirroring fixture generator mandatory; zero-write guardrail CF-06 asserts the temp tree is the only mutated surface.
- [x] **10. Membership-assert rule (D-13) honored.** Connector/capability/schema-count assertions are membership asserts, never exact-set — the cargo-test binary gains repl-full features via the self dev-dep unification.
- [x] **11. Flag-order rule honored.** `--robot`/`--format` **precede** the subcommand in every Lane C template (not clap-global): `terraphim-agent --robot sessions search "q"`.
- [x] **12. Exit-4 collision rule honored.** Terraphim exit 4 (empty search, machine mode) numerically collides with cass exit 4 (network error): tests assert payload/behavior, never bare code, in any cross-referenced assertion (C06/C30/C41/C80/C88).

---

## 7.4 Consolidated Risks & Open Decisions

| # | Risk / decision | Status | Disposition |
|---|---|---|---|
| R1 | Registry 1.20.4 vs local 1.21.3 — what the suite validates | **DECIDED (veto-able)** | CI stays on the registry build (matches production); nightly `[patch]`-canary lane against local 1.21.3 catches drift (D-1 Option C). Stated as an assumption at delivery; Alex may veto. Binary-lane tests never assert file:line-exact behavior. |
| D-2 | 1.20.x drift (incremental flag, sessions.json writer, TSA ids) | Open (managed) | RG-05 snapshot diff across bumps; membership-not-counts where TSA involved. |
| D-3 | TSA internals UNCLEAR (no local source) | Open | Black-box probes only (SO-07/IX-10); affected rows stay UNCLEAR until probed. |
| D-4 | Server-mode IX-07 requires server process | Accepted | Stays `probe`, excluded from fast CI. |
| D-5 | Time-based flakiness (timeline dates, watcher debounce, staleness) | Managed | Fixed 2026 timestamps, TZ=UTC, watcher nightly-only, `gtimeout`/exit-124 convention. |
| D-6 | Destructive-op guardrails | Mandatory | CF-01/CF-06 preflight + zero-write asserts; Lane D read-only allowlist. |
| D-7 | aider CWD dependence | Managed | Dedicated cwd fixture root (SO-04); harness always sets cwd. |
| D-8 | cline/opencode/codex absent from binary-under-test | Managed | Split lanes: binary absence asserts + feature-gated crate tests (SO-05/06, IX-18/19). |
| D-9 | REPL output rendering volatility | Managed | Substring/regex asserts only (Lane B). |
| D-10 | sessions.json "may-exist" input (T38 read-never-written) | Managed | Planted-file tests assert read path; no writer assertions beyond IX-05 pin. |
| D-11 | Duplicate import (native+TSA) inflates counts | Managed | Membership/relative asserts on mixed corpora; counts only on single-source fixtures. |
| D-12 | REPL quit token unknown | Managed | EOF-termination default + timeout wrapper (RB-00 probe). |
| D-13 | Cargo-test binary gains repl-full features (self dev-dep) — **new, from subagent_08** | Managed | Membership asserts everywhere; no exact connector/command-set equality (feeds §7.3-10). |
| — | macOS/XDG false-pass class | **FIXED BY DESIGN** | HOME-only isolation everywhere; dirs-mirroring fixture generator; XDG assertions confined to Linux-only lanes. The silent-false-pass failure mode (cline tree undiscoverable, opencode SQLite lane) cannot recur. |
| — | Exit-4 collision (terraphim empty-results vs cass network-error) | Managed | Payload/behavior asserts only (§7.3-12); C88 framing kept consumer-contract-scoped. |
| — | Drift 1.20.4 vs 1.21.3 (parser set, connector features, enrichment internals) | Managed | CI crate pin + R1 canary; [DRIFT] rows C87/C91/C92/C97/C99 carry explicit notes. |
| — | **Doc-drift backlog (fix-or-implement decisions for the terraphim team):** (1) robot capabilities advertise `supported_formats [json,jsonl,minimal,table]` vs CLI `OutputFormat = Human|Json|JsonCompact` → DOCS-DRIFT test pins it; decide docs-fix vs enum-alignment. (2) help text omits `index` (RB-07 pin) → fix help or accept pin. (3) `/sessions show` 8-char id prefix from tables does not resolve (EX-01 negative) → implement prefix resolution or correct docs. (4) CLAUDE_SESSIONS_DIR documented-but-unimplemented (SO-09 negative pin) → implement the env or fix the skill docs. | Open (backlog) | Each item lands as a pinned test now; fix-vs-implement decided by owners at assembly. |
| ⚠ | **C91 vs TC-SO-09 conflict:** parity row C91 assumes CLAUDE_SESSIONS_DIR *honored*; skeleton TC-SO-09 pins it *no-effect* (documented-but-unimplemented). | Open — ch6 writer | Runtime probe settles; disposition stays TEST either way (pin whichever behavior holds); CSV note flags the row. |
| ✅ | Reconciler pass 2026-09-03 | Assembly note | Matrix + CSV now carry authoritative TC IDs (81 joined by C-ID, C17 routed by ch5's explicit cross-ref, C91 fixed per fact-check). 1 residual: none — all 82 actionable rows mapped. |

---

### Return-format summary

- **Conclusion:** 100/100 C-rows dispositioned (66 TEST / 14 GAP-DEFERRED / 18 N-A / 2 UNCLEAR); E01–E93 (minus E58) adopted across 9 groups with 18 MUST-tier sample mappings; DoD checklist 11/12 checked (CI lanes = the open item); risks consolidated incl. decided-but-veto-able R1 and new D-13.
- **Evidence:** subagent_05a–d rows (all 100 via `rg '^\| C'`), subagent_04 §1/§2 headers + E-ranges, subagent_06 §4/§4.12/§4.13/§5/§6 + REVIEW CORRECTIONS, review.md (3 reviewer verdicts, all fixes applied), subagent_07 verdict line, subagent_08 §3 verdict.
- **Gaps:** resolved 2026-09-03 — all TC refs authoritative; C91-vs-SO-09 conflict RESOLVED (CLAUDE_SESSIONS_DIR not implemented → probe + DOCS-DRIFT, TC-ROBOT-CLI-15 + TC-DOCS-DRIFT-02); T-ID ≥1-reference completeness now verifiable in traceability.csv.
- **Notes:** CSV is comma-free (no quoting required); verdict vocabulary normalized per review; exit-4 and membership/flag-order rules embedded as acceptance criteria so chapter writers inherit them.


---

## Appendix A — Machine-Readable Traceability

`traceability.csv` (companion file, same directory, delivered as `session-search-traceability.csv`): 100 rows, columns `c_id,verdict,priority,disposition,tc_ids,notes`. Reconciliation status 2026-09-03: 81 rows joined by C-ID from the authoritative TC catalogs, C17 routed via ch5's explicit cross-reference, C91 resolved per fact-check findings (probe + DOCS-DRIFT), 18 N-A rows justified. No unresolved TBD rows.
