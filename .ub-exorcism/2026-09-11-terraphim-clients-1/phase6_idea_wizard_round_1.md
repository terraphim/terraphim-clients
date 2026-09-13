# Phase 6 idea wizard, round 1 of 3 (STRUCTURAL lens)

Run: 2026-09-11-terraphim-clients-1 (Exhaustive; archetype pure-safe-with-env-unsafe). Written by the idea-wizard-orchestrator, 2026-09-12 11:05 BST. Source unmodified; no commits made. Inputs: phase4_unified_findings.md (36 UF rows plus the lead addendum), UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md (18 blocks at the start of the round), phase2_summary.md, phase3_dynamic_findings.md, phase1_notes/A..J.md, the fff-search 0.8.4, self_update 0.42.0, terraphim_automata 1.21.1 and aho-corasick 1.1.4 registry sources, and two scratch probes (sources and logs archived under `phase6_raw/structprobe/`, built with `CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target`).

## Lens (verbatim)

> STRUCTURAL. Focus on data-shape invariants the codebase relies on: content hashes, serialisation round-trips (JSON/JSONL session files, config TOML), sort stability and tie-breaking, trait-derive consistency, Hash/Eq/Ord agreement, atomic-ordering pairings, rayon/crossbeam usage in terraphim_session_analyzer, tokio task/drop ordering, byte-slicing of UTF-8 (Phase 5 already confirmed byte-slice panics in session parsers), integer overflow in counters, and any invariant drift that could feed an unsafe boundary in a dependency (memmap, ahash, aho-corasick, tantivy-style indexes, self_update, git2 FFI).

Prompt applied: "Come up with your very best ideas for clever, non-obvious UB-detection techniques that are specifically suited to THIS codebase, viewed through the STRUCTURAL lens. Consider its specific architecture. Generate 30 ideas. Score each on PROVABILITY (1-5), CRATE-LEVEL IMPACT (1-5), NOVELTY VS EXISTING FINDINGS (1-5). Do NOT cut to a top-5; investigate every single idea."

## Facts established during the round (load-bearing for several ideas)

- Lockfile state. At 10:45 BST the working-tree `Cargo.lock` read crossbeam-epoch 0.9.21, at 10:50 it read 0.9.18 (HEAD 37a1ad1, `git status` clean), and at 10:58 HEAD was ae06cd2 (`build(deps): bump advisory-flagged crates`, 10:51 BST) with crossbeam-epoch 0.9.21 and memmap2 0.9.11. The lead's bump landed mid-round; EXP-101's gate is therefore met.
- fff-search 0.8.4 maps files with `memmap2::Mmap::map` in `get_cached_content` (types.rs:625-646) on every `grep`, gated only by size (`MMAP_THRESHOLD` 16 KiB on aarch64, 4 KiB elsewhere; `MAX_MMAP_FILE_SIZE` 10 MiB) and budget, not by `enable_mmap_cache`. All four workspace call sites pass `watch: false`, which removes the invalidation discipline the mapping's SAFETY comment relies on (see EXP-104).
- terraphim_automata 1.21.1 `find_matches` drops empty and sub-minimum-length keys (matcher.rs:61-78). The only Aho-Corasick automaton built directly by workspace code is `AhoCorasickMatcher::build_automaton` in terraphim-session-analyzer (matcher.rs:97-120), which does not (see EXP-103).
- self_update 0.42.0 `set_ssl_vars!` (macros.rs:12-24) is a safe edition-2021 `std::env::set_var`, Linux only, and writes only when the variable is absent. In the `terraphim_update --lib` binary (130 tests, 10 `#[serial]` all in state.rs) exactly one non-serial test reaches it: `test_check_for_updates_auto` (lib.rs:1712). `tests/r2_update.rs` never selects the GitHub backend (see EXP-102).
- Scratch probe 1 (serde flatten): `SessionMetadata { extra: Value::Null }` serialises to an object without `extra` and deserialises back as `Object {}` (round-trip drift, not equal); scalar or array `extra` fails to serialise with `can only flatten structs and maps`. All in-tree producers pass `Null` or `json!({..})`. Log: `phase6_raw/structprobe/flatten.log`.
- Scratch probe 2 (aho-corasick 1.1.4, `LeftmostLongest`, `ascii_case_insensitive(true)`): with an empty pattern present, `find_iter` on `"a\u{e9}b caf\u{e9} \u{1F600}x"` yields 14 matches (17 for `[""]` alone), 5 of them starting inside a multi-byte character; slicing at those positions panics. Non-empty patterns yield 0 non-boundary matches. Log: `phase6_raw/structprobe/acempty.log`.

