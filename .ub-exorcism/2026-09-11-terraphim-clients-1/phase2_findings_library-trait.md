# Phase 2 findings: bucket 12, library trait invariants

Run: 2026-09-11-terraphim-clients-1. Sweeper: p2 static-bucket-sweeper (library-trait). Written 2026-09-11 23:55 BST.
Scope: Hash/Eq/Ord/Borrow consistency, comparator total order, Iterator/size_hint contracts, str char-boundary slicing, Deserialize-produced invalid states. Workspace crates under `crates/`, plus the registry copies of `terraphim_types-1.22.1`, `terraphim_rolegraph-1.20.2`, `terraphim_automata-1.21.0`, `terraphim_service-1.21.1` and `aho-corasick-1.1.4` read for the contracts the workspace relies on. Source unmodified. Toolchain per phase0: rustc 1.98.0-nightly (bd08c9e71 2026-06-25), edition 2024.

Headline: nothing in this bucket is undefined behaviour. Every finding below is a reachable panic in safe code, or an inconsistent comparator that the std sort handles memory-safely (std `slice::sort_by` docs: "May panic if `compare` does not implement a total order"). Severities used are SUSPICIOUS (panic reachable from ordinary input) and CONTRACTUAL-BUT-DEFENSIBLE (needs a deserialised or hand-edited value, or a caller that only exists in tests). No MUST-BE-UB or LIKELY-UB entries.

## Inputs read

- WORKSPACE/phase1_unsafe_surface_inventory.md (rows for terraphim_grep 150-154, 207-212, 430-434; terraphim_sessions search.rs:135,197; terraphim_negative_contribution scanner.rs:62, 157-171).
- WORKSPACE/phase1_notes/B.md (line 75: no manual trait impls; family.rs:60), D.md (lines 69, 110: float comparator, open NaN question), F.md (lines 225-258: library-trait surface), I.md (lines 60-115: comparator and slicing tiers).

## Sweep commands and raw output

### ast-grep patterns

`~/.claude/skills/rust-undefined-behavior-exorcist/scripts/patterns/` holds 26 files; none carries a `library-trait` prefix, so
`bash .../ast-grep-ub-patterns.sh crates library-trait` prints `No patterns found.` and exits 1.

Pattern files present: aliasing-cast-ref-to-mut, aliasing-deref-while-borrowed, alignment-repr-packed-field-ref, async-drop-block-on, atomic-relaxed-load-store, await-holding-mutex, const-mutation-cast, custom-Hash-without-Eq, float-from-int-bits, get-unchecked, lifetime-escape-as-ptr, manual-Allocator-impl, manual-Iterator-size-hint, MaybeUninit-write-after-assume-init, pin-new-unchecked, provenance-int-cast, raw-fd-without-OwnedFd, refcount-from-raw, repr-Rust-transmute, send-sync-manual-impl, set-len, slice-from-raw-parts, type-punning-transmute, uninit-maybeuninit-assume-init, uninit-mem-zeroed-uninit, utf8-as-bytes-index.

The four patterns relevant to this bucket were run individually against `crates/`:

```
=== custom-Hash-without-Eq ===
=== manual-Iterator-size-hint ===
=== utf8-as-bytes-index ===
=== float-from-int-bits ===
```

All four produced no matches (ast-grep exit 0, empty output).

### rg sweeps (src only; tests and benches excluded)

Manual trait impls: `rg 'impl(<..>)? (Hash|PartialEq|Eq|Ord|PartialOrd|Borrow|Iterator|ExactSizeIterator|DoubleEndedIterator|FusedIterator)(<..>)? for'` over `crates/` returned nothing. Confirms the Phase 1 signature count of zero.

`#[derive(..Hash..)]` on a type with an `f64`/`f32` field: none (multi-line rg over every derive block containing `Hash`, 14 lines of look-ahead). A `derive(Hash)` on a float field would not compile in any case (f64 does not implement Hash).

Float-keyed collections (`HashMap|HashSet|BTreeMap|BTreeSet|IndexMap<f64|f32|(f64|(f32`): none. `BinaryHeap`: none. `ordered_float`/`NotNan`/`OrderedFloat`: absent from `crates/` and `Cargo.lock`. `size_hint`: no occurrence in `crates/`. `total_cmp`: one use, `terraphim_agent/src/forgiving/suggestions.rs:63` (correct).

