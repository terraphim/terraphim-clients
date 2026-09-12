# terraphim-agent memory benchmark

Judge-free measurement of `terraphim-agent memory` retrieval on a committed
fixture: retrieval quality (recall@1, recall@5, MRR), retrieval latency at
100, 1,000 and 10,000 items, and the bytes the memory hook would inject per
query. Every number below was produced by the commands quoted next to it, on
the machine and inputs named in the "Environment" and "Inputs" sections, with
no language model anywhere on the path. Step 5 of terraphim-clients#255
(issue #263); the pipeline is steps 1 to 4 (PRs #277, #279, #282, #273).

## Read this first

The published agent-memory leaderboards (mem0, Zep, ZeroMemory, ByteRover,
Dakera, memU) score **LLM-judged question answering over multi-session chat**
(LoCoMo, LongMemEval, BEAM). `terraphim-agent memory` stores failed commands,
corrections and lessons and ranks them by knowledge-graph concept overlap with
no LLM in the loop. The two are **not commensurable**. Nothing in this
document is a LoCoMo, LongMemEval or BEAM score, and the comparison table
below keeps the peer figures and the Terraphim figures in separate columns so
they cannot be read against each other. Treat every LoCoMo figure as
contested: the same system (Zep) has been reported at 84, 58.44 and 75.14
depending on who ran it, and Penfield Labs' April 2026 audit found 6.4 percent
of the LoCoMo answer key wrong and a gpt-4o-mini judge accepting 62.81 percent
of deliberately wrong answers.

There is deliberately **no single composite "Terraphim score"**. The three
metrics are reported separately, each with the inputs that produced it.

## What is measured

| Metric | Definition | Produced by |
|---|---|---|
| recall@k | per query, the number of expected ids among the top k hits divided by the number of expected ids, mean over queries; k = 1 and 5 | `memory_bench::evaluate` through the unchanged `memory_retrieve::retrieve` with `limit = 5` |
| MRR | per query, `1 / rank` of the first expected id in the top five, else 0; mean over queries | same |
| latency p50, p95 | nearest-rank percentiles over 35 timed `retrieve` calls (7 queries x 5 calls) per corpus size; Criterion mean alongside | `benches/memory_retrieve.rs` |
| injected bytes, estimated tokens | bytes of the text the memory hook would inject for a prompt (top five hits, prompt excluded); tokens = bytes / 4 rounded up, an estimate not a tokeniser result | `memory apply --format json`, via `memory_bench::injected_size` |

Ranking was not changed by any of the steps that produced these numbers.

## Environment

| Item | Value |
|---|---|
| Machine | Apple M3 Pro (`sysctl -n machdep.cpu.brand_string`), 38,654,705,664 bytes RAM (`sysctl -n hw.memsize`, 36 GiB) |
| OS | macOS 26.6.2, build 25G83 (`sw_vers`) |
| rustc | 1.97.1 (8bab26f4f 2026-07-14) |
| cargo | 1.97.1 (c980f4866 2026-06-30) |
| terraphim-agent | 1.21.14 (`terraphim-agent --version`; workspace version in `Cargo.toml`) |
| Source | branch `task/263-benchmark-doc`: `task/261-latency-bench` at `a176a1c` (which contains `task/260-memory-bench` at `02c31d5` and `task/259-memory-fixture` at `5c62133`) with `task/262-rubric-scorer` at `f3cbbdf` merged in |
| Build profile, quality test and apply run | `test` and `dev` profiles (unoptimised, debuginfo) |
| Build profile, latency bench | `bench` profile, which inherits `[profile.release]`: `opt-level = 3`, `lto = false`, `codegen-units = 1`, `panic = "unwind"` |
| Date | 2026-09-12 |

## Inputs

All three inputs are committed under
`crates/terraphim_agent/tests/fixtures/memory_bench/` and described in the
README there. The hashes below were recomputed for this document, not copied.

| Input | Records | SHA-256 |
|---|---|---|
| `corpus.jsonl` | 60 `MemoryItem` records (3 corrections, 57 repeated-failure clusters) | `ea9057b2a807adf8d7602a6dc13104d83bbfff5c94eca45036745730d699214e` |
| `queries.jsonl` | 50 `{query, expected_ids}` records | `fccdba5388bddf5fc3b01ad2f925ffe2d429a684e2b17b47da961eb385227e36` |
| `thesaurus.json` | Terraphim Engineer, 42 entries, 15 concepts | `4009a027a880322504498785e6b588046f8c1fbf815211699662b602f8f7a8fe` |
| KG source (`crates/terraphim_agent/docs/src/kg`, 15 markdown files) | the directory `thesaurus.json` was generated from | `8233465025c9bf9d6469c526ec464fe18e3f65aa7d6c51cb578256a06d5586d8` |

```sh
cd crates/terraphim_agent
shasum -a 256 tests/fixtures/memory_bench/corpus.jsonl \
              tests/fixtures/memory_bench/queries.jsonl \
              tests/fixtures/memory_bench/thesaurus.json
# KG source hash: per-file shasum with the ./ prefix, sorted by path, hashed again.
(cd docs/src/kg && fd -e md . -0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256)
```

The corpus hash is asserted by `tests/memory_fixture_integrity.rs`; the
corpus and thesaurus hashes are also asserted against this document by
`tests/memory_benchmark_doc.rs`. The corpus is built mechanically from private
capture files by `scripts/build_memory_fixture.sh` and redacted structurally;
ground truth is mechanical (a correction's original text maps to that
correction; a repeated command maps to its earliest capture). Nothing was
hand-labelled.

## Retrieval quality

```sh
cargo test -p terraphim_agent --test memory_retrieval_quality
cat target/memory-benchmark/report.json
```

`report.json` as written by that run:

| Field | Value |
|---|---|
| corpus_size | 60 |
| query_count | 50 |
| recall@1 | 0.02 |
| recall@5 | 0.04 |
| MRR | 0.03 |
| corpus_sha256 | `ea9057b2a807adf8d7602a6dc13104d83bbfff5c94eca45036745730d699214e` |
| thesaurus_sha256 | `4009a027a880322504498785e6b588046f8c1fbf815211699662b602f8f7a8fe` |
| terraphim_agent_version | 1.21.14 |

<!-- floor:recall_at_5=0.04 -->

The recall@5 value is the floor recorded in
`tests/fixtures/memory_bench/floor.json` (recorded 2026-09-12 on 1.21.14,
written by hand from the first real run, never by the test). The floor test
fails if a later run scores below it, and `tests/memory_benchmark_doc.rs`
fails if the value quoted in this document ever differs from `floor.json`.
The test run is deterministic: two evaluations of the committed inputs produce
byte-identical reports (`retrieval_quality_is_deterministic_on_committed_fixture`).

### Why the numbers are low, and why they are left alone

The rolegraph only indexes a document that matches **two or more** thesaurus
concepts: `RoleGraph::insert_document` builds co-occurrence edges between
consecutive concept matches, so an item matching a single concept produces no
edge and is unreachable. This is documented at
`crates/terraphim_agent/src/memory_retrieve.rs:88` and is the published
`terraphim_rolegraph` behaviour, not something introduced by the benchmark.

Concept-match histogram over the committed inputs (from PR #279, computed
with `terraphim_automata::find_matches` over each item's content and each
query text against the committed thesaurus):

| Corpus items (60) | Count |
|---|---|
| match zero concepts | 41 |
| match exactly one concept | 11 |
| match two or more concepts (reachable) | 8 |

| Queries (50) | Count |
|---|---|
| match no concept (retrieval returns nothing by design) | 43 |
| match at least one concept | 7 |

At most 8 of 60 items can be retrieved at all with this thesaurus, and 43 of
50 queries name no concept, so recall@5 of 0.04 is the honest baseline of the
existing ranking on shell-command learnings with a 15-concept knowledge
graph. Raising the threshold is a #253 step 3 candidate and is out of scope
for the measurement work (#255 acceptance bullet 3: ranking unchanged).

## Retrieval latency

```sh
uptime
cargo bench -p terraphim_agent --bench memory_retrieve
uptime
```

Corpus: the 60 committed items tiled to 100, 1,000 and 10,000 items with
unique id suffixes and unchanged content. Queries: the 7 fixture queries that
name at least one thesaurus concept. One measurement is one `retrieve` call
with `limit = 5`. The custom summary reports nearest-rank p50 and p95 over 35
calls per size (7 queries x 5 calls); Criterion's own estimate follows it.
Criterion runs 100 samples at 100 items and 10 samples (its minimum) with a
20 s measurement window at 1,000 and 10,000 items, because every `retrieve`
rebuilds a `RoleGraph` over all items.

Run quoted here: load average 9.20 (1 min) before, 5.56 after, on the machine
above with other cargo builds running in parallel on the host.

| Items | p50 | p95 | max (35 calls) | Criterion mean (95 percent CI) | Design target p95 | Met |
|---|---|---|---|---|---|---|
| 100 | 0.604 ms | 0.632 ms | 0.645 ms | 591.75 us (590.01 to 593.60 us) | none | n/a |
| 1,000 | 3.476 ms | 4.138 ms | 4.156 ms | 3.4652 ms (3.4421 to 3.4971 ms) | under 100 ms | yes |
| 10,000 | 127.916 ms | 199.739 ms | 199.796 ms | 128.90 ms (128.13 to 129.65 ms) | under 1 s | yes |

**Numbers vary between runs.** A run a minute earlier on the same build gave
p50/p95 of 0.614/0.742 ms, 3.585/4.129 ms and 130.663/197.164 ms; PR #282
recorded 0.609/0.640 ms, 3.611/4.564 ms and 134.5/202.4 ms on the quietest
of three runs, and p95 at 10,000 items of 430 ms to 578 ms under a host load
of 15 to 19. Both design targets were met in every run. The 10,000-item p95
sits well above the Criterion mean because the 35-call sample includes the
cold first calls per query; Criterion warms up for 3 s first.

## Injected bytes and estimated tokens per query

```sh
cargo build -p terraphim_agent --bin terraphim-agent
scripts/memory_apply_fixture_queries.py
```

The script runs the real binary against a hermetic `HOME` under a temporary
directory: it creates the evolution store through the real `memory capture`,
replaces the store's `short_term` bucket with the 60 fixture items, and runs
`terraphim-agent --format json memory apply --role "Terraphim Engineer"
--prompt <query>` for each of the 50 fixture queries. The role config is the
committed `tests/fixtures/terraphim_engineer_config.json` with its knowledge
graph pointed at `crates/terraphim_agent/docs/src/kg`, the directory the
committed `thesaurus.json` was generated from. The test-suite hermetic
environment (`tests/support/cli_test_env.rs`) points the knowledge graph at
`tests/test_kg` instead and would not reproduce these numbers. Equivalence
with the benchmark thesaurus was checked by comparing all 7 concept-matching
queries against the bench's own injected-size summary: the same 5 queries
inject 11,056 bytes and the same 2 inject 0 in both.

| Population | Queries | Mean bytes | Max bytes | Mean estimated tokens | Max estimated tokens |
|---|---|---|---|---|---|
| all fixture queries | 50 | 1,105.6 | 11,056 | 276.4 | 2,764 |
| queries that retrieved anything | 5 | 11,056.0 | 11,056 | 2,764.0 | 2,764 |

45 of the 50 queries retrieve nothing and inject 0 bytes (43 name no thesaurus
concept; 2 name a concept but no reachable item carries it). Each of the 5
non-zero queries retrieved 5 items totalling exactly 11,056 bytes; only the
sizes were compared, not the item ids. The estimated token figure is bytes
divided by four, rounded up, and is labelled as an estimate in the JSON
(`estimated_tokens`) and in `memory apply --help`; no tokeniser is run.

## Rubric scorer label

The six-dimension memory rubric is heuristic (content length, tag count, item
type, age, keyword hits), not the judge-driven scorer specified in the memory
lifecycle feature request. Since PR #273 the binary says so. As printed by
`terraphim-agent 1.21.14` built from this branch against the 60-item store
above:

```sh
terraphim-agent --format json memory rubric --project .
```

```json
{"status":"ok","action":"rubric","scorer":"heuristic-v1","scorer_note":"heuristic-v1 scores content length, tag count, item type, age and keyword hits; it is not the judge-driven scorer specified in the memory lifecycle feature request.","items_analysed":60}
```

(Fields other than these five are omitted above.) The markdown report from
`terraphim-agent memory rubric --project .` carries `**Scorer:** heuristic-v1`
and the same note, and `terraphim-agent memory --help` lists the subcommand as:

```text
rubric      Run the full Memory Reliability Rubric diagnostic on a project (scorer: heuristic-v1) (6 dimensions: faithfulness, scope, provenance, actionability, decay, risk, scored by heuristic-v1 over content length, tag count, item type, age and keyword hits; this is not the judge-driven scorer specified in the memory lifecycle feature request)
```

No rubric composite is reported in this document; it would be a heuristic
over a heuristic.

## Comparison table

Peer rows are reproduced from the comparison in the private research
notebook (`knowledge/2026-09-11-agent-memory-benchmark-comparison-mem0-terraphim.md`,
sources listed at the end). The "Reported by" column labels every figure in
the row: **self** means the vendor's own publication, **independent** means a
third party ran it, and mixed rows say which figure is which. The Terraphim
row is filled from the sections above. Its LoCoMo, LongMemEval and BEAM cells
are "not applicable" because there is no judge and no QA task; its retrieval
quality lives in the last column and is not a leaderboard score.

| System | LoCoMo | LongMemEval | BEAM | Tokens or bytes per retrieval | Latency | Reported by | What is being measured |
|---|---|---|---|---|---|---|---|
| mem0 (paper, Apr 2025) | 26 percent relative gain over OpenAI memory; graph variant about 2 percent higher; Zep's rerun puts Mem0 Graph at about 68 J | not reported | not reported | more than 90 percent fewer tokens than full-context | 91 percent lower p95 than full-context (self); p95 0.778 s (base) and 0.657 s (graph) as quoted by Zep from mem0's own report | Self (arXiv 2504.19413); Zep rerun (independent) for the 68 J figure | LLM-judged QA over about 26k-token chats |
| mem0 (Apr 2026 ADD-only algorithm) | 92.5 overall; single-hop 94.6, multi-hop 95.4, temporal 82.3 | 94.4 (one mem0 page says 93.4) | 64.1 (1M), 48.6 (10M) | about 6.9k tokens per query versus 25k-plus full-context | p50 at or under 1.1 s | Self, methodology published | LLM-judged QA |
| Zep / Graphiti | 84 (original claim, self); 58.44 (mem0's rerun, independent); 75.14 plus or minus 0.17 (Zep corrected, self); 94.7 (2026 claim, self) versus 75.1 (independent) | 71.2 with GPT-4o judge, consistent across sources | not reported | not reported | p95 search 0.632 s corrected (self) | Self and independent, per cell | Temporal KG, LLM-judged QA |
| ZeroMemory | 96.1 | not reported | not reported | not reported | not reported | Self, unverified | LLM-judged QA |
| ByteRover | 92.2 or 96.1 (conflicting publications) | 92.8 (LongMemEval-S) | not reported | not reported | not reported | Self | LLM-judged QA |
| Dakera | 88.2, no LLM reranking | not reported | not reported | not reported | not reported | Self | LLM-judged QA |
| memU | about 92 (from a March 2026 survey; unverified) | not reported | not reported | not reported | not reported | Second-hand (neither self nor independently verified) | LLM-judged QA |
| OpenViking (Volcengine) | LoCoMo10 task completion 35.65 percent to 52.08 percent for OpenClaw with OpenViking | not reported | not reported | input tokens 24.6M to 4.3M across the run | not measured | Vendor (self) | Task completion, not J-score |
| Headroom (vendored harness, local HNSW backend or mem0) | Harness computes Recall@k, MRR, Precision@k against LoCoMo evidence ids plus optional judge; no results recorded in the checkout | not run | not run | not measured | not measured | Nothing published | Retrieval recall, judge-free; the metric shape Terraphim adopts |
| Full-context baseline | about 73 J on LoCoMo (Zep's measurement) | LongMemEval-S fits in context; single-session categories 96 to 99 | not applicable | 25k-plus tokens per query | highest | Independent | The ceiling the benchmarks are supposed to beat |
| terraphim-agent memory 1.21.14 (this document) | not applicable (no judge, no QA task) | not applicable | not applicable | mean 1,105.6 bytes / 276.4 estimated tokens over 50 fixture queries, of which 45 inject 0; the 5 non-zero queries inject 11,056 bytes / 2,764 estimated tokens each (max) | p50/p95 0.604/0.632 ms at 100 items, 3.476/4.138 ms at 1,000, 127.9/199.7 ms at 10,000 (Apple M3 Pro, bench profile, varies between runs) | Self, pipeline fully disclosed in this document; not independently run | Rolegraph-ranked retrieval of captured learnings and corrections, judge-free: recall@1 0.02, recall@5 0.04, MRR 0.03 on 60 items and 50 mechanical queries; heuristic-v1 rubric |

## Checklist against Penfield Labs' six requirements

Penfield Labs' LoCoMo audit (2026-04-08) lists six requirements for a
trustworthy memory benchmark. Four of them presume an LLM-judged QA benchmark.
For each, whether it applies to this judge-free retrieval benchmark and
whether it is met.

| # | Requirement | Applies here | Met | Notes |
|---|---|---|---|---|
| 1 | Corpus larger than the context window | Yes, in spirit: a memory that is only tested on what fits in context proves little | No | 60 items, 11,056 bytes for a full five-hit injection, fits in any current context window. The 10,000-item tiling is for latency only; its content is the same 60 items repeated and it carries no ground truth. |
| 2 | Current-generation models for the system under test and the judge | No | n/a | There is no answer model and no judge. The system under test is the rolegraph ranking in `terraphim_rolegraph`; its version is pinned by `Cargo.lock`. |
| 3 | Adversarially tested judge | No | n/a | No judge. The scoring is set arithmetic over ids; there is nothing to fool. |
| 4 | Realistic multi-turn ingestion | Partly: the corpus should be real usage, not synthetic | Partly | Items are real captured failures and corrections, redacted structurally, not synthetic conversations. They are single failing commands, not multi-turn chat, so the multi-turn part does not apply and is not claimed. |
| 5 | Fully disclosed pipeline | Yes | Yes | This document: machine, build profile, input hashes, every command, the fixture build and redaction rules in the fixture README, and the reason the numbers are low. |
| 6 | Verified ground truth with an error ceiling (3.3 percent cited) | Yes | No | Ground truth is mechanical, not verified. The fixture README lists 6 of 50 queries (12 percent) as test artefacts or chain fragments, above the cited ceiling, and does not filter them because that would be a hand judgement of relevance. |

## Known gaps

- **Two-concept reachability threshold.** An item is indexed only if it
  matches two or more concepts (`crates/terraphim_agent/src/memory_retrieve.rs:88`;
  `RoleGraph::insert_document` in the published `terraphim_rolegraph`). This
  bounds recall at 8 of 60 items on the committed inputs. Candidate for #253
  step 3; not changed by the measurement work.
- **`memory capture` hard-codes Medium importance** (#274), so High and
  Critical items cannot be created from the CLI; the rubric CLI tests route a
  Critical item through the real `MemoryState::add_memory` instead.
- **High and Critical visibility.** PR #273 makes `rubric`, `validate`,
  `export`, `list` and `show` read both retention buckets (#207) through
  `collect_memory_items`; the proper `MemoryState::iter_all()` accessor is
  #208 and each call site carries a `TODO(#208)`.
- **`memory second-run` has nothing to read.** It is a reader of `RunMetrics`
  that nothing writes; emission from the ADF runner is
  terraphim/terraphim-ai#3373 (Gitea).
- **Rubric is heuristic.** `heuristic-v1` scores string length, tag count,
  item type, age and keyword hits. A judge-driven scorer is out of scope for
  #255.
- **Corpus is small and the fixture queries are mostly outside the knowledge
  graph** (43 of 50). A larger reviewed fixture and a thesaurus that covers
  shell-command vocabulary would move the numbers; both would be new work and
  a new floor, recorded the same way.

## Reproduce everything

From the repository root, on the branch named in "Environment":

```sh
# 1. Inputs
cd crates/terraphim_agent && shasum -a 256 tests/fixtures/memory_bench/*.json* && cd ../..
# 2. Retrieval quality (writes target/memory-benchmark/report.json)
cargo test -p terraphim_agent --test memory_retrieval_quality
# 3. Latency (custom p50/p95 summary first, then Criterion)
cargo bench -p terraphim_agent --bench memory_retrieve
# 4. Injected bytes and estimated tokens over the 50 fixture queries
cargo build -p terraphim_agent --bin terraphim-agent
scripts/memory_apply_fixture_queries.py
# 5. Rubric scorer label
target/debug/terraphim-agent memory rubric --help
# 6. The floor quoted in this document equals floor.json
cargo test -p terraphim_agent --test memory_benchmark_doc
```

## Sources

- https://arxiv.org/abs/2504.19413 (mem0 paper)
- https://mem0.ai/research and https://mem0.ai/blog/ai-memory-benchmarks-in-2026 (mem0 self-reports and leaderboard)
- https://blog.getzep.com/lies-damn-lies-statistics-is-mem0-really-sota-in-agent-memory/ and https://github.com/getzep/zep-papers/issues/5 (Zep rebuttal and correction)
- https://penfieldlabs.substack.com/p/we-audited-locomo-64-of-the-answer (LoCoMo audit and the six requirements)
- https://github.com/volcengine/OpenViking (OpenViking numbers)
- https://arxiv.org/abs/2602.02474 (MemSkill)
- terraphim-clients: `crates/terraphim_agent/src/memory_bench.rs`, `memory_retrieve.rs`, `memory_command.rs`, `benches/memory_retrieve.rs`, `tests/memory_retrieval_quality.rs`, `tests/fixtures/memory_bench/README.md`; issues #255, #259, #260, #261, #262, #263, #207, #208, #253, #274; PRs #277, #279, #282, #273
