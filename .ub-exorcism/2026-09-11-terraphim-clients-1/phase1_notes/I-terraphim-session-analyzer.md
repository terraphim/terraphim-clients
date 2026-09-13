# Module: I. terraphim-session-analyzer

Crate root: `crates/terraphim-session-analyzer` (library `terraphim_session_analyzer`, binary `tsa` at `src/main.rs`).
Mapped 2026-09-11 by p1-I. Source paths below are relative to the crate root unless stated otherwise.

Line budget (wc -l): `src/` 12 965 lines across 20 files; `tests/` 3 219 lines across 5 files; total 16 184 lines of Rust (the "about 15k" estimate holds).

Features: `default = []`; `terraphim` (terraphim_automata, terraphim_types, terraphim_config); `connectors` (rusqlite bundled); `full = both`. `libc = "0.2"` is declared under `[target.'cfg(unix)'.dependencies]` (Cargo.toml:83-84).

## Purpose

Parses AI coding-assistant session logs (Claude Code JSONL first; Cursor, Aider, Codex, OpenCode via the optional `connectors` feature), extracts agent invocations, file operations and Bash tool invocations, classifies tools with an Aho-Corasick pattern matcher (or the terraphim thesaurus matcher when `terraphim` is on), aggregates statistics with rayon, and renders reports (terminal, JSON, CSV, HTML via handlebars). Optional knowledge-graph query/search (`src/kg/`) with a small boolean query parser.

## Unsafe-surface tally

| Category | Count | Evidence |
|---|---|---|
| `unsafe` blocks / fns / impls / traits in source | 0 | `rg -n 'unsafe\|SAFETY' src tests examples` returns nothing |
| `SAFETY:` comments | 0 | same sweep |
| `extern "C"` / FFI call sites | 0 | `rg -n 'libc::\|extern "C"' src tests examples` returns nothing; the `libc` dependency (Cargo.toml:83-84, `cfg(unix)`) has zero call sites in this crate |
| Raw pointers, `from_utf8_unchecked`, `get_unchecked`, `static mut`, `transmute` | 0 | targeted sweep returns nothing |
| `std::env::set_var` / `remove_var` | 0 | only `env::var("APPDATA")` read at `src/connectors/cursor.rs:85` (feature `connectors`, Windows path) and `env!("CARGO_*")` compile-time macros (`src/lib.rs:64`, `tests/integration_tests.rs:19,707-746`, `tests/filename_target_filtering_tests.rs:22,558-710`) |
| Unsafe in macro expansion | see "Expansion" below | |

Archetype: pure-safe. No unsafe of any kind in the hand-written source of this crate.

### Expansion (`cargo +nightly expand -p terraphim-session-analyzer --lib`)

Command run from the workspace root with output to `/private/tmp/claude-501/ubx-expand/terraphim-session-analyzer.rs`. Default features only, so `main.rs`, the `connectors` (rusqlite) modules and the `terraphim`-gated code are NOT in this expansion.

Result: SKIPPED by lead. The job was started at 23:04 BST, blocked on the shared build-directory lock behind other mappers' expansions, and was killed at the lead's instruction at 23:12 BST; `/private/tmp/claude-501/ubx-expand/terraphim-session-analyzer.rs` is empty. Lead's note: lib expansions elsewhere in this workspace show only std `vec!` and derive marker impls, so no `unsafe` is expected from the derive macros in use here (serde `Serialize`/`Deserialize`, `clap::Parser`/`Subcommand`, `tabled::Tabled`, `thiserror::Error`).

## Concurrency surface

| Site | Kind | Shared state | Notes |
|---|---|---|---|
| `src/parser.rs:177` | `self.entries.par_iter().filter_map(..).collect()` | none; closure captures `&self` (`SessionParser`) read-only, calls `self.parse_task_invocation` | `extract_agent_invocations` |
| `src/parser.rs:242` | `self.entries.par_iter()` | none | `extract_file_operations` |
| `src/parser.rs:284` | `self.entries.par_iter()` capturing `&dyn PatternMatcher` and `&self.session_id` | none | `extract_tool_invocations`; `pub trait PatternMatcher: Send + Sync` (`src/patterns/matcher.rs:19`), so the trait object is `Sync` by supertrait bound, compiler-enforced |
| `src/analyzer.rs:72` | `self.parsers.par_iter().filter_map(..).collect::<Result<Vec<_>>>()` | none; `analyze_session(&self, ..)` is `&self` | per-session analysis fans out over rayon; results collected, then sorted deterministically afterwards |

