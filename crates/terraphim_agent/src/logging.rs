//! Logging initialisation for `terraphim-agent`.
//!
//! This wraps the standard [`env_logger`] backend with a thin, message-aware
//! filter that drops a single benign `ERROR` line emitted by `terraphim_service`
//! on every knowledge-graph subcommand.
//!
//! ## Why this exists
//!
//! When a knowledge-graph call (`extract`, `replace`, `validate`, `suggest`)
//! resolves a role whose thesaurus has not yet been persisted, the service's
//! `ensure_thesaurus_loaded` first attempts to load the optional persisted
//! thesaurus and receives a `terraphim_persistence::Error::NotFound`. It then
//! transparently rebuilds the thesaurus from the local KG and succeeds. The
//! service already tries to downgrade this expected miss to `debug`, but its
//! guard matches the lowercase substrings `"file not found"` / `"not found:"`
//! against the error's `Display` form (`"Not found: thesaurus_default.json"`,
//! capital `N`) and so the case-sensitive check slips through to:
//!
//! ```text
//! ERROR terraphim_service] Failed to load thesaurus: NotFound("thesaurus_default.json")
//! ```
//!
//! That `ERROR` is misleading (the operation succeeds) and pollutes stderr and
//! scripted/JSON usage. The fix lives in the external `terraphim_service`
//! crate, which we cannot edit here, so the agent installs its own logger that
//! suppresses exactly this benign record while passing every other log line —
//! including genuine thesaurus failures — through untouched.
//!
//! See terraphim/terraphim-clients#48.

use std::sync::Once;

use log::{Level, LevelFilter, Log, Metadata, Record};

static INIT: Once = Once::new();

/// Returns `true` when `record` data describes the known-benign "optional
/// persisted thesaurus absent" `ERROR` produced by `terraphim_service` when it
/// successfully falls back to rebuilding the thesaurus from the local KG.
///
/// The match is deliberately narrow: only an `ERROR` originating in
/// `terraphim_service` whose message reports a failure to *load* the thesaurus
/// because of a `NotFound` is suppressed. Genuine failures — for example
/// `"Failed to build thesaurus from local KG"` — do not match and are always
/// emitted. The `NotFound` check accepts both the `Display`
/// (`"Not found: ..."`) and `Debug` (`NotFound(...)`) renderings, since the
/// service logs the underlying error with `{:?}`.
fn is_benign_thesaurus_not_found(level: Level, target: &str, message: &str) -> bool {
    if level != Level::Error || !target.starts_with("terraphim_service") {
        return false;
    }
    if !message.contains("Failed to load thesaurus") {
        return false;
    }
    let lower = message.to_ascii_lowercase();
    lower.contains("not found") || lower.contains("notfound")
}

/// A [`Log`] wrapper that drops the benign thesaurus-not-found `ERROR` and
/// delegates every other record to the inner logger unchanged.
struct FilteredLogger<L: Log> {
    inner: L,
}

impl<L: Log> Log for FilteredLogger<L> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if is_benign_thesaurus_not_found(
            record.level(),
            record.target(),
            &record.args().to_string(),
        ) {
            return;
        }
        self.inner.log(record);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// Build the inner `env_logger` backend, mirroring the level and format that
/// `terraphim_service::logging` selected previously so that output is
/// unchanged apart from the suppressed benign line.
///
/// Selection order matches the service's `detect_logging_config`:
/// an explicit `LOG_LEVEL` wins, otherwise `DEBUG`-assertion builds log at
/// `INFO` and release builds at `WARN`.
fn build_inner_logger() -> env_logger::Logger {
    let mut builder = env_logger::Builder::new();
    builder.format_timestamp_secs();

    if let Some(level) = std::env::var("LOG_LEVEL")
        .ok()
        .and_then(|s| s.parse::<LevelFilter>().ok())
    {
        builder.filter_level(level);
    } else if cfg!(debug_assertions) {
        builder.filter_level(LevelFilter::Info);
    } else {
        builder.filter_level(LevelFilter::Warn);
        builder.format_module_path(false);
    }

    builder.build()
}

