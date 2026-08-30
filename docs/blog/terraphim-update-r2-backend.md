# Self-update R2 backend

`terraphim-agent update` downloads and verifies the next binary
release. The default backend is Cloudflare R2; if R2 is
unreachable, the updater falls back to GitHub Releases. This post
walks through the manifest, the verification path, and how to
override the backend.

## Quick start

```bash
# Check whether a newer version exists (stateless, no install)
terraphim-agent check-update

# Download and replace the running binary
terraphim-agent update
```

## The manifest

R2 hosts a `manifest.json` keyed by platform:

```json
{
  "version": "1.21.13",
  "platforms": {
    "darwin-aarch64": {
      "url": "https://downloads.terraphim.ai/v1.21.13/terraphim-agent-darwin-aarch64.tar.gz",
      "sha256": "...",
      "signature": "..."
    },
    "linux-x86_64": { "...": "..." }
  }
}
```

The manifest itself is fetched over HTTPS; the archive signature
is verified against the embedded public-key list
(`terraphim_update::signature::EMBEDDED_PUBLIC_KEYS`, see
`adr/ADR-001`).

## Verification

The updater tries every key in the embedded list and accepts the
archive on the first match. A tampered archive (signature present,
no trusted key matches) is rejected as `Invalid`. An unsigned
archive is currently `MissingSignature` — historically
`warn-and-proceed`, scheduled to flip to `Reject` in a follow-up
ADR.

## Fallback chain

```
R2 manifest (default)
  └── 200 OK → use R2 URL
  └── 4xx/5xx → GitHub Releases (latest) as fallback
        └── 200 OK → use GitHub asset URL
        └── 4xx/5xx → exit with ERROR_NETWORK
```

The fallback is automatic; users do not need to configure it.

## Overriding the backend

Set `TERRAPHIM_UPDATE_BACKEND=github` (or `r2`) to force one or the
other. Useful for air-gapped environments where R2 is unreachable
and you want the updater to skip the R2 probe entirely.

```bash
TERRAPHIM_UPDATE_BACKEND=github terraphim-agent update
```

## Cross-platform support

The updater knows the current platform triple via
`cargo_metadata::BuildInfo` or uname. Cross-compiled binaries can
override with `TERRAPHIM_TARGET_TRIPLE=aarch64-unknown-linux-musl`.

## References

* Source: `crates/terraphim_update/`
* ADR: `adr/ADR-001-release-signing-key-rotation.md`
* Reference: `docs/agent-reference.md` (`check-update`, `update`)