`BTreeMap`/`BTreeSet` keys: `terraphim_agent/src/listener.rs:1271,1315` (`BTreeSet<String>`), `terraphim_agent/src/judge/tier.rs:53` (`BTreeMap<String, TierConfig>`). `String: Ord` is total.

Float comparators found (`partial_cmp` / `max_by` / `sort_by` on `f64`/`f32`):

| Site | Form | Score producer | Verdict |
|---|---|---|---|
| terraphim_grep/src/hybrid_searcher.rs:210-214 | `sort_by(partial_cmp.unwrap_or(Equal))` on `RetrievedChunk.relevance_score: f64` | `1.0` (:398), `rank as f64` (`From<Document>` :46), plus `score_kg_boost` (:106-127) | F-003 |
| terraphim_grep/src/hybrid_searcher.rs:428-432 | same | same | F-003 |
| terraphim_sessions/src/search.rs:135-139, 197-201 | same on `Scored<Session>::score()` | `terraphim_types` `Scored` | non-finding (see table) |
| terraphim_agent/src/shared_learning/store.rs:242 | `max_by(partial_cmp.unwrap())` | local `Bm25Scorer` | F-004 |
| terraphim_agent/src/shared_learning/store.rs:500 | `sort_by(partial_cmp.unwrap())` | local `Bm25Scorer`, filtered `> 0.0` at :497 | non-finding |
| terraphim_agent/src/learnings/capture.rs:1445-1463 | `partial_cmp.unwrap_or(Equal).then_with(..)` on `ImportanceScore.total` | `ImportanceScore::calculate` or markdown frontmatter parse | F-005 |
| terraphim_agent/src/memory_command.rs:513 | `sort_by(partial_cmp.unwrap_or(Equal))` on `RubricScore::composite()` | literals and `60/(1+days)` | non-finding |
| terraphim_agent/src/commands/registry.rs:697-701 | same on `match_score: f64` | `terraphim_automata::autocomplete_search` | non-finding |
| terraphim-session-analyzer/src/kg/search.rs:89-93, main.rs:864 | same on `f32` | `concepts.len() as f32 + literal bonus` | non-finding (Phase 1 note I, item 5) |

Range-slicing sweep (`\[x..y\]`, `\[..y\]`, `\[x..\]` on `str`), 60 production sites triaged. Grouped results are in F-001, F-002, F-006, F-007 and the non-findings table.

## Findings

### F-001 Byte-window context extraction slices mid-character

- File:line: crates/terraphim_sessions/src/enrichment/enricher.rs:177-188 (`extract_context`), called from :153 (`matched_to_concept`), reached from :103-108 (`enrich_session`).
- Kind: str char-boundary slicing on input-controlled text.
- Bucket(s): 12 (str boundary contract), cross-tag 11 (panic safety).
- Severity: SUSPICIOUS.
- Static evidence: `ctx_start = start.saturating_sub(window)` and `ctx_end = (end + window).min(text.len())` with `window = self.config.context_window` (default 50 bytes, :32; documented as "characters" at :18-19). Neither bound is aligned with `is_char_boundary`. `start`/`end` themselves are valid boundaries (they come from `find_matches`, see non-findings), but `start - 50` and `end + 50` are arbitrary byte offsets. `text` is `message.content` of a session (:98-99), i.e. arbitrary user, assistant or tool text. `include_context` defaults to `true` (:31). Trigger condition: any multi-byte code point spans byte `start - 50` or `end + 50` of a message that contains a thesaurus term. Production caller: crates/terraphim_agent/src/repl/handler.rs:2518 (`enricher.enrich_session(&session)` in the REPL sessions enrichment command).
- Draft experiment: standalone test under WORKSPACE/phase3_raw (do not edit `crates/`): build a `Session` with one message whose content is `"\u{00e9}".repeat(40) + "a" + "terraphim"`. The 40 two-byte characters occupy bytes 0-79 with lead bytes at even offsets, `a` is byte 80, the term starts at byte 81, so `ctx_start = 31` is odd and therefore a continuation byte. (A run of two-byte characters ending directly at the term never triggers because the 50-byte window is even; the single ASCII byte between run and term is what breaks parity.) Thesaurus with the single term `terraphim`, `SessionEnricher` with default config, call `enrich_session`; expect panic "byte index 31 is not a char boundary". Second case independent of parity: `"\u{20ac}".repeat(40) + "terraphim"` (three-byte characters, term at byte 120, `ctx_start = 70`, `70 mod 3 = 1`, a continuation byte). Then a proptest over `Vec<char>` prefixes of random length to show the failure density. Fix shape (for Phase 4, not applied): walk `ctx_start` down and `ctx_end` up with `is_char_boundary`, as `truncate_title` does in connector/cursor.rs:411-414.
- Cross-refs: Phase 1 note D (not listed there; new). Same idiom is done correctly at terraphim_sessions/src/search.rs:80-85 and connector/cursor.rs:401-416.

