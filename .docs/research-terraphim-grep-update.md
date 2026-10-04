# Research Document: terraphim-grep Update Support

**Status**: Approved for implementation
**Author**: OpenCode
**Date**: 2026-07-04
**Reviewers**: User-directed session

## Executive Summary

`terraphim-grep` currently has no update or autoupdate command, while `terraphim-agent` already uses the shared `terraphim_update` crate. The minimal correct path is to reuse `terraphim_update`, add `check-update` and `update` subcommands to `terraphim-grep`, and add a small repository override to `UpdaterConfig` so the grep binary checks `terraphim/terraphim-clients` releases rather than the updater crate default.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energising? | Yes | The just-released `terraphim-grep 1.21.1` had to be installed manually from a GitHub tag. |
| Leverages strengths? | Yes | The workspace already has `terraphim_update`; this is reuse, not greenfield updater work. |
| Meets real need? | Yes | User explicitly asked to check update support, then requested update support for `terraphim-grep`. |

**Proceed**: Yes - 3/3 YES.

## Problem Statement

### Description

`terraphim-grep` users cannot ask the binary to check whether a newer GitHub release exists or trigger the shared update workflow. `terraphim-agent` supports this via `check-update` and `update`, but `terraphim-grep` only accepts a search query and search options.

### Impact

Manual install steps are required after each release. This created a stale local binary during this session even after `v1.21.1` was tagged and released.

### Success Criteria

- `terraphim-grep --help` shows `check-update` and `update` commands.
- Existing usage such as `terraphim-grep "auth" --haystack code` remains valid.
- `terraphim-grep check-update` uses `terraphim/terraphim-clients` as the release repository.
- `terraphim-grep update` calls the shared `terraphim_update` updater rather than implementing a separate update path.

## Current State Analysis

### Existing Implementation

`crates/terraphim_grep/src/main.rs` defines a flat Clap parser with a required positional `query`. There is no subcommand enum and no dependency on `terraphim_update`.

`crates/terraphim_agent/src/main.rs` already exposes `CheckUpdate` and `Update` commands. It constructs `UpdaterConfig::new("terraphim-agent").with_version(env!("CARGO_PKG_VERSION"))` and calls `TerraphimUpdater::check_update()` or `check_and_update()`.

`crates/terraphim_update/src/lib.rs` implements shared update logic with GitHub Releases, but `UpdaterConfig::new` defaults to `repo_owner = "terraphim"` and `repo_name = "terraphim-ai"`.

### Code Locations

| Component | Location | Purpose |
|-----------|----------|---------|
| Grep CLI parser | `crates/terraphim_grep/src/main.rs` | Current CLI args and search execution. |
| Grep manifest | `crates/terraphim_grep/Cargo.toml` | Dependencies and binary definition. |
| Shared updater | `crates/terraphim_update/src/lib.rs` | GitHub Releases check/update implementation. |
| Existing update command example | `crates/terraphim_agent/src/main.rs` | Working command wiring pattern. |
| Existing update tests | `crates/terraphim_agent/tests/update_functionality_tests.rs` | CLI test style for update commands. |

### Data Flow

Current search flow:

```text
CLI args -> load role/thesaurus/search config -> TerraphimGrep::search -> print results
```

Desired update flow:

```text
CLI subcommand -> UpdaterConfig("terraphim-grep") + repo override -> TerraphimUpdater -> print status
```

### Integration Points

- GitHub Releases API through `self_update`, already wrapped by `terraphim_update`.
- Local binary replacement path is currently controlled by `terraphim_update` and defaults to `/usr/local/bin/<bin_name>`.

## Constraints

### Technical Constraints

- Preserve existing `terraphim-grep <QUERY>` invocation shape.
- Reuse `terraphim_update`; do not create a second updater implementation.
- Release repository must be `terraphim/terraphim-clients`, not the `terraphim_update` default `terraphim/terraphim-ai`.
- `update` may require release assets and signatures; existing GitHub releases currently only have source archives unless binary assets are attached separately.

### Business Constraints

- Keep the change small and releasable as a patch update.
- Avoid public repository references to private project names.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Backwards compatibility | Existing search CLI keeps working | Currently flat required query. |
| Update check latency | Network-bound, no local blocking beyond updater | Shared updater uses blocking task internally. |
| Maintainability | One shared updater path | `terraphim-agent` already reuses crate. |

