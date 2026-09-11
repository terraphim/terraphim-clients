# Design: Pacman-Managed Updates (Gitea #247)

- **Base commit**: `112018079dffd86fd99e434aedd6121476a66638`
- **Worktree**: `/home/alex/worktrees/clients-247`
- **Branch**: `task/247-pacman-managed-updates`
- **Status**: APPROVED — implementation contract for Gitea #247
- **Author**: Kairo (orchestrator), drafted by Claude Sonnet
- **Date**: 2026-09-11

## 1. Goal

Allow Terraphim binaries (`terraphim_agent`, `terraphim_grep`) installed via a
system package manager (pacman, for the Omarchy PKGBUILD / O4) to be
**detected at runtime**, deterministically, such that when running as a
package-managed install they:

- never perform a self-update network check at startup,
- never write to `/usr/bin`, `/usr/local/bin`, or `~/.local/bin`,
- never invoke the download/install self-update code path,
- still expose `check-update` / `update` subcommands, but these return
  deterministic, stable guidance telling the operator to run the
  package manager's own update command (e.g. `sudo pacman -Syu`) instead.

There is **no compile-time opt-in**. Every build of `terraphim_agent` and
`terraphim_grep` contains exactly the same code; the choice between
self-managed and package-managed behavior is made **at process startup**, by
inspecting the filesystem, per the deterministic contract in §3. Builds that
are not installed via a supported package manager (the default — `cargo
install`, direct binary downloads, CI-built release artifacts run outside a
package-manager install) must keep exactly the current self-update behavior
unchanged, because detection deterministically resolves to "self-managed" for
them.

## 2. Verified Current State

- `terraphim_update::platform::get_binary_path` — prefers a writable
  directory containing the current executable, then falls back to
  `/usr/local/bin`, then creates/falls back to `~/.local/bin`. This logic is
  unsafe to run against a pacman-owned executable living in `/usr/bin`: it
  may attempt to write there or silently redirect installs to
  `~/.local/bin`, producing a second, un-managed copy that shadows the
  pacman-owned one.
- `crates/terraphim_agent/src/main.rs:448-456` — an unconditional startup
  path performs a network update check every time the agent starts, with no
  opt-out today.
- `terraphim_agent` exposes `check-update` and `update` through both
  `main.rs` (CLI entry / arg parsing) and `server_command.rs` (presumably
  the long-running server subcommand path) — i.e. there are two call sites
  in the agent crate that need to route through the same gate.
- `terraphim_grep/src/main.rs` also exposes `check-update` and `update`
  (grep is a separate binary with its own copy of this surface).
- Both binaries construct `UpdaterConfig` using `CARGO_PKG_VERSION` — the
  version plumbing is shared/parallel between the two crates, not routed
  through one shared call.
- No marker-file detection code exists anywhere in the repository today —
  `crates/terraphim_update` has no notion of an install-time policy; §3
  introduces this fresh, threaded through `UpdaterConfig`/`TerraphimUpdater`
  rather than gated by a Cargo feature.

Exact line numbers, call-site count, and the deeper choke point this design
relies on are confirmed in §2.1 below.

### 2.1 Symbol map (confirmed against base commit)

| Symbol | File:lines | Role |
|---|---|---|
| `get_binary_path` | `crates/terraphim_update/src/platform.rs:39-89` | Resolution order: writable current-exe dir → `/usr/local/bin` → creates+falls back to `~/.local/bin`. Only reached from `TerraphimUpdater`'s GitHub-backend paths (`check_update_github`, `update_github`, `get_latest_release_info`, `check_for_updates_auto`) via `builder.bin_install_path(...)`; unsafe under a package-managed install rooted at `/usr/bin`. |
| `install_verified_archive` / `promote_staged_binaries` | `crates/terraphim_update/src/lib.rs:1014-1098` | R2-backend install path; writes to `current_exe().parent()` (not one of the three named paths, but still a write we must not perform when the running binary is package-managed). |
| startup update check | `crates/terraphim_agent/src/main.rs:448-456` | Unconditional `updater.check_update().await` on every agent startup, inside a freshly-constructed `Runtime`. |
| `check-update` (agent CLI/offline) | `crates/terraphim_agent/src/main.rs:759-773` (`handle_check_update_command`), dispatch at `:1364-1367,1579-1581` | Offline/TUI-mode arm. |
| `update` (agent CLI/offline) | `crates/terraphim_agent/src/main.rs:775-789` (`handle_update_command`), dispatch at `:1369-1372,1582-1584` | Calls `updater.check_and_update()`. |
| `check-update` / `update` (agent server mode) | `crates/terraphim_agent/src/server_command.rs:485-515` (`Command::CheckUpdate` / `Command::Update` arms of `run_server_command`) | Second, independent call site — same `UpdaterConfig`/`TerraphimUpdater` construction duplicated here, not shared with `main.rs`'s offline arm. |
| `check-update` / `update` (grep) | `crates/terraphim_grep/src/main.rs:156-175` (`grep_updater`, `handle_update_command`) | Third, independent call site; own `Cargo.toml`/feature set. |
| `UpdaterConfig::new(..).with_version(env!("CARGO_PKG_VERSION"))` | 4 call sites above | Not centralized — each site builds its own config; confirms no single choke point exists today at the *binary* layer for gating. |
| `TerraphimUpdater::check_update` / `check_and_update` / `update` | `crates/terraphim_update/src/lib.rs:242-251, 522-533, 1184-1189` | **The one choke point that *does* exist**, at the *library* layer: every one of the 4 call sites above eventually calls one of these three methods and nothing else. This is the gate insertion point (see §3). |

