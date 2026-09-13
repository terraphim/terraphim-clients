# Phase 6 idea wizard, round 2 of 3 (ADVERSARIAL lens)

Run: 2026-09-11-terraphim-clients-1 (Exhaustive; archetype pure-safe-with-env-unsafe; branch task/254-ci-pipeline, HEAD ae06cd2). Written by the idea-wizard-orchestrator, 2026-09-12 BST. Source unmodified; no commits made. Inputs read before writing: phase6_idea_wizard_round_1.md (STRUCTURAL), phase4_unified_findings.md (39 UF rows plus three lead addenda), UNDEFINED_BEHAVIOR_EXPERIMENT_DESIGNS.md (28 blocks at round start), phase2_summary.md, phase3_dynamic_findings.md, phase1_notes/A..J.md, and the fff-search 0.8.4, self_update 0.42.0, semver 1.0.28, heed 0.22.1, terraphim_automata 1.21.1 and libgit2-sys/libsqlite3-sys bundled-C registry sources. Four scratch probes were run under `CARGO_TARGET_DIR=/private/tmp/claude-501/ubx-exp-target` (learnings-hook slicing, nested-input parsers, and the do-now checks below); web lookups covered the SQLite CVE table, the libgit2 release notes and the libgit2-sys crates.io version list.

## Lens (verbatim)

> ADVERSARIAL. Inputs an adversary or a careless user could craft: NUL bytes and control bytes in pattern, thesaurus and config strings; alias and id collisions; timing-dependent state transitions; schema-version mismatches; malicious or oversized session JSONL (sessions import/export); hostile TOML/JSON configs; crafted release archives and manifests against terraphim_update (zipsign is the gate; also path traversal in unpack, symlink entries, version-string parsing); hostile MCP requests to terraphim_mcp_server (rmcp 0.9.1, stdio and streamable HTTP); hostile git repositories reached through git2 (fff-search Repository::discover); hostile markdown through html2md; environment-variable injection (TERRAPHIM_*, HOME, XDG_*); and any input path that can reach an `unsafe` boundary inside a dependency (memmap2 via fff-search, ahash, aho-corasick, rusqlite, libgit2, self_update).

Prompt applied: "Come up with your very best ideas for clever, non-obvious UB-detection techniques that are specifically suited to THIS codebase, viewed through the ADVERSARIAL lens. Consider its specific architecture. Generate 30 ideas. Score each on PROVABILITY (1-5), CRATE-LEVEL IMPACT (1-5), NOVELTY VS EXISTING FINDINGS (1-5). Do NOT cut to a top-5; investigate every single idea."

## Facts established during the round (load-bearing for several ideas)