Absent: `Arc`, `Mutex`, `RwLock`, atomics, `OnceLock`, `LazyLock`, `lazy_static!` (dependency declared, macro never invoked), `thread::spawn`, `tokio::spawn`, `thread_local!`, `static mut`. Both matchers (`AhoCorasickMatcher`, `TerraphimMatcher`) are built once via `&mut self` in `initialize` and then only read through `&self` inside `find_matches`. rayon is the only concurrency primitive and every closure is a pure map/filter over immutable input. No loom or Miri-relevant data-race surface in the crate's own code.

Non-deterministic ordering from rayon `collect()` is not an issue: `Vec` collection from an indexed `par_iter` preserves order, and downstream code re-sorts by timestamp anyway (`src/parser.rs:443`, `src/analyzer.rs:105,207`).

## Library-trait invariant surface

Checklist, each item stated even when empty:

1. **Manual `impl Hash / PartialEq / Eq / Ord / PartialOrd / Borrow / Iterator / ExactSizeIterator / DoubleEndedIterator for`**: none. `rg -n 'impl\s*(<[^>]*>)?\s*(Hash|PartialEq|Eq|Ord|PartialOrd|Borrow|Iterator|ExactSizeIterator|DoubleEndedIterator)(<[^>]*>)?\s+for' src tests examples` returns nothing.
2. **`derive(Hash)` paired with manual `PartialEq`**: none. Every `derive(Hash)` also derives `PartialEq, Eq`:
   - `src/models.rs:9` `SessionId(String)`
   - `src/models.rs:42` `AgentType(String)`
   - `src/models.rs:75` `MessageId(String)`
   - `src/models.rs:238` `enum ToolCategory { .., Other(String) }`
   Hash/Eq consistency therefore follows from `String`'s derived impls.
3. **Map/set/heap keys whose `Ord`/`Hash` could be non-total**: none. `BinaryHeap`, `BTreeMap`, `BTreeSet` are not used anywhere in the crate. Key types in use are all `String`, `&str`, `usize`, `(String, String)`, `Vec<String>` and `ToolCategory`:
   - `HashMap<usize, ToolInfo>` `src/patterns/matcher.rs:67`
   - `HashMap<String, (String, f32)>` `src/patterns/matcher.rs:168` (f32 is a value, not a key)
   - `HashMap<(String, String), ..>` `src/analyzer.rs:573`, `src/reporter.rs:615`, `src/main.rs:665`, `src/patterns/knowledge_graph.rs:669`
   - `HashMap<Vec<String>, SequenceData>` `src/analyzer.rs:775`
   - `IndexMap<ToolCategory, u32>` `src/analyzer.rs:726-738`, `src/models.rs:377`
   - `HashSet<String>` / `HashSet<&str>` throughout (`src/analyzer.rs:118,331,338,443,536,896-909`, `src/lib.rs:135`, `src/connectors/cursor.rs:152-224`)
   No `f32`/`f64` appears in any key position. `total_cmp` is unused.
4. **`sort_by_key` comparators**: all on totally ordered keys: `u32`, `Reverse<u32>`, `String`, `jiff::Timestamp` (total `Ord`), `(usize, usize)`:
   `src/reporter.rs:168,227,446,570,763,784`; `src/parser.rs:443`; `src/analyzer.rs:105,207,561,634,783,878`; `src/main.rs:669,731,964-966`; `src/kg/search.rs:230`.
5. **`sort_by` with a hand-written comparator**:
   - `src/reporter.rs:548-553` compares `b.count.parse::<u32>().unwrap_or(0)` against the same for `a`: the count field is a `String` that was formatted from a `u32` a few lines earlier; consistent total order, just round-tripped through text.
   - `src/analyzer.rs:716` `v2.total_invocations.cmp(&v1.total_invocations)` (u32), and `:738` `v2.cmp(v1)` (u32): total.
   - `src/patterns/loader.rs:157` `a.name.cmp(&b.name)` (String): total.
   - `src/kg/search.rs:89-93` `b.relevance_score.partial_cmp(&a.relevance_score).unwrap_or(Ordering::Equal)` on `f32`.
   - `src/main.rs:864` `b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal)` on `f32` (sum of `relevance_score` over results, `src/main.rs:846`).
   The two `partial_cmp(..).unwrap_or(Equal)` sites are the only non-trivially-total comparators. They do not `unwrap()` so NaN cannot panic directly; instead NaN would compare `Equal` to every element, which is an inconsistent (non-transitive) order that `slice::sort_by` since Rust 1.81 may detect and panic on. Reachability: `relevance_score` is computed only by `calculate_relevance` at `src/kg/search.rs:246-261` as `concepts.len() as f32 + bonus` where `bonus` is a fold of literals `0.2 / 0.1 / 0.0`; both operands are finite and non-negative, so NaN and infinity are unreachable and the order is total in practice. `main.rs:846` sums finitely many such finite values (overflow to `inf` would need ~3.4e38 matches, not reachable). Verdict: NOT REACHABLE; noted as a pattern only.