/// Initialise global logging for `terraphim-agent`.
///
/// Installs a [`FilteredLogger`] wrapping `env_logger`. Safe to call multiple
/// times: only the first call installs a logger, and installation is skipped if
/// another logger is already set (so it never panics in test harnesses).
pub fn init_logging() {
    INIT.call_once(|| {
        let inner = build_inner_logger();
        let max_level = inner.filter();
        let logger = FilteredLogger { inner };
        if log::set_boxed_logger(Box::new(logger)).is_ok() {
            log::set_max_level(max_level);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A real, in-memory [`Log`] implementation used to observe which records
    /// survive the filter. Not a mock: it fully implements the trait and
    /// records every delivered message.
    struct CapturingLogger {
        records: Mutex<Vec<String>>,
    }

    impl CapturingLogger {
        fn new() -> Self {
            Self {
                records: Mutex::new(Vec::new()),
            }
        }
    }

    impl Log for CapturingLogger {
        fn enabled(&self, _metadata: &Metadata) -> bool {
            true
        }

        fn log(&self, record: &Record) {
            self.records.lock().unwrap().push(format!(
                "{} {}: {}",
                record.level(),
                record.target(),
                record.args()
            ));
        }

        fn flush(&self) {}
    }

    /// The benign message must be derived from the *real* persistence error
    /// type and the *real* `{:?}` format the service uses, so the predicate is
    /// pinned to the exact string observed at runtime.
    fn real_benign_message() -> String {
        let err = terraphim_persistence::Error::NotFound("thesaurus_default.json".to_string());
        format!("Failed to load thesaurus: {err:?}")
    }

    #[test]
    fn predicate_matches_real_persistence_notfound_debug_form() {
        let msg = real_benign_message();
        // Sanity-check the reproduction: this is the exact stderr line.
        assert_eq!(
            msg,
            "Failed to load thesaurus: NotFound(\"thesaurus_default.json\")"
        );
        assert!(is_benign_thesaurus_not_found(
            Level::Error,
            "terraphim_service",
            &msg
        ));
    }

    #[test]
    fn predicate_matches_display_form_not_found() {
        // Defensive: also match the Display rendering ("Not found: ...").
        let msg = "Failed to load thesaurus: Not found: thesaurus_default.json";
        assert!(is_benign_thesaurus_not_found(
            Level::Error,
            "terraphim_service",
            msg
        ));
    }

    #[test]
    fn predicate_ignores_non_error_levels() {
        let msg = real_benign_message();
        assert!(!is_benign_thesaurus_not_found(
            Level::Warn,
            "terraphim_service",
            &msg
        ));
    }

    #[test]
    fn predicate_ignores_other_targets() {
        let msg = real_benign_message();
        assert!(!is_benign_thesaurus_not_found(
            Level::Error,
            "terraphim_mcp_server",
            &msg
        ));
    }

    #[test]
    fn predicate_preserves_genuine_thesaurus_failures() {
        // A real build failure must never be suppressed.
        let msg = "Failed to build thesaurus from local KG for role Default: parse error";
        assert!(!is_benign_thesaurus_not_found(
            Level::Error,
            "terraphim_service",
            msg
        ));
    }

    #[test]
    fn predicate_preserves_unrelated_errors() {
        let msg = "database connection refused";
        assert!(!is_benign_thesaurus_not_found(
            Level::Error,
            "terraphim_service",
            msg
        ));
    }

    #[test]
    fn filtered_logger_drops_benign_and_keeps_the_rest() {
        let capture = CapturingLogger::new();
        let logger = FilteredLogger { inner: capture };

        // Benign thesaurus-not-found ERROR -> dropped.
        logger.log(
            &Record::builder()
                .level(Level::Error)
                .target("terraphim_service")
                .args(format_args!(
                    "Failed to load thesaurus: NotFound(\"thesaurus_default.json\")"
                ))
                .build(),
        );
        // Genuine ERROR from the same crate -> kept.
        logger.log(
            &Record::builder()
                .level(Level::Error)
                .target("terraphim_service")
                .args(format_args!("Failed to build thesaurus from local KG"))
                .build(),
        );
        // Ordinary INFO -> kept.
        logger.log(
            &Record::builder()
                .level(Level::Info)
                .target("terraphim_agent::service")
                .args(format_args!("Initializing TUI service"))
                .build(),
        );

        let records = logger.inner.records.lock().unwrap();
        assert_eq!(records.len(), 2, "exactly one record should be suppressed");
        assert!(records.iter().all(|r| !r.contains("NotFound")));
        assert!(
            records
                .iter()
                .any(|r| r.contains("Failed to build thesaurus"))
        );
        assert!(
            records
                .iter()
                .any(|r| r.contains("Initializing TUI service"))
        );
    }
}
