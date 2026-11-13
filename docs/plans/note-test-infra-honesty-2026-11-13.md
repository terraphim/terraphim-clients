# Research, Design, Verification and Validation: the test infrastructure stops hiding failures (#513)

**Issue**: terraphim-clients#513
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13
**Mode**: `remediation` (explicit defects from the post-merge review, local change)

## Research

The issue's theme is precise: "Test-infrastructure changes that make failures
disappear instead of reporting them. Common shape: an absent prerequisite is
treated as a pass, or a guard cannot fail because it inspects the wrong
thing."

Five findings were addressed here; three of them are in this session's own
work (#496, #499, #500).

### 1. An empty `TERRAPHIM_SERVER_BIN` skipped the suite

`resolve_server_binary` matched on `Option<&str>` and, for `Some(value)`,
returned `path.is_file().then_some(path)`. `std::env::var` returns `Ok("")` for
an empty-but-set variable, so `Some("")` -> `PathBuf::from("").is_file()` is
false -> `None` -> the suite skipped. The pre-PR resolvers did the opposite:
they fell through to `target/debug/terraphim_server`. Setting a variable to
the empty string is how a CI matrix spells "not configured", and it made five
suites skip silently.

### 2. A file that cannot be run counted as a server

`resolve_server_binary` accepted a path on the sole criterion `path.is_file()`.
A stale or truncated artifact left by an interrupted build is a regular file,
so the suite proceeded, the spawn failed, and the failure read like a server
regression rather than a broken local artifact.

### 3. "No server" and "the server failed to start" were conflated

`integration_tests.rs`:

```rust
let server_info = if let Ok(Some((server, url))) = start_test_server().await {
    Some((server, url))
} else {
    println!("⚠ Skipping server mode tests - could not start server");
    None
};
```

The `else` arm is reached for `Ok(None)` (no binary - a legitimate skip) **and**
for `Err(_)` (a binary was present and the server exited early or never became
ready). The second case is exactly the invariant the design doc claims to
preserve: "server present but misbehaves must still fail the test".

### 4. The guard excluded every file named `ci_guards.rs`

`workspace_rust_sources()` excluded the guard by **basename**:

```rust
let own_name = Path::new(file!()).file_name().expect("guard file name").to_owned();
out.retain(|p| p.file_name() != Some(own_name.as_os_str()));
```

Any other file in the tree called `ci_guards.rs` was dropped from the scan, so
a new environment-mutation site could hide behind that name - a silent hole
in the very guard that exists to prevent silent failures.

### 5. The manifest rule could not see a renamed dependency

`member_manifests_depending_on` only inspected the three literal top-level
dependency tables of `crates/*/Cargo.toml` and only matched the **key**. It
could not see:

- a renamed dependency (`tutils = { package = "terraphim_test_utils", ... }`),
- a target-specific table (`[target.'cfg(unix)'.dev-dependencies]`),
- the root `[workspace.dependencies]` table.

Each of those is a legal way to re-add the dependency the rule exists to stop.

### 6. The workspace CI lane ran the guard without the strict toolchain

`TERRAPHIM_STRICT_TOOLCHAIN=1` was set on exactly one lane (the focused
`cargo test -p terraphim_agent --test ci_guards`). But the workspace lane
(`cargo test --workspace --all-targets`) runs the same guard binary, and the
drift check there was report-only. The workflow's own comment justified this
by claiming that the workspace lane's PATH "does not carry the pinned
installs" - but the pinned installs run *before* every test lane, land in
`CARGO_HOME/bin` (no `--root`), and the lane puts exactly that directory first
on `PATH`.

## Design

**`crates/terraphim_agent/tests/support/binary.rs`**

- `is_usable_binary(path)`: a file **and** (on Unix) the executable bit.
- `resolve_server_binary`: an environment value that is empty or only
  whitespace counts as *unset*; a non-empty value that points at a missing or
  non-executable file stays *absent* (no silent fallback); the fallback is used
  when the value is unset.
- `skip_without_server`: when a configured file exists but cannot be run, the
  notice says so ("found but not executable") instead of reporting absence.

**`crates/terraphim_agent/tests/integration_tests.rs`**

- The three cases are separated: `Ok(Some(_))` runs the server mode, `Ok(None)`
  skips with the shared notice, and `Err(_)` **fails** the test with the
  server's own error.

**`crates/terraphim_agent/tests/ci_guards.rs`**

- `own_guard_path()` locates this guard file by absolute path (the file name
  combined with the package's `tests` directory, because `file!()` is relative
  to the package root in some cargo invocations and to the workspace root in
  others), and the exclusion compares **canonicalised paths**. The name-based
  exclusion remains only as the fallback for a checkout where the guard file
  cannot be located at all.
- `table_mentions_dependency` checks the dependency key **and** the `package`
  key of a renamed dependency; `dependency_tables` collects the top-level and
  the `[target.*]` tables; `manifest_mentions_dependency` adds the workspace
  manifest's `[workspace.dependencies]`.

**`.gitea/workflows/native-ci.yml`**

- The workspace lane exports `TERRAPHIM_STRICT_TOOLCHAIN=1` as well, because
  it runs the guard and carries the same pinned installs.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1268 passed, 0 failed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `cargo test -p terraphim_agent --test ci_guards` | 27 passed |
| `cargo test -p terraphim_agent --test server_binary_resolution` | 9 passed |

New tests:

- an empty and a whitespace-only environment value fall back to the documented
  path instead of skipping the suite;
- a file that is not executable is not a server;
- the guard excludes **its own file** and still scans a different file with the
  same name;
- the manifest rule detects a plain, a renamed, a target-specific and a
  workspace-level dependency, and ignores an unrelated manifest.

The existing tests that wrote a fake binary were updated to make it runnable -
a file without the executable bit is now (correctly) not a server.

## Validation

| Acceptance criterion (issue #513) | Result |
|---|---|
| "Treat an empty or whitespace-only env value as unset so the documented fallback still applies" | PASS |
| "Require the executable bit on Unix before treating a path as a usable server" | PASS |
| "the notice should say so explicitly ('found but not executable')" | PASS |
| "Distinguish the two cases explicitly" (no binary vs a broken server) | PASS |
| "Fix: exclude by full canonical path instead of by file name" | PASS |
| "Resolve renamed deps via the `package` key, scan `[target.*.dependencies]` tables, and scan the root `[workspace.dependencies]` table" | PASS |
| "Export TERRAPHIM_STRICT_TOOLCHAIN=1 on the workspace lane too" | PASS |

## Limits

- The manifest rule is still **deferred**: while the allow-list has wrapper
  sites, the arm only asserts that the dependency is still present. What this
  change fixes is that the arm will *see* every way to re-add the dependency
  once it activates.
- The `is_usable_binary` check is a permission-bit test, not a "does this
  binary run" test: a file that is executable but broken is still accepted,
  and the failure then appears as a spawn error.
- The executable-bit rule is Unix-only; on other platforms `is_file` remains
  the criterion.
