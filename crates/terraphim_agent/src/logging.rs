//! Logging initialisation for `terraphim-agent`.
//!
//! This wraps the standard [`env_logger`] backend with a thin, message-aware
//! filter that drops two classes of record: a benign `ERROR` line emitted by
//! `terraphim_service` on every knowledge-graph subcommand, and the flood of
//! per-line parse warnings the session analyzer emits for the event
//! envelopes it does not recognise.
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
//!
//! ## The session analyzer's parse warnings
//!
//! `terraphim_session_analyzer` parses every line of a Claude Code JSONL
//! session and warns for each line whose envelope it does not recognise:
//!
//! ```text
//! WARN terraphim_session_analyzer::parser] Failed to parse line 9:
//!   {"type":"atis-latch",...} - Error: missing field `uuid` at line 1 column 82
//! ```
//!
//! Claude Code adds event types faster than the analyzer learns them
//! (`atis-latch`, `api-request-shape`, `cost-state`, ...), so a single search
//! over a session directory produced thousands of these lines - stderr became
//! unreadable and the real warnings were lost in the flood. The analyzer
//! already skips the metadata event types it knows
//! (`SKIP_ENTRY_TYPES`), so the fix belongs upstream; until that lands, the
//! agent drops the warnings for the envelopes it can recognise as unrecognised
//! (a valid JSON object that simply has no `uuid` field), and keeps every
//! other parse warning.
//!
//! See terraphim/terraphim-clients#402.

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

/// Returns `true` when the record is the session analyzer's warning about a
/// line whose envelope it does not recognise.
///
/// The analyzer logs `Failed to parse line <n>: <line> - Error: missing field
/// \`uuid\` at line 1 column <c>` for every event type it does not know. The
/// line itself is valid JSON - it simply is not a conversational entry - so
/// the record carries no information about a problem the user can act on,
/// and there is one per line of every session.
///
/// The match is deliberately narrow: only a `WARN` from the session analyzer
/// whose message reports a **missing field** is dropped. A line that is
/// actually malformed (`expected value at line 1 column 1`) keeps its warning.
fn is_unrecognised_session_event(level: Level, target: &str, message: &str) -> bool {
    if level != Level::Warn || !target.starts_with("terraphim_session_analyzer") {
        return false;
    }
    if !message.contains("Failed to parse line") {
        return false;
    }
    message.contains("missing field")
}

/// A [`Log`] wrapper that drops the benign thesaurus-not-found `ERROR` and
/// the session analyzer's unrecognised-event warnings, and delegates every
/// other record to the inner logger unchanged.
struct FilteredLogger<L: Log> {
    inner: L,
}

impl<L: Log> Log for FilteredLogger<L> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        let message = record.args().to_string();
        if is_benign_thesaurus_not_found(record.level(), record.target(), &message) {
            return;
        }
        if is_unrecognised_session_event(record.level(), record.target(), &message) {
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

    /// The real warning the analyzer emits for an event type it does not
    /// recognise, pinned to the format it uses.
    fn real_unrecognised_event_warning() -> String {
        "Failed to parse line 9: {\"type\":\"atis-latch\",\"atis\":\"\",\"sessionId\":\"bc40\"} - Error: missing field `uuid` at line 1 column 82".to_string()
    }

    #[test]
    fn predicate_matches_the_real_unrecognised_event_warning() {
        let msg = real_unrecognised_event_warning();
        assert!(is_unrecognised_session_event(
            Level::Warn,
            "terraphim_session_analyzer::parser",
            &msg
        ));
    }

    #[test]
    fn predicate_keeps_a_genuinely_malformed_line() {
        // A line that is not valid JSON at all is a real problem, and its
        // warning must survive.
        let msg = "Failed to parse line 3: {not json - Error: expected value at line 1 column 2";
        assert!(!is_unrecognised_session_event(
            Level::Warn,
            "terraphim_session_analyzer::parser",
            msg
        ));
    }

    #[test]
    fn predicate_keeps_warnings_from_other_targets() {
        let msg = real_unrecognised_event_warning();
        assert!(!is_unrecognised_session_event(
            Level::Warn,
            "terraphim_agent::sessions",
            &msg
        ));
    }

    #[test]
    fn predicate_keeps_other_levels() {
        let msg = real_unrecognised_event_warning();
        assert!(!is_unrecognised_session_event(
            Level::Error,
            "terraphim_session_analyzer::parser",
            &msg
        ));
    }

    #[test]
    fn filtered_logger_drops_the_session_flood_and_keeps_real_warnings() {
        let capture = CapturingLogger::new();
        let logger = FilteredLogger { inner: capture };

        // The flood: dropped.
        logger.log(
            &Record::builder()
                .level(Level::Warn)
                .target("terraphim_session_analyzer::parser")
                .args(format_args!("{}", real_unrecognised_event_warning()))
                .build(),
        );
        // A genuine parse failure: kept.
        logger.log(
            &Record::builder()
                .level(Level::Warn)
                .target("terraphim_session_analyzer::parser")
                .args(format_args!(
                    "Failed to parse line 3: {{not json - Error: expected value at line 1 column 2"
                ))
                .build(),
        );

        let records = logger.inner.records.lock().unwrap();
        assert_eq!(records.len(), 1, "only the genuine warning should survive");
        assert!(
            records[0].contains("expected value"),
            "the genuine warning must survive, got: {:?}",
            records
        );
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
