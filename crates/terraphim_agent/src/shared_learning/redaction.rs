//! Secret redaction for `terraphim_agent::shared_learning` (Refs #178).
//!
//! **CANONICAL SOURCE**: `terraphim_agent::learnings::redaction::redact_secrets`.
//! This module exists because `shared_learning` lives at the lib root
//! while `learnings` is only declared in `main.rs` (binary entry), so
//! the lib cannot `use crate::learnings::redaction`. The pattern list is
//! intentionally duplicated here with a drift-guard assertion that fails
//! CI when the two lists disagree.
//!
//! **TODO**: extract to a shared `terraphim_redaction` crate, OR promote
//! `learnings` to a lib-root module (then this duplicate can be removed).
//! Tracked as follow-up ADR; see `terraphim-clients#178` discussion thread.
//!
//! ## Public API
//!
//! - [`redact_secrets`] — apply known credential-pattern redaction to a
//!   string. Behaviourally equivalent to
//!   `terraphim_agent::learnings::redaction::redact_secrets`.

/// Standard secret patterns for redaction. Patterns are matched using regex.
///
/// **MIRROR**: kept in lockstep with
/// `terraphim_agent::learnings::redaction::SECRET_PATTERNS`. Drift is
/// caught by `assert_secret_patterns_in_sync`.
const SECRET_PATTERNS: &[(&str, &str)] = &[
    // AWS Access Key IDs (AKIA followed by 16 alphanumeric chars)
    (r"AKIA[A-Z0-9]{16}", "[AWS_KEY_REDACTED]"),
    // AWS Secret Access Keys (40 char base64-ish)
    (r"[A-Za-z0-9/+=]{40}", "[AWS_SECRET_REDACTED]"),
    // Generic API keys with common prefixes
    (r"sk-[A-Za-z0-9-_]{20,}", "[OPENAI_KEY_REDACTED]"),
    (r"xox[baprs]-[A-Za-z0-9-]+", "[SLACK_TOKEN_REDACTED]"),
    (r"ghp_[A-Za-z0-9]{36}", "[GITHUB_TOKEN_REDACTED]"),
    (r"gho_[A-Za-z0-9]{36}", "[GITHUB_TOKEN_REDACTED]"),
    // Connection strings
    (r"postgresql://[^@\s]+:[^@\s]+@", "postgresql://[REDACTED]@"),
    (r"mysql://[^@\s]+:[^@\s]+@", "mysql://[REDACTED]@"),
    (
        r"mongodb(\+srv)?://[^@\s]+:[^@\s]+@",
        "mongodb://[REDACTED]@",
    ),
    (r"redis://[^@\s]+:[^@\s]+@", "redis://[REDACTED]@"),
];

/// Environment variable patterns to strip entirely.
const ENV_VAR_PATTERNS: &[&str] = &[
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "DATABASE_URL",
    "API_KEY",
    "SECRET_KEY",
    "PASSWORD",
    "TOKEN",
    "AUTH",
    "CREDENTIAL",
];

