# Changelog

All notable changes to terraphim_grep are documented here.

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
