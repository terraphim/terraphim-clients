# Changelog

All notable changes to `terraphim_update` are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/) and the project
adheres to [Semantic Versioning](https://semver.org/).

## Unreleased

### Added
- **R2 manifest backend** (`manifest` module): storage-agnostic distribution via
  a per-binary `stable.json` on `downloads.terraphim.ai` (Cloudflare R2). No
  GitHub API, no embedded secrets, no per-IP rate limit.
- `UpdateBackend` selector (`R2` default, `GitHub` fallback). `TerraphimUpdater`
  dispatches `check_update` / `update` / `check_and_update` by backend; R2
  transport failures transparently fall back to the GitHub backend.
- `UpdaterConfig::with_backend`, `with_manifest_base_url`, `with_auth_token`
  builders. Runtime env overrides: `TERRAPHIM_UPDATE_BACKEND` (`r2`|`github`)
  and `TERRAPHIM_UPDATE_BASE_URL`.
- Multi-key signature verification: `EMBEDDED_PUBLIC_KEYS` lists the current
  signing key first, then legacy keys; `verify_archive_signature` tries each
  (`Valid` on first match). `get_embedded_public_keys()` added;
  `get_embedded_public_key()` kept (returns the primary) for compatibility.
- `promote_staged_binaries` (unix/windows): atomic-rename install that can
  replace the currently-running executable.

### Changed
- **Default repo** for the GitHub fallback: `terraphim-ai` → `terraphim-clients`
  (releases moved; the old default pointed at a stale repo).
- `GITHUB_TOKEN` is now picked up and forwarded to the GitHub fallback backend
  (avoids unauthenticated rate limiting).
- **Primary signing key** rotated to the 2026-07 clients key; the 2025-01-12
  key is retained as a legacy fallback. See `adr/ADR-001.md`.
- `verify_archive_signature`: unsigned archives now return `MissingSignature`
  (was `Invalid`); tampered archives still return `Invalid`.
- `install_verified_archive`: stages extraction then atomic-rename (was direct
  in-place overwrite, which failed with `ETXTBSY` on the running binary).
- `platform::get_binary_path`: prefers the running executable's directory over
  `/usr/local/bin` (fixes install-path shadowing by `~/.cargo/bin`).

### Fixed
- Wrong repo default (`terraphim-ai` → `terraphim-clients`).
- `GITHUB_TOKEN` not forwarded → `403 rate limit exceeded` on shared IPs.
- Install-path shadowing: updates to `/usr/local/bin` masked by `~/.cargo/bin`.
- `ETXTBSY` when self-updating the running binary.
- Unsigned archives misreported as `Invalid` (now `MissingSignature`).

### Security
- zipsign Ed25519 verification now honours key rotation (multi-key). Signing is
  wired into the release pipeline. `MissingSignature` is warn-and-proceed
  during the transition; the flip to hard-reject is deferred to a follow-up
  (see `adr/ADR-001.md`).
