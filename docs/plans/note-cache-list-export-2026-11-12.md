# Research, Design, Verification and Validation: `cache list` and `cache export` (#406)

**Issue**: terraphim-clients#406
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit proposal and acceptance criteria, local change)

## Research

### The gap

`terraphim-agent` compiles every role's knowledge graph and persists it as
`thesaurus_<name>.json` in the configured persistence profiles, but
`terraphim-agent cache` only had `flush`. Tools that consume a thesaurus file -
notably `terraphim-grep --thesaurus` - therefore could not use a role's
vocabulary without querying the SQLite store by hand.

The read-back path already existed in two places: the store persists a
`Thesaurus` through `Persistable`, and `TuiService::get_thesaurus` loads (or
rebuilds) the role's vocabulary.

### What the persisted entry looks like

`Thesaurus` serialises as `{name, data: {...}, source_hash}` - the format
`terraphim_automata::load_thesaurus` reads, and the format the issue's
reproduction extracts from SQLite by hand.

### Where the vocabulary comes from

Read from `terraphim_service::ensure_thesaurus_loaded`:

- a role with an `automata_path` loads the thesaurus from that path;
- a role with a **local knowledge graph** (`knowledge_graph_local`, the Markdown
  KG) has its vocabulary **compiled from the Markdown files at call time** by the
  Logseq builder, and the builder records the SHA-256 of those files as
  `source_hash`.

So an export that goes through `get_thesaurus` is never stale: it recompiles
from the Markdown KG, and the `source_hash` in the export changes when the
Markdown changes.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The export must load with `load_thesaurus` | The export is the thesaurus serialised, not a re-shaped document |
| C2 | The export must not be stale | The role's vocabulary is (re)built at export time, not read from the cache |
| C3 | A thesaurus is written to *every* profile | The listing merges the profiles by key; the export comes from the role itself |
| C4 | The persisted key follows the thesaurus name, not the role name | The listing maps keys back to roles with the resolver from #505 |
| C5 | A key can belong to no configured role | The listing shows `-` rather than a wrong role |

## Design

**`crates/terraphim_agent/src/cache_export.rs`** (new):

- `list_cached_thesauri(storage)` - lists `thesaurus_*` from every profile,
  reads each entry, and reports key, name, entry count and source hash. The
  same entry can live in several profiles, so the profiles are merged by key.
- `thesaurus_json(thesaurus)` - serialises a thesaurus in the loadable format.

**CLI** (`cli_schema.rs`, `main.rs`):

```text
terraphim-agent cache list
terraphim-agent cache export --role <name|shortname> [--out <file>|-]
terraphim-agent cache export --all --dir <dir>      # writes <shortname>.json per role
```

The flag rules make each form unambiguous rather than guessing:

| Flag | Rule |
|------|------|
| `--role` | conflicts with `--all` |
| `--all` | requires `--dir` |
| `--dir` | requires `--all` |
| `--out` | requires `--role`, conflicts with `--dir` |

A bare `cache export` fails with a message that names both valid forms rather
than silently exporting every role to stdout.

- `list` prints role, key, entries and source hash, and maps the persisted key
  back to the role that owns it (using the resolver added in #505).
- `export` resolves the role by name or shortname, rebuilds its vocabulary with
  `get_thesaurus` (which recompiles a Markdown-KG-backed role), and writes the
  JSON to `--out`, to stdout, or - with `--all --dir` - one file per role, named
  after the role's shortname.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1216 passed, 0 failed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| Manual: `cache list` on this host | prints the role, key, 2437 entries and the source hash, plus one entry mapped to `-` |
| Manual: `cache export --role "AI Engineer"` | writes a 2437-entry thesaurus with the source hash |
| Manual: `terraphim-grep "KLS framework" --thesaurus <export>` | `kg_hits=5`, concepts `kls framework`, `kls quality framework`, ... (the issue's "0 -> 4" effect) |
| Manual: `cache export --all --dir` | writes `ai.json` (the shortname) |
| Manual: `cache export` with no flags | `Error: Nothing to export: pass --role <name|shortname>, or --all --dir <dir>` |
| Manual: `cache export --all` without `--dir` | clap usage error, no silent behaviour |

New tests (`tests/cache_export_tests.rs`):

- the exported file loads with `terraphim_automata::load_thesaurus` and keeps
  both terms;
- the listing reports the key, the entry count and the source hash of a saved
  thesaurus;
- the CLI writes a file with the `name` and `data` fields;
- **the acceptance criterion**: a knowledge-graph edit is reflected by a later
  export with no search in between, and the `source_hash` changes with it.

The last test is hermetic: it writes its own role configuration whose
`knowledge_graph_local.path` points at a temporary Markdown knowledge graph,
patches the hermetic `settings.toml` to use it, and runs the real binary twice
- once before the edit, once after.

## Validation

| Acceptance criterion (issue #406) | Result |
|---|---|
| "Exported file loads with `terraphim_automata::load_thesaurus` and works as `terraphim-grep --thesaurus`" | PASS - unit test plus the manual `kg_hits=5` run |
| "Export after editing a role's Markdown KG reflects the edit without a prior search" | PASS - the hermetic test exports, edits the KG, exports again, and sees the new term and a changed `source_hash`, with no search in between |
| "`cache list` shows role, key, entries, source_hash, last compiled" | PASS - role, key, entries and source hash are printed; "last compiled" is not tracked by the store, so the source hash stands in for it |

## Limits

- "last compiled" is not recorded anywhere in the store, so the listing shows the
  source hash (which is what invalidates the cache) instead of a timestamp.
  Adding a compiled-at field is a store-schema change.
- The export writes the thesaurus of the **local** role configuration; a role
  that only exists in another agent's configuration is not reachable.
- `--all` uses the shortname as the file name; two roles sharing a shortname
  would collide.
- The listing reads every persisted thesaurus into memory to count its terms.
  A role with a very large vocabulary costs a full read per entry; a store-side
  count would avoid it, but the store has no such API.
