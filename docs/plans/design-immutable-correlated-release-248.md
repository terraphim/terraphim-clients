# Design: Immutable Correlated Client Release (Issue #248)

- **Status:** Approved by continuous task authorization
- **Date:** 2026-09-18
- **Base commit:** `bab603255151c132b404b7f0a8171fe12191cb24`
- **Research:** `/tmp/codex-clients-248-last.md`

## Goal and scope

Produce a read-only, single-source-SHA release workflow for version `1.21.15`,
strict deterministic manifests, verified final artifacts, and a separately
authorized fail-closed promotion handoff. The five essential components are:

1. checked-in source/version provenance;
2. exact manifest and target contracts;
3. deterministic build, validation, and sealing;
4. stage-only workflow artifacts;
5. updater, CI, health-check, and operator evidence.

DEB/RPM nFPM work, workflow dispatch, publication, tags, commits, and forge
mutation are explicitly excluded. Publication credentials and write permissions
must not appear in the producer workflow.

## Architecture and decisions

```text
tag + expected SHA
       |
       v
read-only preflight --> six build lanes --> signed/final bytes
       |                                      |
       +-- checked-in Cargo metadata          v
                                      deterministic archives
                                                |
                                                v
                                validate exact closed asset set
                                                |
                                                v
                              SHA256SUMS + archive signatures
                                                |
                                                v
                                  immutable workflow artifact

separate authorized operator --> validate staged artifact/release state
                              --> upload every immutable object/candidate
                              --> verify every upload
                              --> advance stable manifests last
```

### D1: Strict manifest schema

The exact top-level keys are `version`, `released_at`, `assets`, and
`notes_url`. Each asset value is an exact object with `path`, `sha256`, and
positive integer `size`; unknown fields at either level are rejected. Asset
paths are relative, canonical `<binary>/<filename>` keys. SHA-256 values are
64 lowercase hex characters. The archive filename must encode the manifest
binary, version, target, and target-appropriate extension. Strict clients
reject legacy string asset values, but the legacy shape remains at the
separate `stable.json` compatibility pointer so pre-1.21.15 clients can
discover and install the migration release. Strict clients consume only
`stable-v2.json`.

`build-manifest.sh` writes through Python's `json` serializer with sorted keys
and stable separators. `SOURCE_DATE_EPOCH` supplies `released_at`, making equal
inputs byte-identical. It creates a candidate file and atomically renames it to
the requested output path; stdout mode remains available for inspection but is
not the promotion path.

### D2: Exact per-binary target sets

Current main builds six platform lanes and creates universal macOS binaries
only for agent and grep. Therefore the closed sets are:

- `terraphim-agent`: GNU Linux, both Omarchy MUSL targets, both native macOS
  targets, universal macOS, and Windows MSVC (7);
- `terraphim-grep`: the same 7 targets;
- `terraphim-cli`: the six matrix targets, without universal macOS (6).

Windows assets are ZIP files; other targets are deterministic `tar.gz`
archives. Every archive contains exactly its executable and both repository
license files. This resolves the stale live CLI universal entry in favor of
current-main build truth.

### D3: Staged promotion handoff

The producer has only `contents: read`, creates no release/R2 objects, and
uploads one sealed workflow artifact. The artifact includes archives,
signatures, `SHA256SUMS`, versioned candidate manifests, and a provenance file.
A separate operator command validates that the destination release exists and
is neither draft nor prerelease, uploads all immutable assets and candidate
manifests, verifies every remote object, and only then writes strict
`stable-v2.json` pointers followed by legacy `stable.json` pointers.
Any failure before the stable phase performs zero stable writes. Per-object R2
writes cannot provide a multi-key transaction, so the operator checklist also
requires retrying stable writes from the already verified candidate set; no
producer privilege is reintroduced.

## Interfaces and file plan

- `scripts/build-manifest.sh VERSION BIN ARTIFACTS OUTPUT`: validate the exact
  closed set and atomically write deterministic JSON.
