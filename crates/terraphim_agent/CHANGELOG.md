# Changelog

All notable changes to terraphim_agent are documented here.

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