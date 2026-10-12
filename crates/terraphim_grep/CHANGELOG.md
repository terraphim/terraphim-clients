# Changelog

All notable changes to terraphim_grep are documented here.

## [1.21.19] - 2026-10-11

### Fixed
- Search an exact editor-exported synonym across its complete knowledge-graph
  equivalence group. Display limits no longer omit aliases or synthesis evidence.
- Treat graph alternatives as literal text, including slash and filter-looking
  words, while preserving filters in compound user queries.
- Honor requested `--answer` synthesis when retrieved evidence is sufficient and
  preserve the source's decision/implementation status in the answer.
- Pin fff-search and reject mmap content-cache configuration and unsupported
  filesystem names before indexing. Concurrent-writer hazards remain outside
  this partial mitigation.

### Changed
- Batch synonym searches into one scan per root and document the editor graph
  export → search-only → optional synthesis workflow.

## [Unreleased]

### Fixed
- RLM synthesis is now opt-in (#81). A `NeedsSynthesis`/`NeedsExpansion` verdict no
  longer triggers a chat completion unless `--answer` or `--force-rlm` was passed, so
  an `OPENROUTER_API_KEY` present in the environment can no longer turn a millisecond
  grep into a ~20s LLM round trip (and time out the caller's tool budget).

### Added
- `--search-only` (alias `--no-rlm`): hard-disables LLM synthesis for a run and skips
  building the LLM client entirely. Mutually exclusive with `--answer`/`--force-rlm`.

## [1.21.12] - 2026-08-16

### Fixed
- Report the truthful `chunks_returned` in the `Insufficient` sufficiency path
  (#3190, #94) instead of the total chunks examined.

### Changed
- Canonical-main consolidation after the divergent v1.21.11 release branch (#97);
  workspace version moves to 1.21.12.

## [1.20.0] - 2026-05-25

### Added
- Initial release: hybrid grep with KG boosting and RLM fallback
- fff-search 0.8.2 integration for code-search feature
- Debian packaging, Homebrew formula, multi-platform binaries
