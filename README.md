# terraphim-clients

Client + integration crates extracted from terraphim-ai (#1910):

- `terraphim-cli` / `terraphim_agent` -- CLI + agent REPL
- `terraphim_mcp_server` -- MCP server (graph tools and optional [local agent memory/learning tools](docs/mcp/agent-tools.md))
- `terraphim_sessions` -- session import/search
- `terraphim_lsp` -- LSP server (EDM diagnostics)
- `terraphim_grep`, `terraphim_hooks`, `terraphim_update`, `terraphim_command_runtime`, `terraphim_negative_contribution`

Consumes upstream crates (core, config-persistence, service, agents, kg-agents) from the `terraphim` Gitea cargo registry. Licensed Apache-2.0.

## Current downloads

Verified release links as of 18 October 2026. Core services and CLI clients have
separate releases.

| Component | Release | Use |
| --- | --- | --- |
| Core project | [v1.21.3](https://github.com/terraphim/terraphim-ai/releases/tag/v1.21.3) | Stable core/source release |
| CLI clients | [v1.21.22](https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.22) | Stable `terraphim-grep`, agent and client tools |
| Terraphim Editor | [v0.1.0-rc.4](https://github.com/terraphim/terraphim-editor/releases/tag/v0.1.0-rc.4) | Browser editor prerelease |
| VS Code extension | [0.1.7](https://github.com/terraphim/terraphim-ai/releases/tag/terraphim_vscode-v0.1.7) | Prerelease VSIX for compatible editors |
| Cursor plugin | [v0.2.7](https://github.com/terraphim/terraphim-cursor-plugin/releases/tag/v0.2.7) | Native Cursor plugin; separate from the VSIX catalogue |
| Stock Zed extension | [v0.2.1](https://github.com/terraphim/zed-terraphim/releases/tag/v0.2.1) | GitHub download; official Zed catalogue submission remains pending |
| Continue integration | [0.1.1](https://github.com/terraphim/terraphim-ai/releases/tag/terraphim_continue-v0.1.1) | Continue integration package |

For future stable releases, use [latest core
downloads](https://github.com/terraphim/terraphim-ai/releases/latest) or [latest
CLI client
downloads](https://github.com/terraphim/terraphim-clients/releases/latest). Use
the explicit version links above for prereleases.

### From wording alternatives to search

[Try the browser editor](https://terraphim.ai/editor/): add **Concern** as an
alternative to **Risk**, save the annotated Markdown, then export its knowledge
graph. Knowledge notes can also declare aliases with `synonyms:: Risk, Concern`.
Supply the exported graph explicitly to the released `terraphim-grep` client to
retrieve related passages. Synthesis is optional.

Follow the [saved-output
walkthrough](https://terraphim.ai/how-tos/synonym-search/) for the exact search
commands and example files.

For CLI and agent usage, see the [grep README](crates/terraphim_grep/README.md),
[agent README](crates/terraphim_agent/README.md) and [memory capture
example](docs/memory-capture.md).

## Memory Lifecycle

The `terraphim-agent memory` namespace saves authored lessons, inspects the
evolution store and retrieves memories through a configured role's knowledge
graph. See [capture and store isolation](docs/memory-capture.md) for a complete
example and the required input change.

```
terraphim-agent memory capture --content 'Use the project validation command.'
terraphim-agent memory scope       # Show role/project KG boundaries
terraphim-agent memory retrieve    # Search memory items
terraphim-agent memory apply       # Show hook injection effects
terraphim-agent memory validate    # Score items against reliability rubric
terraphim-agent memory retire      # Propose demotion of stale items
terraphim-agent memory rubric      # Full 6-dimension diagnostic
terraphim-agent memory second-run  # Token delta between ADF runs
terraphim-agent memory list        # Browse evolution store
terraphim-agent memory show        # Inspect a specific item
terraphim-agent memory export      # Dump as JSON or markdown
```

The full eight-stage lifecycle is not qualified by these commands. In the
current implementation, `memory distill` and `memory provenance` only report
status; `memory scope --check` does not verify confidentiality. Use explicit
`learn compile` / `learn export-kg` for the separate reviewed-learning store.
`memory apply` previews context; it does not execute a command. Rubric and
validation results use the named `heuristic-v1` scorer.

See [MEMORY_POLICY.md](MEMORY_POLICY.md) for the public commons vs permissioned memory boundary.
