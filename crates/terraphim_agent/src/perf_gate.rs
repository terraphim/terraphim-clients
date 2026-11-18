//! A timed-commands performance gate for the agent's own CLI (#437).
//!
//! The repository carries Criterion benches for library functions and no
//! ceiling for the product: a command can get an order of magnitude slower
//! and nothing fails. This module is the measuring half of the gate that
//! closes that hole:
//!
//! * a **manifest** (JSON, committed next to the code) declares the commands
//!   to measure and the budget each one has to meet, per build profile;
//! * the **runner** spawns the built binary under the caller's environment
//!   (the caller decides how - the test driver uses the hermetic
//!   environment), times the repetitions and reports the median and the
//!   nearest-rank p95;
//! * the **evaluator** compares the measurements with the manifest's budgets
//!   and the **renderer** prints the table that the pull request carries.
//!
//! Nothing here decides whether a budget was breached: the caller does, so
//! that the ordinary test lane can measure and report without asserting on
//! timings that a loaded machine would make meaningless.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use serde::Deserialize;

/// The manifest schema version this module understands.
pub const MANIFEST_VERSION: u32 = 1;

/// The build profile used when the binary's path does not say which one it is.
pub const DEFAULT_PROFILE: &str = "dev";

/// The ceiling pair a scenario has to meet in one build profile.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Budget {
    /// The ceiling on the median of the timed repetitions, in milliseconds.
    pub median_ms: f64,
    /// The ceiling on the nearest-rank p95 of the timed repetitions, in
    /// milliseconds. It is never tighter than `median_ms`: the p95 catches
    /// the tail the median hides, so it is the wider of the two.
    pub p95_ms: f64,
}

/// One command to measure.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    /// The scenario's name; it appears in the report and in the manifest.
    pub name: String,
    /// The argument vector passed to the agent binary.
    pub args: Vec<String>,
    /// The text piped to the child's stdin. `Some` also makes the child's
    /// stdin a pipe, so an empty string is an immediate end-of-file.
    #[serde(default)]
    pub stdin: Option<String>,
    /// How many timed runs the scenario makes.
    #[serde(default = "default_repetitions")]
    pub repetitions: usize,
    /// The exit statuses the scenario tolerates. The default is the single
    /// status 0. A command whose job is to report a problem - a doctor, for
    /// instance - has a second status for "the environment is incomplete",
    /// and the gate measures its latency either way.
    #[serde(default = "default_allowed_exit_codes")]
    pub allowed_exit_codes: Vec<i32>,
    /// The subcommand that has to exist for the scenario to be measurable.
    /// When the subcommand is absent from the build the scenario is skipped
    /// with a reason, never counted as a pass.
    #[serde(default)]
    pub requires_subcommand: Option<String>,
    /// The reason printed with the skip.
    #[serde(default)]
    pub absent_note: Option<String>,
    /// The budgets, per build profile.
    #[serde(default)]
    pub budgets: BTreeMap<String, Budget>,
}

fn default_repetitions() -> usize {
    5
}

fn default_allowed_exit_codes() -> Vec<i32> {
    vec![0]
}

/// The committed manifest: the scenarios and the budgets they have to meet.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// The schema version.
    pub version: u32,
    /// The ceiling on the measured binary's size, per build profile.
    #[serde(default)]
    pub binary_size_budgets: BTreeMap<String, u64>,
    /// The scenarios to measure.
    pub scenarios: Vec<Scenario>,
}

