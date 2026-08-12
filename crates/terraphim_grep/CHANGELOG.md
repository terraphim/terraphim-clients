# Changelog

All notable changes to terraphim_grep are documented here.

## [1.21.2] - 2026-08-12

### Fixed
- Preserve retrieved chunks and knowledge-graph concepts when sufficiency is `RlmInsufficient`.
- Derive `stats.chunks_returned` and `stats.kg_hits` from the returned collections so structured output remains truthful.
- Add deterministic regression coverage for non-empty insufficient results and retained KG concepts.

## [1.20.0] - 2026-05-25

### Added
- Initial release: hybrid grep with KG boosting and RLM fallback
- fff-search 0.8.2 integration for code-search feature
- Debian packaging, Homebrew formula, multi-platform binaries