### F-002 Session summary cuts the first user message at byte 100

- File:line: crates/terraphim_sessions/src/model.rs:326-336 (`Session::summary`), reached in production via crates/terraphim_sessions/src/search.rs:36 (`session_to_document` sets `description: session.summary()`), which every `search_sessions` call runs over every session (:99). Production callers: crates/terraphim_sessions/src/service.rs:212 (`search_sessions`) and :259 (`search_sessions_hybrid`), the `SessionService` search entry used by the agent `sessions search` command.
- Kind: str char-boundary slicing on input-controlled text.
- Bucket(s): 12, cross-tag 11.
- Severity: SUSPICIOUS.
- Static evidence: `if m.content.len() > 100 { format!("{}...", &m.content[..100]) }`. The guard is a byte-length guard; byte 100 is a boundary only for ASCII-dominant text. Content is the first user message of any imported session. A single non-ASCII prompt longer than 100 bytes whose 100th byte is inside a code point panics `sessions search` for the whole corpus.
- Draft experiment: construct a `Session` whose first user message is 51 copies of `\u{00e9}` (102 bytes; byte 100 is the lead byte of the 51st character, so `[..100]` succeeds) versus `"a"` followed by 50 copies of `\u{00e9}` (101 bytes; byte 100 is the continuation byte of the last character). Call `search_sessions(&[session], "x")`; expect the second to panic. Proptest: `prop::collection::vec(any::<char>(), 60..120)` mapped to a String, assert `summary()` never panics.
- Cross-refs: F-001; same-crate correct idiom at search.rs:80-85.

### F-003 KG-boost comparator is total only while no deserialised score is NaN

- File:line: crates/terraphim_grep/src/hybrid_searcher.rs:210-214 (`boost_chunks_with_kg`), :428-432 (`fuse_and_rank`), with the masking fold at :110 and the division at :123.
- Kind: comparator total order (f64 with `unwrap_or(Equal)`).
- Bucket(s): 12 (comparator), 12(e) Deserialize-produced invalid state.
- Severity: CONTRACTUAL-BUT-DEFENSIBLE.
- Static evidence: All in-tree producers are finite: `relevance_score: 1.0` (:398), `doc.rank.unwrap_or(0) as f64` (:46, exact for u64 below 2^53), `KgConcept.score: 1.0` (:169, :180). `score_kg_boost` guards `max_concept_score <= 0.0` (:111-113) and `weight <= 0.0` (:107). `KgConcept.score` and `RetrievedChunk.relevance_score` are `pub` and `Deserialize` (Phase 1 note F). A NaN concept score does not propagate: `fold(0.0, f64::max)` at :110 skips the NaN operand, `boost += c.score / max_concept_score` at :123 makes `boost` NaN, but the return expression `(boost * weight).min(weight * concepts.len() as f64)` at :126 uses `f64::min`, which returns the other operand when the receiver is NaN, so the function returns the finite cap. The only NaN route into the two comparators is therefore a `RetrievedChunk` whose `relevance_score` is already NaN when it enters `boost_chunks_with_kg` or `fuse_and_rank`, i.e. one constructed directly by a caller or deserialised through a non-JSON serde format (serde_json rejects a bare `NaN` token). A NaN element makes `partial_cmp(..).unwrap_or(Equal)` non-transitive, which the post-1.81 driftsort may reject with a panic (not guaranteed; the docs say "may").
- Draft experiment: unit test calling `fuse_and_rank` (and `boost_chunks_with_kg` with an empty concept list) on 20 `RetrievedChunk` values, three of which carry `relevance_score: f64::NAN` interleaved with finite scores; record whether the current std panics or returns an order. Add a proptest over `Vec<f64>` including NaN/inf via `prop::num::f64::ANY` to characterise the comparator. A separate unit test on `score_kg_boost` with `concepts = [KgConcept{score: f64::NAN, ..}, KgConcept{score: 1.0, ..}]` should confirm the finite-cap masking at :126 (expected: finite result, no NaN).
- Cross-refs: Phase 1 inventory rows for :207-212 and :430-434 (line numbers now :210-214 and :428-432 in the current tree); note F lines 243-249.

