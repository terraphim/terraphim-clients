#!/bin/bash
set -euo pipefail

# Sign and notarize a macOS binary
# Usage: ./sign-macos-binary.sh <binary_path> <apple_id> <team_id> <app_password> <cert_base64> <cert_password>

# Parameters passed from workflow (not hardcoded secrets)
BINARY_PATH="$1"
APPLE_ID="$2"
TEAM_ID="$3"
APP_PASS="$4"
CERT_BASE64="$5"
CERT_PASS="$6"

echo "==> Signing and notarizing: $(basename "$BINARY_PATH")"

# Create temporary keychain
KEYCHAIN_PATH="$RUNNER_TEMP/signing.keychain-db"
KEYCHAIN_PASS=$(openssl rand -base64 32)
CERT_PATH="$RUNNER_TEMP/certificate.p12"
ZIP_PATH="${BINARY_PATH}.zip"

cleanup() {
    rm -f "$CERT_PATH" "$ZIP_PATH"
    security delete-keychain "$KEYCHAIN_PATH" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> Creating temporary keychain"
security create-keychain -p "$KEYCHAIN_PASS" "$KEYCHAIN_PATH"
security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
security unlock-keychain -p "$KEYCHAIN_PASS" "$KEYCHAIN_PATH"

# Import certificate
echo "==> Importing certificate"
# Remove newlines from base64 before decoding (macOS base64 is strict)
echo "$CERT_BASE64" | tr -d '\n' | base64 --decode > "$CERT_PATH"

security import "$CERT_PATH" \
    -k "$KEYCHAIN_PATH" \
    -P "$CERT_PASS" \
    -T /usr/bin/codesign \
    -T /usr/bin/security

# Set key partition list to allow codesign to access the key
security set-key-partition-list \
    -S apple-tool:,apple: \
    -s -k "$KEYCHAIN_PASS" \
    "$KEYCHAIN_PATH"

# Add keychain to search list
security list-keychains -d user -s "$KEYCHAIN_PATH" $(security list-keychains -d user | sed s/\"//g)

# Find signing identity
SIGNING_IDENTITY=$(security find-identity -v -p codesigning "$KEYCHAIN_PATH" | grep "Developer ID Application" | head -1 | awk -F'"' '{print $2}')
echo "==> Found signing identity: $SIGNING_IDENTITY"

# Sign the binary
echo "==> Signing binary"
codesign \
    --sign "$SIGNING_IDENTITY" \
    --options runtime \
    --timestamp \
    --verbose \
    "$BINARY_PATH"

# Verify signature
echo "==> Verifying signature"
codesign --verify --deep --strict --verbose=2 "$BINARY_PATH"

# Create ZIP for notarization
echo "==> Creating ZIP for notarization"
ditto -c -k --keepParent "$BINARY_PATH" "$ZIP_PATH"

# Submit for notarization and bind all later evidence to this exact submission.
echo "==> Submitting for notarization"
SUBMISSION_JSON=$(xcrun notarytool submit "$ZIP_PATH" \
    --apple-id "$APPLE_ID" \
    --team-id "$TEAM_ID" \
    --password "$APP_PASS" \
    --wait \
    --output-format json)
printf '%s\n' "$SUBMISSION_JSON"

read -r SUBMISSION_ID SUBMISSION_STATUS < <(
    python3 -c 'import json,sys; data=json.load(sys.stdin); print(data["id"], data["status"])' \
        <<<"$SUBMISSION_JSON"
)
if [ "$SUBMISSION_STATUS" != "Accepted" ]; then
    echo "ERROR: notarization submission $SUBMISSION_ID returned $SUBMISSION_STATUS" >&2
    exit 1
fi

# Apple's accepted submission log may lag briefly. Retrieve it with a bounded
# retry; never fall back to global history, which can select another binary's ID.
echo "==> Retrieving notarization log for $SUBMISSION_ID"
log_ok=false
for attempt in 1 2 3 4 5; do
    if xcrun notarytool log "$SUBMISSION_ID" \
        --apple-id "$APPLE_ID" \
        --team-id "$TEAM_ID" \
        --password "$APP_PASS"; then
        log_ok=true
        break
    fi
    echo "Notarization log not ready (attempt $attempt/5)" >&2
    sleep 5
done
if [ "$log_ok" != true ]; then
    echo "ERROR: notarization log unavailable for accepted submission $SUBMISSION_ID" >&2
    exit 1
fi

# Gatekeeper's application-policy assessment returns "does not seem to be an
# app" for standalone CLI binaries. For this artifact type, strict codesign
# verification above plus the exact Accepted notarization submission are the
# fail-closed proof.

# Cleanup
echo "==> Cleaning up"
cleanup
trap - EXIT

echo "✅ Successfully signed and notarized: $(basename "$BINARY_PATH")"
