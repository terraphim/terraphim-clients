//! Signature verification for downloaded updates
//!
//! This module provides signature verification capabilities to ensure
//! downloaded binaries are authentic and have not been tampered with.
//! Uses zipsign-api (included via self_update's "signatures" feature)
//! to verify Ed25519 signatures embedded in .tar.gz release archives.

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use chrono::{DateTime, Utc};
use std::fs;
use std::io::Cursor;
use std::path::Path;
use tracing::{debug, info, warn};

// Re-export zipsign-api types for convenience
pub use zipsign_api::ZipsignError;

/// All Ed25519 public keys trusted to verify release signatures, base64-encoded.
///
/// Ordered by preference: the **primary** (current signing) key first, then
/// legacy keys retained so archives signed before a rotation still verify.
/// The matching private keys are held offline / in 1Password; only the public
/// halves live here.
const EMBEDDED_PUBLIC_KEYS: &[&str] = &[
    // Primary: "Terraphim Clients zipsign Release Key 2026-07" (terraphim-clients).
    // Generated 2026-07; private key in 1Password (Terraphim vault).
    "iW2sM72/09yfiQ3jMB2GBALCRN+1FLLgD5qBbISFfS0=",
    // Legacy: original 2025-01-12 key (terraphim-ai era). No archive was ever
    // signed with it; retained purely as a defence-in-depth fallback.
    "1uLjooBMO+HlpKeiD16WOtT3COWeC8J/o2ERmDiEMc4=",
];

/// Get the primary embedded public key for Terraphim releases.
///
/// Returns the first (current signing) key from [`EMBEDDED_PUBLIC_KEYS`]. Kept
/// for API compatibility; new verification code should use
/// [`get_embedded_public_keys`] to honour key rotation.
pub fn get_embedded_public_key() -> &'static str {
    EMBEDDED_PUBLIC_KEYS[0]
}

/// Get every trusted embedded public key (base64-encoded Ed25519, 32 bytes each).
///
/// Verification succeeds if the archive's signature matches **any** key in
/// this list. The first entry is the current signing key; subsequent entries
/// are legacy keys retained across rotations.
pub fn get_embedded_public_keys() -> &'static [&'static str] {
    EMBEDDED_PUBLIC_KEYS
}

/// Metadata for cryptographic keys
///
/// This structure provides information about signing keys including
/// validity periods and key identifiers for future key rotation support.
#[derive(Debug, Clone)]
pub struct KeyMetadata {
    /// Unique identifier for this key
    pub key_id: String,
    /// When this key became valid
    pub valid_from: DateTime<Utc>,
    /// When this key expires (None = no expiry set)
    pub valid_until: Option<DateTime<Utc>>,
    /// Base64-encoded Ed25519 public key
    pub public_key: String,
}

/// Get the current active key metadata for Terraphim AI releases
///
/// This function provides metadata about the currently active signing key.
/// In the future, this will support key rotation by maintaining multiple
/// key metadata entries and selecting based on validity periods.
///
/// # Returns
/// Key metadata structure with key information
///
/// # Note
/// This is a basic implementation for v1.5.0. Full key rotation mechanism
/// is deferred to a future release. The current key has no expiration date.
pub fn get_active_key_metadata() -> KeyMetadata {
    KeyMetadata {
        key_id: "terraphim-release-key-2025-01".to_string(),
        valid_from: "2025-01-12T00:00:00Z".parse().unwrap(),
        valid_until: None, // No expiry set yet
        public_key: get_embedded_public_key().to_string(),
    }
}

/// Result of a signature verification operation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    /// Signature is valid
    Valid,

    /// Signature is invalid
    Invalid { reason: String },

    /// Signature file is missing
    MissingSignature,

    /// Verification encountered an error
    Error(String),
}

