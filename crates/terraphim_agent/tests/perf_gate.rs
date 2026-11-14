//! The timed-commands performance gate for the agent CLI (#437).
//!
//! The gate measures the real binary - the one cargo built for this test -
//! by spawning it under the hermetic environment, so a measured command never
//! touches the shared `/tmp` persistence profiles that the CI lanes contend
//! over (#398). No mocks: the commands that run here are the commands a user
//! runs.
//!
//! Two things are asserted, and they are not the same thing:
//!
//! * **always**: every command that this build carries runs successfully, and
//!   the measurements are coherent. That is a real check of the agent's
//!   start-up path, and it holds in the ordinary test lane.
//! * **under the gate** (`TERRAPHIM_PERF_GATE=1`, which the CI lane sets):
//!   the measurements meet the budgets in
//!   `docs/benchmarks/agent-cli-budgets.json`. The ceilings are wall-clock
//!   numbers, so they are only meaningful on a machine that is not busy with
//!   something else; that is why the ordinary lane measures without
//!   asserting on them.
//!
//! The second test keeps the document honest: `docs/agent-perf-gate.md` has
//! to quote exactly the budgets the manifest enforces, in both directions, so
//! the document can never describe a gate that is not the one in the tree.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::cli_test_env::{create_hermetic_root, set_hermetic_env};
use terraphim_agent::perf_gate::{
    Manifest, Outcome, Status, binary_size, detect_profile, evaluate, parse_manifest,
    render_markdown, run_scenario,
};

/// The agent binary cargo built for this test.
const AGENT_BINARY: &str = env!("CARGO_BIN_EXE_terraphim-agent");

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .to_path_buf()
}

fn manifest_path() -> PathBuf {
    workspace_root()
        .join("docs")
        .join("benchmarks")
        .join("agent-cli-budgets.json")
}

fn document_path() -> PathBuf {
    workspace_root().join("docs").join("agent-perf-gate.md")
}

fn manifest() -> Manifest {
    let path = manifest_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    parse_manifest(&text).unwrap_or_else(|error| panic!("{error}"))
}

/// Whether the budgets are enforced. The CI lane sets the variable; the
/// ordinary test lane does not.
fn gate_enabled() -> bool {
    std::env::var_os("TERRAPHIM_PERF_GATE").is_some()
}

/// The binary the gate measures.
///
/// The default is the binary cargo built for this test. `TERRAPHIM_PERF_BINARY`
/// points the gate at another build - the release binary, for instance - so
/// that the profile a user feels can be measured with the same harness.
fn measured_binary() -> PathBuf {
    match std::env::var("TERRAPHIM_PERF_BINARY") {
        Ok(path) => PathBuf::from(path),
        Err(_) => PathBuf::from(AGENT_BINARY),
    }
}

/// The profile the measured binary was built with.
fn profile(binary: &Path) -> String {
    match std::env::var("TERRAPHIM_PERF_PROFILE") {
        Ok(profile) => profile,
        Err(_) => detect_profile(binary),
    }
}

