# Local agent memory and learning over MCP

The existing `terraphim_mcp_server` exposes agent tools only when started with an
explicit absolute data directory. Check that your installed server's `--help`
includes `--agent-data-dir`; older releases expose graph tools only.

```sh
terraphim_mcp_server --profile server --config-dir /absolute/project \
  --agent-data-dir /absolute/agent-data
```

This option is available for local stdio transport. Combining it with `--sse`
is refused. Omitting it exposes no `agent_*` tools and does not choose a default
agent store. The graph's role/configuration and the agent store are separate:
a working directory or selected role does not isolate memories.

Each successful result contains `structuredContent.storage` with the effective
canonical root, memory file and learning directory, followed by typed `data`.
The same envelope is described by `outputSchema` in `tools/list`. Inputs reject
unknown fields, incorrect types and out-of-range limits. Persistence failures
return `isError: true`; no success result is generated from input alone.

## Tools

| Tool | Input | Saved result or effect |
| --- | --- | --- |
| `agent_memory_capture` | `content`, optional `provenance_tag` | Saved item with ID, exact authored content and source tag, read back from disk. Existing memory semantics retain the latest 100 short-term items. |
| `agent_memory_show` | Exact `id` | One saved item; an unknown ID is an error. |
| `agent_memory_list` | Optional `limit` (1–100), `offset` | Stable paginated saved records, newest first. This is enumeration, not semantic retrieval. |
| `agent_learning_capture` | Observed `command`, `error_output`, non-zero `exit_code` | A saved failure observation using the existing redaction and learning types. It does not execute the command. |
| `agent_learning_query` | `query`, optional `limit` (1–100) | Case-insensitive text matches over actual failure and correction records. |
| `agent_learning_correct` | Exact failure `id`, `correction` | An annotation on that failure; no compiled rule is created. |
| `agent_correction_add` | `original`, `corrected`, optional `context` | An explicit `ToolPreference` correction record. |
| `agent_correction_compile` | `{}` | Fresh thesaurus JSON, actual mapping count and mappings; `activated` is always `false`. |
| `agent_correction_export_kg` | `{}` | Fresh Markdown concept files containing `synonyms::`; actual paths and saved text; `registered` is always `false`. |

For example, call `agent_memory_capture` with:

```json
{"content":"Risk requires a Decision: approve a second supplier.","provenance_tag":"public-project-brief"}
```

Pass the returned `data.id` to `agent_memory_show`. A provenance tag is a source
label stored on the item; it does not replace the content or independently prove
its provenance.

For an observed failed `printf '%Z'`, retain the real exit code and stderr, and
execute any corrected command separately. Capture the observation, then annotate
its returned ID with `printf '%s\n' 'reviewed correction'`. Compilation still
contains zero rules if no explicit correction exists. Calling
`agent_correction_add` with those original/corrected strings creates the separate
rule; compilation then returns its actual mapping. Exported notes use the same
Logseq-compatible `synonyms::` syntax as other Terraphim knowledge graphs.

Before committing a new learning or correction, the server parses the staged
Markdown with the existing Terraphim parser and checks that its redacted fields
round-trip exactly. Unrepresentable fields (for example multiline command or
annotation text, embedded Markdown delimiters, or a command chain transformed
by the existing capture parser) return an error before a destination record is
changed. Supply the single failing command and inspect the saved result.

Compilation and export create new generation directories. They do not overwrite
an active hook configuration, register a role or activate rewriting. Originals
containing commas/newlines and replacements containing newlines cannot be
represented exactly by the current one-line comma-separated synonym syntax, so
KG export refuses them; JSON compilation remains available.

## Storage and CLI interoperability

The memory snapshot is `<root>/terraphim/evolution/cli-agent.json`; learning
Markdown is in `<root>/terraphim/learnings/`. The explicit MCP flag takes
precedence over any inherited `TERRAPHIM_DEFAULT_DATA_PATH`; it never falls back
to that variable. To read the same files from the CLI, deliberately select the
same root:

```sh
TERRAPHIM_DEFAULT_DATA_PATH=/absolute/agent-data terraphim-agent \
  --format json memory show RETURNED_MEMORY_ID
TERRAPHIM_DEFAULT_DATA_PATH=/absolute/agent-data terraphim-agent learn query printf
```

The CLI preserves its existing environment-variable precedence over its OS-local
configuration root. Without that override, changing directory does not change
its memory store. `learn query` currently emits human-readable text even with
the global JSON output option.

Updated CLI and MCP writers share advisory locks; a busy writer receives an
error and may retry. Older CLI versions do not cooperate with these new locks:
stop those writers before sharing a store with this server. Static symlinks,
traversal, corrupt records and failed readback are refused. These checks do not
isolate storage from another process that already controls the same OS account.

Cancellation is honoured before the store transaction starts, including while
waiting behind another MCP operation. Once a synchronous transaction commits,
a cancellation or connection loss is not rollback; use list/query/readback to
reconcile the store before retrying a non-idempotent mutation. Memory capture and
learning annotation are marked destructive in tool metadata because retention
can remove old short-term items and annotation replaces the selected field.
The server never provides arbitrary command execution or hook installation.

## Verification

The real stdio contract test launches the server and CLI against temporary
stores, checks restart/readback, actual files, schema and read/write metadata,
invalid inputs, held-lock refusals and concurrent requests. It separately runs
a failed and corrected `printf`, then checks zero-to-one compilation and the
exported notes. The dispatcher cancellation test uses the real store mutex and
filesystem. Run after building both binaries:

```sh
cargo build -p terraphim_agent -p terraphim_mcp_server
cargo test -p terraphim_agent --test agent_store_contract --test agent_learning_store_contract
cargo test -p terraphim_mcp_server --test test_mcp_agent_contract
```

`TERRAPHIM_MCP_SERVER_BIN` and `TERRAPHIM_AGENT_TEST_BIN` can select downloaded
released binaries for the same protocol/interoperability tests.