## 3. Architecture / Runtime Policy Detection

### 3.1 `UpdatePolicy`

New module `crates/terraphim_update/src/policy.rs`:

```rust
pub enum PackageManager {
    Pacman,
    // extensible: future supported managers add a variant + table entry.
}

pub enum UpdatePolicy {
    SelfManaged,
    PackageManaged {
        manager: PackageManager,
        update_command: String, // e.g. "sudo pacman -Syu"
    },
}
```

`UpdatePolicy` is a plain data type — no `cfg!`, no Cargo feature. It is
constructed once, at `UpdaterConfig` build time (or explicitly injected; see
§3.3), and carried as data through `UpdaterConfig` → `TerraphimUpdater`.

### 3.2 Detection contract (deterministic, both conditions required)

A running binary is considered package-managed **only if both** of the
following hold; **marker alone or prefix alone must never claim managed
ownership**:

1. **Marker**: the file at `/usr/share/terraphim/package-manager` exists,
   is readable, and its trimmed contents are *exactly* one entry from the
   supported-manager table (currently just `pacman`) — no trailing
   garbage, no multiple lines, no partial/prefix match.
2. **Prefix**: the *canonicalized* path of the current executable
   (`std::env::current_exe()`, then `fs::canonicalize` to resolve symlinks
   and `..`/`.` components) is a **path-component-wise descendant** of the
   managed prefix associated with that manager in the table (`pacman` →
   `/usr/bin`). Comparison is done on canonical `Path` components, never on
   raw string prefixes, specifically so `/usr/bin2/...` or
   `/usr/bin-evil/...` cannot spoof `/usr/bin`.

If either check fails — marker missing/unreadable/unsupported content, or
the executable resolves outside the matching prefix, or `current_exe()`/
canonicalization itself errors — detection resolves to `UpdatePolicy::
SelfManaged`. Detection never panics and never falls back to "managed" on
ambiguity; the fail-safe direction is always toward preserving today's
self-update behavior.

### 3.3 Pure, injectable detection API

The detection logic is split so it is fully testable without touching real
`/usr` paths or process environment:

```rust
/// Pure: takes every filesystem input as a parameter. No global state,
/// no env var reads, no hardcoded paths. Safe to call with temp-dir
/// stand-ins for the executable path, marker path, and prefix table.
pub fn detect_update_policy(
    current_exe: &Path,
    marker_path: &Path,
    managed_prefixes: &[(PackageManager, &Path)],
) -> UpdatePolicy { ... }

/// The only function that touches real process state: resolves
/// `std::env::current_exe()`, the real marker path
/// (`/usr/share/terraphim/package-manager`), and the real prefix table,
/// then delegates to `detect_update_policy`. Called exactly once, at
/// startup / `UpdaterConfig` construction.
pub fn detect_update_policy_default() -> UpdatePolicy { ... }

pub const MARKER_PATH: &str = "/usr/share/terraphim/package-manager";
pub const MANAGED_PREFIXES: &[(PackageManager, &str)] =
    &[(PackageManager::Pacman, "/usr/bin")];

/// Stable operator-facing guidance for a `PackageManaged` policy.
pub fn guidance(policy: &UpdatePolicy, bin_name: &str) -> String { ... }
```

Tests call `detect_update_policy` directly with `tempfile::TempDir`-rooted
paths standing in for `/usr/share/terraphim/package-manager` and `/usr/bin`
("fake-root" tests, §5) — they never write to, read from, or otherwise
mutate the real filesystem root or process environment.