6. **`max_by_key` / `min_by_key`**: `src/analyzer.rs:857`, `src/patterns/knowledge_graph.rs:262` (u32 counts), `src/main.rs:801` (Timestamp), `src/lib.rs:157` (usize). All total.
7. **`dedup_by`**: `src/kg/search.rs:231` on `(usize, usize)` after a `sort_by_key` on the same key: consistent.
8. **`f32` confidence values** (`src/models.rs:356`, `src/patterns/matcher.rs:55,74`, `src/patterns/knowledge_graph.rs:274-284,492,652,699`) are never sorted, hashed or used as keys. The loader validation `confidence < 0.0 || confidence > 1.0` (`src/patterns/loader.rs:83,165`) does not reject NaN (both comparisons are false for NaN, and TOML 1.0 accepts a `nan` literal), but because confidence is never compared for ordering this has no invariant consequence.

## Indexing and slicing invariants

Classification: every item here is PANIC in safe Rust (str slicing checks char boundaries), never UB. Three tiers.

### Tier A: byte slicing at a fixed count on input-controlled UTF-8 text (PANIC reachable from input)

| Site | Expression | Guard | Input source | Verdict |
|---|---|---|---|---|
| `src/reporter.rs:419` | `&path[..start_len]` and `&path[path.len() - end_len..]` | `path.len() <= max_len` early return (`:414`); caller `:114` passes `max_len = 40`, so `start_len = 13`, `end_len = 24`, no underflow | file path from session JSONL | PANIC if the byte at offset 13 or `len-24` is inside a multi-byte code point (non-ASCII file names). `test_truncate_path` at `:1042-1045` exercises only an ASCII path |
| `src/reporter.rs:427` | `&text[..max_len - 3]` | `text.len() <= max_len` early return (`:424`); callers `:173` (60), `:464` (40), `:540` (30), `:623` (10), `:636` (13): all `>= 3`, no underflow | agent description / joined agent names / tool names from session JSONL | PANIC on non-ASCII text cut mid-code-point |
| `src/analyzer.rs:686` | `invocation.command_line[..100]` | `command_line.len() > 100` | Bash command string from session JSONL | PANIC if byte 100 is mid-code-point (any command containing non-ASCII before column 100, e.g. a commit message with accented letters) |
| `src/connectors/cursor.rs:323` | `&m.content[..50]` | `m.content.len() > 50` | Cursor SQLite message content (feature `connectors`) | PANIC on non-ASCII content |
| `src/connectors/cursor.rs:378` | `&m.content[..50]` | same | same | same |

None of these use `is_char_boundary`, `floor_char_boundary`, `char_indices` or `chars().count()`; the sweep for those helpers returns nothing in `src/`, so no site converts a char count into a byte index (the "char-count-to-byte-index" pattern is absent).

### Tier B: byte slicing at automaton-reported offsets (SAFE by construction)

| Site | Expression | Offset origin | Verdict |
|---|---|---|---|
| `src/patterns/matcher.rs:143` | `&text[mat.start()..mat.end()]` | `aho_corasick::AhoCorasick::find_iter(text)` over the same `text`, automaton built with `ascii_case_insensitive(true)` and `LeftmostLongest` (`:114-117`) | Offsets are byte positions of a match of a valid-UTF-8 pattern inside valid-UTF-8 `text`; ASCII case folding only touches single bytes. UTF-8 self-synchronisation means both ends land on char boundaries. Safe |
| `src/patterns/matcher.rs:271` | `&text[start..end]` from `m.pos` | `terraphim_automata::find_matches(text, thesaurus, true)`; in registry copy `terraphim_automata-1.21.0/src/matcher.rs:56-70` the automaton runs `ac.find_iter(text)` on the original haystack (no lowercasing or normalisation of `text`; `ascii_case_insensitive(true)` at `:58`), positions are `(mat.start(), mat.end())` at `:68-69` | Same argument; safe. Feature `terraphim` only |