### F-004 Dedup best-match uses `partial_cmp(..).unwrap()` with no NaN filter

- File:line: crates/terraphim_agent/src/shared_learning/store.rs:242 (`store_with_dedup`), scorer at :88-124, normaliser at :126-132.
- Kind: comparator total order; direct `unwrap()` on `partial_cmp`.
- Bucket(s): 12, cross-tag 11.
- Severity: CONTRACTUAL-BUT-DEFENSIBLE (only test callers in-tree; precondition needs every stored learning to be textually empty).
- Static evidence: `max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())`. The sibling `find_similar` path at :500 uses the same `unwrap()` but is protected by `.filter(|(score, _)| *score > 0.0)` at :497, which drops NaN (NaN > 0.0 is false). `store_with_dedup` has no such filter. NaN derivation: `avg_doc_len = total_doc_len / n` (:230) is `0.0` when every stored learning's `extract_searchable_text()` (terraphim_types shared_learning.rs:649-665: title, content, keywords, original_command, error_context) has no whitespace-separated token; then for a doc with `doc_len == 0`, `B * doc_len / self.avg_doc_len` at :118 is `0.0/0.0 = NaN`, the denominator is NaN, `0 / NaN = NaN`, `idf * NaN = NaN`, and `normalize_score` returns `NaN.tanh().clamp(0.0, 1.0) = NaN` (`f64::clamp` propagates NaN). The comparator is invoked only when there are at least two stored learnings, and NaN reaches `unwrap()` only when the query (the new learning's own text) has at least one token (:131 returns 0.0 for an empty query). `SharedLearning::new()` is `Self::default()` (shared_learning.rs:302-304), so an empty title and content are constructible; no non-empty guard was found in `crates/terraphim_agent/src/shared_learning/` or `main.rs` (rg for `title.*is_empty|content.*is_empty|.trim()` on those paths returned nothing relevant). Untested precondition: whether any production caller can persist two textually empty learnings; the only in-tree callers of `store_with_dedup` are the tests at :912 and :922.
- Draft experiment: reliable half first: `Bm25Scorer::new(2, 0.0).score("x", "", &HashMap::new())` in isolation, assert `is_nan()`, then `normalize_score(NaN, 1)` assert `is_nan()`. Integration half: `SharedLearningStore` with two textually empty learnings that have distinct ids (`SharedLearning{ id: "a".into(), ..Default::default() }` and `id: "b"`; two plain `default()` values share the empty id and the second `insert` at :198 overwrites the first, so the comparator would never run), inserted via `insert`, then `store_with_dedup(SharedLearning{ id: "c".into(), title: "x".into(), ..Default::default() })`; expect panic "called `Option::unwrap()` on a `None` value" at store.rs:242. Untested precondition: whether `MarkdownLearningStore::save` (reached via `persist` at :190-193) accepts an empty title; the markdown backend was not read. If it rejects, populate the index directly in the test.
- Cross-refs: Phase 1 note B (concurrency surface only; this site was not listed). Same-file safe variant at :497-500.

### F-005 Markdown frontmatter parse accepts `NaN` importance and feeds a sort