/// Redact secrets from text using regex pattern matching.
///
/// Behaviourally equivalent to
/// `terraphim_agent::learnings::redaction::redact_secrets`. Applied at
/// every persistence boundary in `terraphim_agent::shared_learning`
/// (Refs #178).
pub fn redact_secrets(text: &str) -> String {
    let mut result = strip_env_vars(text);
    for (pattern, replacement) in SECRET_PATTERNS {
        if let Ok(re) = regex::Regex::new(pattern) {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }
    result
}

fn strip_env_vars(text: &str) -> String {
    let mut result = text.to_string();
    for var_name in ENV_VAR_PATTERNS {
        let pattern_unquoted = format!("{0}\\s*=\\s*[^\\s]+", var_name);
        let pattern_double = format!("{0}\\s*=\\s*\"[^\"]+\"", var_name);
        let pattern_single = format!("{0}\\s*=\\s*'[^']+'", var_name);
        let patterns = [pattern_unquoted, pattern_double, pattern_single];
        for pattern in patterns {
            if let Ok(re) = regex::Regex::new(&pattern) {
                let replacement = format!("{}=[ENV_REDACTED]", var_name);
                result = re.replace_all(&result, replacement.as_str()).to_string();
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drift guard: assert the count and tuple-shape of SECRET_PATTERNS
    /// here match the canonical list in
    /// `terraphim_agent::learnings::redaction::SECRET_PATTERNS`.
    ///
    /// Strategy: read the canonical source file at test time (relative
    /// path resolved from `CARGO_MANIFEST_DIR`), count tuples of the
    /// form `(r"...", "[...]")`, and assert the count matches our local
    /// constant. If either side drifts, this test fails CI.
    #[test]
    fn assert_secret_patterns_in_sync() {
        // The canonical file is at `<crate>/src/learnings/redaction.rs`
        // relative to the terraphim_agent crate root.
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let canonical_path = std::path::Path::new(manifest_dir)
            .join("src/learnings/redaction.rs");
        let canonical_src = std::fs::read_to_string(&canonical_path).unwrap_or_else(|e| {
            panic!(
                "could not read canonical redaction.rs at {}: {}",
                canonical_path.display(),
                e
            )
        });

        let secret_patterns_start = canonical_src
            .find("SECRET_PATTERNS: &[(&str, &str)]")
            .expect("SECRET_PATTERNS declaration must exist in canonical file");
        let after_start = &canonical_src[secret_patterns_start..];

        let canonical_count: usize = after_start
            .lines()
            .take_while(|l| l.trim() != "];")
            .filter(|l| {
                let t = l.trim_start();
                (t.starts_with("(r\"") || t == "(") && !t.starts_with("//")
            })
            .count();

        assert_eq!(
            canonical_count,
            SECRET_PATTERNS.len(),
            "SECRET_PATTERNS drift between terraphim_agent::shared_learning::redaction \
             ({} entries) and terraphim_agent::learnings::redaction ({} entries). \
             Update both sides to match.",
            SECRET_PATTERNS.len(),
            canonical_count,
        );
    }

    #[test]
    fn test_redact_aws_key() {
        let input = "Using key AKIAIOSFODNN7EXAMPLE to connect";
        let redacted = redact_secrets(input);
        assert!(redacted.contains("[AWS_KEY_REDACTED]"));
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn test_redact_connection_string() {
        let input = "postgresql://user:password@localhost:5432/db";
        let redacted = redact_secrets(input);
        assert!(redacted.contains("[REDACTED]"));
        assert!(!redacted.contains("password"));
    }

    #[test]
    fn test_strip_env_vars() {
        let input = r#"DATABASE_URL=postgres://user:pass@host API_KEY="secret123""#;
        let stripped = strip_env_vars(input);
        assert!(stripped.contains("DATABASE_URL=[ENV_REDACTED]"));
        assert!(stripped.contains("API_KEY=[ENV_REDACTED]"));
        assert!(!stripped.contains("secret123"));
    }

    #[test]
    fn test_no_change_for_benign_text() {
        let inputs = [
            "I ran cargo test --workspace and it passed",
            "The endpoint is /api/v2/users",
            "We use Result<T> not unwrap()",
        ];
        for input in inputs {
            assert_eq!(redact_secrets(input), input, "benign text was modified: \"{input}\"");
        }
    }

    #[test]
    fn test_redact_multiple_secrets() {
        let input = "Key: AKIAIOSFODNN7EXAMPLE and sk-proj-abcdefghijklmnopqrst";
        let redacted = redact_secrets(input);
        assert!(redacted.contains("[AWS_KEY_REDACTED]"));
        assert!(redacted.contains("[OPENAI_KEY_REDACTED]"));
    }

    #[test]
    fn test_idempotent_on_already_redacted_text() {
        let once = redact_secrets("AWS_KEY=AKIAIOSFODNN7EXAMPLE");
        let twice = redact_secrets(&once);
        assert_eq!(once, twice, "redaction is not idempotent");
    }
}