### 3.4 Threading through `UpdaterConfig` / `TerraphimUpdater`

- `UpdaterConfig` gains a `policy: UpdatePolicy` field. The normal
  constructor path resolves it via `detect_update_policy_default()`; a
  `with_policy(UpdatePolicy)` builder method allows explicit injection —
  used by both production call sites that need to short-circuit before
  constructing a `Runtime` (§3.5) and by integration tests that want to
  exercise `TerraphimUpdater` under a specific policy without relying on
  real `/usr` state.
- `TerraphimUpdater::check_update`, `::update`, `::check_and_update` each
  gain, as their **first statement**, before touching `self.config.backend`
  or anything else:
  ```rust
  if let UpdatePolicy::PackageManaged { manager, update_command } = &self.config.policy {
      return Ok(UpdateStatus::PackageManaged {
          manager: manager.clone(),
          update_command: update_command.clone(),
      });
  }
  ```
  This is a single check per method (3 call sites inside the *library*, not
  4+ inside the *binaries*), and it returns before any `UpdateBackend`
  dispatch, before `platform::get_binary_path`, before any
  download/fetch-manifest/install/destination-fallback logic — so R2 and
  GitHub backends, and all of `platform`'s destination resolution, are
  equally and totally unreachable when the policy is package-managed.
- New `UpdateStatus` variant: `UpdateStatus::PackageManaged { manager,
  update_command }`, with a `Display` impl analogous to the existing
  variants, producing the same stable guidance text as `policy::guidance`.
  Chosen over reusing `Failed` because `Failed` reads as an error to a
  human/script, and `check-update` under a package-managed policy is *not*
  a failure — it is the deterministic, correct answer for this install.
  Chosen over reusing `Available`/`UpToDate` because callers must be able
  to branch on "this is a package-managed no-op" independent of version
  comparison, and because `update` (§4) needs to treat it as a distinct,
  typed refusal rather than a success.

### 3.5 Startup and both binaries' call sites

- **Agent startup** (`main.rs:448-456`): calls
  `terraphim_update::policy::detect_update_policy_default()` **before**
  constructing the `tokio::runtime::Runtime` used for the network check. If
  the result is `PackageManaged { .. }`, the entire block is skipped — no
  `Runtime::new()`, no `UpdaterConfig`/`TerraphimUpdater` construction, no
  network call, no filesystem write. This is a genuine safety requirement
  here (not just a performance nicety), since detection happens once at
  startup, ahead of and independent from the later `UpdaterConfig`-level
  guard used by the `check-update`/`update` subcommands.
