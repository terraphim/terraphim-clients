//! Repository guards that must run in CI.
//!
//! These are Rust tests rather than shell steps because the Gitea runner
//! enforces a program allowlist on workflow steps:
//!
//! ```text
//! runner error: policy rejected command:
//!   program `scripts/tests/publish-gate-test.sh` is not on the allowlist
//! ```
//!
//! Every other terraphim repo's `native-ci` runs `cargo` and nothing else. A
//! test binary invoked by `cargo test` is allowlisted, and spawning tools from
//! inside it is fine -- `packaged_install_graph_regression` already runs
//! `cargo package` this way. Refs #118.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/terraphim_agent
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

/// No `terraphim_*` crate may appear at more than one version or source.
///
/// Two copies of a crate mean two copies of its types, which the compiler
/// reports as `expected terraphim_config::ConfigState, found ConfigState` --
/// indistinguishable from a bug in the calling code, and the reason #112 and
/// #118 each cost hours. Fail here, with the crate named.
///
/// Third-party duplicates are ignored: they are normal in a graph this size and
/// nothing in this repo can resolve them.
#[test]
fn no_duplicate_terraphim_crates() {
    let root = workspace_root();
    let out = Command::new(env!("CARGO"))
        .args(["tree", "--workspace", "--all-features", "--duplicates"])
        .current_dir(&root)
        .output()
        .expect("run cargo tree");

    assert!(
        out.status.success(),
        "`cargo tree --duplicates` failed ({}); this is an environment problem, \
         not a duplicate, and is not being treated as a pass:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut dupes: Vec<&str> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|l| l.starts_with("terraphim") && l.contains(" v"))
        .collect();
    dupes.sort_unstable();
    dupes.dedup();

    assert!(
        dupes.is_empty(),
        "terraphim crates resolved at more than one version:\n  {}\n\n\
         Every terraphim_* dependency must resolve to a single version from the \
         Gitea registry. A crates.io copy creeps in when a dependency names a \
         version the [patch.crates-io] entry does not satisfy (an exact `=x.y.z` \
         pin does not satisfy a `^x.y.w` requirement, and cargo falls back to \
         crates.io silently), or when a manifest omits `registry = \"terraphim\"`. \
         Run `cargo tree -i <crate>@<version>` to find the offender.",
        dupes.join("\n  "),
    );
}

/// The GitHub Actions `taiki-e/install-action` tool versions must match the
/// locally-installed `cargo-llvm-cov` and `cargo-nextest` so a `cargo install`
/// on the native lane (which is `--locked` to the workspace) and the GH lane
/// (which is hand-pinned in `.github/workflows/ci.yml:30`) stay in lockstep.
///
/// If you upgrade the local toolchain and forget to bump the GH `with: tool:`
/// block, the two runners will produce coverage reports from different
/// rustc-instrumentation ABIs. The drift shows up as identical test sets but
/// divergent SF: counts in the lcov artefacts. Refs #313.
#[test]
fn coverage_tool_pinning_matches_local_toolchain() {
    let root = workspace_root();

    // The GH ci.yml `with: tool:` line we want to keep in sync with.
    let ci_yml = root.join(".github/workflows/ci.yml");
    assert!(ci_yml.is_file(), "missing {}", ci_yml.display());
    let ci_text = std::fs::read_to_string(&ci_yml).expect("read ci.yml");

    // Extract the `tool: cargo-llvm-cov@vX.Y.Z,nextest@vX.Y.Z` value.
    let pinned_block = ci_text
        .lines()
        .find(|l| l.trim_start().starts_with("tool:"))
        .expect("ci.yml has no `tool:` line; the GH coverage toolchain is unpinned");
    let pinned_block = pinned_block.trim_start();
    let pinned_block = pinned_block
        .strip_prefix("tool:")
        .expect("expected `tool:` prefix")
        .trim();

    let mut pinned = std::collections::HashMap::<&str, &str>::new();
    for entry in pinned_block.split(',') {
        let entry = entry.trim();
        let (name, version) = entry
            .split_once('@')
            .unwrap_or_else(|| panic!("expected `name@version` in `tool:` block, got `{}`", entry));
        // Strip the leading `v` so `cargo-llvm-cov@v0.8.5` matches the
        // local `cargo llvm-cov --version` output of `cargo-llvm-cov 0.8.5`.
        let version = version.strip_prefix('v').unwrap_or(version);
        pinned.insert(name, version);
    }
    let gh_cov = pinned.get("cargo-llvm-cov").copied().unwrap_or_else(|| {
        panic!(
            "ci.yml `tool:` block does not pin cargo-llvm-cov; got `{}`",
            pinned_block
        )
    });
    let gh_nextest = pinned.get("nextest").copied().unwrap_or_else(|| {
        panic!(
            "ci.yml `tool:` block does not pin nextest; got `{}`",
            pinned_block
        )
    });

    // Resolve the locally-installed versions.
    let cov_out = Command::new(env!("CARGO"))
        .args(["llvm-cov", "--version"])
        .output()
        .expect("run cargo llvm-cov --version");
    assert!(
        cov_out.status.success(),
        "cargo llvm-cov --version failed ({}):\n{}",
        cov_out.status,
        String::from_utf8_lossy(&cov_out.stderr),
    );
    let local_cov = String::from_utf8_lossy(&cov_out.stdout)
        .trim()
        .trim_start_matches("cargo-llvm-cov ")
        .trim()
        .to_string();

    let nextest_out = Command::new("cargo-nextest")
        .args(["--version"])
        .output()
        .expect("run cargo-nextest --version");
    let local_nextest = if nextest_out.status.success() {
        String::from_utf8_lossy(&nextest_out.stdout)
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string()
    } else {
        // cargo nextest --version is also valid.
        let nextest_alt = Command::new(env!("CARGO"))
            .args(["nextest", "--version"])
            .output()
            .expect("run cargo nextest --version");
        assert!(
            nextest_alt.status.success(),
            "cargo nextest --version failed ({}):\n{}",
            nextest_alt.status,
            String::from_utf8_lossy(&nextest_alt.stderr),
        );
        String::from_utf8_lossy(&nextest_alt.stdout)
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or("")
            .trim_start_matches('v')
            .to_string()
    };

    assert_eq!(
        local_cov, gh_cov,
        "cargo-llvm-cov version drift: local toolchain has {local_cov}, but \
         .github/workflows/ci.yml pins {gh_cov}. Bump the GH `with: tool:` block \
         (or downgrade the local toolchain) so the two runners use the same \
         rustc-instrumentation ABI. Refs #313."
    );
    assert_eq!(
        local_nextest, gh_nextest,
        "cargo-nextest version drift: local toolchain has {local_nextest}, but \
         .github/workflows/ci.yml pins {gh_nextest}. Bump the GH `with: tool:` \
         block (or downgrade the local toolchain) so the two runners agree. \
         Refs #313."
    );
}

/// The native-ci lanes whose cargo (transitively, via the nested
/// `cargo package` inside `packaged_install_graph_regression`) resolves the
/// private `terraphim` registry must alias
/// `CARGO_REGISTRIES_TERRAPHIM_TOKEN="$GITEA_TOKEN"` inline.
///
/// The terraphim-gitea-runner inherits `GITEA_TOKEN` but applies neither
/// workflow job `env:` nor step `env:` (documented in native-ci.yml), so the
/// only wiring that reaches the command is a leading `VAR=value` assignment,
/// which the runner policy strips token-wise before the allowlist check.
/// Without the alias the nested `cargo package` hits
/// `failed to get successful HTTP response ... got 401` resolving
/// `terraphim_command_runtime` (PR #332 CI run 33975 / web run 537,
/// job 68638).
///
/// The alias must reference the runner-inherited `$GITEA_TOKEN` shell
/// variable: no literal token and no `${{ secrets.* }}` expression may appear
/// in the workflow shell text, and the alias must stay scoped to exactly the
/// lanes that need it (the broad `--workspace --all-targets` lane and the
/// focused packaged-graph lane), not exposed to unrelated commands.
/// The alias value must carry the HTTP authentication scheme: the Gitea
/// sparse registry rejects a raw token with
/// `note: the token does not include an authentication scheme` followed by
/// HTTP 401 (PR #332 CI run 33980 / web run 538, job 68643). Double quotes
/// wrap the complete `Bearer $GITEA_TOKEN` value so the shell expands the
/// runner-inherited variable; the raw-token form
/// `CARGO_REGISTRIES_TERRAPHIM_TOKEN="$GITEA_TOKEN"` is a regression.
const NATIVE_CI_TOKEN_ALIAS: &str = "CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Bearer $GITEA_TOKEN\"";
const NATIVE_CI_TOKEN_RAW_ALIAS: &str = "CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"$GITEA_TOKEN\"";
const NATIVE_CI_TOKEN_GUARDED_COMMANDS: [&str; 3] = [
    "cargo test --workspace --all-targets",
    "cargo test -p terraphim_agent --test packaged_install_graph_regression",
    "cargo llvm-cov nextest --workspace --all-targets",
];

/// Validate the CARGO_REGISTRIES_TERRAPHIM_TOKEN wiring of native-ci.yml
/// text. Returns Err with a diagnostic on the first violation.
fn validate_native_ci_token_aliases(text: &str) -> Result<(), String> {
    for command in NATIVE_CI_TOKEN_GUARDED_COMMANDS {
        let mut matched = 0usize;
        for line in text.lines() {
            if !line.contains(command) {
                continue;
            }
            matched += 1;
            if line.contains(NATIVE_CI_TOKEN_RAW_ALIAS) {
                return Err(format!(
                    "native-ci.yml lane `{command}` uses the raw-token alias \
                     {NATIVE_CI_TOKEN_RAW_ALIAS}: the Gitea sparse registry \
                     rejects a token without an authentication scheme (HTTP \
                     401, PR #332 run 33980 / job 68643). The value must be \
                     exactly \"Bearer $GITEA_TOKEN\". Line: {line}"
                ));
            }
            let cargo_at = line.find("cargo").expect("command line contains cargo");
            let alias_at = line.find(NATIVE_CI_TOKEN_ALIAS).ok_or_else(|| {
                format!(
                    "native-ci.yml lane `{command}` lacks the inline alias \
                     {NATIVE_CI_TOKEN_ALIAS}; the runner applies no job/step env, \
                     so the nested `cargo package` in \
                     packaged_install_graph_regression resolves the private \
                     terraphim registry without a token and fails with HTTP 401 \
                     (PR #332 job 68638). Line: {line}"
                )
            })?;
            if alias_at >= cargo_at {
                return Err(format!(
                    "native-ci.yml lane `{command}` must place \
                     {NATIVE_CI_TOKEN_ALIAS} as a leading VAR=value assignment \
                     before `cargo` so the runner policy strips it token-wise \
                     and the shell applies it. Line: {line}"
                ));
            }
            if line.contains("${{") {
                return Err(format!(
                    "native-ci.yml lane `{command}` must not embed a ${{ ... }} \
                     expression (no secrets interpolation in runner shell text). \
                     Line: {line}"
                ));
            }
            if line.contains("credentials.toml") {
                return Err(format!(
                    "native-ci.yml lane `{command}` must not write or reference \
                     credentials.toml. Line: {line}"
                ));
            }
        }
        if matched == 0 {
            return Err(format!(
                "native-ci.yml no longer runs `{command}`; if the lane was \
                 removed on purpose, update this guard's command list"
            ));
        }
    }

    // The token alias must stay narrowly scoped: EVERY non-comment
    // occurrence of the variable must be a guarded single-line `- run: `
    // command. Gating on `run:` alone would skip an alias smuggled onto a
    // continuation or otherwise non-run line (the current runner rejects
    // multiline commands, but the guard must not rely on that). Indented
    // `#` documentation comments stay legitimate.
    for (index, line) in text.lines().enumerate() {
        if !line.contains("CARGO_REGISTRIES_TERRAPHIM_TOKEN") {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if !trimmed.starts_with("- run: ") && !trimmed.starts_with("run: ") {
            return Err(format!(
                "CARGO_REGISTRIES_TERRAPHIM_TOKEN appears on a non-comment \
                 native-ci.yml line that is not a single-line `run:` command \
                 (either `- run: ...` or a `run:` mapping value under an \
                 earlier `- name:` step, line {}): {line}",
                index + 1
            ));
        }
        if !NATIVE_CI_TOKEN_GUARDED_COMMANDS
            .iter()
            .any(|c| line.contains(c))
        {
            return Err(format!(
                "CARGO_REGISTRIES_TERRAPHIM_TOKEN appears on an unguarded \
                 native-ci.yml line {}: {line}",
                index + 1
            ));
        }
        // An alias-carrying run line must use the exact Bearer-schemed
        // value; a raw token, a wrong scheme, or a typo is a regression.
        // (A run line that merely mentions the variable without assigning
        // it is caught by the per-lane checks above.)
        if line.contains("CARGO_REGISTRIES_TERRAPHIM_TOKEN=")
            && !line.contains(NATIVE_CI_TOKEN_ALIAS)
        {
            return Err(format!(
                "native-ci.yml line {} assigns CARGO_REGISTRIES_TERRAPHIM_TOKEN \
                 but not exactly {NATIVE_CI_TOKEN_ALIAS}: the registry requires \
                 the Bearer authentication scheme (HTTP 401 otherwise, PR #332 \
                 run 33980 / job 68643). Line: {line}",
                index + 1
            ));
        }
    }
    Ok(())
}

#[test]
fn native_ci_aliases_gitea_token_for_packaged_graph_lanes() {
    let root = workspace_root();
    let workflow = root.join(".gitea/workflows/native-ci.yml");
    assert!(workflow.is_file(), "missing {}", workflow.display());
    let text = std::fs::read_to_string(&workflow).expect("read native-ci.yml");
    if let Err(diagnostic) = validate_native_ci_token_aliases(&text) {
        panic!("{diagnostic}");
    }
}

/// Mutation coverage for the scope sweep: an alias anywhere except a guarded
/// single-line run command must be rejected, while documentation comments
/// mentioning the variable stay legitimate.
#[test]
fn native_ci_token_alias_rejected_off_guarded_run_lanes() {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join(".gitea/workflows/native-ci.yml"))
        .expect("read native-ci.yml");

    // Mutate from a Bearer-schemed text in which every guarded lane already
    // carries the alias, so substitutions below are meaningful regardless
    // of the workflow's current state (the main guard above validates the
    // as-shipped workflow).
    let bearer_text = {
        let mut t = text.replace(NATIVE_CI_TOKEN_RAW_ALIAS, NATIVE_CI_TOKEN_ALIAS);
        for command in NATIVE_CI_TOKEN_GUARDED_COMMANDS {
            if t.lines()
                .any(|l| l.contains(command) && l.contains(NATIVE_CI_TOKEN_ALIAS))
            {
                continue;
            }
            // Lane currently carries no alias: inject one as a leading
            // assignment so this mutation harness always has something to
            // replace/remove. The main guard rejects the unshipped form.
            if let Some(at) = t.find(command) {
                t.insert_str(at, &format!("{alias} ", alias = NATIVE_CI_TOKEN_ALIAS));
            }
        }
        t
    };
    let anchored = NATIVE_CI_TOKEN_ALIAS;

    // Unrelated run lane must not carry the token.
    let unrelated = format!(
        "{bearer_text}\n      - run: {alias} cargo test -p terraphim_sessions --all-features\n",
        alias = NATIVE_CI_TOKEN_ALIAS
    );
    assert!(
        validate_native_ci_token_aliases(&unrelated).is_err(),
        "alias on an unrelated run lane must be rejected"
    );

    // Alias on a continuation/non-run line (indented like a folded run
    // block's second line) must be rejected even though it names a guarded
    // command: the runner would never classify it as the guarded lane.
    let continuation = format!(
        "{bearer_text}\n        {alias} cargo test -p terraphim_agent --test packaged_install_graph_regression -- --nocapture\n",
        alias = NATIVE_CI_TOKEN_ALIAS
    );
    assert!(
        validate_native_ci_token_aliases(&continuation).is_err(),
        "alias on a continuation/non-run line must be rejected"
    );

    // Plain non-run shell text carrying the alias must be rejected.
    let bare = format!(
        "{bearer_text}\n        echo wiring {alias} >/dev/null\n",
        alias = NATIVE_CI_TOKEN_ALIAS
    );
    assert!(
        validate_native_ci_token_aliases(&bare).is_err(),
        "alias on a non-run line must be rejected"
    );

    // Raw token without the Bearer scheme must be rejected: the registry
    // answers `note: the token does not include an authentication scheme`
    // and HTTP 401 (PR #332 run 33980 / job 68643).
    let raw = bearer_text.replace(anchored, NATIVE_CI_TOKEN_RAW_ALIAS);
    assert_ne!(raw, bearer_text, "mutation must change the workflow text");
    assert!(
        validate_native_ci_token_aliases(&raw).is_err(),
        "raw-token alias without the Bearer scheme must be rejected"
    );

    // A wrong scheme must be rejected too.
    let wrong_scheme = bearer_text.replace(
        anchored,
        "CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Token $GITEA_TOKEN\"",
    );
    assert_ne!(
        wrong_scheme, bearer_text,
        "mutation must change the workflow text"
    );
    assert!(
        validate_native_ci_token_aliases(&wrong_scheme).is_err(),
        "alias with a non-Bearer scheme must be rejected"
    );

    // Dropping the alias from any guarded lane that currently carries it
    // must be rejected. Lanes that lack the alias are caught separately by
    // the per-lane "lacks the inline alias" check in the main guard.
    for command in NATIVE_CI_TOKEN_GUARDED_COMMANDS {
        let lane_line = match bearer_text
            .lines()
            .find(|l| l.contains(command) && l.contains(anchored))
        {
            Some(line) => line.to_string(),
            None => continue,
        };
        let stripped_lane = lane_line.replace(anchored, "");
        let dropped = bearer_text.replacen(&lane_line, &stripped_lane, 1);
        assert_ne!(
            dropped, bearer_text,
            "mutation must change the workflow text"
        );
        assert!(
            validate_native_ci_token_aliases(&dropped).is_err(),
            "guarded lane `{command}` without the alias must be rejected"
        );
    }

    // Documentation comments mentioning the variable remain legitimate.
    let commented = format!(
        "{bearer_text}\n      # CARGO_REGISTRIES_TERRAPHIM_TOKEN aliases the runner-inherited GITEA_TOKEN.\n"
    );
    assert!(
        validate_native_ci_token_aliases(&commented).is_ok(),
        "documentation comments mentioning the variable must stay legitimate"
    );
}

/// The publish provenance gate must keep working.
///
/// It is what stops another unreproducible release: four of the last four
/// artefacts before #112 were published from dirty trees or commits unreachable
/// from `main`. Its own tests build throwaway repos per failure mode.
#[test]
fn publish_gate_tests_pass() {
    let root = workspace_root();
    let script = root.join("scripts/tests/publish-gate-test.sh");
    assert!(script.is_file(), "missing {}", script.display());

    let out = Command::new("bash")
        .arg(&script)
        .current_dir(&root)
        .output()
        .expect("run publish-gate tests");

    assert!(
        out.status.success(),
        "publish-gate tests failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}