- `scripts/promote-release.sh VERSION STAGED_DIR TARGET_REPO EXPECTED_SOURCE_SHA
  CORRELATION_ID`: privileged operator-only promotion with machine-checked
  provenance, draft/prerelease, no-clobber, and upload-before-stable gates.
- `scripts/rollback-release-pointers.sh VERSION STAGED_DIR EXPECTED_SOURCE_SHA
  CORRELATION_ID --authorized-pointers-only`: separately authorized rollback
  using retained pre-promotion pointer bytes/state.
- `ReleaseAsset { path: String, sha256: String, size: u64 }` and strict
  `ReleaseManifest` deserialization in `terraphim_update`.
- `resolve_asset` returns both the URL and expected integrity metadata;
  `update_r2` verifies byte size and SHA-256 before signature verification and
  installation.
- Workflow contract tests cover immutable source, version equality, exact
  matrix/target sets, archive content/mode/architecture, post-final sealing,
  stage-only permissions, and promotion ordering.
- CI executes Python release contracts; native CI uses the existing Rust
  updater gates because arbitrary Python commands are disallowed there.

## Vertical RED -> GREEN sequence

1. Source metadata and immutable workflow preflight.
2. Deterministic strict manifest generator and promotion ordering.
3. Strict Rust manifest model and integrity-before-install rollback behavior.
4. Deterministic archives, architecture/version checks, exact asset sealing,
   and stage-only workflow.
5. CI/health wiring and operator documentation.

Each slice adds one focused failing test invocation captured in
`/tmp/clients-248-red-*.log`, then implements the minimum behavior and captures
the passing invocation in `/tmp/clients-248-green-*.log`.

## Acceptance and verification

- Python release/package contracts pass with no skips.
- Focused `terraphim_update` manifest and R2 tests pass with no skips.
- Generated fixture manifests pass `python3 -m json.tool` and deterministic
  byte comparison.
- `SHA256SUMS` verifies every sealed artifact.
- shell syntax, dependency-free YAML parsing, Rust fmt/check/clippy, UBS,
  focused security review, and `git diff --check` pass.

## Specification findings

- Missing, duplicate, empty, wrongly named, wrong-version, wrong-architecture,
  non-executable, layout-invalid, or prematurely sealed assets fail closed.
- Candidate creation never replaces a stable manifest implicitly.
- Size/hash mismatch is definitive and occurs before signature verification or
  install, preserving the installed binary.
- Unknown manifest keys and legacy string assets at the v2 pointer fail parse
  rather than being ignored.
- QEMU execution is required only for Linux foreign binaries supported by the
  hosted Linux runner; unsupported cross-platform execution is covered by
  file-format architecture validation.
- The stage artifact name binds version and source SHA; `provenance.json`
  separately binds and machine-checks the correlation identity.

## Eliminated options and rollback

- CI-time Cargo rewrites: violate immutable provenance.
- Direct producer publication or `--clobber`: violates privilege separation
  and immutability.
- Open-ended target discovery: permits missing/extra platform drift.
- Shell-built JSON: unsafe escaping and nondeterministic output.
- Replacing legacy `stable.json` with strict data: strands the installed fleet.
- DEB/RPM packaging: owned by the separate worktree.

Before forward pointer writes, promotion retains all six old pointer states and
bytes. A separately authorized pointers-only rollback restores legacy bytes
first and removes the newly introduced strict pointers, causing strict clients
to use GitHub fallback. Immutable versioned assets are never changed. Pointer
writes are read back but are not a multi-key transaction, and Wrangler exposes
no atomic conditional put for the remaining final-404-to-put race.

## Quality evaluation

KLS scores: Physical 4, Empirical 4, Syntactic 5, Semantic 5, Pragmatic 5,
Social 5 (the user explicitly approved continuous implementation from the
research and supplied all disputed decisions). Average 4.7/5; no dimension is
below 3. The five-component essential scope and excluded-work list pass the
essentialism gate.