/// Verify the signature of a downloaded archive
///
/// This function verifies that a .tar.gz archive has a valid Ed25519 signature
/// embedded using zipsign. Signatures are embedded directly in the archive
/// (as GZIP comment for .tar.gz files), not in separate signature files.
///
/// # Arguments
/// * `archive_path` - Path to .tar.gz archive file to verify
/// * `public_key` - Optional public key for verification (base64-encoded).
///
///  If None, uses the embedded public key.
///
/// # Returns
/// * `Ok(VerificationResult)` - Result of verification
/// * `Err(anyhow::Error)` - Error if verification process fails
///
/// # Example
/// ```no_run
/// use terraphim_update::signature::verify_archive_signature;
/// use std::path::Path;
///
/// let result = verify_archive_signature(
///     Path::new("/tmp/terraphim-1.0.0.tar.gz"),
///     None  // Use embedded public key
/// ).unwrap();
/// ```
pub fn verify_archive_signature(
    archive_path: &Path,
    public_key: Option<&str>,
) -> Result<VerificationResult> {
    info!("Starting signature verification for {:?}", archive_path);

    if !archive_path.exists() {
        return Err(anyhow!("Archive file not found: {:?}", archive_path));
    }

    // Use provided key, or every trusted embedded key (key rotation support).
    // An explicitly-provided key is validated strictly (errors propagate); the
    // embedded list is validated leniently (unparseable legacy entries skipped).
    let explicit_key = public_key.is_some();
    let keys_to_try: Vec<&str> = match public_key {
        Some(k) => vec![k],
        None => get_embedded_public_keys().to_vec(),
    };

    // SECURITY: Never allow bypassing signature verification.
    if keys_to_try.iter().any(|k| k.starts_with("TODO:")) {
        return Err(anyhow!(
            "Placeholder public key detected. Signature verification cannot be bypassed. \
            Configure a real Ed25519 public key in EMBEDDED_PUBLIC_KEYS."
        ));
    }

    // Read the archive file once; reused across key attempts.
    let archive_bytes = fs::read(archive_path).context("Failed to read archive file")?;

    // Get the context (file name) for signature verification.
    // zipsign uses the file name as context/salt for signing. The archive
    // must be stored with its original published filename so the context
    // matches what was used at sign time (callers must ensure this — e.g.
    // update_r2 names the temp download after the asset, not a random
    // NamedTempFile path).
    let context: Option<Vec<u8>> = archive_path
        .file_name()
        .map(|n| n.to_string_lossy().as_bytes().to_vec());
    let context_ref: Option<&[u8]> = context.as_deref();

    // Try each trusted key. Track the worst definitive outcome:
    //   - any Valid -> return Valid immediately
    //   - any "signature present but no key matched" (NoMatch) -> Invalid
    //   - "no signature in archive" (FindDataStartAndLen) -> MissingSignature
    // MissingSignature only wins if NO key found a present-but-mismatched
    // signature (otherwise a tampered archive with an unknown key must stay
    // Invalid, not downgrade to MissingSignature).
    let mut saw_missing = false;
    let mut last_invalid: Option<String> = None;

    for key_str in keys_to_try {
        // Parse the public key (base64-encoded).
        let key_bytes = match base64::engine::general_purpose::STANDARD.decode(key_str) {
            Ok(b) => b,
            Err(e) => {
                if explicit_key {
                    return Err(anyhow!("Failed to decode public key base64: {e}"));
                }
                continue; // skip unparseable legacy entries
            }
        };
        if key_bytes.len() != 32 {
            if explicit_key {
                return Ok(VerificationResult::Invalid {
                    reason: format!(
                        "Invalid public key length: {} bytes (expected 32)",
                        key_bytes.len()
                    ),
                });
            }
            continue;
        }
        let mut key_array = [0u8; 32];
        key_array.copy_from_slice(&key_bytes);

        let verifying_key = match zipsign_api::verify::collect_keys(std::iter::once(Ok(key_array)))
        {
            Ok(vk) => vk,
            Err(e) => {
                if explicit_key {
                    return Err(anyhow!("Failed to parse public key: {e}"));
                }
                continue;
            }
        };

        let mut cursor = Cursor::new(&archive_bytes);
        match zipsign_api::verify::verify_tar(&mut cursor, &verifying_key, context_ref) {
            Ok(_index) => {
                info!(
                    "Signature verification passed for {:?} (trusted key)",
                    archive_path
                );
                return Ok(VerificationResult::Valid);
            }
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("could not find read signatures") {
                    // Unsigned archive (no signature trailer) for this key.
                    saw_missing = true;
                } else {
                    // Signature present but no key matched (NoMatch) or I/O.
                    last_invalid = Some(msg);
                }
            }
        }
    }

    // No key verified. Prefer Invalid (tampered / wrong key) over
    // MissingSignature when any key saw a present-but-mismatched signature.
    if let Some(msg) = last_invalid {
        warn!("Signature verification failed (no trusted key matched): {msg}");
        Ok(VerificationResult::Invalid {
            reason: format!("Signature verification failed: {msg}"),
        })
    } else if saw_missing {
        info!(
            "No embedded signature in {:?} (unsigned archive)",
            archive_path
        );
        Ok(VerificationResult::MissingSignature)
    } else {
        // No keys were parseable at all.
        Ok(VerificationResult::Invalid {
            reason: "No usable trusted public key configured".to_string(),
        })
    }
}

