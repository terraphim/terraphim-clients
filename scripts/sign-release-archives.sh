#!/usr/bin/env bash
#
# Sign release .tar.gz archives with zipsign (Ed25519) and verify them.
#
# The signing private key is supplied base64-encoded in the ZIPSIGN_PRIVATE_KEY
# environment variable (stored in 1Password / GitHub Actions secret). It is the
# 64-byte zipsign private key (32-byte seed || 32-byte public key) produced by
# `zipsign gen-key`. The matching public key is embedded in
# crates/terraphim_update/src/signature.rs (EMBEDDED_PUBLIC_KEYS[0]).
#
# Usage:
#   ZIPSIGN_PRIVATE_KEY=<base64> scripts/sign-release-archives.sh <artifacts_dir>
#
# Signs every *.tar.gz in <artifacts_dir> in place (zipsign appends the
# signature trailer to the archive) and verifies each with the public half of
# the same key. Exits non-zero on any failure so CI fails closed.
#
set -euo pipefail

if [ "$#" -lt 1 ]; then
    echo "Usage: ZIPSIGN_PRIVATE_KEY=<base64> $0 <artifacts_dir>" >&2
    exit 2
fi

ARTIFACTS_DIR="$1"
if [ -z "${ZIPSIGN_PRIVATE_KEY:-}" ]; then
    echo "ERROR: ZIPSIGN_PRIVATE_KEY env var is not set" >&2
    exit 2
fi

if ! command -v zipsign >/dev/null 2>&1; then
    echo "ERROR: zipsign CLI not installed. Run: cargo install zipsign" >&2
    exit 2
fi

# Materialise the private key into a chmod-600 temp file (decoded from base64
# to the raw 64-byte zipsign format). Cleaned up on exit.
KEY_FILE="$(mktemp)"
PUB_FILE="$(mktemp)"
trap 'rm -f "$KEY_FILE" "$PUB_FILE"' EXIT
chmod 600 "$KEY_FILE"

base64 -d <<< "$ZIPSIGN_PRIVATE_KEY" > "$KEY_FILE"
# Derive the matching public key (last 32 bytes of the 64-byte private key) so
# verification always uses the exact counterpart of the signing key.
tail -c 32 "$KEY_FILE" > "$PUB_FILE"

if [ "$(stat -c %s "$KEY_FILE" 2>/dev/null || stat -f %z "$KEY_FILE")" -ne 64 ]; then
    echo "ERROR: decoded ZIPSIGN_PRIVATE_KEY is not 64 bytes" >&2
    exit 2
fi

shopt -s nullglob
archives=( "$ARTIFACTS_DIR"/*.tar.gz )
if [ "${#archives[@]}" -eq 0 ]; then
    echo "ERROR: no .tar.gz archives found in $ARTIFACTS_DIR" >&2
    exit 1
fi

signed=0
for archive in "${archives[@]}"; do
    name="$(basename "$archive")"
    echo "→ signing $name"
    if ! zipsign sign tar "$archive" "$KEY_FILE"; then
        echo "ERROR: failed to sign $name" >&2
        exit 1
    fi
    # Fail-closed: verify the just-signed archive before accepting it.
    if ! zipsign verify tar "$archive" "$PUB_FILE"; then
        echo "ERROR: post-sign verification failed for $name" >&2
        exit 1
    fi
    echo "  ✓ signed + verified"
    signed=$((signed + 1))
done

echo "Signed and verified $signed archive(s)."