## Vital Few (Essentialism)

### Essential Constraints

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| Preserve search invocation | Breaking grep usage would invalidate the release. | Existing users call `terraphim-grep <QUERY>`. |
| Use `terraphim_update` | Avoids duplicated update/security logic. | User explicitly requested this crate. |
| Override release repo | Otherwise `terraphim-grep` checks the wrong repository. | `UpdaterConfig::new` defaults to `terraphim-ai`. |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Background autoupdate on every grep run | Search should stay fast and predictable; no user request for implicit network calls. |
| New asset-building/signing pipeline | Larger release-engineering task; not required to wire CLI support. |
| Custom updater implementation | Violates reuse of `terraphim_update`. |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `terraphim_update` | Provides check/update implementation. | Defaults to the wrong repo without extension. |
| Clap parser in `terraphim_grep` | Must accept both subcommands and legacy query form. | Subcommand design can accidentally break positional queries. |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `self_update` | Transitive via `terraphim_update` | Requires suitable release assets for actual update installation. | Manual `cargo install` fallback. |
| GitHub Releases | API endpoint | Rate limiting if unauthenticated. | `GITHUB_TOKEN` as supported by updater. |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| `update` cannot install without binary assets | Medium | Command reports failure despite check working | Document as release asset requirement; preserve manual install path. |
| CLI parser breaks query mode | Medium | Regression for all users | Use optional query plus subcommands and tests. |
| Update writes to `/usr/local/bin` | Medium | Permission failure for users installed in `~/.cargo/bin` | Leave as existing updater behaviour for now; do not add unplanned install-path logic. |

### Open Questions

1. Should `terraphim_update` support configurable install paths for Cargo-installed tools? Deferred; not required for command wiring.
2. Should release assets be attached to `terraphim-clients` releases? Deferred to release engineering.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| Users expect explicit commands, not implicit autoupdate | Existing `terraphim-agent` uses explicit `check-update`/`update`. | Might still want background checks later. | Yes |
| `terraphim-grep` releases are hosted in `terraphim/terraphim-clients` | `v1.21.1` was released there. | Wrong repo check if hosting changes. | Yes |
| Existing updater output is acceptable | Reuse requested; agent already uses it. | UX inconsistency if grep needs custom messages. | Yes |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| Add explicit `check-update` and `update` subcommands | Small, matches agent CLI | Chosen |
| Add flags `--check-update` and `--update` | Avoids subcommand parser but less consistent | Rejected |
| Automatic startup update check | Network call on grep execution | Rejected |

## Research Findings

### Key Insights

1. `terraphim_update` is reusable but needs repo override support for non-`terraphim-ai` binaries.
2. `terraphim-grep` can preserve legacy query mode with an optional query and subcommand enum.
3. `check-update` is fully useful with GitHub release metadata; `update` depends on binary assets/signature availability.

### Relevant Prior Art

- `terraphim-agent check-update` and `terraphim-agent update` command wiring.
- `terraphim_update::TerraphimUpdater` and `UpdaterConfig`.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Clap parser compatibility test | Ensure `terraphim-grep <QUERY>` remains valid. | <1 hour |
| Check-update smoke run | Confirm repo override talks to `terraphim-clients`. | <1 hour |

## Recommendations

### Proceed/No-Proceed

Proceed with minimal explicit update commands.

### Scope Recommendations

- Add `UpdaterConfig::with_repo` to the shared updater crate.
- Add `terraphim_update` dependency to `terraphim_grep`.
- Add `check-update` and `update` subcommands to `terraphim-grep`.
- Add parser/CLI tests for help and legacy search mode.

### Risk Mitigation Recommendations

- Do not add implicit autoupdate.
- Clearly print updater failures rather than hiding them.
- Test no-thesaurus fallback and hybrid scoring after parser changes.

## Next Steps

If approved:
1. Write Phase 2 design.
2. Implement the minimal command wiring and repo override.
3. Run focused grep tests and `check-update` smoke validation.

## Appendix

### Reference Materials

- `crates/terraphim_update/src/lib.rs`
- `crates/terraphim_agent/src/main.rs`
- `crates/terraphim_grep/src/main.rs`
