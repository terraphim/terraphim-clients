# Onboarding wizard: 10 templates for first-time setup

The Terraphim agent ships with ten curated role templates so a
newcomer can go from `cargo install terraphim_agent` to a working
knowledge graph in under a minute. This post walks through the
wizard.

## Quick start

```bash
# Show the template catalogue
terraphim-agent setup --list-templates

# Add a template to your local settings.toml
terraphim-agent setup --add-role terraphim_engineer
```

The wizard writes a `role_config` entry to
`~/.config/terraphim/settings.toml` pointing at the template's JSON
config. On the next `terraphim-agent search` invocation the role is
loaded and indexed.

## The template catalogue

| Template id | Description |
|-------------|-------------|
| `terraphim_engineer` | Rust + Terraphim KG, default for engineering work |
| `frontend_engineer` | React + TypeScript + Tailwind + Zustand |
| `backend_engineer` | Axum / actix / tonic, Postgres, sqlx |
| `data_engineer` | Polars / DuckDB / Arrow |
| `ml_engineer` | Hugging Face + sentence-transformers |
| `devops` | Caddy + Cloudflare Workers + R2 |
| `security` | OWASP-aligned threat-modelling KG |
| `technical_writer` | mdBook + Vale + prose linting |
| `researcher` | arXiv + Connected Papers + Zotero |
| `personal_assistant` | Apple Notes + Reminders + Calendar |

Each template bundles:

* A curated thesaurus (synonyms → canonical concepts)
* A default haystack (the directories and knowledge sources the
  role searches by default)
* Pre-flight connectivity checks that catch missing tools
  (`bun` not installed, `cargo` not on PATH, etc.)

## What gets written

`terraphim-agent setup --add-role <id>` appends to
`~/.config/terraphim/settings.toml`:

```toml
[[roles]]
id = "terraphim_engineer"
config = "/usr/local/share/terraphim/templates/terraphim_engineer.json"
default_data_path = "~/.local/share/terraphim"
```

## Re-running the wizard

The wizard is idempotent. Running `setup --add-role
terraphim_engineer` twice does not duplicate entries — it merges.

## References

* Source: `crates/terraphim_agent/src/onboarding.rs`
* Reference: `docs/agent-reference.md` (`setup`)