- File:line: crates/terraphim_agent/src/learnings/capture.rs:433 (`"importance_total" => imp_total = value.parse().ok()`), :436 (recency, same), :462-473 (struct assembled when all five fields parse), :326/:332 (writer uses `{:.4}`, which prints `NaN` for NaN), :1445-1463 (`list_all_entries` comparator).
- Kind: Deserialize-produced invalid state (bucket 12(e)) feeding a comparator.
- Bucket(s): 12(e), 12 (comparator).
- Severity: CONTRACTUAL-BUT-DEFENSIBLE.
- Static evidence: `str::parse::<f64>` accepts `NaN`, `nan`, `inf`, `infinity` (std `f64: FromStr`). The in-tree writer cannot produce NaN: `ImportanceScore::calculate` (:127-146) takes `recency` as a literal `1.0` from its only production caller (:1083) and the other terms are bounded literals, so `total` is finite. The parser is therefore only reachable with a hand-edited or externally generated `~/.../learnings/*.md`. Once parsed, the comparator `ib.partial_cmp(&ia).unwrap_or(Equal).then_with(..)` at :1455-1458 treats NaN as equal to everything, which is not transitive; with three or more scored entries the post-1.81 sort may panic. `ImportanceScore` is also `Deserialize` (:104) but the JSONL route (`procedures.jsonl`, :1434-1439) goes through serde_json, which rejects a `NaN` token, so the markdown frontmatter is the only route.
- Draft experiment: fixture directory under the scratchpad with three learning markdown files whose frontmatter carries `importance_total: NaN` (plus finite `importance_severity`, `importance_repetition`, `importance_recency` and `importance_has_correction: false` so :462-470 yields `Some`), call `list_all_entries(&dir, 10)`; record whether driftsort panics. Property test: `CapturedLearning::from_markdown` over frontmatter values drawn from `prop_oneof!["NaN", "inf", "-inf", "1e400", "0.5"]` and assert `importance.total.is_finite()` (expected to fail, which is the finding).
- Cross-refs: same shape as F-003; Phase 1 note B does not cover this file.

### F-006 Paragraph finder mixes char indices with byte offsets

- File:line: crates/terraphim_agent/src/commands/markdown_parser.rs:423 (`&content[paragraph_start..paragraph_end]`), :433-449 (`find_paragraph_start`), :452-470 (`find_paragraph_end`), :421 (`start + 20`).
- Kind: str char-boundary slicing; char-count-to-byte-index confusion.
- Bucket(s): 12, cross-tag 11.
- Severity: CONTRACTUAL-BUT-DEFENSIBLE (latent: `analyze_content` is called only from `parse_content_with_analysis` at :175, whose only caller is the test at :1222, and from the test at :1326).
- Static evidence: `pos` is a byte offset from `find_matches` (:419). `find_paragraph_start` walks `start -= 1` and inspects `content.chars().nth(start - 1)`, i.e. it indexes characters with a byte position; the returned `start` is then used as a byte index. `find_paragraph_end` starts at `pos + 20`, which is not boundary-aligned, and walks by one. On non-ASCII content either bound can land inside a code point, and `:423` panics. The `chars().nth()` in a loop is also O(n^2) in the paragraph length.
- Draft experiment: the bug needs the `\n\n` detection to return a char-count index that, read as a byte index, lands inside a multi-byte run, so the content must contain a paragraph break after non-ASCII text. Construction: `"terraphim " + "\u{00e9}".repeat(11) + "\n\n" + "tail"` with a thesaurus containing `terraphim` (match at bytes 0-9). The prefix is 10 bytes and 10 chars; the run is bytes 10-31 (chars 10-20); the double newline is chars 21-22 (bytes 32-33). `find_paragraph_end(content, 20)` inspects char 20 (an `e-acute`), then char 21 and 22 (both `\n`), and returns 23. Byte 23 is offset 13 into the run, odd, a continuation byte; `find_paragraph_start(content, 0)` returns 0; the slice `[0..23]` at :423 panics with "byte index 23 is not a char boundary". Without the paragraph break the walk runs to `content_len`, which is a boundary, and nothing fires. Proptest over `(String, usize)` for `find_paragraph_end(content, pos)` asserting `content.is_char_boundary(result)`.
- Cross-refs: Phase 1 note B (not listed).

### F-007 Fixed-byte display truncation on input-controlled strings (grouped)

