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
/// private `terraphim` registry must conditionally alias
/// `CARGO_REGISTRIES_TERRAPHIM_TOKEN="Bearer $GITEA_TOKEN"`.
///
/// Whether `$GITEA_TOKEN` reaches a step shell is runner-dependent (#335):
/// host-mode runners inherit it, but the Firecracker-VM runners POST each
/// step as `{code, working_dir}` with no env payload (vm_executor), so the
/// alias expands to `Bearer ` (empty) and the Gitea sparse registry answers
/// `401 Failed to authenticate user` (runs #541/#542; main red 09-19..09-24).
/// The canonical wiring is therefore the first line of the lane's run block:
///
/// ```text
/// test -z "$GITEA_TOKEN" || export CARGO_REGISTRIES_TERRAPHIM_TOKEN="Bearer $GITEA_TOKEN"
/// <guarded cargo command>
/// ```
///
/// Host runners export the alias; VM runners skip it and cargo falls back to
/// the baked CARGO_HOME credentials.toml, which carries a scheme-qualified
/// token (this is what made runs #527/#529 green and keeps build lanes
/// fetching registry crates). `test` and `export` are both on the runner
/// command-policy allowlist, and the export persists to the cargo line in
/// the same step shell.
///
/// The alias must reference the runner-inherited `$GITEA_TOKEN` shell
/// variable: no literal token, no `${{ secrets.* }}` expression, and no
/// credentials.toml writing may appear in the workflow shell text. The alias
/// must stay scoped to exactly the lanes that need it (the broad
/// `--workspace --all-targets` lane, the focused packaged-graph lane, and
/// the coverage lane). The alias value must carry the HTTP authentication
/// scheme: the Gitea sparse registry rejects a raw token with HTTP 401
/// (PR #332 CI run 33980 / web run 538, job 68643); the raw-token form
/// `CARGO_REGISTRIES_TERRAPHIM_TOKEN="$GITEA_TOKEN"` is a regression, and
/// so is the unconditional leading-assignment form
/// `CARGO_REGISTRIES_TERRAPHIM_TOKEN="Bearer $GITEA_TOKEN"` (empty
/// expansion on VM runners, #335).
const NATIVE_CI_TOKEN_ALIAS: &str = "CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Bearer $GITEA_TOKEN\"";
const NATIVE_CI_TOKEN_RAW_ALIAS: &str = "CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"$GITEA_TOKEN\"";
const NATIVE_CI_TOKEN_CONDITIONAL: &str =
    "test -z \"$GITEA_TOKEN\" || export CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Bearer $GITEA_TOKEN\"";
const NATIVE_CI_TOKEN_GUARDED_COMMANDS: [&str; 3] = [
    "cargo test --workspace --all-targets",
    "cargo test -p terraphim_agent --test packaged_install_graph_regression",
    "cargo llvm-cov nextest --workspace --all-targets",
];

/// The nearest preceding non-comment, non-empty line of `lines[idx]`.
fn previous_shell_line<'a>(lines: &[&'a str], idx: usize) -> Option<&'a str> {
    lines[..idx]
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#'))
}

