# Authored synonyms and role-scoped autocomplete

The Rust MCP server reads the same `synonyms::` concept notes used by Terraphim's knowledge graph:

```markdown
# Risk
synonyms:: Concern
```

With this concept in a role's local KG, `search` for the exact term `Concern`
retrieves documents containing `Risk` as well as documents containing `Concern`.
Both names resolve through Terraphim's normalised concept identity. The server
retrieves each alias through the existing service, deduplicates document identities,
and ranks the combined results with Terraphim RoleGraph before applying `skip`
and `limit`. Synonym expansion preserves resource identities and does not replace words in the
returned document; the role's existing content-processing settings still apply.
Compound/free-text queries retain their existing query semantics.

```json
{"name":"search","arguments":{"query":"Concern","role":"project","limit":10,"skip":0}}
```

Autocomplete indexes belong to the role used to build them. Build each required
role once; switching roles never borrows the last role's index. Omitting `role`
from either autocomplete tool uses the selected role. An unknown role or a role
whose index has not been built returns an error without suggestions.

```json
{"name":"build_autocomplete_index","arguments":{"role":"project"}}
{"name":"autocomplete_terms","arguments":{"query":"Con","role":"project"}}
```

`autocomplete_with_snippets` uses that same role's haystacks. Updating the
configuration clears cached indexes: rebuild before requesting suggestions.
Deserialising an index makes it available to the explicit index/fuzzy operations;
it does not register a graph or replace a role-bound index.

The subprocess regression suite creates isolated Markdown graphs, corpora and
memory storage and calls the real server over MCP stdio:

```sh
cargo test --locked -p terraphim_mcp_server --test test_mcp_role_contract
```