impl Manifest {
    /// Parse a manifest and check that it is coherent.
    pub fn parse(json: &str) -> Result<Self, String> {
        let manifest: Manifest = serde_json::from_str(json)
            .map_err(|error| format!("the budget manifest is not valid JSON: {error}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Reject a manifest that would make the gate meaningless.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != MANIFEST_VERSION {
            return Err(format!(
                "the budget manifest declares version {}, but this gate understands version {MANIFEST_VERSION}",
                self.version
            ));
        }
        if self.scenarios.is_empty() {
            return Err("the budget manifest declares no scenarios".to_string());
        }

        let mut names = BTreeSet::new();
        for scenario in &self.scenarios {
            if scenario.name.trim().is_empty() {
                return Err("a scenario has no name".to_string());
            }
            if !names.insert(scenario.name.as_str()) {
                return Err(format!(
                    "two scenarios are named `{}`; a name identifies one measurement",
                    scenario.name
                ));
            }
            if scenario.args.is_empty() {
                return Err(format!(
                    "the scenario `{}` has no arguments; there is nothing to run",
                    scenario.name
                ));
            }
            if scenario.repetitions == 0 {
                return Err(format!(
                    "the scenario `{}` asks for zero repetitions; there is nothing to measure",
                    scenario.name
                ));
            }
            for (profile, budget) in &scenario.budgets {
                if budget.median_ms <= 0.0 || budget.p95_ms <= 0.0 {
                    return Err(format!(
                        "the scenario `{}` has a non-positive budget for the profile `{profile}`",
                        scenario.name
                    ));
                }
                if budget.p95_ms < budget.median_ms {
                    return Err(format!(
                        "the scenario `{}` has a p95 budget ({}) tighter than its median budget \
                         ({}) for the profile `{profile}`; the p95 is the wider of the two",
                        scenario.name, budget.p95_ms, budget.median_ms
                    ));
                }
            }
        }
        Ok(())
    }

    /// The scenario with the given name.
    pub fn scenario(&self, name: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.name == name)
    }

    /// The binary size budget for a profile, if it declares one.
    pub fn binary_size_budget(&self, profile: &str) -> Option<u64> {
        self.binary_size_budgets.get(profile).copied()
    }
}

/// What one scenario produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The scenario ran and was measured.
    Measured(Measurement),
    /// The scenario's command is not in this build; the reason says why.
    Skipped {
        /// The scenario's name.
        name: String,
        /// Why the scenario could not be measured.
        reason: String,
    },
}

/// The timings of one scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// The scenario's name.
    pub name: String,
    /// The median of the timed repetitions, in milliseconds.
    pub median_ms: f64,
    /// The nearest-rank p95 of the timed repetitions, in milliseconds.
    pub p95_ms: f64,
    /// How many repetitions were timed.
    pub repetitions: usize,
}

/// How a scenario stands against its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// The measurement met the budget.
    Pass,
    /// The measurement broke the budget.
    Breach,
    /// The scenario's command is not in this build.
    Skipped,
    /// The scenario was measured and this profile has no budget for it.
    Unbudgeted,
}

impl Status {
    /// The word the report prints for this status.
    pub fn label(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Breach => "BREACH",
            Status::Skipped => "skipped",
            Status::Unbudgeted => "not gated",
        }
    }
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The scenario's name.
    pub name: String,
    /// The measured median, in milliseconds.
    pub median_ms: Option<f64>,
    /// The measured p95, in milliseconds.
    pub p95_ms: Option<f64>,
    /// The budget the measurement was compared with, if any.
    pub budget: Option<Budget>,
    /// How the scenario stands.
    pub status: Status,
    /// The reason for a skip, or the detail of a breach.
    pub note: String,
}

/// Parse a manifest from JSON text.
pub fn parse_manifest(json: &str) -> Result<Manifest, String> {
    Manifest::parse(json)
}

/// The median of an ascending slice.
pub fn median(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[middle]
    } else {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    }
}

/// The nearest-rank percentile of an ascending slice.
///
/// The nearest rank is the smallest value at or above the requested
/// percentile, so the p95 of five samples is the slowest of the five.
pub fn percentile_nearest_rank(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (percentile * sorted.len() as f64).ceil().max(1.0) as usize;
    sorted[rank.min(sorted.len()) - 1]
}

/// The build profile a binary was produced with, read from its path.
///
/// Cargo writes the dev profile to `target/debug` and the release profile to
/// `target/release`; the path is the only signal available to a test that did
/// not build the binary itself.
pub fn detect_profile(binary: &Path) -> String {
    let release = binary
        .components()
        .any(|component| component.as_os_str() == "release");
    if release {
        "release".to_string()
    } else {
        DEFAULT_PROFILE.to_string()
    }
}

/// The size of the measured binary, in bytes.
pub fn binary_size(binary: &Path) -> Result<u64, String> {
    let metadata = std::fs::metadata(binary)
        .map_err(|error| format!("the binary {} could not be read: {error}", binary.display()))?;
    Ok(metadata.len())
}