- File:line: crates/terraphim_agent/src/repl/handler.rs:994 (`&paragraph[..97]`), :1995 and :2226 and :2321 (`&t[..40]` on session titles), :2051 and :2098 (`&t[..50]`), :2172 (`&msg.content[..80]`), :2585 (`&first_msg.content[..100]`); crates/terraphim_agent/src/main.rs:2774 (`&local.command[..60]`, `shared-learning import`); crates/terraphim_agent/src/learnings/procedure.rs:427 (`&first[..60]`, command captured from a session).
- Kind: str char-boundary slicing on input-controlled text.
- Bucket(s): 12, cross-tag 11.
- Severity: SUSPICIOUS, low impact (each is a display path that panics one REPL command or one CLI invocation).
- Static evidence: each site guards only `len() > N` and slices at byte `N`. Titles, message contents and shell commands are user-controlled UTF-8. The same file already carries the correct idiom at handler.rs:2780-2786 and :2795-2801 (`char_indices().take(n).last()`), and `cli_helpers::truncate_snippet` (:19-29) exists for exactly this purpose.
- Bounds, not boundary: handler.rs:2241 and :2329 (`&session.id[..8]`) panic on `id.len() < 8`. Ids from the native, cursor, codex and opencode connectors are prefixed (`claude-code-native:`, `cursor:`, `codex:`, `opencode:prompt-history`, always 8 or more bytes and ASCII in the prefix); the aider (`session_id.clone()`, aider.rs:163) and cline (`item.id.clone()`, cline.rs:379) id sources were not traced to their origin.
- Draft experiment: one parameterised test that feeds two inputs through each truncation closure: `"\u{00e9}".repeat(60)` (lead bytes at even offsets, so only the odd cut at :994, byte 97, panics) and `"a" + "\u{00e9}".repeat(60)` (lead bytes at odd offsets, so the even cuts at 40, 50, 60, 80 and 100 panic). Expect no panic at the `char_indices` sites (:2780-2786, :2795-2801). Recommend a Phase 4 sweep replacing all of them with `cli_helpers::truncate_snippet`.
- Cross-refs: terraphim-session-analyzer Tier A sites in Phase 1 note I (reporter.rs:419, :427; analyzer.rs:686; connectors/cursor.rs:323, :378) are the same family and are not repeated here.

### F-008 Rank drift note (bucket 12/25 cross-tag, brief)

Recorded because the lead asked for it; another agent owns the symptom. What the trace established, and what it did not:

- Rolegraph ranks are `u64` integer sums (`node.rank + edge.rank + document_rank`, rolegraph lib.rs:706, aggregated at :712/:720 into an `AHashMap`). Integer addition is order-independent, so the aggregation itself cannot drift.
- The only float sum over hash iteration order found is the trigger fallback cosine at rolegraph lib.rs:174 (`query_tfidf.values()...sum`), :187 (`doc_tfidf.values()...sum`) and :193-199 (`dot` over `query_tfidf.iter()`), all `AHashMap<&str, f64>`. That feeds `similarity >= self.threshold` (:201) and a `partial_cmp` sort (:207), not the printed rank directly.
- The float-to-integer truncation that touches the printed rank is terraphim_service search.rs:1006, `rank + (tfidf_score * 0.3) as u64`. `TFIDFScorer::score` (terraphim_types bm25_additional.rs:160-187) sums over the query-term `Vec`, so the sum is deterministic for a fixed corpus.
- The plausible chain, not verified at runtime: `results.into_iter()` from `AHashMap` (rolegraph lib.rs:730) followed by a stable `sort_by_key(Reverse(rank))` (:731) leaves equal-rank documents in hash order, then `.take(limit)` at :736 selects a different subset per process (assuming the default ahash `RandomState`; the ahash feature set was not checked). `TFIDFScorer::initialize` (bm25_additional.rs:132-157) then sees a different `doc_count` and different document frequencies, `idf = ln(doc_count / n)` changes, and the truncated `(tfidf_score * 0.3) as u64` moves by one or two. The printed value is `doc.rank` at terraphim_agent main.rs:1322 and :3133.

## Non-findings (proved safe or out of scope for this bucket)