#[test]
fn the_cli_scenarios_run_and_meet_their_budgets() {
    let manifest = manifest();
    let binary = measured_binary();
    let profile = profile(&binary);

    let mut outcomes: Vec<Outcome> = Vec::with_capacity(manifest.scenarios.len());
    for scenario in &manifest.scenarios {
        // One hermetic root per scenario: the repetitions of a scenario see
        // the same settings and the same store, so they measure the same
        // work rather than a cold start and four warm starts.
        let root = create_hermetic_root().unwrap_or_else(|error| {
            panic!(
                "the hermetic root for `{}` could not be created: {error}",
                scenario.name
            )
        });
        let outcome = run_scenario(binary.as_path(), scenario, &|command: &mut Command| {
            // The coverage lane gives every test process an
            // `LLVM_PROFILE_FILE`. Thirty child processes inheriting it would
            // write their profile data into the parent's file and truncate
            // it, so the children are measured without instrumentation. The
            // agent's own coverage is measured by the suites that spawn it
            // once.
            command.env_remove("LLVM_PROFILE_FILE");
            set_hermetic_env(command, &root).map_err(|error| error.to_string())
        })
        .unwrap_or_else(|error| panic!("the scenario `{}` failed: {error}", scenario.name));
        outcomes.push(outcome);
    }

    let rows = evaluate(&manifest, &profile, &outcomes);
    let bytes = binary_size(binary.as_path())
        .unwrap_or_else(|error| panic!("the binary size could not be read: {error}"));
    let size_budget = manifest.binary_size_budget(&profile);
    let report = render_markdown(&profile, &rows, bytes, size_budget);
    println!("{report}");

    // Every scenario this build carries has to run: a command that fails is a
    // failure of the agent, not a slow measurement, and `run_scenario` has
    // already refused it above. A scenario that names a subcommand may still
    // be skipped - that is how the code-agent scenarios wait for PR #432 -
    // but then the skip has to carry the manifest's reason.
    for scenario in &manifest.scenarios {
        let outcome = outcomes
            .iter()
            .find(|outcome| match outcome {
                Outcome::Measured(measurement) => measurement.name == scenario.name,
                Outcome::Skipped { name, .. } => name == &scenario.name,
            })
            .unwrap_or_else(|| panic!("the scenario `{}` produced no outcome", scenario.name));
        match outcome {
            Outcome::Measured(_) => {}
            Outcome::Skipped { reason, .. } => {
                assert!(
                    scenario.requires_subcommand.is_some(),
                    "the scenario `{}` was skipped but it names no subcommand to wait for: {reason}",
                    scenario.name
                );
                if let Some(note) = &scenario.absent_note {
                    assert_eq!(
                        reason, note,
                        "the scenario `{}` was skipped with a reason the manifest does not state",
                        scenario.name
                    );
                }
            }
        }
    }

    if !gate_enabled() {
        return;
    }

    let breaches: Vec<String> = rows
        .iter()
        .filter(|row| row.status == Status::Breach)
        .map(|row| {
            format!(
                "`{}`: {} (measured median {:?} ms, p95 {:?} ms)",
                row.name, row.note, row.median_ms, row.p95_ms
            )
        })
        .collect();
    assert!(
        breaches.is_empty(),
        "the agent CLI broke its performance budget in the {profile} profile:\n  {}\n\n\
         The budgets live in {}. A ceiling is there to stop a regression, so it is \
         raised only with the measurement that justifies it.",
        breaches.join("\n  "),
        manifest_path().display()
    );

    if let Some(budget) = size_budget {
        assert!(
            bytes <= budget,
            "the agent binary is {bytes} bytes, over the {budget} byte budget for the \
             {profile} profile"
        );
    }
}

/// The document and the manifest must agree, in both directions.
///
/// A budget that is only in the document is a promise the gate does not keep;
/// a budget that is only in the manifest is a ceiling nobody can read. Both
/// are refused here, so `docs/agent-perf-gate.md` describes the gate that is
/// actually in the tree.
#[test]
fn the_document_quotes_the_manifests_budgets() {
    let manifest = manifest();
    let path = document_path();
    let document = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));

    for scenario in &manifest.scenarios {
        for (profile, budget) in &scenario.budgets {
            let quoted = format!(
                "| `{}` | `{}` | {:.1} | {:.1} |",
                scenario.name, profile, budget.median_ms, budget.p95_ms
            );
            assert!(
                document.contains(&quoted),
                "the document does not quote the manifest's budget for the scenario \
                 `{}` in the {profile} profile; the row to add is:\n  {quoted}",
                scenario.name
            );
        }
    }

    // The other direction: every budget the document quotes has to be one the
    // manifest enforces, with the same numbers.
    for line in document.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("| `") {
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().trim_matches('`'))
            .collect();
        if cells.len() != 4 {
            continue;
        }
        let (name, profile) = (cells[0], cells[1]);
        let (Ok(median), Ok(p95)) = (cells[2].parse::<f64>(), cells[3].parse::<f64>()) else {
            continue;
        };
        let scenario = manifest.scenario(name).unwrap_or_else(|| {
            panic!("the document quotes the scenario `{name}`, which the manifest does not declare")
        });
        let budget = scenario.budgets.get(profile).unwrap_or_else(|| {
            panic!(
                "the document quotes a {profile} budget for the scenario `{name}`, which the \
                 manifest does not enforce"
            )
        });
        assert!(
            (budget.median_ms - median).abs() < f64::EPSILON
                && (budget.p95_ms - p95).abs() < f64::EPSILON,
            "the document quotes {median}/{p95} ms for the scenario `{name}` in the \
             {profile} profile, but the manifest enforces {:.1}/{:.1} ms",
            budget.median_ms,
            budget.p95_ms
        );
    }
}
