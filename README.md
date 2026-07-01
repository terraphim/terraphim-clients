# terraphim-clients

Client + integration crates extracted from terraphim-ai (#1910):

- `terraphim-cli` / `terraphim_agent` -- CLI + agent REPL
- `terraphim_mcp_server` -- MCP server
- `terraphim_sessions` -- session import/search
- `terraphim_lsp` -- LSP server (EDM diagnostics)
- `terraphim_grep`, `terraphim_hooks`, `terraphim_update`, `terraphim_command_runtime`, `terraphim_negative_contribution`

Consumes upstream crates (core, config-persistence, service, agents, kg-agents) from the `terraphim` Gitea cargo registry. Licensed Apache-2.0.

## Memory Lifecycle

The `terraphim-agent memory` CLI namespace implements the eight-stage agentic memory lifecycle:

```
terraphim-agent memory capture     # Write a memory item with provenance tags
terraphim-agent memory distill     # Compile learnings into KG entries
terraphim-agent memory scope       # Show role/project KG boundaries
terraphim-agent memory provenance  # Search session history
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

See [MEMORY_POLICY.md](MEMORY_POLICY.md) for the public commons vs permissioned memory boundary.