- **Both agent command paths** (`main.rs`'s offline arm and
  `server_command.rs`'s server-mode arm) and **grep's command path** funnel
  through the same `UpdaterConfig`/`TerraphimUpdater` construction, so they
  automatically inherit the §3.4 short-circuit. Each of the three call
  sites additionally needs a `match` arm on `UpdateStatus::PackageManaged`
  to decide exit-code behavior (§4) and to print the stable guidance text —
  all three must emit the same message shape (produced by
  `policy::guidance`/`UpdateStatus`'s `Display`), not three independently
  worded strings.

### 3.6 No compile-time feature

There is no `package-managed` Cargo feature anywhere in this design. No
`Cargo.toml` in `terraphim_update`, `terraphim_agent`, or `terraphim_grep`
gains a new `[features]` entry. Every built binary contains both code paths;
the marker file plus executable-location check (§3.2), evaluated at
runtime, is the only thing that selects between them. O4's PKGBUILD does
**not** need a special `cargo build --features ...` invocation — see §12.

## 4. Exact CLI Behavior and Exit-Code Contract

| Command / call site | `SelfManaged` (default — no marker, or marker without matching prefix) | `PackageManaged` |
|---|---|---|
| startup check — `main.rs:448-456` | performs network check as today (unchanged) | skipped entirely per §3.5: no `Runtime::new()`, no network call, no output beyond an optional log line |
| `check-update` — agent offline (`main.rs:759-773`), agent server (`server_command.rs:485-500`), grep (`main.rs:164-167`) | network check via `check_update()`, prints `UpdateStatus` via `Display`, exit 0 on `Ok`, exit 1 on `Err` (current behavior, unchanged) | `check_update()` returns `Ok(UpdateStatus::PackageManaged { .. })` with zero network calls and zero writes; the existing `Ok(status) => { println!("{status}"); Ok(()) }` arm already prints the stable guidance and exits **0**. This is a stable, documented success: reporting "here's how to update" is a correct, successful answer for a read-only query command. |
| `update` — agent offline (`main.rs:775-789`), agent server (`server_command.rs:501-515`), grep (`main.rs:168-170`) | `check_and_update()` performs the real download+install, exit 0 on `Ok` (any status incl. `UpToDate`/`Updated`), exit 1 on `Err` (current behavior, unchanged) | `check_and_update()` returns `Ok(UpdateStatus::PackageManaged { .. })`; all three call sites gain a `match` arm that treats this as a **typed refusal**: print the guidance to stderr and exit with the **same generic nonzero code (`1`) already used for the `Err` arm at that call site**, via `std::process::exit(1)`, instead of falling into the existing `Ok(_) => { .. Ok(()) }` success path. |

**Exit-code choice is a deliberate, documented compatibility decision, not a
gap.** `update` under a package-managed policy performs zero mutation, so
exit 0 would be a false positive to any packaging CI/script that invokes
`terraphim-agent update`/`terraphim-grep update` expecting a real update to
have happened — it must be nonzero. This design reuses the **existing
generic failure code `1`** (the same code the `Err` arm already returns)
rather than inventing a new value (e.g. `3`), because Gitea **#181** — a
currently-open, separate task — owns introducing a final, stable, typed
exit-code scheme across all `update`/`check-update` outcomes. Minting a new
ad hoc code here risks colliding with whatever numbering #181 settles on.
The refusal is still **typed** at the Rust level (`UpdateStatus::
PackageManaged` is a distinct, matched variant, not a generic `Err`
downcast), so the moment #181 lands its exit-code remap, the binary-side
`match` arms in this design only need their `std::process::exit(1)` call
swapped for whatever #181 assigns — no further plumbing changes.

`check-update` needs **no exit-code change**: it already returns 0 on any
`Ok(status)` today, and `PackageManaged`'s `Display` impl supplies the
guidance text for free.

## 5. No-Network / No-Write Test Seam

All tests exercise the **pure** `detect_update_policy` function and/or
`UpdaterConfig::with_policy(...)` injection (§3.3/§3.4) — none mutate real
process env vars or write under the real `/usr` tree (which is typically
root-owned and not writable by the test user anyway).

- **`crates/terraphim_update/tests/policy.rs`** (new, plain unit/integration
  tests, no special build flags) — "fake-root" tests: each builds a
  `tempfile::TempDir` containing a stand-in marker file and a stand-in
  `bin/` directory, and calls `detect_update_policy` directly with paths
  rooted in that tempdir:
  1. **Valid marker + matching prefix** → `PackageManaged { manager:
     Pacman, .. }`. Marker file contains exactly `pacman`; stand-in
     executable path is a descendant of the stand-in `/usr/bin`-equivalent
     directory in the prefix table passed to the call.
  2. **Marker-only** (valid marker content, executable path *outside* the
     matching prefix, e.g. under a stand-in `~/.local/bin`-equivalent) →
     `SelfManaged`.
  3. **Prefix-only** (executable under the matching prefix, marker file
     absent or empty) → `SelfManaged`.
  4. **Invalid marker content** (unsupported manager name, multiple lines,
     trailing garbage, wrong case) → `SelfManaged`.
  5. **Traversal / symlink / canonicalization**: executable path expressed
     via `..`-traversal or a symlink that resolves (after
     `fs::canonicalize`) into the matching prefix → still detected as
     managed (canonicalization must run before the component comparison);
     conversely, a symlink or traversal that resolves *outside* the prefix,
     or a sibling directory whose name merely string-prefixes the managed
     prefix (e.g. `/usr/bin-evil`), must resolve to `SelfManaged` — this
     pins that comparison is component-wise, not a raw string
     `starts_with`.
  6. **Detection never panics** on a missing/unreadable marker file or an
     executable path that fails to canonicalize (e.g. dangling symlink) —
     asserts `SelfManaged`, not a panic or `Result::Err` bubbling out.
- **`crates/terraphim_update/tests/managed_mode.rs`** (new, plain
  integration test, no special build flags) — exercises
  `TerraphimUpdater` end-to-end under an *injected* policy
  (`UpdaterConfig::with_policy(UpdatePolicy::PackageManaged { .. })`), so it
  never depends on real `/usr` state:
  1. **Real local HTTP server, no mocks**: spin up a real
     `std::net::TcpListener`-backed local server (bound to `127.0.0.1:0`,
     ephemeral port) that increments an atomic request counter on every
     accepted connection, and point `TERRAPHIM_UPDATE_BASE_URL` /
     `UpdaterConfig`'s backend URL at it. Assert the counter is still `0`
     after calling `check_update()`, `update()`, and `check_and_update()`
     under the injected `PackageManaged` policy. As a regression control,
     the same harness is reused (in a companion test / the existing
     unmanaged suites) to assert the counter is **non-zero** under
     `SelfManaged`, proving the harness actually observes real requests
     rather than trivially passing.
  2. **Zero-write assertions**: run the same three calls with a fake
     `/usr/bin`-equivalent, `$HOME/.local/bin`-equivalent, and any
     update-cache/history directory the updater uses, all rooted under a
     fresh `tempfile::TempDir` and passed in via `UpdaterConfig`
     (destination/cache paths, not global env mutation — `HOME` itself is
     not overridden). Assert every one of those directories is
     byte-for-byte unchanged (or still absent/empty, whichever it started
     as) after the calls.
  3. **Message contract**: the `Display` of the returned
     `UpdateStatus::PackageManaged` (and `policy::guidance`) contains the
     manager's real update command, e.g. the literal substring
     `pacman -Syu` for the `Pacman` manager.
- **Exit-code contract tests** at the binary layer
  (`crates/terraphim_agent/tests/`, `crates/terraphim_grep/tests/`, plain
  tests, no special build flags): call the command handlers
  (`handle_check_update_command`, `handle_update_command`, and the
  server-mode `Command::CheckUpdate`/`Command::Update` arms) in-process with
  an injected `PackageManaged` `UpdaterConfig`, asserting: `check-update`
  returns success (exit-equivalent 0) with output containing the manager's
  update command; `update` returns the exit-equivalent of `1` with
  stderr/output containing the same text. These are in-process calls, not
  spawned-process tests against a faked real filesystem root, because
  `/usr/share/terraphim` and `/usr/bin` are not writable by an unprivileged
  test user — the injectable `UpdaterConfig`/`policy` API (§3.3/§3.4) exists
  precisely so this doesn't require root or real-path mutation.
- **Unmanaged-mode regression**: existing suites
  (`crates/terraphim_update/tests/integration_test.rs`,
  `tests/r2_update.rs`) continue to pass unmodified — they exercise the
  default `UpdaterConfig` (policy resolves to `SelfManaged` because nothing
  in the test environment matches the marker/prefix contract), proving §6's
  "unmanaged builds/installs preserve current self-update behavior"
  requirement.

## 6. Scope / Non-Goals

**In scope:**
- `UpdatePolicy`/`PackageManager` types, marker-file + executable-prefix
  detection contract (§3.2), the pure/injectable detection API (§3.3).
- Threading `policy` through `UpdaterConfig` and the `TerraphimUpdater`
  short-circuit (§3.4).
- Gating: startup check, `check-update`, `update` (both agent call sites +
  grep), all via runtime detection — no Cargo feature.
- Deterministic guidance string + the documented exit-code compatibility
  choice (§4).
- Test seam described in §5, including the real local HTTP server and
  fake-root filesystem tests.

**Non-goals:**
- Writing/maintaining the actual Omarchy PKGBUILD (O4's responsibility —
  this design only guarantees the marker-file contract O4 must satisfy and
  documents exactly what it must install; see §12).
- Any change to the self-managed/default update flow's actual network or
  install logic.
- Supporting partial/mixed states (e.g. one binary package-managed, the
  other not) within a single install beyond what naturally falls out of
  each binary independently evaluating the same marker file and its own
  `current_exe()` — no cross-binary coordination is added.
- Final, stable typed exit codes across all `update`/`check-update`
  outcomes — that is Gitea **#181**'s scope; this design deliberately
  reuses the existing generic `1` for the package-managed `update` refusal
  and does not invent a new code (see §4).
- Supporting package managers other than `pacman` in this change — the
  detection table (§3.2/§3.3) is structured to add more (`apt`, `dnf`,
  etc.) later without changing the contract shape, but only `pacman` is
  wired up now, matching the O4/Omarchy PKGBUILD need.

## 7. Exact File Plan

| File | Change |
|---|---|
| `crates/terraphim_update/src/policy.rs` (new) | `UpdatePolicy`, `PackageManager`, `MARKER_PATH`, `MANAGED_PREFIXES`, `detect_update_policy` (pure), `detect_update_policy_default` (wrapper), `guidance(...)` |
| `crates/terraphim_update/src/lib.rs:6-14` | add `pub mod policy;`; add `UpdateStatus::PackageManaged { manager, update_command }` variant + `Display`/`log_status` arms (near `:32-113`) |
| `crates/terraphim_update/src/lib.rs` (`UpdaterConfig`) | add `policy: UpdatePolicy` field, resolved via `policy::detect_update_policy_default()` in the default constructor; add `with_policy(UpdatePolicy)` builder for injection |
| `crates/terraphim_update/src/lib.rs:242-251` (`check_update`), `:522-533` (`update`), `:1184-1189` (`check_and_update`) | insert the policy short-circuit (§3.4) as the first statement of each, before any backend/`platform::get_binary_path`/download/install logic |
| `crates/terraphim_update/tests/policy.rs` (new) | fake-root pure-function tests (§5): valid marker+prefix, marker-only, prefix-only, invalid marker, traversal/symlink, canonicalization-failure/no-panic |
| `crates/terraphim_update/tests/managed_mode.rs` (new) | real local HTTP server request-counting test, zero-write test, message-contract test, all via `UpdaterConfig::with_policy` injection |
| `crates/terraphim_agent/src/main.rs:448-456` | call `policy::detect_update_policy_default()` before constructing the startup `Runtime`; skip the whole block when `PackageManaged` |
| `crates/terraphim_agent/src/main.rs:775-789` (`handle_update_command`) | match on `UpdateStatus::PackageManaged` and `std::process::exit(1)` (documented generic/compat code, §4) instead of falling into the existing `Ok(status) => { println!(..); Ok(()) }` arm |
| `crates/terraphim_agent/src/server_command.rs:501-515` (`Command::Update` arm) | same match-and-exit change as above |
| `crates/terraphim_grep/src/main.rs:161-175` (`handle_update_command`, `Command::Update` case) | same match-and-exit change |
| `crates/terraphim_agent/tests/managed_mode.rs` (new) | in-process exit-code contract test via injected `PackageManaged` config (§5) |
| `crates/terraphim_grep/tests/managed_mode.rs` (new) | in-process exit-code contract test via injected `PackageManaged` config (§5) |

Note: `check-update` call sites (`main.rs:759-773`, `server_command.rs:485-500`,
`terraphim_grep/src/main.rs:164-167`) need **no exit-code change** — per §4
they already exit 0 on any `Ok(status)`, and `PackageManaged`'s `Display`
impl supplies the guidance text for free. No `Cargo.toml` in any of the
three crates changes.

## 8. Vertical RED→GREEN Sequence

Each step below is a single compiling, independently-mergeable slice; later
steps build on earlier ones. "RED" describes the failure the new test
produces on the base commit / previous step, before its GREEN change. None
of these steps require a special `--features` flag — every test runs
against the default build.

1. **RED**: add `crates/terraphim_update/src/policy.rs` fake-root tests —
   valid marker+prefix, marker-only, prefix-only, invalid marker,
   traversal/symlink, no-panic-on-error (§5.1). Fails: module doesn't
   exist → compile error.
   **GREEN**: add `policy.rs` (`UpdatePolicy`, `PackageManager`,
   `detect_update_policy`, `detect_update_policy_default`, `guidance`) and
   `pub mod policy;` in `lib.rs`.
2. **RED**: add `UpdateStatus::PackageManaged` construction/Display test
   in `terraphim_update`. Fails: variant doesn't exist → compile error.
   **GREEN**: add the variant + `Display`/`log_status` arms; add
   `UpdaterConfig::policy`/`with_policy`.
3. **RED**: `crates/terraphim_update/tests/managed_mode.rs` — with a real
   local HTTP server (request-counting, §5.2) and an injected
   `PackageManaged` policy, `check_update()`/`update()`/
   `check_and_update()` must return `Ok(PackageManaged { .. })` and the
   server's request counter must stay `0`. Fails: methods still dispatch to
   `UpdateBackend::R2`/`GitHub` and the counter increments (or the call
   hangs/errors against an unreachable backend).
   **GREEN**: insert the policy short-circuit as the first statement of
   `check_update`, `update`, `check_and_update` in `lib.rs`.
4. **RED**: same file, zero-write assertion — with fake `/usr/bin`,
   `$HOME/.local/bin`, and cache/history destinations rooted in a temp
   dir and passed via `UpdaterConfig`, the three calls above must leave
   every one of them unchanged. Given step 3's guard already prevents any
   backend dispatch, this step is expected to pass as a direct structural
   consequence and is included to pin the guarantee, not to drive new
   production code — flag if it fails, since that would mean step 3's
   guard placement missed a path.
5. **RED**: unmanaged-mode regression — with the *default* `UpdaterConfig`
   (policy resolves to `SelfManaged` in the test environment),
   `check_update()`/`update()` still reach real backend dispatch and the
   local HTTP server's request counter increments (assert via existing
   `terraphim_update/tests/integration_test.rs` / `tests/r2_update.rs`
   continuing to pass unmodified, plus the counter check as the harness's
   own regression control). This should already be GREEN after step 3 if
   the guard is correctly scoped to the `PackageManaged` arm only; treat
   any RED here as a signal the guard leaked into the self-managed path.
6. **RED**: `crates/terraphim_agent/tests/managed_mode.rs` — call
   `handle_check_update_command`/`handle_update_command` in-process with an
   injected `PackageManaged` `UpdaterConfig`: `check-update` succeeds
   (exit-equivalent 0) with output containing the manager's update command;
   `update` returns the exit-equivalent of `1` with output containing the
   same text. Fails: `handle_update_command` (`main.rs:775-789`) still
   falls into the generic `Ok(status) => { println!(..); Ok(()) }` arm and
   exits 0.
   **GREEN**: add the match-and-exit branch in `handle_update_command`.
7. **RED**: same file, exercised through the *server* command path
   (`server_command.rs:501-515`) — same assertion shape as step 6, against
   `run_server_command`'s `Command::Update` arm instead of the offline arm.
   Fails: that arm is untouched and still exits 0.
   **GREEN**: mirror the match-and-exit branch in `server_command.rs`.
8. **RED**: `crates/terraphim_grep/tests/managed_mode.rs` — same shape as
   step 6 for `terraphim-grep`'s `check-update`/`update` handlers. Fails:
   grep's `handle_update_command` (`main.rs:161-175`) still exits 0 for
   `update`.
   **GREEN**: add the match-and-exit branch in grep's
   `handle_update_command`.
9. **RED**: agent startup no-op test — with `policy::
   detect_update_policy_default()` (or, for a deterministic unit test, the
   pure `detect_update_policy` fed fake-root inputs resolving to
   `PackageManaged`) wired ahead of the startup block, assert the block is
   skipped: no `Runtime::new()` for the startup check, no network call.
   Fails: `main.rs:448-456` still runs unconditionally regardless of
   policy.
   **GREEN**: call `policy::detect_update_policy_default()` before
   constructing the startup `Runtime`, and skip the block entirely when the
   result is `PackageManaged`.
10. **RED**: full regression — both self-managed (no marker present /
    executable outside any managed prefix in the test environment) and
    package-managed (fake-root inputs) behavior exercised end-to-end for
    both binaries: self-managed must be byte-for-byte unchanged (network
    call happens, `update` performs a real install attempt, startup check
    fires); package-managed must show zero network, zero writes, stable
    guidance, and the documented exit codes from §4. Expected GREEN
    immediately if every prior gate is correctly scoped; any RED here is a
    blocking regression.

## 9. Verification Commands

Focused (per step in §8, run against the crate touched — steps 1-5 in
`terraphim_update`, steps 6-7 in `terraphim_agent`, step 8 in
`terraphim_grep`). No `--features` flags are needed anywhere:
```
# Steps 1-2: policy.rs + UpdateStatus variant (fake-root, plain unit tests)
cargo test -p terraphim_update policy

# Steps 3-4: no-network (real local HTTP server, request-counted)/no-write
# structural guarantees, via injected PackageManaged UpdaterConfig
cargo test -p terraphim_update --test managed_mode

# Step 5, step 10 (self-managed regression): existing suites must stay
# green, unmodified
cargo test -p terraphim_update --test integration_test
cargo test -p terraphim_update --test r2_update

# Steps 6-7: agent CLI + server-mode exit-code contract (in-process,
# injected PackageManaged config)
cargo test -p terraphim_agent --test managed_mode

# Step 8: grep exit-code contract
cargo test -p terraphim_grep --test managed_mode

# Step 9: agent startup no-op
cargo test -p terraphim_agent managed_startup
```

Full:
```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## 10. Acceptance Mapping

| Requirement | Verified by (§8 step) |
|---|---|
| No startup network check when running a package-managed install | Step 9 |
| `check-update`/`update` return deterministic package-manager guidance, both agent call paths (offline + server) + grep | Steps 6, 7, 8 |
| Never call network/download/install code when package-managed | Step 3 (structural short-circuit before backend dispatch, exercised against a real request-counting local HTTP server) |
| Never write `/usr/bin`, `/usr/local/bin`, `~/.local/bin`, or cache/history state when package-managed | Step 4 (fake-root/temp-dir harness) — a direct consequence of step 3's guard, pinned explicitly |
| Marker alone or prefix alone never claims managed ownership | Step 1 (marker-only / prefix-only fake-root cases) |
| Traversal/symlink/canonicalization cannot spoof the managed prefix | Step 1 |
| Self-managed installs preserve current self-update behavior unchanged | Steps 5, 10 |

## 11. Risks / Rollback

- **Risk (largely closed by design)**: a future call site bypassing the
  gate. Because the guard sits inside `TerraphimUpdater::check_update` /
  `update` / `check_and_update` themselves (§3.4) rather than at each of
  the 4 current call sites, any *future* call site — a new subcommand, a
  new binary, a library consumer — automatically inherits the protection
  as long as it goes through `UpdaterConfig`. The residual risk is
  narrower: a future method added directly to `TerraphimUpdater` that
  performs network/install work without going through these three entry
  points, or a caller that hand-builds state bypassing `UpdaterConfig`
  entirely. Mitigation: `cargo doc` review of `terraphim_update`'s public
  API during implementation to confirm `check_update`/`update`/
  `check_and_update` remain the only public entry points that reach
  `downloader`/`platform::get_binary_path`/`install_verified_archive`; the
  crate's convenience free functions (`check_for_updates`,
  `update_binary`, `update_binary_silent`, `check_for_updates_auto`,
  `check_for_updates_startup`, `start_update_scheduler` —
  `lib.rs:1234-1457`) call into the same `TerraphimUpdater` methods but are
  **not currently used by `terraphim_agent`/`terraphim_grep`**; if
  implementation confirms that, they're out of scope for this change but
  should get the same guard for consistency, noted as a follow-up if not
  folded into step 1-3's diff.
- **Risk**: marker/prefix detection false-positive or false-negative.
  A false positive (a self-managed install wrongly detected as
  package-managed) would silently disable self-update for a user who
  didn't install via pacman; a false negative (a real pacman install not
  detected) would let a package-managed binary attempt to write into a
  pacman-owned `/usr/bin`. Mitigation: §3.2's both-conditions-required rule
  plus the fail-safe-to-`SelfManaged` behavior on any ambiguity/error, and
  the dedicated marker-only/prefix-only/traversal/symlink test matrix in
  §5/§8 step 1, are the primary defenses; no other mitigation (e.g.
  querying `pacman -Qo`) is added, since shelling out to the package
  manager itself would introduce a new runtime dependency and failure mode
  this design avoids.
- **Risk**: the new `UpdateStatus::PackageManaged` variant is
  non-exhaustively matched somewhere existing code already does
  `match status { .. }` without a wildcard arm (e.g. `log_status`'s
  `match` at `lib.rs:66-83`), causing a compile error since the variant
  exists in all builds (there is no feature gate to hide it behind).
  Mitigation: this is expected and desired — the compiler will point at
  every match site needing an arm; audit `log_status` and any other
  exhaustive match over `UpdateStatus` as part of step 2's GREEN, not left
  for later.
- **Rollback**: the detection logic is purely additive and fails safe to
  `SelfManaged`; reverting is deleting the `policy` module, the
  `UpdaterConfig::policy` field and short-circuit guard in the three
  `TerraphimUpdater` methods, and the exit-code match arms in the three
  binary call sites. Because there is no feature flag, rollback is a
  straightforward code revert, not a build-configuration change — every
  existing build/install is affected identically by either state.

## 12. O4 Handoff

- O4's Omarchy PKGBUILD builds `terraphim_agent`/`terraphim_grep` exactly
  as today — **no special `cargo build` flags, no `--features` argument**.
  Detection is runtime-only (§3.2/§3.6); there is nothing to opt into at
  build time.
- O4's PKGBUILD **must install the marker file** as part of the package's
  install step (e.g. a `post_install`/packaged data file, per pacman
  packaging conventions): write `/usr/share/terraphim/package-manager`
  containing exactly the single line `pacman` (no trailing content beyond
  a single trailing newline, which detection trims). This, combined with
  the binaries already living under `/usr/bin` as pacman installs them, is
  what makes detection resolve to `PackageManaged` — both conditions in
  §3.2 are satisfied by a normal pacman package layout without any further
  PKGBUILD changes to the binaries themselves.
- O4 should NOT add any runtime flag/env-var workaround for this; if a
  runtime toggle is later desired (e.g. to let a user on a pacman install
  still opt into manual self-update for testing), that is a separate,
  explicitly-scoped follow-up, not part of this design.
- O4 does not need to do anything else to satisfy "no self-update" — once
  the marker file is present and the binaries are installed under
  `/usr/bin`, they refuse to touch the network or filesystem update paths
  on their own, with no other install-time step required.
