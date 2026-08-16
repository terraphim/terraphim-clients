# Changelog

All notable changes to terraphim_agent are documented here.

## [1.21.12] - 2026-08-16

### Fixed
- Packaged dependency graph repair (#95, #96): the published package resolves
  `terraphim_sessions >= 1.21.2` from the canonical terraphim sparse index;
  guarded by the `packaged_install_graph_regression` end-to-end test.
- Release-workflow hardening: strict semver/`release_tag`/`target_repo` input
  validation, version propagation asserted across shipped binaries (#67, #95).

### Added
- Cursor IDE session import via `terraphim_sessions` `CursorConnector` (#2515).

### Changed
- Canonical-main consolidation after the divergent v1.21.11 release branch (#97);
  explicit package version moves 1.21.2 -> 1.21.12 in lockstep with the workspace.

## Unreleased

### Changed
- `check-update` / `update` now use the **R2 manifest backend** by default
  (`downloads.terraphim.ai`), with GitHub Releases as an automatic fallback.
  This removes the `403 rate limit exceeded` failure on shared IPs and requires
  no `GITHUB_TOKEN` for the common case.
- Self-update installs to the running executable's directory (no longer
  shadowed by a stale `~/.cargo/bin` copy).
- `update` can now replace the currently-running binary (atomic rename).

### Environment variables (new)
- `TERRAPHIM_UPDATE_BACKEND` — `r2` (default) | `github` (force the fallback).
- `TERRAPHIM_UPDATE_BASE_URL` — override the manifest base URL (e.g. a staging
  bucket).
- `GITHUB_TOKEN` — forwarded to the GitHub fallback backend to avoid
  rate limiting.

## [1.20.4] - 2026-06-13

### Fixed
- Hermetic integration tests now bootstrap `Terraphim Engineer` via `role_config` in
  test settings instead of a missing `terraphim_server/` path from the polyrepo split.

### Added
- Crate-specific `README.md` with install instructions, feature flags, and quick start.