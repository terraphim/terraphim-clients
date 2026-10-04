# Knowledge Graph directory

This directory holds the **Logseq-format** concept files that `terraphim_automata::builder::Logseq` consumes to build thesaurus structures used by `terraphim_mcp_server` and `terraphim_agent` integration tests.

## File format

Each `.md` file in this directory represents one concept. The filename stem becomes the canonical `NormalizedTerm::value` (used as a KG link target via `kg:filename-stem`). The first H1 heading becomes `NormalizedTerm::display_value`. The `synonyms::` line maps alternative spellings, abbreviations, and related terms to the same canonical entry.

Required structure:

```markdown
# Display Name (Title Case)

One or more paragraphs describing the concept. Plain Markdown, no frontmatter
required. The body is informational only; it does not affect the thesaurus.

synonyms:: lowercase, comma, separated, list, of, synonyms, and, related, terms
```

### Filename → value mapping

- Filename: `bun.md` → value: `bun` → KG link: `[bun](kg:bun)`
- Filename: `terraphim-graph.md` → value: `terraphim-graph` → KG link: `[Terraphim Graph](kg:terraphim-graph)`

Use **kebab-case** for multi-word filenames. Underscores are not transformed, so `machine_learning.md` becomes the value `machine_learning` (not `machine-learning`).

### Synonyms line rules

- Must be exactly `synonyms::` followed by a space, then a comma-separated list.
- All entries are lowercased and whitespace-trimmed at parse time.
- A trailing comma is allowed but ignored.
- The concept's own value (filename stem) is added to the synonym set automatically; you do not need to repeat it.
- Empty synonym lines are valid (the concept still appears once, keyed by its filename).
- Indenting the `synonyms::` line with leading spaces is not supported and will silently drop the line.

### Display value rules

- Only the first `# H1` heading in the file is used.
- If no H1 is present, `NormalizedTerm::display()` falls back to the value (filename stem).
- H2/H3 and below are ignored for display purposes.

## Adding a new concept

1. Create `docs/src/kg/<kebab-case-name>.md`.
2. Start the file with an H1 heading that is the human-readable display name.
3. Add a `synonyms::` line with at least one entry (lowercase, comma-separated).
4. Optionally add a body paragraph explaining the concept for human readers.
5. Run `cargo test -p terraphim_mcp_server --test mcp_rolegraph_validation_test` to confirm the thesaurus builder parses the new file.

## Reference

The canonical writer of this format lives at `crates/terraphim_grep/src/kg_curation.rs` (function `format_concept_markdown`, lines ~100-120). The canonical reader is `terraphim_automata::builder::Logseq` (in the private `terraphim-ai` dependency installed via `cargo install --locked --git ... terraphim-ai --tag v1.21.3`).

If you change the format here, update both ends. Adding a new field (e.g., `related::`) requires:
1. Updating `format_concept_markdown` in `kg_curation.rs` to emit it.
2. Updating `Logseq` builder to parse it.
3. Documenting it in this README.