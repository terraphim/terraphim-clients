# terraphim-agent reference

A complete enumeration of every top-level subcommand of
`terraphim-agent`, with one worked example per command and a pointer
to the README, the source, or the relevant ADR.

This file exists to close the documentation gap surfaced in
`terraphim-clients#130`: the README listed 8 commands under "Key
Commands" but `terraphim-agent --help` ships 22 top-level
subcommands. The remaining 14 are documented below.

For the high-level overview see `crates/terraphim_agent/README.md`.
For architectural decisions see `adr/`.

## Conventions

* `--robot --format json` are **global flags** that must precede
  the subcommand (see `crates/terraphim_agent/README.md` "Robot /
  automation output"). Putting them after the subcommand errors
  with `unexpected argument '--robot' found`.
* Every subcommand supports `--help` for the full flag list.
* Most subcommands support `--role <name>` to pick a knowledge
  graph role; otherwise the default role from `~/.config/terraphim/settings.toml`
  is used.

## Core commands (README "Key Commands" set)

These are documented in the README.

| Command | Summary | README anchor |
|---------|---------|---------------|
| `search` | Search documents using the knowledge graph | Quick Start |
| `graph` | Display the knowledge graph for a role | KG tools |
| `validate` | Validate text against the KG | Quick Start |
| `replace` | Replace terms in text using the thesaurus | KG tools |
| `hook` | Unified hook handler (PreToolUse / PostToolUse / pre-commit / prepare-commit-msg) | Hooks |
| `guard` | Check a command against safety guard patterns; `--explain` prints the trace | Safety guard |
| `learn` | Capture / list / replay procedural learnings | Learning |
| `sessions` | Import and search Claude Code / Aider / Cursor session history | Sessions |

## The remaining 14 commands

### `roles`

Manage the active knowledge-graph role (list, show details, select).

```bash
# List all configured roles with their haystacks
terraphim-agent roles list

# Select a role for the current invocation
terraphim-agent --role "Terraphim Engineer" search "guard priority"
```

`roles select` updates the default role in `settings.toml`.

### `config`

Inspect and modify the running configuration. Subcommands:
`show`, `set`, `validate`, `reload`.

```bash
# Pretty-print the full configuration as JSON
terraphim-agent config show

# Set a config key (dotted path)
terraphim-agent config set default_role "Terraphim Engineer"
```

`config show` and `config validate` are stateless — they do not
build the thesaurus (Refs #120).

### `kg`

Alias of `graph` plus KG-management helpers (list concepts, dump
thesaurus entries).

```bash
terraphim-agent kg --top-k 5
```

### `chat` (CLI, `--features llm` — default-on)

One-shot chat with the AI for a specific role. Takes a required
`prompt` argument and optional `--role` and `--model` flags. Always
available in default builds (the `llm` feature is on by default). This
is the top-level CLI subcommand; the interactive `/chat` REPL command
is a separate entry in `robot schemas` with `repl_only: true`. Refs
terraphim-clients#134 P1.

```bash
# One-shot chat with the active role
terraphim-agent chat "What is the guard priority order?"

# Chat scoped to a specific role and model
terraphim-agent --role "Terraphim Engineer" chat "Summarise the ADR-002 rationale" --model gpt-4o-mini
```

### `chat` (REPL-only, `--features repl-chat`)

Open an interactive chat REPL scoped to a role. The REPL command is
what consumers should expect; see `terraphim-agent robot schemas`
(the entry with `repl_only: true`). Distinct from the CLI `chat`
subcommand above.

```bash
terraphim-agent --features repl-chat chat
```

### `extract`

Extract paragraphs from text that match knowledge-graph terms.
Output is the matched paragraph followed by the term that triggered
the match.

```bash
terraphim-agent extract "The guard pipeline runs in three stages: allowlist, destructive, and suspicious."
```

### `replace`

Replace KG-known substrings in arbitrary text. Unlike the hook's
inline replacement, `replace` is a one-shot CLI you can invoke on
files or stdin.

```bash
echo "npm install foo" | terraphim-agent replace
# bun add foo
```

### `validate`

Validate a piece of text against the active role's knowledge graph:
reports terms that have known alternatives, terms that have no
match, and connectivity (whether the terms co-occur in the graph).

```bash
terraphim-agent validate --connectivity "guard pipeline runs in three stages"
```

### `suggest`

Suggest similar terms using fuzzy matching over the thesaurus.
Useful for typo recovery and for discovering alternative
spellings.

```bash
terraphim-agent suggest "guard priorty" --limit 5
```

### `interactive`

Start the fullscreen TUI (requires a running Terraphim server).
Use `--server --server-url` to point at a non-default server.

```bash
terraphim-agent --server --server-url http://127.0.0.1:8000 interactive
```

### `repl` (`--features repl`)

Start the line-oriented Read-Eval-Print-Loop. The REPL exposes
every `CommandDoc` entry from `terraphim-agent robot schemas`
including the REPL-only ones (`vm`, `chat`, `summarize`,
`autocomplete`).

```bash
terraphim-agent --features repl repl
```

### `setup`

First-time setup wizard. Prints a list of templates (each with a
one-line description); `--add-role <id>` wires a template into the
local `settings.toml`.

```bash
terraphim-agent setup --list-templates
terraphim-agent setup --add-role terraphim_engineer
```

### `check-update`

Stateless: queries the configured update backend (R2 by default,
GitHub releases as fallback) and prints the version status without
installing. Useful in CI.

```bash
terraphim-agent check-update
```

### `update`

Same as `check-update` but downloads and replaces the running
binary if a newer version is available. Verifies the archive
signature against the embedded keys (see `adr/ADR-001`).

```bash
terraphim-agent update
```

### `learn`

Manage the procedural-learning store. Subcommands:
`list` (recent learnings), `capture` (record a new one),
`hook` (auto-capture from PostToolUse failures), `correct`
(replace a stale learning), `replay` (run a procedure).

```bash
terraphim-agent learn list --recent
terraphim-agent learn capture "use bun instead of npm install"
```

### `sessions`

Import and search AI coding-assistant session history from
Claude Code, Cursor, and Aider. Subcommands: `import`, `search`,
`stats`, `list`.

```bash
terraphim-agent sessions import
terraphim-agent sessions search "guard priority"
```

### `listen`

Start the offline listener mode that accepts agent commands over a
local socket. Useful for tooling that prefers IPC over spawning
subprocesses.

```bash
terraphim-agent listen --socket ~/.terraphim.sock
```

### `cache`

Manage the compiled thesaurus cache. Subcommands: `list` (cached
roles), `clear` (force rebuild), `info` (size, age).

```bash
terraphim-agent cache list
terraphim-agent cache clear --role "Terraphim Engineer"
```

### `robot`

Robot mode self-documentation. Subcommands: `capabilities`,
`schemas`, `examples`. Use `--robot --format json robot schemas`
to machine-discover every command, including the `repl_only` flag
introduced in `terraphim-clients#131`.

```bash
terraphim-agent --robot --format json robot schemas | jq '.[].name'
```

## Quick reference

| Family | Commands |
|--------|----------|
| **Search & KG** | `search`, `graph`, `kg`, `validate`, `suggest`, `replace`, `extract` |
| **Configuration** | `roles`, `config`, `setup`, `cache` |
| **Safety** | `guard`, `hook` |
| **Interactive** | `interactive`, `repl`, `chat` (REPL-only) |
| **Update** | `check-update`, `update` |
| **Learning** | `learn` |
| **Sessions** | `sessions` |
| **IPC** | `listen` |
| **Self-doc** | `robot`, `help` |

## References

* Source: `crates/terraphim_agent/src/main.rs` (`Cli`, `Command` enum)
* README: `crates/terraphim_agent/README.md`
* Robot schemas: `crates/terraphim_agent/src/robot/docs.rs`
* Design doc: `docs/plans/design-terraphim-grep-agent-fixes-2026-08-30.md`
* ADRs: `adr/ADR-002-guard-priority-order.md`, `adr/ADR-003-pretool-hook-rewrite.md`

## Blog posts

* [`docs/blog/terraphim-agent-sessions.md`](blog/terraphim-agent-sessions.md) —
  importing Claude Code / Cursor / Aider session history
* [`docs/blog/terraphim-agent-setup.md`](blog/terraphim-agent-setup.md) —
  onboarding wizard and the 10 templates
* [`docs/blog/terraphim-agent-robot-mode.md`](blog/terraphim-agent-robot-mode.md) —
  JSON output, exit codes, schema self-documentation
* [`docs/blog/terraphim-agent-shared-learning.md`](blog/terraphim-agent-shared-learning.md) —
  markdown-backed BM25-deduped learning store
* [`docs/blog/terraphim-update-r2-backend.md`](blog/terraphim-update-r2-backend.md) —
  R2 update backend with GitHub fallback