| Site | Claim checked | Evidence |
|---|---|---|
| `terraphim_automata::find_matches` positions (registry matcher.rs:22-72) as byte offsets on the same string | Positions are char boundaries of the caller's `text` | `ac.find_iter(text)` runs on the original `&str` (no lowercasing or normalisation of `text`; :55-64); patterns are `&str` (valid UTF-8), filtered to `len() >= 2` and non-blank (:16, :29-41), so no zero-width matches; `ascii_case_insensitive(true)` folds only `a-z`/`A-Z` (aho-corasick 1.1.4 ahocorasick.rs:2442-2456: "for ASCII letters (`a-z` and `A-Z`) only"), so matched bytes equal the pattern bytes up to ASCII case. A match therefore starts with a UTF-8 lead byte and ends after a complete code point; by UTF-8 self-synchronisation both `mat.start()` and `mat.end()` are boundaries of `text`. No normalisation changes string length. |
| terraphim_grep/src/hybrid_searcher.rs:146-147 (`text[..start]`, `text[end..]`) | boundary contract | positions from `find_matches(text, ..)` on the same `text` (:132); proof above. Single-character concepts are filtered by the min-length rule and simply produce no match. |
| terraphim_negative_contribution/src/scanner.rs:62 (`&content[start..end.min(content.len())]`) | boundary contract | `find_matches(content, .., true)` at :35 on the same `content`; `pos` always `Some`. |
| terraphim_negative_contribution/src/scanner.rs:140-171 | boundary contract | `line_starts` are byte offsets immediately after `b'\n'` (ASCII, always a boundary); `end` from `str::find('\n')` on a boundary-aligned suffix. |
| terraphim_agent/src/main.rs:1055 and :1074 (`is_at_word_boundary`, `replace_range(start..end)`) | boundary contract | positions from `ReplacementService::find_matches(&input_text)` (terraphim_hooks/src/replacement.rs:127-136), which calls `terraphim_automata::find_matches` on the same string; LeftmostLongest matches are non-overlapping and are applied in reverse `pos` order (:1069). `cli_helpers::is_at_word_boundary` (:116-136) slices only at those positions. |
| terraphim_lsp/src/kg_analysis.rs:70, :93-115 | boundary contract | `text.get(start..end)` is checked (:70); the unknown-word walk advances by `word.len()` of `split` pieces plus `len_utf8()` of the delimiter char (:110-116), so `byte_offset` is always a boundary. |
| terraphim_grep/src/signatures.rs:15-23, :53; terraphim_agent/src/learnings/capture.rs:447-449, :713-727; learnings/hook.rs:238-269; shared_learning/validation.rs:128-137; wiki_sync.rs:749; markdown_store.rs:817; repl/handler.rs:104 | boundary contract | every index derives from `str::find` of an ASCII needle or from `+ needle.len()` after such a find, which lands on a boundary. |
| terraphim_agent/src/cli_helpers.rs:19-29; robot/output.rs:281-294; terraphim_sessions/src/search.rs:80-85; connector/cursor.rs:401-416; repl/handler.rs:2780-2786, :2795-2801 | boundary contract | explicit `is_char_boundary` walk or `char_indices().take(n).last()`. |
| repl/handler.rs:2369-2374 (`&date[..10]`, `[..4]`, `[5..7]`) | boundary contract | `date` is `strftime("%Y-%m-%d")` output, ASCII by construction. |
| terraphim_agent/src/main.rs:2468, :2551, :2599, :2705 (`&id[..id.len().min(12)]`) | bounds and boundary | generated ids are `format!("{}-{}", Uuid::new_v4().simple(), timestamp_millis())` (capture.rs:234, :544; terraphim_types shared_learning.rs:464), ASCII and 32+ bytes. `min(12)` covers bounds. Not covered: an id hand-edited into markdown frontmatter (`id:` is parsed back at capture.rs:433-441 region) that is non-ASCII at byte 12; recorded as an assumption, not traced. |
| terraphim_sessions/src/search.rs:135-139, :197-201 comparator | total order in practice | `Scored::set_score` asserts `score.is_finite()` (terraphim_types scored.rs:125-128) and `with_score` routes through it (:134-137); every score in `scored` passes through `with_score` (:131) or `set_score` (:193). `SearchResults::normalize` (:29-42) divides by a non-zero top score. NaN and infinity are unreachable, so `partial_cmp` always returns `Some`. The registry's manual `Ord for Scored<T>` (scored.rs:189-193, `partial_cmp().unwrap()`) is consistent with `PartialEq` (:183-188) under the same finite invariant. |
| `OkapiBM25Scorer::score` (terraphim_types bm25_additional.rs:75-113) | NaN unreachable | `n_docs_with_term > 0` implies at least one non-empty body, so `avg_doc_length > 0` (:44-49); `length_norm >= 1 - b = 0.25` (common.rs:21-29, `b: 0.75`), so `k1 * length_norm + tf > 0`; `idf = ln(x + 1)` with `x > 0`. Finite for all inputs. |
| terraphim_agent/src/shared_learning/store.rs:500 comparator | NaN filtered | `.filter(|(score, _)| *score > 0.0)` at :497 discards NaN before `sort_by`. The `suggest` path (:552-561) uses the same filter and no sort on floats. |
| terraphim_agent/src/memory_command.rs:513 comparator | NaN unreachable | `RubricScore::composite` (:956-964) is a weighted sum of `score_memory_item` outputs (:977-1017): literals, `0.8.min(0.5 + n * 0.1)`, `1.0.min(60 / (1 + days))` with `days >= 0` (:1020-1027), `compute_risk` literals. |
| terraphim_agent/src/commands/registry.rs:697-701 comparator | NaN unreachable | `match_score` is `AutocompleteResult.score` from `autocomplete_search` (registry autocomplete.rs): `score as f64` of the FST `u64` value 1 (:118, :224), `1.0 / (1.0 + d)` (:297-300) or `strsim::jaro_winkler * 0.8` (:378-392); all finite. |
| terraphim_mcp_server/src/lib.rs:460-465, :1327 | total order | `usize`/`u64` and `String` comparisons. |
| terraphim-session-analyzer f32 comparators (kg/search.rs:89-93, main.rs:864) | NaN unreachable | Phase 1 note I item 5: `concepts.len() as f32 + literal bonus`, finite. |
| terraphim_agent/src/judge/family.rs:59-62 (`impl Iterator<Item = &'static str>`) | size_hint contract | returns `BANNED_MODEL_PREFIXES.iter().copied()`, i.e. `core::iter::Copied<slice::Iter>`; `size_hint` is std's exact `(len, Some(len))`. No manual `Iterator` impl exists in the workspace. |
| `#[derive(PartialEq, Eq, Hash)]` consistency | derived impls agree | zero manual impls of Hash/PartialEq/Eq/Ord/PartialOrd/Borrow anywhere in `crates/` (rg above); derives are field-wise and consistent by construction. No HashMap key type derives `Eq` without `Hash` (would not compile) and none contains a float. |
| terraphim_grep `SufficiencyMetrics` f64 fields (sufficiency_judge.rs) | not sorted or hashed | compared only against thresholds; a NaN metric makes every threshold test false (falls to the LLM path), which is a behaviour question outside this bucket. |
| terraphim-session-analyzer `confidence: f32` accepting NaN from TOML (Phase 1 note I item 8) | not sorted or hashed | unchanged; no invariant consequence. |