/// Validate the CARGO_REGISTRIES_TERRAPHIM_TOKEN wiring of native-ci.yml
/// text. Returns Err with a diagnostic on the first violation.
fn validate_native_ci_token_aliases(text: &str) -> Result<(), String> {
    let lines: Vec<&str> = text.lines().collect();
    for command in NATIVE_CI_TOKEN_GUARDED_COMMANDS {
        let mut matched = 0usize;
        for (idx, line) in lines.iter().enumerate() {
            if !line.contains(command) || line.trim_start().starts_with('#') {
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
            let prev = previous_shell_line(&lines, idx).ok_or_else(|| {
                format!(
                    "native-ci.yml lane `{command}` has no preceding shell line \
                     to carry the conditional registry alias"
                )
            })?;
            if prev != NATIVE_CI_TOKEN_CONDITIONAL {
                return Err(format!(
                    "native-ci.yml lane `{command}` must be preceded immediately \
                     by the conditional alias line `{NATIVE_CI_TOKEN_CONDITIONAL}` \
                     (#335: an unconditional leading alias expands empty on the \
                     Firecracker-VM runners -- no step env -- and the registry \
                     answers 401; the conditional lets VM runners fall back to \
                     the baked CARGO_HOME credential). Line: {line}"
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
    // occurrence of the variable must be the exact conditional alias line,
    // and every conditional alias line must be immediately followed by a
    // guarded command (an alias above an unrelated lane is a regression).
    for (idx, line) in lines.iter().enumerate() {
        if !line.contains("CARGO_REGISTRIES_TERRAPHIM_TOKEN") {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.trim() != NATIVE_CI_TOKEN_CONDITIONAL {
            return Err(format!(
                "CARGO_REGISTRIES_TERRAPHIM_TOKEN appears on a non-comment \
                 native-ci.yml line that is not exactly the conditional alias \
                 `{NATIVE_CI_TOKEN_CONDITIONAL}` (line {}): the old \
                 single-line leading-assignment forms are regressions -- the \
                 unconditional Bearer form expands empty on VM runners and the \
                 raw form lacks the authentication scheme (#335, PR #332): {line}",
                idx + 1
            ));
        }
        let next = lines[idx + 1..]
            .iter()
            .map(|l| l.trim())
            .find(|l| !l.is_empty() && !l.starts_with('#'));
        match next {
            Some(n)
                if NATIVE_CI_TOKEN_GUARDED_COMMANDS
                    .iter()
                    .any(|c| n.contains(c)) => {}
            _ => {
                return Err(format!(
                    "native-ci.yml line {} carries the conditional registry \
                     alias but is not immediately followed by a guarded lane \
                     command; the alias must stay scoped to the lanes that \
                     resolve the private registry: {line}",
                    idx + 1
                ));
            }
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

/// Mutation coverage for the scope sweep: an alias anywhere except the
/// conditional line directly above a guarded lane must be rejected, while
/// documentation comments mentioning the variable stay legitimate.
#[test]
fn native_ci_token_alias_rejected_off_guarded_run_lanes() {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join(".gitea/workflows/native-ci.yml"))
        .expect("read native-ci.yml");

    // Mutate from a text in which every guarded lane already carries the
    // conditional alias, so substitutions below are meaningful regardless of
    // the workflow's current state (the main guard above validates the
    // as-shipped workflow).
    let conditional_text = {
        let mut t = text.to_string();
        for command in NATIVE_CI_TOKEN_GUARDED_COMMANDS {
            if t.lines().any(|l| {
                l.trim() == NATIVE_CI_TOKEN_CONDITIONAL
                    || (l.contains(command) && l.contains(NATIVE_CI_TOKEN_ALIAS))
            }) {
                continue;
            }
            // Lane currently lacks the alias: inject the conditional line
            // directly above the lane so this mutation harness always has
            // something to replace/remove. The main guard rejects the
            // unshipped form.
            if let Some(at) = t.find(command) {
                let line_start = t[..at].rfind('\n').map(|p| p + 1).unwrap_or(0);
                t.insert_str(
                    line_start,
                    &format!("          {NATIVE_CI_TOKEN_CONDITIONAL}\n"),
                );
            }
        }
        t
    };

    // Unrelated run lane carrying the conditional alias must be rejected.
    let unrelated = format!(
        "{conditional_text}\n          {NATIVE_CI_TOKEN_CONDITIONAL}\n      - run: cargo test -p terraphim_sessions --all-features\n"
    );
    assert!(
        validate_native_ci_token_aliases(&unrelated).is_err(),
        "conditional alias above an unrelated run lane must be rejected"
    );

    // The old unconditional leading-assignment form must be rejected: it
    // expands empty on the VM runners (#335).
    let unconditional = conditional_text.replace(
        &format!("          {NATIVE_CI_TOKEN_CONDITIONAL}\n"),
        &format!("      - run: {NATIVE_CI_TOKEN_ALIAS} cargo test -p terraphim_agent --test packaged_install_graph_regression -- --nocapture\n"),
    );
    assert_ne!(
        unconditional, conditional_text,
        "mutation must change the workflow text"
    );
    assert!(
        validate_native_ci_token_aliases(&unconditional).is_err(),
        "unconditional single-line alias must be rejected (empty expansion on VM runners)"
    );

    // Raw token without the Bearer scheme must be rejected: the registry
    // answers `note: the token does not include an authentication scheme`
    // and HTTP 401 (PR #332 run 33980 / job 68643).
    let raw = conditional_text.replace(
        "export CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Bearer $GITEA_TOKEN\"",
        "export CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"$GITEA_TOKEN\"",
    );
    assert_ne!(
        raw, conditional_text,
        "mutation must change the workflow text"
    );
    assert!(
        validate_native_ci_token_aliases(&raw).is_err(),
        "raw-token alias without the Bearer scheme must be rejected"
    );

    // A wrong scheme must be rejected too.
    let wrong_scheme = conditional_text.replace(
        "export CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Bearer $GITEA_TOKEN\"",
        "export CARGO_REGISTRIES_TERRAPHIM_TOKEN=\"Token $GITEA_TOKEN\"",
    );
    assert_ne!(
        wrong_scheme, conditional_text,
        "mutation must change the workflow text"
    );
    assert!(
        validate_native_ci_token_aliases(&wrong_scheme).is_err(),
        "alias with a non-Bearer scheme must be rejected"
    );

    // Dropping the conditional line from any guarded lane must be rejected.
    for command in NATIVE_CI_TOKEN_GUARDED_COMMANDS {
        let cl: Vec<&str> = conditional_text.lines().collect();
        let mut out: Vec<&str> = Vec::new();
        for (i, line) in cl.iter().enumerate() {
            if line.contains(command) && !line.trim_start().starts_with('#') {
                if i > 0 && cl[i - 1].trim() == NATIVE_CI_TOKEN_CONDITIONAL {
                    out.pop();
                }
                out.push(line);
            } else {
                out.push(line);
            }
        }
        let dropped = out.join("\n");
        assert_ne!(
            dropped, conditional_text,
            "mutation must change the workflow text"
        );
        assert!(
            validate_native_ci_token_aliases(&dropped).is_err(),
            "guarded lane `{command}` without the conditional alias must be rejected"
        );
    }

    // Documentation comments mentioning the variable remain legitimate.
    let commented = format!(
        "{conditional_text}\n      # CARGO_REGISTRIES_TERRAPHIM_TOKEN aliases the runner-inherited GITEA_TOKEN.\n"
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