- **fff-search stores paths lossily.** Scanned relative paths are `Path::to_string_lossy().into_owned()` (file_picker.rs:232,352,406); no non-UTF-8 filter sits between the `ignore` walker and arena storage. On Linux each invalid filename byte inflates to a three-byte `U+FFFD`. `read_to_buf` (simd_path.rs:170-186) truncates at `PATH_BUF_SIZE = 4096` (simd_path.rs:63) then `from_utf8_unchecked`. This underpins EXP-201.
- **libgit2-sys bump is lockfile-only.** git2 0.20.4 requires `libgit2-sys = "0.18.3"` (`^0.18.3`); crates.io has `libgit2-sys 0.18.8+1.9.7` (2026-08-21, libgit2 1.9.7). `cargo update -p libgit2-sys --precise 0.18.8+1.9.7` needs NO fff-search change; #257 is not the blocker for the libgit2 C-library CVE fixes. Bundled version today is 1.9.4 (version.h).
- **SQLite is 3.46.0 with FTS5 on** (libsqlite3-sys 0.30.1 build.rs `-DSQLITE_ENABLE_FTS5`), a single copy shared by rusqlite 0.32 and sqlx-sqlite 0.8.6. No attacker-controlled SQL text anywhere (all query strings are compile-time literals), so CVE-2025-6965/29087/3277 are unreachable. terraphim_persistence does not reach FTS5 (`rg -ni fts5|MATCH` over its src = only Rust `match`). The only crafted-db-file surface is the Cursor connectors (EXP-206).
- **serde_json and toml 0.8 are recursion-limited; toml 0.5 is not.** Scratch probe: 100k-deep nested input gives serde_json `recursion limit exceeded` and toml 0.8 a parse error, but toml 0.5.11 overflows the stack and aborts. toml 0.5.11 is reached only through `twelf` -> terraphim_settings (registry), so it is a DoS robustness note, not workspace UB.
- **The learnings-hook slicing panics on non-ASCII prompts** and silently corrupts ASCII input with leading whitespace. Scratch probe (`scratchpad/advprobe`): `"\u{2003}\u{2003}use bun instead of npm"` panicked at byte 4 inside `U+2003`; `"use \u{0130}\u{0130} instead of npm"` panicked out of bounds; `"  use bun instead of npm"` returned `("npm","e bun")`. This underpins EXP-204.
- **No Web haystack in this workspace.** `rg` for a web/ripgrep/queryrs `ServiceType` haystack or an html2md call site returns only onboarding template strings, so hostile-markdown-through-html2md is INAPPLICABLE for this workspace (html2md is a licence finding, terraphim/terraphim-ai#3374).

## The 30 ideas

Scores: P = provability, I = crate-level impact, N = novelty versus existing findings (1 to 5 each).

| Id | Idea (technique, target) | P | I | N |
|---|---|---|---|---|
| I-201 | Hostile non-UTF-8 filenames inflate the lossy path past `PATH_BUF_SIZE`; `read_to_buf` truncates mid-`U+FFFD` and returns an invalid `str` via `from_utf8_unchecked` (fff-search, Linux) | 4 | 5 | 5 |
| I-202 | Hostile YAML frontmatter reaches `unsafe-libyaml` 0.2.11 through serde_yaml 0.9 (command md, shared learnings); Miri-runnable | 4 | 4 | 5 |
| I-203 | Attacker-influenced manifest version string reaches semver 1.0.28 inline-storage `unsafe` before the zipsign gate; Miri-runnable | 4 | 3 | 4 |
| I-204 | `parse_correction_pattern` applies transformed-string byte offsets to the source string; panic on non-ASCII prompt, silent corruption on ASCII whitespace (safe panic) | 5 | 3 | 4 |
| I-205 | Hostile local git repo (oversize delta result-size header, pathological `.git/config`) reaches libgit2 1.9.4 via `Repository::discover` + status | 3 | 3 | 4 |
| I-206 | Crafted SQLite db file reaches SQLite 3.46.0 C reader through the Cursor connectors (page/btree, FTS5 CVE-2025-7709) | 3 | 3 | 4 |
| I-207 | `FFF_FRECENCY_PATH` env injection points heed/LMDB `unsafe open` at a hostile mmap'd `data.mdb` | 3 | 2 | 4 |
| I-208 | User-controlled grep needle (NUL bytes, crafted lengths) through the fff-search `case_insensitive_memmem` SIMD kernels | 4 | 3 | 4 |
| I-209 | NUL/control bytes in tools.toml patterns beyond the empty string (aho-corasick literal match at boundaries) | 4 | 2 | 2 |
| I-210 | Hostile session JSONL (deep nesting, giant lines, non-object `extra`) through `sessions import`/`export` | 4 | 2 | 2 |
| I-211 | Crafted release archive against tar `unpack`, zip `mangled_name`, and the zipsign gate ordering | 4 | 3 | 2 |
| I-212 | Schema/config that forces `NormalizedTerm` id collisions; any consumer indexing a `Vec` by id | 4 | 1 | 2 |
| I-213 | NUL/control bytes in `.terraphim/thesaurus-<role>.json` keys into `terraphim_automata::find_matches` | 4 | 2 | 3 |
| I-214 | Control-byte patterns into the session-analyzer-built automaton (matcher.rs) at non-boundary slice | 5 | 2 | 2 |
| I-215 | toml 0.5.11 stack overflow via `twelf` on hostile `settings.toml` | 5 | 1 | 3 |
| I-216 | serde_json recursion / giant scalar on hostile config and session JSONL | 5 | 1 | 2 |
| I-217 | Hostile MCP request (rmcp 0.9.1) over stdio and streamable HTTP | 4 | 2 | 3 |
| I-218 | MCP negative `i64` `limit`/`skip` cast `as i32 as usize` to a huge value | 5 | 1 | 2 |
| I-219 | Env injection (TERRAPHIM_*, HOME, XDG_*) binding `DEVICE_STORAGE` to an attacker path or dangling dir | 4 | 2 | 1 |
| I-220 | self_update `set_ssl_vars!` env writer triggered on a hostile-reachable update path | 3 | 3 | 1 |
| I-221 | Hostile markdown through html2md / pulldown-cmark | 5 | 1 | 2 |
| I-222 | Hostile `.git/config` parsed by libgit2 as `discover` walks up parent directories | 3 | 2 | 3 |
| I-223 | ReDoS / catastrophic backtracking in the redaction regex set on crafted session content | 5 | 1 | 3 |
| I-224 | Interior-NUL path into `write_relative_cstr` `CStr::from_bytes_with_nul_unchecked` (fff-search) | 4 | 2 | 3 |
| I-225 | Alias/name collisions in `CommandRegistry` from hostile command `.md` files | 4 | 1 | 2 |
| I-226 | Deep-nested JSON inside Aider markdown / Cursor SQLite values | 5 | 1 | 2 |
| I-227 | fff-search bigram `offset_from` / `get_unchecked` with a crafted file set | 4 | 1 | 1 |
| I-228 | Timing-dependent `OnceLock` init (`KG_THESAURUS`, `COMPILED_PATTERNS`) under adversarial concurrency | 4 | 1 | 1 |
| I-229 | Crafted manifest `assets` map with a traversing asset filename into the download path | 5 | 1 | 2 |
| I-230 | NUL bytes in the MCP `path` argument reaching fff-search `base_path` / `write_absolute_path` | 4 | 2 | 2 |

## Per-idea verdicts

Each entry: classification; action; EXP id if filed. Evidence cited as file:line.

- **I-201** NEW_EXP_PROMOTED. Lossy storage (file_picker.rs:232,352,406), no non-UTF-8 filter before arena storage (scan.rs/file_picker.rs static read), truncate-then-`from_utf8_unchecked` (simd_path.rs:170-186, comment at :168 admits truncation), `write_absolute_path` passes the 4096 array (types.rs:337-356). Base path attacker-chosen via `--paths` and MCP `path`. Linux only (APFS rejects invalid names). **EXP-201, verdict OPEN.**
- **I-202** NEW_EXP_PROMOTED. serde_yaml 0.9.34 -> unsafe-libyaml 0.2.11 (pure-Rust c2rust), reached at markdown_parser.rs:143 (dir walk :572), injector.rs:248, markdown_store.rs:348 (`TERRAPHIM_LEARNINGS_DIR`). Miri can execute the transpiled pointer code; no phase has run it. **EXP-202, verdict OPEN.**
- **I-203** NEW_EXP_PROMOTED. `is_newer_version_static` -> `semver::Version::parse` on `m.version`/`release.version` at lib.rs:264,312,440,1304, before `verify_archive_signature` at lib.rs:359,783. semver 1.0.28 identifier.rs holds 43 `unsafe` tokens (inline SSO). The value even if Miri is clean is the ordering (unsigned data parsed pre-signature), routed to Phase 8. **EXP-203, verdict OPEN.**
- **I-204** NEW_EXP_PROMOTED (as NOT-UB robustness, on the EXP-103/EXP-012 precedent). Transformed-copy offsets applied to source (hook.rs:235,238-240,268-270), reached from hook.rs:205 with prompt text. Mechanism CONFIRMED in scratch. Safe panic and silent corruption; cannot feed an unsafe boundary. **EXP-204, verdict OPEN.**
- **I-205** NEEDS_DEEPER_INVESTIGATION. libgit2 1.9.4 (version.h); reachable delta/config surface via `discover`+status (phase1_notes/F); network-only CVEs unreachable (no fetch/revparse/TLS). Deeper check (crafted repo + ASan on bigbox) exceeds 5 minutes. **EXP-205, verdict DEFERRED.** Lockfile bump to 0.18.8+1.9.7 is the independent remedy (see facts).
- **I-206** NEEDS_DEEPER_INVESTIGATION. Cursor connectors `Connection::open` on an attacker-chosen path (cursor.rs:163 / :156), SQLite 3.46.0 + FTS5. Crafted-db-file corpus + ASan on bigbox needed; the bump needs rusqlite 0.34+ (0.32 pins libsqlite3-sys 0.30). **EXP-206, verdict DEFERRED.**
- **I-207** NEEDS_DEEPER_INVESTIGATION. `FFF_FRECENCY_PATH` -> `FrecencyTracker::open` (mcp lib.rs:109-118) -> heed 0.22.1 `unsafe open` (lmdb.rs:154) over LMDB 0.9.70; heed docs warn a broken lock file "may introduce UB". Corrupted-`data.mdb` corpus + ASan on bigbox needed. **EXP-207, verdict DEFERRED.**
- **I-208** NO_EVIDENCE (with bounds argument); NOT covered by EXP-104's review. `search_packed_pair`/`search` (case_insensitive_memmem.rs) reached at grep.rs:1281 with the user needle. Guards: `n == 0` -> true, `n > hlen` -> false; every `verify_dispatch(ptr.add(candidate))` has `candidate <= last_start = hlen - n`; NEON/AVX2 loops bounded by `max_offset = hlen - (max_idx + 16|32)`; the tail slice `haystack[tail_start..tail_end]` is a checked slice with `tail_end = last_start + rare_pos + 1 <= hlen`. `verify_scalar/avx2/neon` read exactly `needle_lower.len()` bytes from an in-bounds start. No unguarded read found. EXP-104's notes covered grep.rs:525-569 (`ascii_case_eq`), a different function; this file was unreviewed and is now recorded.
- **I-209** NO_EVIDENCE (I-08 carry-over, generalised). aho-corasick 1.1.4 is byte-based, so a NUL or control byte in a non-empty pattern matches literally and, because the pattern is valid UTF-8, only at char boundaries (terraphim_automata matcher.rs `is_word_boundary_match` slices only at match boundaries; the argument at matcher.rs:33-38 holds). The only unfiltered-empty-pattern path is EXP-103; NUL/control bytes add no new non-boundary mechanism.
- **I-210** NO_EVIDENCE (I-09 carry-over). Round-1 probe: scalar/array `extra` fails serialisation with `Err`, so `sessions export` (handler.rs:2446) propagates via `?`, no panic. Import: `str::lines()` + per-line `serde_json::from_str`; serde_json recursion limit 128 (scratch probe) caps nesting; no unsafe, no index slicing. Giant lines are an allocation/DoS note, not UB.
- **I-211** NO_EVIDENCE (I-21 carry-over; gate now demonstrated). Signature is verified before extraction in every path: lib.rs:359->382, :783->810, and self_update update.rs:282->301. `extract_tarball` pulls only the entry whose `file_name() == bin_name` (lib.rs:1148-1173), so tar paths cannot escape; zip `mangled_name` is Windows-only. flate2 backend is zlib-rs (pure Rust), not C zlib. Asset filename via `rsplit_once('/')` cannot traverse and a wrong name fails the zipsign context. Robustness note only: `verify_archive_signature` `fs::read`s the whole archive (signature.rs:159).
- **I-212** NO_EVIDENCE (I-13 carry-over). `rg` for `[id as usize]`-style indexing = 0; kg/builder.rs:88 assigns colliding ids on duplicate synonym keys, but the only id consumer is the `HashSet<u64>` dedup at mcp lib.rs:445. No `Vec` is indexed by id, so a collision is a correctness issue (same family as UF-029), not an unsafe boundary.
- **I-213** NO_EVIDENCE. Thesaurus keys flow to `terraphim_automata::find_matches`, which drops empty and sub-`MIN_FIND_PATTERN_LENGTH` (2) keys (matcher.rs:61-78) and slices only at match boundaries (`is_word_boundary_match`, matcher.rs:290-320). Control bytes in a >=2-byte valid-UTF-8 key match literally at char boundaries; no non-boundary slice.
- **I-214** ALREADY_COVERED (EXP-103, generalised). The session-analyzer automaton (matcher.rs:97-120) skips the empty-key filter; that is EXP-103. Control bytes in a non-empty pattern add no new mechanism for the same reason as I-209.
- **I-215** NO_EVIDENCE for UB; Phase 8 DoS note. Scratch probe: toml 0.5.11 stack-overflows and aborts on 100k-deep nesting; serde_json and toml 0.8 return errors. toml 0.5.11 is reached only through `twelf` -> terraphim_settings (registry). Abort is defined behaviour; route alongside rmcp/quick-xml DoS notes.
- **I-216** NO_EVIDENCE. serde_json 1.0.150 enforces a recursion limit of 128 by default (scratch probe: `recursion limit exceeded`); no `disable_recursion_limit`/`unbounded_depth` anywhere (`rg` = 0). Giant scalars are allocation, not UB.
- **I-217** NO_EVIDENCE. rmcp 0.9.1 src has zero `unsafe` (`rg -c unsafe` = 0). Hostile requests reach serde_json (recursion-limited) and the manual `match`-based tool dispatch (mcp lib.rs:1694), all safe Rust. rmcp's DNS-rebinding advisory (#256) is a network-security DoS, not UB.
- **I-218** NO_EVIDENCE. `limit`/`skip` are `as_i64().map(|i| i as i32)` then `as usize` (mcp lib.rs:2381-2387, :226-227). A negative value becomes a huge `usize`, which the search treats as "no limit" and the `idx >= limit` loop bound (lib.rs:247) tolerates. No unsafe consumer, no indexing by the value.
- **I-219** ALREADY_COVERED (UF-002, UF-023). `DEVICE_STORAGE` binds to the first HOME/TERRAPHIM_DATA_PATH observed; env injection is the same shape those rows characterise. No new mechanism.
- **I-220** ALREADY_COVERED (UF-037, EXP-105). The self_update SSL env writer on the production update-check path is EXP-105 (OPEN); a hostile trigger does not change the writer/reader pair.
- **I-221** INAPPLICABLE for this workspace. No Web haystack or html2md call site (`rg` = 0 outside onboarding template strings). pulldown-cmark 0.13 is safe Rust with `default-features = false`. html2md is a licence finding (terraphim/terraphim-ai#3374), not a UB surface here.
- **I-222** NEEDS_DEEPER_INVESTIGATION; folded into EXP-205. `Repository::discover` walks up parent directories and libgit2 reads any `.git/config` it encounters; a pathological config (deep `include.path` chains) is part of the EXP-205 crafted-repo gate rather than a separate experiment.
- **I-223** NO_EVIDENCE. The `regex` crate (1.12.4) guarantees linear-time matching with no backtracking, so the redaction pattern set (redaction.rs:30-70) cannot catastrophically backtrack. Patterns are compiled once (`OnceLock`, redaction.rs:55). A large input is O(n), an allocation note at worst.
- **I-224** NO_EVIDENCE. `write_relative_cstr` (types.rs:366-379) reserves the last byte for NUL and asserts filesystem paths carry no interior NUL. The lossy path storage cannot introduce a NUL (`U+FFFD` has no NUL byte) and the OS rejects interior NULs at creation, so `from_bytes_with_nul_unchecked` sees exactly one trailing NUL. The `str`-validity hazard on the same buffer is EXP-201, not this.
- **I-225** NO_EVIDENCE for UB; correctness only. `CommandRegistry::register_command` is last-writer-wins on the name; a collision overwrites, and readers tolerate the torn alias/command shapes with `Option` (UF-020/UF-021). No unsafe.
- **I-226** NO_EVIDENCE. Aider markdown parsing and Cursor value JSON both go through serde_json's recursion-limited parser (I-216); no index slicing on the parsed structure in unsafe code.
- **I-227** ALREADY_COVERED (EXP-104 notes). `offset_from`/`get_unchecked` in the bigram path (grep.rs:1040,1976,2104) run only under `enable_content_indexing`, which every workspace call site leaves at its `false` default; the reviewed argument in EXP-104's notes stands.
- **I-228** ALREADY_COVERED (round-1 I-25). `KG_THESAURUS` (capture.rs:733) and `COMPILED_PATTERNS` (redaction.rs:55) are `OnceLock`s whose initialisers do only I/O or regex compilation and never re-enter; `OnceLock` re-entrancy is defined (deadlock/panic), not UB.
- **I-229** NO_EVIDENCE. The download filename is `asset_url.rsplit_once('/')` (lib.rs:340-345), which cannot yield a traversing component, and staging is a fresh `tempfile::tempdir()`; a mismatched name fails the zipsign filename context anyway. Covered by the EXP-211 gate argument.
- **I-230** NO_EVIDENCE (feeds I-201). A NUL byte in the MCP `path` argument makes a `PathBuf` that `std`/opendal reject before fff-search stores it; it cannot reach `write_absolute_path`. The invalid-`str` hazard on that buffer is the inflation path (EXP-201), not an interior NUL.

## Round summary

| Classification | Count | Ideas |
|---|---|---|
| NEW_EXP_PROMOTED | 4 | I-201 (EXP-201), I-202 (EXP-202), I-203 (EXP-203), I-204 (EXP-204, NOT-UB) |
| NEEDS_DEEPER_INVESTIGATION | 4 | I-205 (EXP-205), I-206 (EXP-206), I-207 (EXP-207), I-222 (folded into EXP-205) |
| ALREADY_COVERED | 5 | I-214, I-219, I-220, I-227, I-228 |
| NO_EVIDENCE | 16 | I-208, I-209, I-210, I-211, I-212, I-213, I-215, I-216, I-217, I-218, I-223, I-224, I-225, I-226, I-229, I-230 |
| INAPPLICABLE | 1 | I-221 |

New EXP blocks filed: 7 (EXP-201 OPEN, EXP-202 OPEN, EXP-203 OPEN, EXP-204 OPEN, EXP-205 DEFERRED, EXP-206 DEFERRED, EXP-207 DEFERRED). Registry now holds 35 blocks; `lint-experiment-designs.py` prints `[OK] ... all blocks well-formed` (exit 0).

Honest reading of the round: 22 of 30 ideas land ALREADY_COVERED, NO_EVIDENCE or INAPPLICABLE, consistent with a pure-safe workspace whose own code has no `unsafe` outside the six env-setter blocks. The four promotions all sit where attacker-controlled input reaches a dependency's `unsafe` on a path the dependency's own tests never exercise: a fixed stack buffer defeated by lossy path inflation (EXP-201, the round's only fresh LIKELY-UB candidate), a c2rust YAML parser fed hostile frontmatter (EXP-202), an inline-SSO version parser run before the signature gate (EXP-203), and a transformed-offset string slice (EXP-204, safe panic on the EXP-103 precedent). The three deferrals are all bundled-C libraries (libgit2, SQLite, LMDB) reachable from a crafted file or repo, each gated on an ASan build on bigbox.

## Non-UB notes for Phase 8 (not registry material)

- **libgit2-sys bump is lockfile-only** to 0.18.8+1.9.7 (git2 accepts `^0.18.3`); it clears the 1.9.5-1.9.7 CVEs (delta allocation, submodule traversal, smart-protocol OOB, TLS SAN, redirect credential leak) without a fff-search change. Correct phase1_notes/J's "needs fff-search bump (#257)" for the C-library version.
- **SQLite bump needs rusqlite 0.34+** (0.32 pins libsqlite3-sys 0.30 = SQLite 3.46.0); 3.50.3+ closes CVE-2025-7709 (FTS5). No SQL-text CVE is reachable (all queries are literals).
- **toml 0.5.11 stack overflow** on deeply nested `settings.toml` through `twelf`; route with the rmcp/quick-xml DoS notes. serde_json and toml 0.8 are recursion-limited.
- **serde_yaml 0.9 is deprecated/unmaintained**; regardless of EXP-202's outcome, migrating command and learning frontmatter parsing to a maintained YAML crate removes the largest attacker-reachable pure-Rust `unsafe` parser.
- **EXP-204 fix**: compute the correction offsets on the same string that is sliced; the current code corrupts even ASCII prompts with leading whitespace.
- **EXP-203 defence-in-depth**: verify the archive signature before parsing the manifest version string.

## Carry-over to next round (CROSS-SYSTEM lens)

Better examined under the CROSS-SYSTEM lens (two processes, two runtimes, or CI hosts):
- **EXP-201 and the AVX2 memmem path (I-208) are bigbox-only.** Invalid-UTF-8 filenames cannot exist on the macOS audit host; the path-inflation `from_utf8_unchecked` and the x86_64 AVX2 memmem kernel both need a Linux runner to observe. A CROSS-SYSTEM probe should run EXP-201 on bigbox where `MMAP_THRESHOLD` is 4 KiB and far more files map.
- **MCP over streamable HTTP with a hostile `path` argument** combines EXP-201 (inflation), EXP-104 (live mmap with `watch: false`) and EXP-205 (libgit2 status over a hostile repo) in one long-lived server process; that concurrency is invisible to the single-shot CLI probes.
- **EXP-205/206/207 are all bundled-C-library ASan gates on bigbox**; they need the crafted-input corpora (packfile/`.git/config`, SQLite db, LMDB `data.mdb`) and an `-Zsanitizer=address -Zbuild-std` build that only the Linux runner supports.
- **EXP-203's ordering** (unsigned manifest parsed before the signature check) matters most when the update endpoint is MITM-able across the network boundary, which is the CROSS-SYSTEM framing of the same finding.
- **EXP-207's env-injection precondition** (`FFF_FRECENCY_PATH`) is a two-process concern: whoever launches the MCP server controls its environment, so the CROSS-SYSTEM lens should ask which launchers set it and whether the path is ever attacker-writable.