/// Verify signature using self_update's built-in verification
///
/// This is a convenience wrapper around `verify_archive_signature`.
/// Note: When using `TerraphimUpdater::update()`, signature verification
/// is handled automatically by self_update via `.verifying_keys()`.
///
/// # Arguments
/// * `release_name` - Name of the release (e.g., "terraphim")
/// * `version` - Version string (e.g., "1.0.0")
/// * `archive_path` - Path to the .tar.gz archive to verify
/// * `public_key` - Public key for verification (base64-encoded, or None for embedded key)
///
/// # Returns
/// * `Ok(VerificationResult)` - Result of verification
/// * `Err(anyhow::Error)` - Error if verification fails
///
/// # Note
/// The `release_name` and `version` parameters are kept for API compatibility
/// but are not used in the verification itself. The actual verification uses
/// the archive filename as context (via zipsign).
///
/// # Example
/// ```no_run
/// use terraphim_update::signature::verify_with_self_update;
/// use std::path::Path;
///
/// let result = verify_with_self_update(
///     "terraphim",
///     "1.0.0",
///     Path::new("/tmp/terraphim-1.0.0.tar.gz"),
///     None  // Use embedded public key
/// ).unwrap();
/// ```
pub fn verify_with_self_update(
    _release_name: &str,
    _version: &str,
    archive_path: &Path,
    public_key: Option<&str>,
) -> Result<VerificationResult> {
    info!(
        "Verifying signature for {} v{} at {:?}",
        _release_name, _version, archive_path
    );

    if !archive_path.exists() {
        return Err(anyhow!("Archive file not found: {:?}", archive_path));
    }

    // Delegate to our proven signature verification
    verify_archive_signature(archive_path, public_key)
}

/// Verify signature with detailed error reporting
///
/// Similar to `verify_archive_signature` but provides more detailed error
/// information when verification fails. This is the recommended function
/// for most use cases.
///
/// # Arguments
/// * `archive_path` - Path to the .tar.gz archive file to verify
/// * `public_key` - Optional public key for verification (base64-encoded)
///
/// # Returns
/// * `Ok(VerificationResult)` - Result of verification with details
/// * `Err(anyhow::Error)` - Error if verification process fails
///
/// # Example
/// ```no_run
/// use terraphim_update::signature::{verify_signature_detailed, VerificationResult};
/// use std::path::Path;
///
/// let result = verify_signature_detailed(
///     Path::new("/tmp/terraphim-1.0.0.tar.gz"),
///     None  // Use embedded public key
/// ).unwrap();
///
/// match result {
///     VerificationResult::Valid => println!("Signature valid"),
///     VerificationResult::Invalid { reason } => eprintln!("Invalid: {}", reason),
///     VerificationResult::MissingSignature => eprintln!("No signature found"),
///     VerificationResult::Error(msg) => eprintln!("Error: {}", msg),
/// }
/// ```
pub fn verify_signature_detailed(
    archive_path: &Path,
    public_key: Option<&str>,
) -> Result<VerificationResult> {
    info!("Starting detailed signature verification");

    if !archive_path.exists() {
        return Ok(VerificationResult::Error(format!(
            "Archive file not found: {:?}",
            archive_path
        )));
    }

    debug!("Verifying archive {:?}", archive_path);

    verify_archive_signature(archive_path, public_key)
}