Caveat for both: an empty pattern string would make aho-corasick report zero-width matches at every byte offset including mid-code-point ones, and `&text[i..i]` still requires `i` to be a char boundary. The terraphim path filters patterns with `trim().is_empty()` or `len() < MIN_FIND_PATTERN_LENGTH (2)` (`terraphim_automata-1.21.0/src/matcher.rs:16,32`). The Aho-Corasick path in this crate does NOT filter individual pattern strings: `src/patterns/loader.rs:79,161` only check that `tool.patterns` (the `Vec`) is non-empty. The built-in `src/patterns.toml` has 15 `patterns = [` arrays and none contains `""`. A user pattern file (`load_patterns_from_file`, `src/patterns/loader.rs:61`) is the only route. Recorded as an open question, not a finding.

### Tier C: integer index arithmetic (guarded)

| Site | Expression | Guard |
|---|---|---|
| `src/reporter.rs:187` | `events.len() - 10` | inside `if events.len() > 10` (`:183`) |
| `src/analyzer.rs:791` | `window[window_size - 1]` | `window` comes from `tools.windows(window_size)` with `window_size in 2..=5.min(tools.len())` (`:786-788`) |
| `src/analyzer.rs:813` | `for i in 0..window.len() - 1` then `window[i + 1]` | same `windows(window_size)`, `window_size >= 2` |
| `src/analyzer.rs:808` | `window[0]` | same |
| `src/analyzer.rs:168` | `w[0]`, `w[1]` in `debug_assert!` over `windows(2)` | windows(2) yields exactly 2 |
| `src/analyzer.rs:472` | `agent_types.windows(sequence.len())` | `windows(0)` would panic, but the only caller is `detect_sequential_pattern` (`:401`) passing three literal 3-element sequences (`:394-398`); guarded by construction |

### Test-only slicing

`tests/integration_tests.rs:774`, `tests/filename_target_filtering_tests.rs:594,670,738`: `lines[start_idx..]` on a `Vec<&str>` (not a str; bounds-checked index only). `tests/terraphim_integration_tests.rs:197`: `&text[start..end]` on an ASCII literal.

## Miri suitability

Per `#[cfg(test)]` module (line of the attribute, count of `#[test]` fns, IO used inside the module only):

| Module | Tests | IO / clock / FFI inside the test module | Miri verdict |
|---|---|---|---|
| `src/models.rs:468` | 11 | none (pure parsing/Display/FromStr; includes `proptest!` at `:606`) | PURE |
| `src/kg/query.rs:209` | 26 | none (`proptest!` at `:598`) | PURE, with the proptest caveat below |
| `src/tool_analyzer.rs:203` | 20 | none | PURE |
| `src/patterns/matcher.rs:311` | 33 (the block at `:443` is `cfg(feature = "terraphim")`) | none; aho-corasick only | PURE (default features) |
| `src/patterns/knowledge_graph.rs:749` | 30 | none inside the module (the `fs`/`home_dir` hits are in non-test code above `:749`) | PURE |
| `src/patterns/loader.rs:177` | 10 | none; `load_patterns()` uses `include_str!("../patterns.toml")` (`:52`), so the default-pattern tests are compile-time embedded; the two `load_patterns_from_file` hits are behind `if path.exists()`-style helpers, not verified line by line | PURE (likely; see open questions) |
| `src/lib.rs:178` | 3 | none | PURE |
| `src/kg/builder.rs:271` | 8 | `Timestamp::now()` at `:279` | clock read; Miri refuses under default isolation, runs with `-Zmiri-disable-isolation` |
| `src/kg/search.rs:263` | 10 | `Timestamp::now()` at `:276,288` | same |
| `src/reporter.rs:928` | 14 | `Timestamp::now()` at `:934,961,1188`; `println!` to stdout | clock read plus stdout; Miri can run with isolation disabled but output is noisy |
| `src/analyzer.rs:942` | 11 | `Timestamp::now()` at 8 sites (`:1023-1490`); one `tempfile` plus `std::fs` write and a parse-from-file | mixed; clock plus filesystem |
| `src/parser.rs:521` | 17 | 3 `tempfile` sites, `std::fs` writes, 4 parse-from-file calls | IO; not Miri-suitable without isolation off |
| `src/main.rs:1114` | 2 | `home_dir` (4 hits) | environment lookup; binary crate, not in `--lib` |
| `src/connectors/{cursor,codex,opencode,aider}.rs` | 2+3+3+2 | none inside modules, but the modules only compile with `connectors` and `cursor.rs` links bundled SQLite (`rusqlite::Connection::open` at `:156`) | NOT Miri-runnable when `connectors` is on (C code); modules absent from the default build |
| `tests/terraphim_integration_tests.rs` | integration | no fs/clock; pure matcher tests | PURE (integration binary) |
| `tests/knowledge_graph_tests.rs` | integration | `tempfile` at 2 sites | IO |
| `tests/kg_integration_tests.rs` | integration | `Timestamp::now()` at `:14` | clock |
| `tests/integration_tests.rs`, `tests/filename_target_filtering_tests.rs` | integration | `tempfile`, `std::fs`, and `Command::new(env!("CARGO_BIN_EXE_tsa"))` (4 sites each) | spawn the `tsa` binary; NOT Miri-suitable |

