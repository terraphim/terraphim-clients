# Self-update R2 backend

`terraphim-agent`, `terraphim-cli`, and `terraphim-grep` discover stable client
releases through strict per-binary manifests at
`https://downloads.terraphim.ai/<binary>/stable-v2.json`. R2 is the default
read backend; fetch and parse failures fall back to GitHub Releases so a bad
or unavailable pointer cannot strand an installation. Once strict metadata
has selected and downloaded an archive, size, digest, or signature failures
are definitive and never fall back.

The legacy `stable.json` remains a string-valued manifest for every client
older than 1.21.15. It must not be replaced with the strict schema. Publication
stores immutable v1 and v2 candidates, advances `stable-v2.json`, verifies it,
and advances legacy `stable.json` last.

## Strict v2 stable manifest

The manifest schema has four exact top-level keys. Unknown or missing keys,
legacy string asset values, duplicate targets, invalid filenames, zero sizes,
or malformed checksums are rejected.

```json
{
  "assets": {
    "x86_64-unknown-linux-musl": {
      "path": "terraphim-agent/terraphim-agent-1.21.15-x86_64-unknown-linux-musl.tar.gz",
      "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
      "size": 12345678
    }
  },
  "notes_url": "https://github.com/terraphim/terraphim-clients/releases/tag/v1.21.15",
  "released_at": "2026-09-18T00:00:00Z",
  "version": "1.21.15"
}
```

Official manifests use closed target sets. Agent and grep have the six matrix
targets plus `universal-apple-darwin`; CLI has the six matrix targets and no
universal entry. Windows uses ZIP; other targets use `tar.gz`.

## Verification and rollback safety

The updater performs these steps in order:

1. parse and validate the strict manifest and filename/version identity;
2. select the current target;
3. download to a temporary directory;
4. compare positive byte size and SHA-256 against the final archive metadata;
5. verify the embedded zipsign Ed25519 signature against the trusted key list;
6. extract and atomically replace the installed executable.

An unsigned archive is rejected. A size, checksum, or signature mismatch occurs
before installation, leaving the installed binary untouched.

## Production and publication boundaries

`release-binaries.yml` builds one exact source SHA, notarizes macOS binaries,
creates archives containing the executable and both repository licenses,
signs them, validates the exact post-sign bytes, then emits `SHA256SUMS`, dual
candidate manifests, canonical stripped Linux binaries with
`BINARY_SHA256SUMS`, and provenance in one immutable workflow artifact. Linux
and Windows outputs are byte-reproducible. Timestamped/notarized macOS outputs
are deterministic in structure and correlation, not byte-identical across
independent rebuilds. It has no GitHub release or R2 write path.

Publication is separately authorized and uses the candidate-first procedure in
[the release operator checklist](../release-operator-checklist.md). Stable
manifests are advanced only after every immutable object and candidate has been
uploaded and verified.

## Backend override

Set `TERRAPHIM_UPDATE_BACKEND=github` or `r2` to force a backend. For staging
or tests, `TERRAPHIM_UPDATE_BASE_URL` overrides the manifest host.

```bash
TERRAPHIM_UPDATE_BACKEND=github terraphim-agent check-update
```

The signing trust roots are documented in
`adr/ADR-001-release-signing-key-rotation.md`; implementation lives under
`crates/terraphim_update/`.