## The 30 ideas

Scores: P = provability, I = crate-level impact, N = novelty versus existing findings (1 to 5 each).

| Id | Idea (technique, target) | P | I | N |
|---|---|---|---|---|
| I-01 | Post-bump Stacked Borrows re-run of the six rayon tests in terraphim-session-analyzer (lead lead (a), follow-up of EXP-004) | 5 | 2 | 2 |
| I-02 | Reachability of UF-036 (`set_ssl_vars!` writer) from non-`#[serial]` terraphim_update tests, per binary (lead lead (b)) | 4 | 3 | 3 |
| I-03 | fff-search mmap over a live working tree during workspace grep with `watch: false`; concurrent in-place writer versus the once-per-mapping UTF-8 validation behind `from_utf8_unchecked` | 4 | 5 | 5 |
| I-04 | Re-check RUSTSEC-2026-0186 (memmap2 unchecked offset) reachability now that the mapping is known to be live in every grep | 5 | 2 | 3 |
| I-05 | fff-search `unsafe impl Sync for BigramIndexBuilder`: word-partition invariant of the parallel `|=` writes | 4 | 3 | 4 |
| I-06 | fff-search `offset_from` between `&FileItem` references in the grep prefilter (single-allocation invariant) | 4 | 2 | 4 |
| I-07 | fff-search `PostScanUnsafeSnapshot` (`unsafe impl Send/Sync`) aliasing with watcher mutations under the scan-thread constructor | 3 | 3 | 4 |
| I-08 | Empty pattern string in tools.toml giving zero-width Aho-Corasick matches at non-char boundaries (session-analyzer matcher slice) | 5 | 3 | 4 |
| I-09 | `#[serde(flatten)] extra: serde_json::Value` round-trip and serialisation shape (JSONL session export) | 5 | 2 | 3 |
| I-10 | Hash/Eq/Ord derive agreement and manual-impl consistency across the workspace | 5 | 1 | 1 |
| I-11 | NaN in float comparators outside the EXP-011 sites (sessions search, registry discovery, kg search, memory_command, tsa main, store.rs:500) | 5 | 2 | 1 |
| I-12 | Sort stability and tie order fed by HashMap or AHashMap iteration (tsa reporter/analyzer, `tools_used`, mcp candidates) | 4 | 2 | 2 |
| I-13 | NormalizedTerm id assignment from iteration order or `thesaurus.len() + idx` (cli schema thesaurus, kg/builder.rs, capture.rs) and downstream `HashSet<u64>` dedup | 4 | 2 | 2 |
| I-14 | Atomic-ordering pairings in the workspace (test counters, fff-search Relaxed column allocation) | 4 | 1 | 2 |
| I-15 | Integer overflow and float-to-int casts in counters (tsa `total_time_ms`, `sum::<u64>`, `as u32` counts, `(confidence*100.0) as u32`) | 4 | 1 | 2 |
| I-16 | Fixed-byte string cuts in terraphim-session-analyzer not listed in EXP-012 (reporter.rs:419, connectors/cursor.rs:323,378, analyzer.rs:686) | 5 | 2 | 1 |
| I-17 | `line_starts` binary-search sortedness invariant in terraphim_negative_contribution scanner (byte offsets from automata) | 5 | 1 | 2 |
| I-18 | LSP `Position.character` UTF-16 versus byte offset in terraphim_lsp diagnostics | 4 | 2 | 3 |
| I-19 | JSONL line splitting and per-line `serde_json::from_str` in the native connector (`\r`, U+2028, oversized lines) | 4 | 1 | 2 |
| I-20 | Rollback SHA-256 checksum verify-then-copy TOCTOU and `HashMap<String, Backup>` rotation order | 4 | 2 | 1 |
| I-21 | self_update archive extraction path invariants (`ZipArchive` `mangled_name`, `tar::Archive::unpack`) | 3 | 2 | 2 |
| I-22 | git2 FFI global-init and thread discipline in `collect_files` (`git_libgit2_init` Once, `spawn_git_status` thread, `Repository: !Sync`) | 3 | 2 | 2 |
| I-23 | rayon `par_iter` executed from inside a tokio worker (terraphim_sessions importing via terraphim-session-analyzer) | 3 | 2 | 2 |
| I-24 | `static OnceLock<Arc<tokio::sync::Mutex<SessionService>>>` outliving runtimes (repl/handler.rs:1909) | 3 | 1 | 2 |
| I-25 | `OnceLock` initialiser re-entrancy (`KG_THESAURUS` capture.rs:733, `COMPILED_PATTERNS` redaction.rs:55) | 4 | 1 | 2 |
| I-26 | crossbeam-epoch integer-to-pointer casts under Tree Borrows and the rayon pool leak-check exit (EXP-003f notes) | 4 | 1 | 1 |
| I-27 | event-listener `StackSlot` Send/Sync through sqlx after the lockfile bump | 5 | 1 | 1 |
| I-28 | Second `notify` copy (9.0.0-rc.4 via fff-notify-debouncer-full) hosting an FSEvents `extern "C"` callback | 4 | 1 | 3 |
| I-29 | ahash/hashbrown keyed by derived `Hash` types (`AHashMap<NormalizedTermValue>`, `HashSet<(usize, usize)>`) | 5 | 1 | 1 |
| I-30 | Missing serde and slicing round-trip properties in the existing proptest sites (models.rs:572, kg/query.rs:598) and the stale `proptest-regressions/patterns` directory | 3 | 1 | 2 |

