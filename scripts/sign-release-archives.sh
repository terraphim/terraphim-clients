#!/usr/bin/env bash
#
# Sign release .tar.gz and .zip archives with zipsign (Ed25519) and verify them.
#
# The signing private key is supplied base64-encoded in the ZIPSIGN_PRIVATE_KEY
# environment variable (stored in 1Password / GitHub Actions secret). It is the
# 64-byte zipsign private key (32-byte seed || 32-byte public key) produced by
# `zipsign gen-key`. The matching public key is embedded in
# crates/terraphim_update/src/signature.rs (EMBEDDED_PUBLIC_KEYS[0]).
#
# Usage:
#   ZIPSIGN_PRIVATE_KEY=<base64> scripts/sign-release-archives.sh <artifacts_dir>
#   [ZIPSIGN_PUBLIC_KEY=<base64>] scripts/sign-release-archives.sh --verify-only <artifacts_dir>
#
# Signs every *.tar.gz and *.zip in <artifacts_dir> in place (zipsign appends
# the signature trailer to the archive) and verifies each with the public half
# of the same key. Exits non-zero on any failure so CI fails closed.
#
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: ZIPSIGN_PRIVATE_KEY=<base64> $0 [--verify-only] <artifacts_dir>" >&2
    exit 2
fi

MODE="sign"
if [ "$1" = "--verify-only" ]; then
    [ "$#" -eq 2 ] || { echo "ERROR: --verify-only requires an artifacts directory" >&2; exit 2; }
    MODE="verify"
    ARTIFACTS_DIR="$2"
else
    [ "$#" -eq 1 ] || { echo "ERROR: unexpected argument '$2'" >&2; exit 2; }
    ARTIFACTS_DIR="$1"
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
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
EMBEDDED_PUBLIC_KEY="$(python3 - "$REPO_ROOT/crates/terraphim_update/src/signature.rs" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8")
match = re.search(r'const EMBEDDED_PUBLIC_KEYS:.*?=\s*&\[\s*.*?\n\s*"([A-Za-z0-9+/=]+)"', text, re.S)
if match is None:
    raise SystemExit("unable to resolve EMBEDDED_PUBLIC_KEYS[0]")
print(match.group(1))
PY
)"

if [ "$MODE" = "sign" ]; then
    if [ -z "${ZIPSIGN_PRIVATE_KEY:-}" ]; then
        echo "ERROR: ZIPSIGN_PRIVATE_KEY env var is not set" >&2
        exit 2
    fi
    base64 -d <<< "$ZIPSIGN_PRIVATE_KEY" > "$KEY_FILE"
    if [ "$(stat -c %s "$KEY_FILE" 2>/dev/null || stat -f %z "$KEY_FILE")" -ne 64 ]; then
        echo "ERROR: decoded ZIPSIGN_PRIVATE_KEY is not 64 bytes" >&2
        exit 2
    fi
    tail -c 32 "$KEY_FILE" > "$PUB_FILE"
    DERIVED_PUBLIC_KEY="$(base64 < "$PUB_FILE" | tr -d '\r\n')"
    if [ "$DERIVED_PUBLIC_KEY" != "$EMBEDDED_PUBLIC_KEY" ]; then
        echo "ERROR: signing key does not match EMBEDDED_PUBLIC_KEYS[0]" >&2
        exit 2
    fi
else
    PUBLIC_KEY="${ZIPSIGN_PUBLIC_KEY:-$EMBEDDED_PUBLIC_KEY}"
    base64 -d <<< "$PUBLIC_KEY" > "$PUB_FILE"
    if [ "$(stat -c %s "$PUB_FILE" 2>/dev/null || stat -f %z "$PUB_FILE")" -ne 32 ]; then
        echo "ERROR: decoded ZIPSIGN_PUBLIC_KEY is not 32 bytes" >&2
        exit 2
    fi
fi

shopt -s nullglob
archives=( "$ARTIFACTS_DIR"/*.tar.gz "$ARTIFACTS_DIR"/*.zip )
if [ "${#archives[@]}" -eq 0 ]; then
    echo "ERROR: no .tar.gz or .zip archives found in $ARTIFACTS_DIR" >&2
    exit 1
fi

signed=0
for archive in "${archives[@]}"; do
    name="$(basename "$archive")"
    format="tar"
    [[ "$name" != *.zip ]] || format="zip"
    if [ "$MODE" = "sign" ]; then
        echo "→ signing $name"
        if ! zipsign sign "$format" "$archive" "$KEY_FILE"; then
            echo "ERROR: failed to sign $name" >&2
            exit 1
        fi
    fi
    # Fail-closed: verify the just-signed archive before accepting it.
    if ! zipsign verify "$format" "$archive" "$PUB_FILE"; then
        echo "ERROR: post-sign verification failed for $name" >&2
        exit 1
    fi
    echo "  ✓ verified"
    signed=$((signed + 1))
done

if [ "$MODE" = "sign" ]; then
    echo "Signed and verified $signed archive(s)."
else
    echo "Verified $signed archive(s)."
fi
