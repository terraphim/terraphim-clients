# Terraphim Graph

The Terraphim knowledge graph used by `terraphim_mcp_server` integration tests to build
a deterministic thesaurus under `docs/src/kg/`. The file exists so that the
`mcp_autocomplete_e2e_test`, `mcp_rolegraph_validation_test`, and `test_all_mcp_tools`
test fixtures (which assert `terraphim-graph.md` exists under `docs/src/kg/`) can resolve
their knowledge-graph directory.

synonyms:: terraphim, terraphim graph, knowledge graph, ontology, thesaurus, kg, role, role graph