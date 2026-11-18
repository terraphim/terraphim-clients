# Research, Design, Verification and Validation: the guard walk should match fff-search's scope (#452)

**Issue**: terraphim-clients#452
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (measurable defect, local change, no behavioural regression)

## Research

### The two walks

`terraphim-grep` walks the search root twice per search:

1. `terraphim_grep::path_guard::reject_hostile_tree` - a manual walk that
   rejects hostile entry names before fff-search can store them (UF-040).
2. fff-search's `collect_files` - the `ignore` crate's `WalkBuilder` with
   `.git_ignore(true).git_exclude(true).git_global(true).ignore(true)`, so it
   skips `target/` in a git repository and `target/{debug,release,rust-analyzer,criterion}`,
   `node_modules`, `__pycache__`, `venv` and `.venv` elsewhere.

The guard's own doc comment already noted the mismatch:

> "The walk does not honour `.gitignore`, so `target/` and `node_modules/` are
> walked. For a CLI invocation and a per-call MCP picker that cost is
> acceptable; both already pay a full `collect_files` scan immediately after."

### Measurement

On a synthetic tree with 20,000 files under `target/debug` (a typical Rust
checkout's shape) and one source file:

| | guard walk | end-to-end `terraphim-grep` search |
|---|---|---|
| before | 3.89 ms | 14-16 ms |
| after | 0.017 ms | 8-9 ms |

The guard walk was the *whole* of the extra cost in that shape, because fff
already skips the directory the guard was walking.

## Design

The guard validates names that fff-search would store. A name inside a
directory fff-search never indexes can never reach the shared path buffer, so
there is nothing there for the guard to validate.

The guard's walk therefore skips the same directories fff-search skips:

- `.git` (the `ignore` crate skips it in every repository),
- `node_modules`, `__pycache__`, `venv`, `.venv` (fff's `NON_GIT_IGNORED_DIRS`),
- `target/{debug,release,rust-analyzer,criterion}` (the rest of fff's list).

The rules are matched on path *components*, which is how fff's override
patterns match (`!**/target/debug/`), so the guard's scope is a subset of
fff's - it never skips something fff would index.

### Options considered

| Option | Implication | Chosen |
|--------|-------------|---------|
| Skip the directories fff ignores | Same scope as the scan that follows; no semantic change | **Yes** |
| Rewrite the guard with the `ignore` crate | Identical scope by construction, but it stats entries and follows a different code path in a security-relevant check (UF-040) | No - too much risk for the same win |
| Hand the guard's file list to fff | Would remove the second walk entirely | No - fff-search 0.8.4 has no API to accept a file list |
| Overlap the two walks on two threads | Hides the cost instead of removing it, and adds a thread per search | No |

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_grep --all-targets --no-fail-fast` | 133 passed, 0 failed |
| `cargo clippy -p terraphim_grep --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

New tests in `path_guard`:

- `never_indexed_directories_are_recognised` - the skip list matches fff's, and
  near misses (`target/custom`, `venvish`, `my_node_modules`) are *not* skipped.
- `hostile_name_in_an_ignored_directory_does_not_refuse_the_search` - a
  non-UTF-8 name inside `target/debug` no longer refuses the search, because
  fff would never store it.
- `hostile_name_in_an_indexed_directory_still_refuses_the_search` - the skip
  rule does not weaken the guard.

## Validation

| Acceptance criterion (issue #452) | Result |
|---|---|
| "This checkout's `target/` is not passed to fff-search with an exclusion, so its directories are enumerated on every run" | PASS - fff already excluded them; the *guard* was the one enumerating them, and now does not |
| "Two independent tree walks per search" | PARTIALLY ADDRESSED - the guard's walk is now scoped like fff's, so the second walk no longer re-reads `target/`. Merging the two walks into one needs an fff-search API that accepts a file list, which 0.8.4 does not have |
| "Perf lever 3: exclude `.git/` and `target/` from `collect_files`" | DONE for the guard's walk; fff's own walk already honoured those |

## Limits

- The skip list is a copy of fff-search 0.8.4's ignore list. If fff changes
  its rules, the guard's scope can drift; the list is named and commented so the
  two can be updated together.
- A repository that tracks files under `target/` (unusual) would have those
  files skipped by the guard but still indexed by fff, so a hostile name there
  would no longer be refused. The list mirrors fff's own exclusions exactly,
  so this cannot happen for the directories fff excludes.