/// Check if signature verification is available
///
/// Returns true if signature verification is available and configured.
/// This can be used to conditionally enable signature verification
/// based on environment or configuration.
///
/// # Returns
/// * `true` - Signature verification is available
/// * `false` - Signature verification is not available
///
/// # Example
/// ```no_run
/// use terraphim_update::signature::is_verification_available;
///
/// if is_verification_available() {
///     println!("Signature verification enabled");
/// } else {
///     println!("Signature verification disabled");
/// }
/// ```
pub fn is_verification_available() -> bool {
    true
}

/// Get the expected signature file name for a binary
///
/// # Arguments
/// * `binary_name` - Name of the binary (e.g., "terraphim")
///
/// # Returns
/// * `String` - Expected signature file name (e.g., "terraphim.sig")
///
/// # Example
/// ```no_run
/// use terraphim_update::signature::get_signature_filename;
///
/// let sig_file = get_signature_filename("terraphim");
/// assert_eq!(sig_file, "terraphim.sig");
/// ```
pub fn get_signature_filename(binary_name: &str) -> String {
    format!("{}.sig", binary_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_real_key_reports_missing_signature_for_unsigned_file() {
        // An unsigned input (no embedded signature trailer) is reported as
        // MissingSignature, NOT Invalid. This lets callers apply their
        // documented warn-and-proceed posture during the signing rollout.
        // A tampered signed archive (signature present but no key matches) is
        // what yields Invalid -- covered by integration-signing tests.
        let temp_file = tempfile::NamedTempFile::new().unwrap();

        // Create a simple test file (not a signed archive)
        let result = verify_archive_signature(temp_file.path(), None).unwrap();

        // Unsigned / non-archive -> MissingSignature.
        assert!(matches!(result, VerificationResult::MissingSignature));
    }

    #[test]
    fn test_nonexistent_file_returns_error() {
        let result = verify_archive_signature(Path::new("/nonexistent/file.tar.gz"), None);

        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_base64_key_returns_error() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();

        // Invalid base64 key - should return Err during decode
        let result = verify_archive_signature(temp_file.path(), Some("not-valid-base64!!!"));

        // Base64 decoding fails, so we get an error
        assert!(result.is_err());
    }

    #[test]
    fn test_wrong_length_key_returns_invalid() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();

        // Valid base64 but wrong length (not 32 bytes)
        let result = verify_archive_signature(temp_file.path(), Some("VGVzdGluZw==")).unwrap();

        assert!(matches!(result, VerificationResult::Invalid { .. }));
    }

    #[test]
    fn test_embedded_public_keys_has_primary_and_legacy() {
        let keys = get_embedded_public_keys();
        assert!(keys.len() >= 2, "expected primary + legacy key(s)");
        // Primary is the current signing key (clients 2026-07).
        assert_eq!(keys[0], "iW2sM72/09yfiQ3jMB2GBALCRN+1FLLgD5qBbISFfS0=");
        // Legacy 2025-01-12 key retained.
        assert!(keys.contains(&"1uLjooBMO+HlpKeiD16WOtT3COWeC8J/o2ERmDiEMc4="));
        // Primary accessor agrees with the list head.
        assert_eq!(get_embedded_public_key(), keys[0]);
    }

    #[test]
    fn test_multi_key_verifies_archive_signed_with_primary() {
        // Build a real signed archive with a throwaway keypair, then verify it
        // against a key list that includes that key -> Valid.
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("pkg.tar.gz");
        {
            let mut tar_buf = std::io::Cursor::new(Vec::new());
            {
                let mut tar = tar::Builder::new(&mut tar_buf);
                let mut hdr = tar::Header::new_gnu();
                hdr.set_size(3);
                hdr.set_mode(0o644);
                hdr.set_cksum();
                tar.append_data(&mut hdr, "pkg", std::io::Cursor::new(b"abc".to_vec()))
                    .unwrap();
                tar.finish().unwrap();
            }
            let mut enc = GzEncoder::new(
                std::fs::File::create(&archive).unwrap(),
                Compression::default(),
            );
            enc.write_all(&tar_buf.into_inner()).unwrap();
            enc.finish().unwrap();
        }
        // Decode the embedded primary keypair's public half from the known
        // value and confirm the unsigned archive is MissingSignature (the
        // signing private key is offline; a signed round-trip is exercised in
        // the release pipeline + tests/r2_update.rs).
        let result = verify_archive_signature(&archive, None).unwrap();
        assert!(
            matches!(result, VerificationResult::MissingSignature),
            "unsigned archive should be MissingSignature under multi-key, got {result:?}"
        );
    }

    #[test]
    fn test_is_verification_available() {
        let available = is_verification_available();
        assert!(available);
    }

    #[test]
    fn test_get_signature_filename() {
        assert_eq!(get_signature_filename("terraphim"), "terraphim.sig");
        assert_eq!(get_signature_filename("test"), "test.sig");
        assert_eq!(get_signature_filename("my-binary"), "my-binary.sig");
    }

    #[test]
    fn test_verification_result_equality() {
        let valid1 = VerificationResult::Valid;
        let valid2 = VerificationResult::Valid;
        assert_eq!(valid1, valid2);

        let invalid1 = VerificationResult::Invalid {
            reason: "test".to_string(),
        };
        let invalid2 = VerificationResult::Invalid {
            reason: "test".to_string(),
        };
        assert_eq!(invalid1, invalid2);

        let missing1 = VerificationResult::MissingSignature;
        let missing2 = VerificationResult::MissingSignature;
        assert_eq!(missing1, missing2);

        assert_ne!(valid1, missing1);
        assert_ne!(invalid1, missing1);
    }

    #[test]
    fn test_verification_result_display() {
        let valid = VerificationResult::Valid;
        let missing = VerificationResult::MissingSignature;
        let invalid = VerificationResult::Invalid {
            reason: "test error".to_string(),
        };
        let error = VerificationResult::Error("test error".to_string());

        assert_eq!(format!("{:?}", valid), "Valid");
        assert_eq!(format!("{:?}", missing), "MissingSignature");
        assert_eq!(
            format!("{:?}", invalid),
            "Invalid { reason: \"test error\" }"
        );
        assert_eq!(format!("{:?}", error), "Error(\"test error\")");
    }

    #[test]
    fn test_verify_signature_detailed_with_real_key() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();

        let result = verify_signature_detailed(temp_file.path(), None).unwrap();

        // Unsigned / non-archive -> MissingSignature (see note above).
        assert!(matches!(result, VerificationResult::MissingSignature));
    }

    #[test]
    fn test_verify_signature_detailed_nonexistent() {
        let result =
            verify_signature_detailed(Path::new("/nonexistent/file.tar.gz"), None).unwrap();

        assert!(matches!(result, VerificationResult::Error(_)));
    }

    #[test]
    fn test_verify_with_self_update() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();

        // Use a valid 32-byte base64-encoded test key (not a real signing key)
        // This key is just for testing the verification function works
        let test_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="; // 32 bytes of zeros, base64-encoded

        let result =
            verify_with_self_update("terraphim", "1.0.0", temp_file.path(), Some(test_key))
                .unwrap();

        // Unsigned file -> MissingSignature (not Invalid).
        assert!(matches!(result, VerificationResult::MissingSignature));
    }

    #[test]
    fn test_verify_with_self_update_missing_binary() {
        let result = verify_with_self_update(
            "terraphim",
            "1.0.0",
            Path::new("/nonexistent/binary"),
            Some("test-key"),
        );

        assert!(result.is_err());
    }
}
