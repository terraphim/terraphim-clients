# Research, Design, Verification and Validation: the grep CLI contract defects (#515)

**Issue**: terraphim-clients#515
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13
**Mode**: `remediation` (measured defects from the post-merge review, local change)

## Research

The issue reports three P1s and a set of P2s from the structural review of the
merged PRs. The three P1s change what a user sees for the same command in a
different directory or with overlapping roots.

### P1 — the role-blind filesystem heuristic defeats `--role`

`crates/terraphim_grep/src/main.rs`, `find_default_thesaurus`:

```rust
for entry in entries.flatten() {
    let name = entry.file_name();
    let name_str = name.to_string_lossy();
    if name_str.ends_with("_thesaurus.json") {
        return Some(candidate.join(&name));   // any file, any role
    }
}
```

The heuristic takes the **first** `*_thesaurus.json` it finds, whatever role
it belongs to. The issue's evidence:

> CASE A (role-blind file present): `--role code-reviewer` -> concepts: [],
> 0 chunks. CASE B (file removed, same command/config): concepts: ['code
> review', 'regression risk', 'rollback plan'].

So an unrelated thesaurus silently replaced the requested role's vocabulary -
and the log line that could have shown it (`tracing::debug!`) was invisible at
the default level.

### P1 — overlapping `--paths` roots produce duplicate paths

`crates/terraphim_grep/src/hybrid_searcher.rs`, `search_file_names`: each root
is scanned and every match is pushed into one vector, with no de-duplication.

> `terraphim-grep 'js' --files --paths /tmp/pr491 --paths /tmp/pr491/src` ->
> 4 output lines, 2 distinct paths, each listed twice.

The duplicates also eat the limit: `-n 2` over two overlapping roots returned
two lines that were the **same** file twice, so the user saw one file where
they asked for two.

### P1 — the limit is applied per root

The same function passes the user's limit into fff-search's pagination, so each
root is truncated to the limit *before* the results are pooled, sorted and
truncated again.

### P2 — `check-update` exits 0 when the check fails

`terraphim-grep check-update` prints `[ERROR] Update failed: ... GitHub API
rate limit exceeded` and exits **0**, while `terraphim-grep update` exits 1 for
the same condition. The exit-code contract documented in #484 says a failure is
a failure.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | A requested role must not be replaced by another role's vocabulary | The heuristic accepts only role-scoped files when the role is known |
| C2 | Without a known role the heuristic has always guessed | The guess is kept, but it is logged at INFO |
| C3 | One path per line | The pooled results are de-duplicated by path |
| C4 | The limit is a limit on distinct files | The limit is applied once, over the pooled set |
| C5 | The exit-code contract is documented | A failed check exits non-zero |

## Design

**`crates/terraphim_grep/src/main.rs`**

- `name_tokens(text)` lowercases and splits a name on its separators, so
  `AI Engineer`, `ai-engineer` and `ai_engineer` are the same tokens.
- `thesaurus_file_matches_role(file_name, role_name)` accepts a file when the
  role's tokens appear in it as a **contiguous run** - `code_reviewer_thesaurus`
  matches the role "Code Reviewer", `unrelated_thesaurus` does not.
- `role_is_unknown(role_name, role_requested)` is true only when neither
  `--role` nor the project configuration named a role (the resolution falls
  back to "default"). In that case no file can be role-scoped, and the
  heuristic keeps its old behaviour - the guess it has always made.
- `find_default_thesaurus(role_name, role_requested)` therefore accepts a
  role-scoped file always, and any file only when the role is unknown.
- The "Loaded thesaurus from project file" log is at **INFO** when the loaded
  file is not scoped to the role, so a wrong-role load is never silent.
- `check-update` now classifies the status the same way `update` does: a
  failed check prints the failure on stderr and exits 1.

**`crates/terraphim_grep/src/hybrid_searcher.rs`**

- The per-root pagination limit is removed (`limit: 0`, which fff-search reads
  as "no pagination"), so the user's limit is applied once, over the pooled
  results of all roots.
- The pooled results are de-duplicated by path after the sort, keeping the
  first (highest-scoring) occurrence.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_grep --all-targets --no-fail-fast` | 139 passed, 0 failed |
| `cargo clippy -p terraphim_grep --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

New tests:

- `tests/file_search.rs`: overlapping roots list a path once; the limit counts
  **distinct** files across roots; both roots are represented in the results.
- `tests/role_fallback.rs`: a thesaurus file for another role does not replace
  the requested role's vocabulary; a role-scoped file is still found.
- `src/main.rs`: a failed update check is classified as a failure.

The new tests were checked against the **old** code: the two `--files` tests
and the "another role" test fail there and pass with the fix.

## Validation

| Acceptance criterion (issue #515) | Result |
|---|---|
| "Make the filesystem heuristic role-aware" | PASS - only a role-scoped file is accepted when the role is known |
| "raise the 'Loaded thesaurus from project file' log to INFO when the loaded file is not role-scoped" | PASS |
| "De-duplicate `matches` on `path` after the global sort" | PASS |
| "apply the score sort plus limit once over the pooled set" | PASS |
| "`check-update` ... exit non-zero for a failed check" | PASS |

## Limits

- The remaining P2s of the issue are **not** addressed here: the widened chunk
  content is still truncated at 512 bytes per line while `line_start`/`line_end`
  claim the full span; the dedup key still hashes the widened content; the
  `all` haystack still labels every non-documentation file as `code`; the
  extension allowlists still omit several common source types; and `--files`
  still includes hidden files only inside a git repository.
- The role-scoped test matches the role's tokens as a contiguous run, so a role
  whose name is a single common word ("ai") can still match a longer file name
  that starts with it ("ai_engineer_thesaurus"). The project convention
  (`thesaurus-<role>.json`) is matched exactly, so this only affects the
  filesystem heuristic.
- The JSON output does not carry the thesaurus's provenance (path, role, entry
  count), so a caller cannot see which vocabulary was loaded. The log line can
  now say it, but the machine-readable answer would need a schema change.