## Per-idea verdicts

Each entry: classification; action; EXP id if filed. Evidence is cited as file:line.

- **I-01** NEEDS_DEEPER_INVESTIGATION; the deeper check (a Miri run of 15 minutes or more per axis) exceeds the 5-minute budget, so it is filed with a concrete procedure. Gate verified met at filing (HEAD ae06cd2, crossbeam-epoch 0.9.21). Expected outcome per EXP-004: the `element_of` report persists in dependency frames (0.9.21 internal.rs:564-568 is unchanged). **EXP-101, verdict OPEN.**
- **I-02** NEEDS_DEEPER_INVESTIGATION; static reachability done now. `terraphim_update --lib` binary: config 10, downloader 11, lib 19, manifest 11, notification 15, platform 12, rollback 18, scheduler 10, signature 14, state 10 tests; the 10 `#[serial]` are all in state.rs. One non-serial test reaches the writer: `test_check_for_updates_auto` (lib.rs:1712-1734) -> `check_for_updates_auto` (lib.rs:1277) -> `spawn_blocking` -> `Update::get_latest_release` (self_update backends/github.rs:518) -> `set_ssl_vars!()` (macros.rs:12-24). `tests/r2_update.rs` uses `UpdateBackend::R2` (:102) and only asserts the builder (:236-238). The memory-level verdict needs the EXP-010-a instrument; no new reader exists. **EXP-102, verdict DEFERRED** (follow-up of EXP-010).
- **I-03** NEW_EXP_PROMOTED. `get_content_for_search` (fff-search types.rs:655-667) takes the mmap fast path whenever `get_cached_content` (types.rs:625-646) maps the file; `enable_mmap_cache` is consulted only for warmup (file_picker.rs:436-437, scan.rs:315-318). The SAFETY comment at types.rs:637-639 names the watcher as the mitigation; the workspace passes `watch: false` at hybrid_searcher.rs:354-359 and mcp_server lib.rs:1274-1279, :1372-1377, :1485-1490. `grep_files_parallel` validates the whole mapping once (grep.rs:1685) and builds every line with `from_utf8_unchecked` (grep.rs:1710); `GrepMatch::line_content` is an owned `String` (grep.rs:269), so an invalid `str` is observable through the safe API. CI reachability: `test_find_files.rs:146-164` greps the whole checkout; no in-tree concurrent writer found, so CI exposure is to external writers. **EXP-104, verdict OPEN.**
- **I-04** ALREADY_COVERED (UF-017), rationale refined. The advisory functions are `advise_range`, `unchecked_advise_range`, `flush_range`, `flush_async_range` (RUSTSEC-2026-0186, fixed 0.9.11); fff-search calls only `Mmap::map(&file)` (types.rs:640) and never `MmapOptions`, `.offset(` or the range helpers (rg over fff-search src = 1 hit). UF-017's "no reachable call path" should read "mapping is live in every grep; the advisory-specific functions are not called". Lockfile is at 0.9.11 since ae06cd2. Recorded in EXP-104 Notes.
- **I-05** NO_EVIDENCE for the workspace. `build_bigram_index` (bigram_filter.rs:634-690) runs only under `enable_content_indexing` (scan.rs:305-312), which every workspace call site leaves at its `false` default (file_picker.rs:452-457). The partition is `par_chunks(BIGRAM_CHUNK_FILES)` with `BIGRAM_CHUNK_FILES = 4 * 64` (bigram_filter.rs:596), so chunk boundaries fall on `word_idx` boundaries (`file_idx / 64`) and no two threads share a `u64`; `col_data` is `OnceLock`-initialised before the first write. Sound by construction as far as the static read goes; noted in EXP-104 Notes as reviewed.
- **I-06** NO_EVIDENCE. grep.rs:1030-1045 runs only when `bigram_candidates` is `Some` (needs the index from I-05). `files_to_search` comes from `prepare_files_to_search(files, ..)` over one `&[FileItem]` slice (grep.rs:1013-1023), and overflow items are flagged in the same slice (`FileItem::is_overflow`, types.rs:521-523), so `offset_from` stays within one allocation.
- **I-07** NO_EVIDENCE for the workspace. `post_scan_snapshot` (file_picker.rs:1256-1280, `unsafe impl Send/Sync` at :1675-1679) is reached only from the scan-thread path in scan.rs:201-207; `FilePicker::new` (file_picker.rs:696-726) constructs no thread and sets `background_watcher: None`, and `collect_files` (file_picker.rs:796-835) is synchronous. Carry over to the CROSS-SYSTEM lens if any future workspace caller adopts `new_with_shared_state` or `watch: true`.
- **I-08** NEW_EXP_PROMOTED. Loader validation only checks `tool.patterns.is_empty()` (loader.rs:79-91, :160-170); `build_automaton` passes every string (matcher.rs:97-120); the slice at matcher.rs:143 is unchecked. Reachable from `~/.config/claude-log-analyzer/tools.toml` through `load_user_patterns` (loader.rs:100-111) and `load_all_patterns` (loader.rs:121-126). Mechanism confirmed in scratch (5 non-boundary zero-width matches per input). Safe-Rust panic, not memory UB, filed on the EXP-012 precedent. **EXP-103, verdict OPEN.**
- **I-09** NO_EVIDENCE for UB; correctness note. Probe: `Null` round-trips to `Object {}`; scalar or array `extra` fails to serialise. Producers: `Value::Null` (native.rs:321,337,348,364,505; cursor.rs:304,360; codex.rs:202; redaction.rs:176,205) or `json!({..})` (opencode.rs:100; codex.rs:225-230; cline.rs:163-206). `sessions export` (repl/handler.rs:2446, :2675) therefore cannot fail today; `SessionMetadata` does not derive `PartialEq`, so the drift is invisible in-tree. Carry over to the ADVERSARIAL lens (user-supplied `extra`).
- **I-10** ALREADY_COVERED (phase2_findings_hash-eq.md, bucket 25 NO_SURFACE). Re-checked: `rg 'impl\s*(<[^>]*>)?\s*(Hash|PartialEq|Eq|Ord|PartialOrd|Borrow)\s+for'` over crates/ = 0; the seven `Hash` derives (commands/mod.rs:26,151; judge/family.rs:24; session-analyzer models.rs:9,42,75,238) all derive `PartialEq, Eq` on the same field set.
- **I-11** ALREADY_COVERED (EXP-011, UF-025 to UF-027). The additional sites use `partial_cmp(..).unwrap_or(Equal)` (sessions search.rs:137,199; registry.rs:699; kg/search.rs:91; memory_command.rs:513; tsa main.rs:864), the tolerated-comparator shape EXP-011 characterised; store.rs:500 uses `unwrap()` but is preceded by `.filter(|(score, _)| *score > 0.0)` (store.rs:497), which drops NaN (`NaN > 0.0` is false).
- **I-12** NO_EVIDENCE for UB; correctness cross-reference to UF-029. The stable sorts over `HashMap` iterations (analyzer.rs:561,634; reporter.rs:227,446,570,763,784; service.rs:578,624) give process-dependent tie order; `tools_used` (sessions model.rs:314-322) and `unknown_terms` (kg_analysis.rs:120) sort the whole key, so they are deterministic. No unsafe consumer of the order.
- **I-13** NO_EVIDENCE for UB; correctness note. kg/builder.rs:88 assigns `id = thesaurus.len() + idx` per pattern, so a duplicate key (which does not grow `len`) makes two concepts share an id; cli service.rs:472-478 and grep hybrid_searcher.rs:585-618 assign ids by enumeration over schema entries; mcp lib.rs:445 dedups on `HashSet<u64>` ids. Same family as UF-029 (routed as correctness).
- **I-14** NO_EVIDENCE. Workspace atomics are `SeqCst` test counters only (update scheduler.rs:299-303; grep lib.rs:520-558; mcp tests/support/mod.rs:25-28; agent tests/support/cli_test_env.rs:9-32); no acquire/release protocol exists in workspace code. fff-search's `Relaxed` column CAS (bigram_filter.rs:76-90) publishes an index into a slab initialised through `OnceLock` (acquire/release) and read only after `par_chunks` joins; unreachable anyway (I-05).
- **I-15** NO_EVIDENCE. Float-to-int `as` casts saturate (tsa main.rs:692, analyzer.rs:138,215,798,817; reporter.rs:205); `sum::<u64>()` (analyzer.rs:850) and `+=` counters wrap in release (no `overflow-checks` in `[profile.release]`, Cargo.toml:104-109) or panic in debug, and no counter feeds an index, length or allocation size in unsafe code. `bytes_read as u64` (downloader.rs:238) widens.
- **I-16** ALREADY_COVERED (EXP-012 mechanism); sites for Phase 8. reporter.rs:419 (`&path[..start_len]`, `&path[path.len() - end_len..]`), connectors/cursor.rs:323 and :378 (`&m.content[..50]`, feature `connectors`), analyzer.rs:686 (`command_line[..100]`). Same guard-then-cut idiom as UF-011.
- **I-17** NO_EVIDENCE. `build_line_starts` (scanner.rs:141-149) pushes strictly increasing byte indices; `byte_to_line` (scanner.rs:150-155) binary-searches them; `get_line_content` (scanner.rs:157-171) bounds-checks and slices at `\n` positions, which are always char boundaries. Match positions come from `find_matches` on the same `content`, so `start..end.min(len)` at scanner.rs:64 is boundary-safe (I-08 shows the automata filter that keeps it so).
- **I-18** NO_EVIDENCE. terraphim_lsp emits whole-line ranges only: `Range::new(Position::new(line, 0), Position::new(line, u32::MAX))` (diagnostic.rs:12); `kg_analysis.rs` keeps byte offsets internal and slices with `text.get(start..end)` (kg_analysis.rs:69). No UTF-16 conversion exists to get wrong.
- **I-19** NO_EVIDENCE. native.rs:267-271 and :458-462 iterate `content.lines()` (strips `\n` and a trailing `\r`) and skip lines `serde_json::from_str` rejects; no unsafe, no slicing by index. U+2028 is not a line terminator for `str::lines`.
- **I-20** ALREADY_COVERED (UF-006, panic-safety F-001; routed as non-UB). `verify_integrity` (rollback.rs:53-60) then `fs::copy` (rollback.rs:310) is a check-then-act on the filesystem; `rotate_backups` sorts on `chrono::DateTime<Utc>` (rollback.rs:217, total order) and keys the map by version string.
- **I-21** INAPPLICABLE. Zip-slip and tar path traversal are security defects, not UB; zip 8.6 `mangled_name` (lib.rs:1134) strips traversal components and `tar::Archive::unpack` (lib.rs:1153) refuses `..` by default. Outside the UB registry; note for the security track.
- **I-22** NO_EVIDENCE. `git2::init` is `Once`-guarded in libgit2-sys; `Repository` is `Send` and `!Sync` (compiler-enforced); `spawn_git_status` (file_picker.rs:801) owns its repository on its own thread. The only UB-relevant behaviour of this chain is the env reader, which is UF-001 (CONFIRMED_UB) and UF-012.
- **I-23** NO_EVIDENCE. Rayon `par_iter` (parser.rs:177,242,284; analyzer.rs:72) blocks the calling thread; on a tokio worker that is a stall (the UF-019 family), not UB. No shared mutable state crosses the two pools (phase1_notes/I.md concurrency table).
- **I-24** ALREADY_COVERED (async-drop sweep, UF-033 and UF-034 family). `tokio::sync::Mutex` is not bound to a runtime; the `Arc` in the `OnceLock` (repl/handler.rs:1909-1913) outlives every runtime by design and is dropped at process exit.
- **I-25** NO_EVIDENCE. `KG_THESAURUS` (capture.rs:733) is initialised by `build_kg_thesaurus_from_dir` (capture.rs:746-830), which does filesystem I/O and never calls back into capture; `COMPILED_PATTERNS` (redaction.rs:55) compiles regexes. `OnceLock` re-entrancy would deadlock or panic (defined), and no re-entrant path exists.
- **I-26** ALREADY_COVERED (EXP-003f Notes, EXP-004). The two integer-to-pointer warnings and the "main thread terminated without waiting" leak-check exit are recorded there; EXP-101 re-runs the relevant tests post-bump.
- **I-27** ALREADY_COVERED (UF-016). The bump commit ae06cd2 is the fix vehicle; verify `event-listener` >= 5.4.2 in the lockfile when Phase 5 re-runs.
- **I-28** NO_EVIDENCE for the workspace. notify 9.0.0-rc.4 arrives via fff-notify-debouncer-full -> fff-search (cargo tree); it is used only by `BackgroundWatcher`, which is never constructed with `watch: false` (file_picker.rs:717, :849-863). The workspace's own notify 8.2.0 callback is UF-031.
- **I-29** ALREADY_COVERED (bucket 25; I-10). hashbrown treats an inconsistent `Hash` as a logic error, not UB; every key type in the workspace derives `Hash` and `Eq` together (`NormalizedTermValue` is a registry newtype over `String` with derived impls).
- **I-30** NO_EVIDENCE; recommendation only. The two `proptest!` sites (models.rs:572-590, kg/query.rs:598-620) test normalisation and parsing, not serde or slicing round-trips; `proptest-regressions/patterns` matches no current site (phase1_notes/I.md). EXP-011 and EXP-012 already recommend harness-side proptests; nothing to promote.