## Summary for the lead

- Zero UB in this bucket; the reviewed surface is derived trait impls plus safe-Rust panics.
- Two SUSPICIOUS panics reachable from ordinary session data: F-001 (enricher byte window) and F-002 (session summary at byte 100, on the `sessions search` path). One grouped SUSPICIOUS set of display truncations (F-007).
- Four CONTRACTUAL-BUT-DEFENSIBLE items: F-003 (NaN only via a directly constructed or non-JSON-deserialised `relevance_score` in terraphim_grep; a NaN concept score is masked by the `f64::min` cap at :126), F-004 (`partial_cmp().unwrap()` with a constructible empty-corpus NaN, test-only callers today), F-005 (`importance_total: NaN` accepted from markdown frontmatter), F-006 (char/byte index confusion, test-only callers today).
- The three Phase 1 hand-off sites are resolved: terraphim_sessions search.rs:135,197 is total in practice (finite invariant enforced by `Scored`); terraphim_grep 207-212/430-434 is F-003; the `find_matches` char-boundary contract is proved for hybrid_searcher.rs:146-147, scanner.rs:62 and the hooks replacement path.
- Rank drift: no float-sum-over-hash-order feeds the printed integer rank directly; the candidate mechanism is a hash-ordered tie set truncated by `take(limit)` changing the TF-IDF corpus (F-008), left for the perf agent to confirm.