/// Measure one scenario against the binary.
///
/// `configure` prepares every child command - the hermetic environment, for
/// instance - and is called for each spawn, including the availability probe.
/// A scenario that names `requires_subcommand` is skipped when that
/// subcommand is not in the build; every other scenario must succeed, and a
/// failing command is an error rather than a slow measurement.
pub fn run_scenario<F>(binary: &Path, scenario: &Scenario, configure: &F) -> Result<Outcome, String>
where
    F: Fn(&mut Command) -> Result<(), String>,
{
    if let Some(subcommand) = &scenario.requires_subcommand {
        let present = subcommand_present(binary, subcommand, configure)?;
        if !present {
            let reason = scenario
                .absent_note
                .clone()
                .unwrap_or_else(|| format!("the `{subcommand}` subcommand is not in this build"));
            return Ok(Outcome::Skipped {
                name: scenario.name.clone(),
                reason,
            });
        }
    }

    let mut samples = Vec::with_capacity(scenario.repetitions);
    for _ in 0..scenario.repetitions {
        samples.push(time_once(binary, scenario, configure)?);
    }
    samples.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));

    Ok(Outcome::Measured(Measurement {
        name: scenario.name.clone(),
        median_ms: median(&samples),
        p95_ms: percentile_nearest_rank(&samples, 0.95),
        repetitions: scenario.repetitions,
    }))
}

/// Whether the binary carries the subcommand.
///
/// Clap answers an unknown subcommand with status 2 and "unrecognized
/// subcommand"; that is the difference between "the command is not in this
/// build" and "the command is there and broken".
fn subcommand_present<F>(binary: &Path, subcommand: &str, configure: &F) -> Result<bool, String>
where
    F: Fn(&mut Command) -> Result<(), String>,
{
    let mut command = Command::new(binary);
    command.arg(subcommand).arg("--help");
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(Stdio::null());
    configure(&mut command)?;

    let output = command
        .output()
        .map_err(|error| format!("the agent binary could not be started: {error}"))?;
    if output.status.success() {
        return Ok(true);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("unrecognized subcommand") {
        return Ok(false);
    }
    Err(format!(
        "`{subcommand} --help` failed with {}: {}",
        output.status,
        tail(&stderr)
    ))
}

/// Run the scenario once and return how long it took, in milliseconds.
fn time_once<F>(binary: &Path, scenario: &Scenario, configure: &F) -> Result<f64, String>
where
    F: Fn(&mut Command) -> Result<(), String>,
{
    let mut command = Command::new(binary);
    command.args(&scenario.args);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(if scenario.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    configure(&mut command)?;

    let started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|error| format!("the agent binary could not be started: {error}"))?;
    if let Some(text) = &scenario.stdin {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "the child's stdin was not piped".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("the child's stdin could not be written: {error}"))?;
        // Dropping the handle closes the pipe, which is the end-of-file the
        // REPL scenario needs.
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("the agent binary could not be waited for: {error}"))?;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        if !scenario.allowed_exit_codes.contains(&code) {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "`{}` exited with {code}: {}",
                scenario.args.join(" "),
                tail(&stderr)
            ));
        }
    }
    Ok(elapsed_ms)
}

/// The last few lines of a captured stream, for a diagnostic.
fn tail(text: &str) -> String {
    const LINES: usize = 5;
    let collected: Vec<&str> = text.lines().rev().take(LINES).collect();
    let mut joined = collected
        .into_iter()
        .rev()
        .collect::<Vec<&str>>()
        .join(" | ");
    if joined.chars().count() > 400 {
        joined = joined.chars().take(400).collect();
    }
    joined
}

/// Compare the outcomes with the manifest's budgets for a profile.
pub fn evaluate(manifest: &Manifest, profile: &str, outcomes: &[Outcome]) -> Vec<Row> {
    manifest
        .scenarios
        .iter()
        .map(|scenario| {
            let budget = scenario.budgets.get(profile).copied();
            let outcome = outcomes.iter().find(|outcome| match outcome {
                Outcome::Measured(measurement) => measurement.name == scenario.name,
                Outcome::Skipped { name, .. } => name == &scenario.name,
            });

            match outcome {
                None => Row {
                    name: scenario.name.clone(),
                    median_ms: None,
                    p95_ms: None,
                    budget,
                    status: Status::Skipped,
                    note: "the scenario produced no outcome".to_string(),
                },
                Some(Outcome::Skipped { name, reason }) => Row {
                    name: name.clone(),
                    median_ms: None,
                    p95_ms: None,
                    budget,
                    status: Status::Skipped,
                    note: reason.clone(),
                },
                Some(Outcome::Measured(measurement)) => {
                    let mut note = String::new();
                    let status = match budget {
                        None => Status::Unbudgeted,
                        Some(budget) => {
                            let mut breaches = Vec::new();
                            if measurement.median_ms > budget.median_ms {
                                breaches.push(format!(
                                    "median {:.1} ms is over the {:.1} ms budget",
                                    measurement.median_ms, budget.median_ms
                                ));
                            }
                            if measurement.p95_ms > budget.p95_ms {
                                breaches.push(format!(
                                    "p95 {:.1} ms is over the {:.1} ms budget",
                                    measurement.p95_ms, budget.p95_ms
                                ));
                            }
                            if breaches.is_empty() {
                                Status::Pass
                            } else {
                                note = breaches.join("; ");
                                Status::Breach
                            }
                        }
                    };
                    Row {
                        name: measurement.name.clone(),
                        median_ms: Some(measurement.median_ms),
                        p95_ms: Some(measurement.p95_ms),
                        budget,
                        status,
                        note,
                    }
                }
            }
        })
        .collect()
}