## Round summary

| Classification | Count | Ideas |
|---|---|---|
| ALREADY_COVERED | 9 | I-04, I-10, I-11, I-16, I-20, I-24, I-26, I-27, I-29 |
| NEW_EXP_PROMOTED | 2 | I-03 (EXP-104), I-08 (EXP-103) |
| NEEDS_DEEPER_INVESTIGATION | 2 | I-01 (EXP-101, deeper check exceeds 5 minutes), I-02 (EXP-102, static arm done now) |
| NO_EVIDENCE | 16 | I-05, I-06, I-07, I-09, I-12, I-13, I-14, I-15, I-17, I-18, I-19, I-22, I-23, I-25, I-28, I-30 |
| INAPPLICABLE | 1 | I-21 |

New EXP blocks filed: 4 (EXP-101 OPEN, EXP-102 DEFERRED, EXP-103 OPEN, EXP-104 OPEN). Registry now holds 22 blocks; `lint-experiment-designs.py` prints `[OK] UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md — all blocks well-formed`.

Honest reading of the round: 25 of 30 ideas land ALREADY_COVERED, NO_EVIDENCE or INAPPLICABLE, which is consistent with a pure-safe workspace whose Phase 2 sweeps were thorough. The two promotions both sit at the boundary the lens asked about: a dependency's unsafe block whose precondition the workspace's configuration silently removes (EXP-104), and a workspace-built automaton that skips a filter the registry crate applies (EXP-103). Deeper items the lead should note without waiting for Phase 5: UF-017's memmap2 rationale needs the refinement recorded under I-04, and phase1_notes/I.md open question 1 is answered (I-08).