Recommended Miri target set (pure computation, default features): `models`, `kg::query`, `tool_analyzer`, `patterns::matcher`, `patterns::knowledge_graph`, `patterns::loader`, `lib` unit tests, and the `terraphim_integration_tests` binary. Roughly 133 unit tests.

Caveats:
- `proptest!` in `src/kg/query.rs:598` and `src/models.rs:606` writes regression seeds under `crates/terraphim-session-analyzer/proptest-regressions/` on failure with the default config; under Miri isolation that write is refused. The existing `proptest-regressions/patterns` directory does not correspond to any current `proptest!` site (the only two are in `kg/query.rs` and `models.rs`), so it is a stale artefact.
- rayon `par_iter` in `parser.rs`/`analyzer.rs` spins a thread pool; Miri supports threads but runs them slowly and non-deterministically. The pure modules above do not touch rayon.

## Notable patterns

- The crate is written entirely in safe Rust with a `#![deny(unsafe_code)]`-equivalent posture in practice, although no such lint attribute is present (`rg -n 'deny\(unsafe' src` returns nothing; not verified beyond that sweep).
- `Timestamp` (jiff) is used as the sort key everywhere time matters; jiff `Timestamp` implements total `Ord`, avoiding the f64-time-ordering hazard entirely. Durations are converted to `f64` only for display (`src/reporter.rs:405-409`) and to `u64` via `.total(Millisecond).unwrap_or(0.0).abs() as u64` (`src/analyzer.rs:794-797,815-817`), which saturates rather than wraps.
- All string truncation is by byte length (`len()`), never by `chars()`; the crate has no code that converts char counts to byte offsets, so the "off-by-one on char count" family is absent, but the "fixed byte cut on UTF-8" family is present at five sites (Tier A).
- Sorting after rayon collection is done with `sort_by_key` on total keys, so output is deterministic regardless of thread scheduling.
- The `f32` comparators (`kg/search.rs:89`, `main.rs:864`) prefer `unwrap_or(Equal)` over `unwrap()`, which trades a NaN panic for a potentially inconsistent comparator; NaN is unreachable by construction for these values.

## Open questions

1. Empty pattern strings in a user-supplied `patterns.toml` (via `load_patterns_from_file`, `src/patterns/loader.rs:61`) are not rejected (`:79,161` check only the `Vec`). Whether aho-corasick 1.1 with `LeftmostLongest` actually yields zero-width matches for an empty pattern, and therefore whether `&text[i..i]` at `src/patterns/matcher.rs:143` can hit a non-boundary `i`, was not executed. Phase 2 could confirm with a one-line test.
2. (resolved) `agent_types.windows(sequence.len())` at `src/analyzer.rs:472` only receives literal 3-element sequences from `:394-398`; no empty-window path.
3. Expansion covered `--lib` with default features only. `main.rs` (binary), `connectors/*` (rusqlite) and the `terraphim`-gated paths (`patterns/matcher.rs:162-297`, `patterns/knowledge_graph.rs`, `main.rs` search command) were not expanded. If the lead wants those counted, `cargo +nightly expand -p terraphim-session-analyzer --lib --features full` and `--bin tsa` are the two extra runs.
4. The `libc` dependency (`Cargo.toml:83-84`) has no call site in this crate; whether it is pulled in for another workspace member's benefit or is simply dead was not determined.
5. Phase 1 did not run `cargo miri`; the "PURE" verdicts above are from static inspection of IO calls inside each `#[cfg(test)]` block, not from an execution.
