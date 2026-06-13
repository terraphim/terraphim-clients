# terraphim_agent

Terraphim AI Agent CLI — semantic search, knowledge-graph exploration, and an interactive REPL for AI coding workflows.

## Overview

`terraphim_agent` is the primary command-line interface for Terraphim. It combines offline-capable local search with optional server-backed fullscreen TUI mode, knowledge-graph validation, hook integration for AI coding assistants, and operational learning capture.

Install the binary as `terraphim-agent`.

## Features

- **Semantic search** across configured haystacks with KG-aware ranking
- **Interactive REPL** with markdown-defined commands and autocomplete
- **Knowledge graph tools** — validate, replace, extract, and visualize concepts
- **Session search** — import and query AI coding assistant session history
- **Operational learning** — capture corrections, query learnings, replay procedures
- **Safety guards** — block destructive git/filesystem commands before execution
- **Robot mode** — structured JSON output for automation and agent orchestration

## Installation

### From crates.io

```bash
cargo install terraphim_agent
```

### From source

```bash
git clone https://git.terraphim.cloud/terraphim/terraphim-clients.git
cd terraphim-clients
cargo build -p terraphim_agent --release --bin terraphim-agent
```

The release binary is at `target/release/terraphim-agent`.

### Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `repl-interactive` | Yes | Interactive REPL with rustyline |
| `llm` | Yes | Ollama and LLM router integration |
| `repl-sessions` | Yes | Session import and search |
| `server` | No | HTTP client for server-backed TUI |
| `repl-full` | No | All REPL sub-features (chat, MCP, web, file) |
| `shared-learning` | No | Cross-agent learning store and wiki sync |
| `enrichment` | No | Concept enrichment for session search |

Build with extra features:

```bash
cargo build -p terraphim_agent --release --features repl-full,server
```

## Quick Start

```bash
# Show available commands
terraphim-agent --help

# Semantic search (offline)
terraphim-agent search "async error handling"

# List and select roles
terraphim-agent roles list
terraphim-agent roles select "Terraphim Engineer"

# Validate text against the knowledge graph
terraphim-agent validate --connectivity "haystack service uses automata"

# Start the REPL
terraphim-agent repl

# First-time setup wizard
terraphim-agent setup
```

### Server-backed TUI

When a Terraphim server is running:

```bash
terraphim-agent --server --server-url http://127.0.0.1:8000 interactive
```

### Robot / automation output

```bash
terraphim-agent search "retry policy" --robot --format json
```

## Configuration

On first run, the agent reads `settings.toml` from the platform config directory
(`~/.config/terraphim/` on Linux). Point `role_config` at a JSON role definition
to bootstrap roles and haystacks:

```toml
role_config = "/path/to/terraphim_engineer_config.json"
default_data_path = "~/.local/share/terraphim"
```

Use `terraphim-agent config show` to inspect the active configuration and
`terraphim-agent config validate` to check role definitions.

## Key Commands

| Command | Purpose |
|---------|---------|
| `search` | Semantic document search |
| `graph` | ASCII knowledge-graph visualization |
| `validate` | KG connectivity and checklist validation |
| `replace` | Thesaurus-based text replacement |
| `hook` | Pre/post-tool-use hook handler for AI assistants |
| `guard` | Safety pattern check for shell commands |
| `learn` | Operational learning capture and query |
| `sessions` | Import and search AI coding sessions |

## Testing

```bash
# Unit tests
cargo test -p terraphim_agent --lib

# Integration tests (hermetic, no server required)
cargo test -p terraphim_agent --test offline_mode_tests
cargo test -p terraphim_agent --test exit_codes
```

## Related Crates

- [`terraphim-cli`](https://crates.io/crates/terraphim-cli) — lightweight CLI for scripted search
- [`terraphim_grep`](https://crates.io/crates/terraphim_grep) — hybrid grep with KG boost and LLM fallback
- [`terraphim_mcp_server`](https://crates.io/crates/terraphim_mcp_server) — MCP tools for editor integration

## License

Apache-2.0