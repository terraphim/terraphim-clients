# Changelog

All notable changes to terraphim_agent are documented here.

## [1.20.4] - 2026-06-13

### Fixed
- Hermetic integration tests now bootstrap `Terraphim Engineer` via `role_config` in
  test settings instead of a missing `terraphim_server/` path from the polyrepo split.

### Added
- Crate-specific `README.md` with install instructions, feature flags, and quick start.