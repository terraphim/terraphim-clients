# terraphim_grep

Intelligent hybrid grep with RLM (Retrieval-Language Model) fallback and knowledge-graph curation.

## Overview

`terraphim_grep` is a search crate that combines local code search, knowledge-graph (KG) concept boosting, and optional LLM synthesis into a single pipeline. It retrieves local evidence using the configured thesaurus, applies KG-aware boosting, and optionally synthesizes an answer when explicitly requested and enough evidence is available.

## Features

- **Hybrid Search**: Local code search combined with KG concept lookup
- **Synonym Retrieval**: An exact KG term searches every alias of the same normalized concept
- **KG-Aware Boosting**: Chunks matching thesaurus concepts rank higher than generic matches
- **Sufficiency Judging**: Heuristic-based decision on whether results are sufficient or need LLM synthesis
- **RLM Fallback**: Graceful degradation to search-only mode when no LLM is configured
- **KG Curation**: Optional extraction and indexing of new concepts from LLM responses
- **CLI Binary**: Standalone `terraphim-grep` binary with JSON output support

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
terraphim_grep = { path = "../terraphim_grep" }
```

### Feature Flags

| Feature       | Default | Description                                        |
|---------------|---------|----------------------------------------------------|
| `llm`         | Yes     | Enable LLM integration via `terraphim_service`     |
| `code-search` | Yes     | Enable `fff-search` code retrieval backend          |
| `openrouter`  | No      | Enable OpenRouter provider for live LLM tests       |

> **Note:** `code-search` is enabled by default so the tool greps file contents out of
> the box. If you build with `--no-default-features` and omit `code-search`, file-content
> search is disabled and every query returns no matches; the binary logs a warning on the
> first search to make this obvious.

## Usage

### Library

```rust
use std::sync::Arc;
use terraphim_grep::{
    GrepOptions, Haystack, HybridSearcher, SufficiencyJudge, TerraphimGrep,
};
use terraphim_types::Thesaurus;

#[tokio::main]
async fn main() -> terraphim_grep::Result<()> {
    let hybrid = HybridSearcher::new(
        "my-role".to_string(),
        Thesaurus::new("my-thesaurus".to_string()),
    )?.with_search_path("src".into());

    let grep = TerraphimGrep::new(
        Arc::new(hybrid),
        Arc::new(SufficiencyJudge::default()),
    );

    let result = grep.search(
        "error handling",
        GrepOptions {
            haystack: Haystack::Code,
            max_results: 20,
            ..GrepOptions::default()
        },
    ).await?;

    for chunk in &result.chunks {
        println!("{}: {}", chunk.source, chunk.content.chars().take(80).collect::<String>());
    }

    Ok(())
}
```

### CLI

```bash
# Basic search
terraphim-grep "async fn spawn"

# Search with context lines and JSON output
terraphim-grep "error handling" -C 3 --json

# Force LLM synthesis
terraphim-grep "explain token budget" --force-rlm --answer

# Never touch the LLM, even if OPENROUTER_API_KEY is exported
terraphim-grep "async fn spawn" --search-only

# Search specific paths
terraphim-grep "struct Config" --paths src/ crates/
```

> **LLM synthesis is opt-in.** A plain query always returns retrieved chunks at grep
> speed; the LLM is only called when you pass `--answer` or `--force-rlm`. Having
> `OPENROUTER_API_KEY` in the environment is not, on its own, a request for synthesis
> (terraphim/terraphim-clients#81). Use `--search-only` (alias `--no-rlm`) to also skip
> building the LLM client.

### Use an editor-exported synonym graph

In Terraphim Editor, add `Concern` as an alternative to `Risk`, then open
**Knowledge graph** and download the graph JSON and the document text. Store the
text under a dedicated corpus directory and keep the graph and run receipts
outside that directory:

```text
project/
  knowledge-graph.json
  corpus/
    project-brief.md
```

The editor's `synonyms:: Risk, Concern` group uses Terraphim's existing normalized
term types. With that exported graph, an exact query for `concern` retrieves text
written with `risk`, even when `concern` is absent from the document:

```bash
# Original passages and locations; no synthesis requested
terraphim-grep concern --thesaurus knowledge-graph.json --paths corpus/ --json

# Same retrieval, with a requested source-grounded answer
terraphim-grep concern --thesaurus knowledge-graph.json --paths corpus/ --json --answer
```

Exact concept queries use all aliases, including aliases beyond the displayed
concept limit. `-n` bounds returned passages and displayed concepts; it does not
truncate the synonym group used for retrieval or evidence coverage. Canonical
terms and aliases are searched as literal text, so an alternative containing
`/team/` or `type:rust` cannot become a filename filter. Multiple aliases matching
one source line produce one passage. Queries that are not exactly a KG term keep
the normal query syntax: `concern *.md`, for example, searches literal `concern`
with the Markdown filter and does not expand synonyms.

`--answer` requests synthesis even when the heuristic considers search results
sufficient on their own. The default evidence threshold remains three passages;
a smaller result limit or missing evidence can still make the result
`RlmInsufficient`. Without a configured provider, the result retains the actual
passages, explains that no LLM is available, and leaves `answer` null. Synthesized
answers are instructed to preserve source modality and distinguish a decision
or plan from an action confirmed as completed. Check the returned citations
against the source; instructions alone are not a guarantee of grounding.

## Architecture

```
Query
  │
  ▼
┌──────────────────┐
│  HybridSearcher  │──→ Code search (fff-search) ──┐
│                  │──→ KG concept lookup  ────────┤
└──────────────────┘                               │
  │                                                ▼
  ▼                                    ┌──────────────────┐
┌──────────────────┐                   │  RetrievedChunk  │
│ SufficiencyJudge │◄─────────────────│  + KgConcept      │
│  (heuristics)    │                   └──────────────────┘
└──────────────────┘
  │
  ├── Sufficient ──→ --answer/--force-rlm ? RLM fallback : chunks (SearchOnly)
  ├── NeedsSynthesis ──→ --answer/--force-rlm ? RLM fallback : chunks (SearchOnly)
  ├── NeedsExpansion ──→ --answer/--force-rlm ? RLM fallback + extra chunks : chunks (SearchOnly)
  └── Insufficient ──→ Return retrieved chunks (RlmInsufficient)
```

## Key Types

- **`TerraphimGrep`** — Main entry point combining search, sufficiency judging, and optional LLM
- **`HybridSearcher`** — Synonym-aware code search and KG concept lookup with boosting
- **`SufficiencyJudge`** — Heuristic thresholds for deciding if results need LLM synthesis
- **`GrepOptions`** — Query configuration (haystack, context lines, max results, force RLM)
- **`GrepResult`** — Search output with chunks, optional answer, concepts, and stats

## Testing

```bash
# Run all tests
cargo test -p terraphim_grep

# Run with code-search feature
cargo test -p terraphim_grep --features code-search

# Run benchmarks
cargo bench -p terraphim_grep --features code-search
```

## License

MIT