/// The report the pull request carries: a Markdown table and a summary.
pub fn render_markdown(
    profile: &str,
    rows: &[Row],
    binary_bytes: u64,
    binary_size_budget: Option<u64>,
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "### Agent CLI performance gate ({profile} profile)\n\n"
    ));
    out.push_str("| Scenario | Median (ms) | p95 (ms) | Median budget (ms) | p95 budget (ms) | Verdict | Note |\n");
    out.push_str("|---|---|---|---|---|---|---|\n");

    for row in rows {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} |\n",
            row.name,
            format_ms(row.median_ms),
            format_ms(row.p95_ms),
            format_ms(row.budget.map(|budget| budget.median_ms)),
            format_ms(row.budget.map(|budget| budget.p95_ms)),
            row.status.label(),
            row.note
        ));
    }

    let measured = rows.iter().filter(|row| row.median_ms.is_some()).count();
    let skipped = rows
        .iter()
        .filter(|row| row.status == Status::Skipped)
        .count();
    let breached = rows
        .iter()
        .filter(|row| row.status == Status::Breach)
        .count();

    out.push('\n');
    match binary_size_budget {
        Some(budget) => out.push_str(&format!(
            "Binary size: {binary_bytes} bytes (budget {budget} bytes).\n"
        )),
        None => out.push_str(&format!("Binary size: {binary_bytes} bytes (no budget).\n")),
    }
    out.push_str(&format!(
        "Summary: {measured} measured, {skipped} skipped, {breached} breached.\n"
    ));
    out
}

