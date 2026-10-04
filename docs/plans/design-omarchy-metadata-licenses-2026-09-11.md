# Design: Omarchy Package Metadata & License Fixes (Gitea #246)

- **Base commit:** `112018079dffd86fd99e434aedd6121476a66638`
- **Worktree:** `/home/alex/worktrees/clients-246`
- **Branch:** `task/246-omarchy-metadata-licenses`
- **Status:** APPROVED — implementation contract for Gitea #246
- **Author:** Kairo (orchestrator), drafted by Claude Sonnet
- **Date:** 2026-09-11

## Goal

Fix incorrect/stale package metadata (repository URL, license identifiers,
deb `license-file` pointers) across the workspace crates that ship Omarchy
(Arch/pacman + deb) packages, and establish a reusable, test-enforced
contract so metadata mismatches (crate license vs. Cargo.toml `license`
field vs. packaged `license-file` vs. actual `LICENSE-*` file on disk) are
caught automatically rather than discovered at package-build/release time.
This issue owns the **metadata and package-content truth** — it does NOT own
release-workflow version-rewrite behavior (that is O3's scope, tracked
separately; see [Handoff to O3](#handoff-to-o3)).

## Verified Current State

- Root `Cargo.toml` workspace `version = "1.21.14"`; workspace
  `repository` field is the canonical Gitea clients repo
  (`[workspace.package]` in root `Cargo.toml`).
- `crates/terraphim_agent/Cargo.toml`:
  - Inherits `version.workspace = true` (1.21.14).
  - `repository` is **stale**: `https://github.com/terraphim/terraphim-ai`
    (points at the old GitHub project, not the canonical Gitea clients
    repo).
  - `license = "Apache-2.0"`.
  - Packaging metadata (deb) references `license-file` as
    `../../LICENSE-Apache-2.0` — relative path from crate dir to repo root.
- `crates/terraphim_grep/Cargo.toml`:
  - Inherits `version.workspace = true` (1.21.14).
  - `repository` is set **explicitly** (not `.workspace = true`) to
    `https://git.terraphim.cloud/terraphim/terraphim-clients`, which
    happens to match the workspace value — correct today but not
    inheritance-guaranteed to stay correct.
  - `license = "MIT"`.
  - Packaging metadata (deb) references `license-file` as
    `["../../LICENSE-MIT", "4"]` (cargo-deb list form: path + a numeric
    field consumed by `cargo-deb`'s license-summary machinery).
- **Both `LICENSE-Apache-2.0` and `LICENSE-MIT` are absent from the repo
  root** — both crates' deb `license-file` pointers are currently dangling
  regardless of the repository-URL bug. `terraphim_agent`'s equivalent
  field is `["../../LICENSE-Apache-2.0", "4"]` (same list form).
- Root `Cargo.toml` `[workspace.package].repository` confirmed literal:
  `https://git.terraphim.cloud/terraphim/terraphim-clients`.
- **New finding (bounded read, not in original verified-facts set):** the
  stale `repository = "https://github.com/terraphim/terraphim-ai"` value
  is not unique to `terraphim_agent`. It also appears, unchanged, in 9
  other workspace crates: `terraphim_hooks`, `terraphim_negative_contribution`,
  `terraphim_mcp_server`, `terraphim_cli`, `terraphim-session-analyzer`,
  `terraphim_sessions`, `terraphim_command_runtime`, `terraphim_update`,
  `terraphim_lsp` (10 total including `terraphim_agent`). Of the 11
  workspace members, only `terraphim_grep` has the correct URL today.
  However, **only `terraphim_agent` and `terraphim_grep` carry a
  `[package.metadata.deb]` section** (confirmed via
  `grep -rn license-file crates/*/Cargo.toml` — exactly two hits). This
  design keeps its fix scope to those two crates (the only ones actually
  packaged for Omarchy/deb release) and flags the other 9 crates' stale
  `repository` field as an out-of-scope follow-up — see
  [Risks / Rollback](#risks--rollback) and
  [Scope / Non-goals](#scope--non-goals).
- `tests/test_release_binaries_workflow_contract.py` exists and currently
  asserts on release-workflow version-rewrite behavior. It does not yet
  assert on per-crate metadata (repository URL correctness, license-file
  existence, license-identifier consistency).
- O3 (separate issue) owns changes to the release workflow itself
  (version-rewrite mechanics). This issue owns: (a) the two root license
  files, (b) correcting `terraphim_agent`'s `repository` field, (c) adding
  reusable pytest assertions for metadata/package-content truth that O3's
  workflow changes must also satisfy.

### Symbol/File References

| File | Symbol / Field | Current value | Correct value |
|---|---|---|---|
| `Cargo.toml` (root) | `[workspace.package].version` | `1.21.14` | unchanged |
| `Cargo.toml` (root) | `[workspace.package].repository` | canonical Gitea clients repo | unchanged (reference for others) |
| `crates/terraphim_agent/Cargo.toml` | `[package].repository` | `https://github.com/terraphim/terraphim-ai` | canonical Gitea clients repo (workspace value) |
| `crates/terraphim_agent/Cargo.toml` | `[package].license` | `Apache-2.0` | unchanged (preserve) |
| `crates/terraphim_agent/Cargo.toml` | deb `license-file` | `../../LICENSE-Apache-2.0` | unchanged path, but target file must exist |
| `crates/terraphim_grep/Cargo.toml` | `[package].repository` | canonical Gitea clients repo (explicit, not inherited) | unchanged |
| `crates/terraphim_grep/Cargo.toml` | `[package].license` | `MIT` | unchanged (preserve) |
| `crates/terraphim_grep/Cargo.toml` | deb `license-file` | `../../LICENSE-MIT` | unchanged path, but target file must exist |
| `LICENSE-Apache-2.0` (root) | — | absent | created |
| `LICENSE-MIT` (root) | — | absent | created |

## Decisions

1. **Preserve `terraphim_agent` under Apache-2.0** and **`terraphim_grep`
   under MIT** — do not harmonize to a single workspace-wide license. These
   are deliberate per-crate choices; this issue only fixes metadata
   correctness (repository URL, presence of the referenced license file),
   not licensing policy.
2. **Fix `terraphim_agent`'s `repository` field** to point at the canonical
   Gitea clients repository (matching workspace value), removing the stale
   GitHub URL.
3. **Add the two missing root license files** (`LICENSE-Apache-2.0`,
   `LICENSE-MIT`) so both crates' existing deb `license-file` relative
   paths resolve.
4. **Encode the contract in reusable pytest assertions**, not ad hoc shell
   checks, so O3's workflow changes and future crates are covered by the
   same test module.
5. Do not touch release-workflow YAML or version-rewrite logic — flag any
   such need for O3.

## Scope / Non-goals

**In scope:**
- `Cargo.toml` (root) — read-only reference, no change expected.
- `crates/terraphim_agent/Cargo.toml` — fix `repository`.
- `crates/terraphim_grep/Cargo.toml` — verify only (already correct).
- `LICENSE-Apache-2.0`, `LICENSE-MIT` (repo root) — create.
- `tests/test_release_binaries_workflow_contract.py` or a new sibling test
  module — add reusable metadata/package-content assertions.

**Non-goals / explicitly out of scope:**
- Release workflow YAML / version-rewrite mechanics (O3).
- Changing either crate's license identifier or relicensing.
- Fixing the stale `repository` field in the 9 other workspace crates that
  share it (`terraphim_hooks`, `terraphim_negative_contribution`,
  `terraphim_mcp_server`, `terraphim_cli`, `terraphim-session-analyzer`,
  `terraphim_sessions`, `terraphim_command_runtime`, `terraphim_update`,
  `terraphim_lsp`) — none of these carry `[package.metadata.deb]` /
  Omarchy packaging metadata, so they are outside this issue's "package
  metadata and license truth" framing. Tracked as a follow-up suggestion,
  not a blocking requirement here (see [Risks](#risks--rollback)).
- Gitea issue/PR interaction, commits, or pushes (design-only task).

## Exact File Plan

1. `LICENSE-Apache-2.0` (new, repo root) — exact bytes from the canonical
   upstream source (see policy below), not invented text.
2. `LICENSE-MIT` (new, repo root) — exact bytes from the canonical
   upstream source, not invented text.
   **Correction (post-implementation, superseding the original proposal
   in this item):** this item originally proposed inventing a
   `Copyright (c) 2024, Terraphim Contributors` line to match the
   crates' `[package.metadata.deb] copyright` fields. That was wrong —
   the implementation brief's authoritative source is the exact bytes at
   `https://raw.githubusercontent.com/terraphim/terraphim-ai/main/LICENSE-Apache-2.0`
   and `.../LICENSE-MIT`, verified by SHA-256 (Apache
   `47528e762efc05e17ae569ffeacf044b65cbe2c94bc9c58c576f267a5cd7d039`, MIT
   `3ec3e4145b74567ba29578785140bc15af1309576cceb9c921c1a23d43060eea`).
   **Policy: fetch/copy the exact upstream bytes and verify the hash; do
   not invent or alter copyright/attribution text to match in-repo
   conventions.** The actual upstream `LICENSE-MIT` bytes carry
   `Copyright (c) 2023 Applied Knowledge Systems Ltd`, not a Terraphim
   attribution line — this is the correct, verified content and is
   deliberately preserved verbatim rather than "corrected" to match the
   deb-metadata `copyright` fields, which remain a separate, unrelated
   piece of packaging metadata.
3. `crates/terraphim_agent/Cargo.toml` and
   `crates/terraphim_grep/Cargo.toml` — edit `[package].repository` (agent
   only) and add a package-level `[package].license-file` field to both
   manifests (`../../LICENSE-Apache-2.0` for `terraphim_agent`,
   `../../LICENSE-MIT` for `terraphim_grep`), distinct from each crate's
   existing `[package.metadata.deb].license-file`. **Correction
   (post-implementation):** the original proposal scoped item 3 to the
   agent's `repository` field only and did not include a package-level
   `license-file`. That omission meant `cargo package --list` (the
   command #246 explicitly names as proof of license inclusion) showed
   zero LICENSE entries for either crate — `[package.metadata.deb]`'s
   `license-file` is consumed only by `cargo-deb`, not by
   `cargo package`/`cargo publish`. Cargo >= 1.43 supports a
   `[package].license-file` outside the package root and copies it into
   the package output under its basename at packaging time; verified
   empirically (this repo's Cargo 1.96) that a package-level
   `license-file` coexists with the existing SPDX `license` field without
   a fatal metadata conflict, and that `cargo package --list` then emits
   `LICENSE-Apache-2.0` / `LICENSE-MIT` respectively.
4. `tests/test_release_binaries_workflow_contract.py` (or new
   `tests/test_package_metadata_contract.py`) — add assertions per
   [Metadata and package-content contracts](#metadata-and-package-content-contracts).

No other files are modified.

## Metadata and Package-Content Contracts

For every workspace crate that produces a packaged artifact (Omarchy
pacman/deb — identified today as any crate with a
`[package.metadata.deb]` section: `terraphim_agent`, `terraphim_grep`),
the following must hold and must be asserted by test:

1. **Repository consistency**: `[package].repository` in the crate's
   `Cargo.toml`, if explicitly set (not inherited), must equal the
   workspace `repository` value in root `Cargo.toml` — no packaged crate
   may point at a different (e.g. stale/legacy) remote. Scoped to
   deb-packaged crates only, per [Scope / Non-goals](#scope--non-goals);
   the same helper can be re-parametrized workspace-wide in a follow-up
   issue once the other 9 crates' stale URLs are addressed.
2. **License identifier validity**: `[package].license` must be a
   recognized SPDX identifier and must match one of the license files
   present at the repo root (`Apache-2.0` → `LICENSE-Apache-2.0`,
   `MIT` → `LICENSE-MIT`).
3. **License-file existence**: any packaging-level `license-file` path
   (deb metadata, pacman packaging scripts, etc.) must resolve to an
   existing file on disk relative to the crate directory.
4. **License-file content sanity**: the referenced license file's content
   must match its SPDX identifier (e.g. `LICENSE-Apache-2.0` contains the
   Apache-2.0 grant text, not MIT text) — cheap heuristic match (e.g.
   distinctive phrase check), not full-text diffing — **and** must match
   the exact authoritative SHA-256 hash (Apache
   `47528e762efc05e17ae569ffeacf044b65cbe2c94bc9c58c576f267a5cd7d039`, MIT
   `3ec3e4145b74567ba29578785140bc15af1309576cceb9c921c1a23d43060eea`) as
   a stronger, exact-byte-provenance regression invariant.
5. **No dangling references**: no crate's packaging metadata may reference
   a license file that does not exist (this is the currently-broken state
   for both `terraphim_agent` and `terraphim_grep`).
6. **Package-level license-file existence (both manifests in scope)**:
   both `crates/terraphim_agent/Cargo.toml` and
   `crates/terraphim_grep/Cargo.toml` must additionally carry a
   *package-level* `[package].license-file` field — the field
   `cargo package`/`cargo publish` actually reads to copy a license into
   the package output. This is distinct from, and asserted separately
   from, each crate's `[package.metadata.deb].license-file` (which
   `cargo-deb` reads but `cargo package --list` never surfaces). Both
   fields must resolve to the same root license file.
7. **Version inheritance and tag/version-mismatch rejection — explicitly
   in scope per #246, not optional.** #246's implementation steps and
   acceptance criteria require, verbatim: implementation step 2, "both
   binary packages resolve to the same workspace version"; step 7, "add a
   reusable assertion that vX.Y.Z equals workspace/package metadata"; a
   required RED/GREEN case, "tag/version input differing ... fails"; and
   acceptance criteria "all releasable binary crates inherit one
   checked-in workspace version" and "release qualification rejects
   version/tag mismatch." These are asserted by
   `test_crate_version_inherits_workspace` (both deb-packaged crates use
   `version.workspace = true`, never a per-crate pin) and
   `test_tag_workspace_mismatch_detected` (the reusable
   `assert_tag_matches_workspace_version` helper, which must reject a
   mismatched tag like `v9.9.9` against the checked-in workspace version
   and accept a matching one). This is intentionally in scope because #246
   makes the checked-in workspace version authoritative for release inputs.

These assertions are packaged as reusable pytest fixtures/helpers so O3's
release-workflow tests can import and reuse them rather than duplicating
Cargo.toml-parsing logic.

## Vertical RED → GREEN Sequence

Each step names the exact command and the failure reason expected before
the corresponding fix lands.

1. **RED — root license files missing**
   - Command: `pytest tests/test_package_metadata_contract.py::test_license_files_exist_at_root -x`
   - Expected failure reason: `AssertionError: LICENSE-Apache-2.0 not found at repo root` (and similarly for `LICENSE-MIT`).
   - Fix: add `LICENSE-Apache-2.0` and `LICENSE-MIT` per [Exact File Plan](#exact-file-plan) item 1–2.
   - GREEN: re-run same command → passes.

2. **RED — `terraphim_agent` repository mismatch**
   - Command: `pytest tests/test_package_metadata_contract.py::test_crate_repository_matches_workspace -x`
   - Expected failure reason: `AssertionError: crates/terraphim_agent repository 'https://github.com/terraphim/terraphim-ai' != workspace repository '<canonical gitea clients repo>'`.
   - Fix: edit `crates/terraphim_agent/Cargo.toml` `[package].repository`.
   - GREEN: re-run same command → passes.

3. **RED — dangling deb `license-file` references (pre-fix baseline, both crates)**
   - Command: `pytest tests/test_package_metadata_contract.py::test_license_file_paths_resolve -x`
   - Expected failure reason: `AssertionError: crates/terraphim_agent license-file '../../LICENSE-Apache-2.0' does not resolve to an existing file` (and same for `terraphim_grep` / `LICENSE-MIT`).
   - Fix: satisfied by step 1 (root license files created) — no crate-file edit needed since paths are already correct.
   - GREEN: re-run same command → passes once step 1 lands.

4. **RED — license-identifier/content sanity check not yet implemented**
   - Command: `pytest tests/test_package_metadata_contract.py::test_license_file_content_matches_identifier -x`
   - Expected failure reason: `AssertionError: LICENSE-Apache-2.0 content does not contain expected Apache-2.0 marker text` (fails until real license text is written, not placeholder).
   - Fix: ensure created license files contain full, correct upstream license text (not stubs).
   - GREEN: re-run same command → passes.

5. **Full contract suite GREEN**
   - Command: `pytest tests/test_package_metadata_contract.py -v`
   - Expected: all tests pass; zero dangling references, zero identifier mismatches, zero repository mismatches across all workspace crates that declare packaging metadata.

## Verification Matrix

| Check | Command | Pass criterion |
|---|---|---|
| Root license files exist | `pytest tests/test_package_metadata_contract.py::test_license_files_exist_at_root` | Both files present, non-empty |
| `terraphim_agent` repository matches workspace | `pytest tests/test_package_metadata_contract.py::test_crate_repository_matches_workspace` | Equal string match |
| `terraphim_grep` repository matches workspace (regression guard) | same test, parametrized over crate list | Equal string match |
| deb `license-file` paths resolve | `pytest tests/test_package_metadata_contract.py::test_license_file_paths_resolve` | `Path.exists()` true for both crates |
| License content sanity | `pytest tests/test_package_metadata_contract.py::test_license_file_content_matches_identifier` | Marker-phrase match |
| License identifiers preserved (no relicensing) | `pytest tests/test_package_metadata_contract.py::test_license_identifiers_unchanged` | `terraphim_agent` == `Apache-2.0`, `terraphim_grep` == `MIT` |
| Package-level `license-file` resolves (`cargo package --list` proof) | `pytest tests/test_package_metadata_contract.py::test_package_level_license_file_resolves` | Both crates' `[package].license-file` resolves; confirmed live via `cargo package --list` showing `LICENSE-Apache-2.0` / `LICENSE-MIT` |
| Deb-packaged crate set is discovered, not hard-coded | `pytest tests/test_package_metadata_contract.py::test_discovered_deb_packaged_crates_matches_intended_set` | `discover_deb_packaged_crates()` scan of `crates/*/Cargo.toml` == `("terraphim_agent", "terraphim_grep")` |
| Version inheritance (#246 step 2 / acceptance criterion) | `pytest tests/test_package_metadata_contract.py::test_crate_version_inherits_workspace` | Both crates use `version.workspace = true` |
| Tag/workspace version-mismatch rejection (#246 step 7 / acceptance criterion) | `pytest tests/test_package_metadata_contract.py::test_tag_workspace_mismatch_detected` | `assert_tag_matches_workspace_version` raises for `v9.9.9`, passes for the matching tag |
| Existing release-workflow contract still passes (no regression) | `pytest tests/test_release_binaries_workflow_contract.py -v` | All existing tests still pass unmodified |
| Full workspace still builds/packages | `cargo metadata --no-deps --format-version 1` | Exits 0, valid JSON, both crates present with corrected fields |
| Whole-repo test run | `pytest tests/ -v` | No new failures introduced |

## Acceptance Mapping

| Acceptance criterion (from #246) | Design element that satisfies it |
|---|---|
| `terraphim_agent` repository URL corrected | Exact File Plan item 3; RED→GREEN step 2 |
| Both crates' licenses preserved (Apache-2.0 / MIT respectively) | Decisions #1; Verification Matrix "License identifiers preserved" |
| Root license files present and correct | Exact File Plan items 1–2; RED→GREEN steps 1, 4 |
| Packaged deb metadata no longer references dangling license files | RED→GREEN step 3; Verification Matrix "deb license-file paths resolve" |
| `cargo package --list` proves license inclusion | Contracts item 6; Verification Matrix "Package-level license-file resolves" |
| All releasable binary crates inherit one checked-in workspace version | Contracts item 7; Verification Matrix "Version inheritance" |
| Release qualification rejects version/tag mismatch | Contracts item 7; Verification Matrix "Tag/workspace version-mismatch rejection" |
| Reusable assertions available for future/other crates and for O3's workflow tests | Metadata and Package-Content Contracts section; Handoff to O3 |
| No regression to existing release workflow contract tests | Verification Matrix "Existing release-workflow contract still passes" |

## Risks / Rollback

- **Confirmed (not hypothetical):** 10 of 11 workspace crates carry the
  stale `https://github.com/terraphim/terraphim-ai` repository URL —
  `terraphim_agent` plus 9 others (`terraphim_hooks`,
  `terraphim_negative_contribution`, `terraphim_mcp_server`,
  `terraphim_cli`, `terraphim-session-analyzer`, `terraphim_sessions`,
  `terraphim_command_runtime`, `terraphim_update`, `terraphim_lsp`). This
  design deliberately scopes the repository-consistency test to
  deb-packaged crates only (`terraphim_agent`, `terraphim_grep`) so it
  does not fail on out-of-scope crates. Mitigation: state this explicitly
  in the PR description, and file a follow-up issue for the other 9
  crates so the stale URL isn't mistaken for "already fixed" once #246
  merges.
- **Risk:** License file text sourced incorrectly (e.g. wrong SPDX
  boilerplate, wrong copyright holder line) could itself create a new
  compliance problem. Mitigation: use canonical upstream license texts
  verbatim (opensource.org / apache.org), confirm attribution line during
  implementation against existing project convention (e.g. any existing
  copyright headers in source files).
- **Risk:** Overlap with O3's release-workflow changes if O3 also touches
  `Cargo.toml` version fields concurrently. Confirmed mechanism (from
  `tests/test_release_binaries_workflow_contract.py::test_build_mutation_trusts_preflight_and_only_rewrites_versions`):
  the release workflow already calls
  `set_section_version("crates/terraphim_agent/Cargo.toml", "package")`
  and `set_section_version("Cargo.toml", "workspace.package")` to rewrite
  version numbers at release time — it edits the same `[package]` table
  in `crates/terraphim_agent/Cargo.toml` this design also edits (different
  field: `version` vs. `repository`). Mitigation: this design's file plan
  touches only `repository`/license fields, never `version`; confirm with
  O3 before merge to avoid conflicting edits to the same `[package]` table
  in the same PR window.
- **Rollback:** All changes are additive (two new files) or single-field
  edits (one `repository` string) plus a new/extended test module — revert
  via `git revert` of the implementation commit(s); no data migration, no
  runtime behavior change, no external system impact.

## Handoff to O3

- O3's release-workflow issue owns version-rewrite mechanics exercised by
  `tests/test_release_binaries_workflow_contract.py`.
- This design's new `tests/test_package_metadata_contract.py` assertions
  (repository consistency, license-file existence/content, license
  identifier preservation) are intended to be **imported/reused** by O3's
  workflow-contract tests so that any workflow step which regenerates or
  rewrites `Cargo.toml` metadata during release is checked against the same
  contract, not a duplicate one.
- Handoff artifact: this document plus the new test module's public
  helper functions (e.g. `assert_repository_matches_workspace(crate_path)`,
  `assert_license_file_resolves(crate_path)`) — O3 should call these from
  their workflow tests rather than re-implementing Cargo.toml parsing.
- Open question for O3 sync: confirm whether the release workflow ever
  regenerates `Cargo.toml` `repository`/`license-file` fields programmatically
  (e.g. via `cargo release` or a sed step) — if so, that step must be
  covered by this same contract to prevent it from reintroducing the stale
  GitHub URL.