## Non-UB notes for Phase 8 (not registry material)

- I-09: `SessionMetadata.extra` should be `#[serde(default)]`-normalised to an object, or the type changed to `serde_json::Map`, so export and re-import agree.
- I-13: kg/builder.rs:88 id collision on duplicate synonym keys; cli service.rs:472-478 ids depend on schema entry order.
- I-16: four more fixed-byte cut sites in terraphim-session-analyzer for the EXP-012 remediation list.
- I-21: archive extraction relies on `zip` and `tar` defaults; document that reliance.

## Carry-over to next round

Better examined under the ADVERSARIAL lens (input is user- or attacker-controlled):
- I-08 generalised: every user-supplied pattern or thesaurus source (tools.toml, `.terraphim/thesaurus-<role>.json` keys with control or format characters, KG markdown synonyms lines) against the `terraphim_automata` filters at matcher.rs:61-78 and :300-320.
- I-09: non-object `extra` values and hostile session JSONL (deeply nested `Value`, giant lines) through `sessions import` and `sessions export`.
- I-21: crafted release archives against `mangled_name` and `unpack`, with the signature verification (`zipsign`) as the gate.
- I-13: schema files that force id collisions and whether any consumer indexes by id into a `Vec`.

Better examined under the CROSS-SYSTEM lens (two processes, two runtimes, or CI hosts):
- I-03 / EXP-104: external in-place writers (editors, `cargo build` outputs not covered by ignore rules, CI checkout mutation) during MCP `grep_files` on the Linux runners, where `MMAP_THRESHOLD` is 4 KiB and far more files map.
- I-02 / EXP-102: the glibc resolver reader on bigbox and ubuntu-latest, plus the CI-side mitigation of presetting `SSL_CERT_FILE` and `SSL_CERT_DIR`.
- I-07 and I-28: any future adoption of `watch: true` or `new_with_shared_state` by the MCP server, which would activate the snapshot, the bigram builder and the second notify callback host in one process with the workspace's own notify watcher.
- I-23: rayon inside tokio when the MCP server and the sessions importer share a process.
