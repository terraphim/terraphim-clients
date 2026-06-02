# terraphim-clients

Client + integration crates extracted from terraphim-ai (#1910):

- `terraphim-cli` / `terraphim_agent` -- CLI + agent REPL
- `terraphim_mcp_server` -- MCP server
- `terraphim_sessions` -- session import/search
- `terraphim_lsp` -- LSP server (EDM diagnostics)
- `terraphim_grep`, `terraphim_hooks`, `terraphim_update`, `terraphim_command_runtime`, `terraphim_negative_contribution`

Consumes upstream crates (core, config-persistence, service, agents, kg-agents) from the `terraphim` Gitea cargo registry. Licensed Apache-2.0.
