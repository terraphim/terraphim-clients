# terraphim-clients v1.21.12

Release candidate prepared from **canonical main** (`release/v1.21.12`), consolidating
the workspace after the divergent v1.21.11 release branch: the v1.21.11 tag was cut
from a short-lived branch off an older main, so this release re-bases the version
line on current main and carries everything landed there since.

## Highlights

### Fixed

- **Truthful grep statistics (#3190, #94):** `terraphim_grep` now reports the real
  `chunks_returned` in the `Insufficient` sufficiency path instead of the total
  chunks examined, so downstream consumers see honest retrieval counts.
- **Packaged-agent dependency repair (#95, #96):** the published `terraphim_agent`
  package now resolves `terraphim_sessions >= 1.21.2` from the canonical terraphim
  sparse index, fixing the broken install graph shipped in 1.21.1 (missing
  `terraphim-markdown-parser`, stale crates.io `terraphim_sessions`). Guarded by the
  `packaged_install_graph_regression` test, which packages, installs, and runs the
  artifact end to end.

### Added

- **Cursor session import (#2515, #32):** new `CursorConnector` in
  `terraphim_sessions` imports Cursor IDE sessions, with char-boundary-safe title
  truncation.

### Improved

- **Learning hooks:** recursive KG walk for `learned/` entries (#810 P3, #93);
  pi-rust learn hooks (`AgentType::Pi` + package) (#91); Claude
  `tool_response`/`exitCode` envelope aliases for multi-client hooks (#90);
  unconditional secret redaction in hook stdout passthrough with tests (#2344).
- **Release-workflow hardening:** strict semver + `release_tag`/`target_repo`
  input validation in `release-binaries.yml`; version-input propagation asserted
  across all shipped binary crates (#67, #95); host `--version` check before the
  build matrix; R2 manifest bin-name prefix strip (#89); bun installed before the
  wrangler upload (#68); portable `sed -i.bak` for macOS runners (#85).

## Versions

- Workspace crates (`terraphim-cli`, `terraphim_grep`, `terraphim_lsp`,
  `terraphim_negative_contribution`, `terraphim-session-analyzer`): **1.21.12**
- `terraphim_agent`: **1.21.12** (explicit package version, kept >= 1.21.2 per #95)