/// One number as the report prints it.
fn format_ms(value: Option<f64>) -> String {
    match value {
        Some(value) => format!("{value:.1}"),
        None => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scenario(name: &str, args: &[&str]) -> Scenario {
        Scenario {
            name: name.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            stdin: None,
            repetitions: 3,
            allowed_exit_codes: default_allowed_exit_codes(),
            requires_subcommand: None,
            absent_note: None,
            budgets: BTreeMap::new(),
        }
    }

    #[test]
    fn the_nearest_rank_percentile_is_the_smallest_value_at_or_above_it() {
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(percentile_nearest_rank(&samples, 0.95), 5.0);
        assert_eq!(percentile_nearest_rank(&samples, 0.5), 3.0);
        assert_eq!(percentile_nearest_rank(&samples, 0.0), 1.0);
        assert_eq!(percentile_nearest_rank(&[], 0.95), 0.0);
    }

    #[test]
    fn the_median_of_an_even_number_of_samples_is_the_middle_pair() {
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(median(&[1.0, 2.0, 3.0]), 2.0);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn the_profile_is_read_from_the_binary_path() {
        assert_eq!(
            detect_profile(Path::new("/w/target/debug/terraphim-agent")),
            "dev"
        );
        assert_eq!(
            detect_profile(Path::new("/w/target/release/terraphim-agent")),
            "release"
        );
    }

    #[test]
    fn a_manifest_with_a_duplicate_scenario_name_is_refused() {
        let json = r#"{
            "version": 1,
            "scenarios": [
                {"name": "startup", "args": ["--version"]},
                {"name": "startup", "args": ["--help"]}
            ]
        }"#;
        let error = parse_manifest(json).expect_err("the duplicate name must be refused");
        assert!(error.contains("two scenarios are named"), "{error}");
    }

    #[test]
    fn a_manifest_with_a_p95_budget_tighter_than_its_median_budget_is_refused() {
        let json = r#"{
            "version": 1,
            "scenarios": [
                {
                    "name": "startup",
                    "args": ["--version"],
                    "budgets": {"dev": {"median_ms": 25.0, "p95_ms": 10.0}}
                }
            ]
        }"#;
        let error = parse_manifest(json).expect_err("the inverted budget must be refused");
        assert!(error.contains("tighter than its median budget"), "{error}");
    }

    #[test]
    fn a_manifest_with_an_unknown_version_is_refused() {
        let json = r#"{"version": 99, "scenarios": [{"name": "a", "args": ["--version"]}]}"#;
        let error = parse_manifest(json).expect_err("the unknown version must be refused");
        assert!(error.contains("version 99"), "{error}");
    }

    #[test]
    fn a_scenario_tolerates_only_the_exit_statuses_it_declares() {
        // `/bin/false` stands in for a command that reports a problem: it
        // ignores its arguments and exits with 1. The scenario declares the
        // status, so the measurement is kept.
        let mut tolerant = scenario("doctor", &["--ignored"]);
        tolerant.allowed_exit_codes = vec![0, 1];
        let outcome = run_scenario(Path::new("/bin/false"), &tolerant, &|_| Ok(()));
        assert!(matches!(outcome, Ok(Outcome::Measured(_))), "{outcome:?}");

        // The same command in a scenario that only tolerates success fails.
        let strict = scenario("doctor", &["--ignored"]);
        let error = run_scenario(Path::new("/bin/false"), &strict, &|_| Ok(()))
            .expect_err("the undeclared exit status must be refused");
        assert!(error.contains("exited with 1"), "{error}");
    }

    #[test]
    fn a_measurement_under_its_budget_passes_and_one_over_it_breaches() {
        let mut under = scenario("startup", &["--version"]);
        under.budgets.insert(
            "dev".to_string(),
            Budget {
                median_ms: 25.0,
                p95_ms: 50.0,
            },
        );
        let manifest = Manifest {
            version: 1,
            binary_size_budgets: BTreeMap::new(),
            scenarios: vec![under],
        };
        let outcomes = vec![Outcome::Measured(Measurement {
            name: "startup".to_string(),
            median_ms: 3.0,
            p95_ms: 4.0,
            repetitions: 3,
        })];
        let rows = evaluate(&manifest, "dev", &outcomes);
        assert_eq!(rows[0].status, Status::Pass);

        let outcomes = vec![Outcome::Measured(Measurement {
            name: "startup".to_string(),
            median_ms: 30.0,
            p95_ms: 60.0,
            repetitions: 3,
        })];
        let rows = evaluate(&manifest, "dev", &outcomes);
        assert_eq!(rows[0].status, Status::Breach);
        assert!(rows[0].note.contains("median"), "{}", rows[0].note);
        assert!(rows[0].note.contains("p95"), "{}", rows[0].note);
    }

    #[test]
    fn a_scenario_without_a_budget_for_the_profile_is_measured_but_not_gated() {
        let manifest = Manifest {
            version: 1,
            binary_size_budgets: BTreeMap::new(),
            scenarios: vec![scenario("startup", &["--version"])],
        };
        let outcomes = vec![Outcome::Measured(Measurement {
            name: "startup".to_string(),
            median_ms: 3.0,
            p95_ms: 4.0,
            repetitions: 3,
        })];
        let rows = evaluate(&manifest, "dev", &outcomes);
        assert_eq!(rows[0].status, Status::Unbudgeted);
    }

    #[test]
    fn a_skipped_scenario_keeps_its_reason_in_the_report() {
        let manifest = Manifest {
            version: 1,
            binary_size_budgets: BTreeMap::new(),
            scenarios: vec![scenario("code_doctor", &["code", "doctor"])],
        };
        let outcomes = vec![Outcome::Skipped {
            name: "code_doctor".to_string(),
            reason: "the coding agent is not merged".to_string(),
        }];
        let rows = evaluate(&manifest, "dev", &outcomes);
        assert_eq!(rows[0].status, Status::Skipped);
        let report = render_markdown("dev", &rows, 1234, Some(2000));
        assert!(report.contains("skipped"), "{report}");
        assert!(
            report.contains("the coding agent is not merged"),
            "{report}"
        );
        assert!(report.contains("1 skipped"), "{report}");
    }
}
