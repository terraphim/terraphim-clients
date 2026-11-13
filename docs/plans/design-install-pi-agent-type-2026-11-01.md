# Design Note — `AgentType::Pi` install guidance (terraphim-clients#458)

**Date:** 2026-11-01
**Issue:** `terraphim-clients#458` — add `pi` to `terraphim-agent learn install-hook`
**Scope:** `crates/terraphim_agent/src/learnings/install.rs` (+ directly relevant CLI/tests)

## Problem

`terraphim-agent learn install-hook` supports `claude`, `codex`, and `opencode`.
Every one of those agents is wired through a **shell hook**: the CLI writes a
`terraphim-hook.sh` into the agent's config dir and asks the user to export an
`*_HOOK` environment variable.

pi (`pi_agent_rust`) is **not** wired that way. pi loads **extension packages**
(`pi install <path>`) that register JS event handlers. The shipped extension is
`packages/pi-terraphim-learn`, which hooks `onToolResult` and pipes a normalized
Claude envelope into `terraphim-agent learn hook --format claude`.

So `pi` is a first-class install target for the CLI, but the *installation
mechanism* is package installation, not a shell hook. This is called out
explicitly in `packages/pi-terraphim-learn/README.md` and in
`docs/plans/design-pi-terraphim-learn-2026-08-08.md` (touchpoint 2: *"pi uses
packages, not shell in ~/.claude"*).

## Decision

Add `AgentType::Pi` to the existing clap `ValueEnum` and **branch the install
flow on the agent**, instead of pretending Pi uses the hook path.

### `AgentType::Pi`

| Concern | Behaviour |
|---|---|
| `as_str()` | `"pi"` |
| clap value | `pi` (auto-derived from `Pi` variant) |
| clap help | `Pi agent (pi_agent_rust) — installs the pi-terraphim-learn extension package` |
| `config_dir()` | `<home>/.pi/agent` — exactly `dirs::home_dir().join(".pi").join("agent")` |
| `hook_script()` | `None` — pi does not consume a shell hook |
| `hook_path()` | `None` — pi has no hook file, so there is no path to point at |
| `install_hook()` | Does **not** write a hook file; prints package-install guidance and returns `Ok(())` |

### Config path: home directory, not XDG config directory

pi resolves its own config root from the **home** directory, not the XDG config
directory. This is confirmed in `pi_agent_rust` itself:

```rust
// pi_agent_rust/src/extensions_js.rs
dirs::home_dir().map(|home| home.join(".pi").join("agent").join("cache").join("modules"))
```

Therefore `AgentType::Pi::config_dir()` must be
`dirs::home_dir().join(".pi").join("agent")` — **not**
`dirs::config_dir().join(".pi").join("agent")` (which would resolve to
`~/.config/.pi/agent` on Linux). The test asserts the path **exactly** against
`dirs::home_dir()`, not merely that it "ends with `.pi/agent`", so a regression to
the XDG path is caught.

The other three agents (`claude`, `codex`, `opencode`) keep using
`dirs::config_dir()`.

### Branching on the agent, not on `hook_script()`

The install flow branches **explicitly on `AgentType::Pi`** and returns early:

```rust
if matches!(agent, AgentType::Pi) {
    println!("{}", pi_install_guidance());
    return Ok(());
}
```

It does **not** infer "this agent has no shell hook" from
`hook_script() == None`. Inferring it that way would silently route any *future*
hook-less agent to the Pi guidance, which is wrong output for that agent. The
explicit branch makes the special case intentional and reviewable.

After that early return, the remaining agents are known to have a shell hook, so
`hook_script()` is unwrapped with `expect("non-Pi agents always have a shell
hook script")` rather than a silent `unwrap`.

The activation-instruction `match` still needs to be exhaustive over
`AgentType`. Its Pi arm is **unreachable** and says so explicitly:

```rust
// Unreachable: Pi returns early above (explicit `AgentType::Pi` branch).
// The arm is retained only so the match stays exhaustive over
// `AgentType`; it must never be reached.
AgentType::Pi => unreachable!("Pi has no shell hook and returns early above"),
```

### Why `hook_script()` returns `Option<String>`

Changing the return type from `String` to `Option<String>` makes the "Pi has no
shell hook" fact a **compile-time-enforced** invariant rather than a runtime
`match` arm that could silently drift. The `hook_script()` arm for Pi returns
`None`. The same applies to `hook_path()`: it returns `None` for Pi so no
hypothetical `~/.pi/agent/terraphim-hook.sh` path is ever surfaced.

### Pi guidance content

The printed guidance mirrors `packages/pi-terraphim-learn/README.md`. It must
state the `terraphim-agent >= 1.21.0` requirement **at installation time**
(not only as a footnote on the smoke-test step), include the `pi doctor`
extension-trust acknowledgement, and reproduce the exact smoke JSON envelope
from the README:

```
Pi agent (pi_agent_rust) uses an extension package, not a shell hook.

  1. Install the extension package (requires terraphim-agent >= 1.21.0 on PATH):
       pi install <terraphim-clients>/packages/pi-terraphim-learn

  2. Acknowledge extension trust if `pi doctor` prompts.

  3. Smoke-test capture without pi:
       echo '{"tool_name":"Bash","tool_input":{"command":"false"},"tool_result":{"exit_code":1,"stdout":"","stderr":"x"}}' \
         | terraphim-agent learn hook --format claude
       ls -lt ~/.local/share/terraphim/learnings/ | head

  The extension listens for pi's onToolResult event and forwards failed
  bash-like commands to `terraphim-agent learn hook --format claude`.
  See packages/pi-terraphim-learn/README.md for details.
```

The guidance text is factored into a small pure function
(`pi_install_guidance()`) so it is directly unit-testable without touching the
filesystem or capturing stdout.

## Alternatives rejected

1. **Reuse the shell-hook path for Pi.** Rejected: it would emit a
   `terraphim-hook.sh` and an `export PI_HOOK=…` instruction that pi does not
   read. That is a lie in the CLI output — exactly what the issue forbids.
2. **Keep Pi out of `AgentType` entirely and document it only in the README.**
   Rejected: the issue asks for the enum variant, clap parsing/help, and
   `as_str="pi"`; and `terraphim-agent learn install-hook pi` is already
   referenced by `packages/pi-terraphim-learn/README.md`, so the CLI must accept
   it rather than erroring on an unknown value.
3. **Add a `pi` config dir + hook file anyway, as a belt-and-braces fallback.**
   Rejected: pi does not load it, so it would be dead code that users would
   think is active.
4. **Derive the Pi branch from `hook_script().is_none()`.** Rejected: it
   conflates "this agent is Pi" with "this agent has no shell hook", so any
   future hook-less agent would silently print Pi-specific guidance.

## Test plan

Focused unit tests in `install.rs` (`mod tests`), which is where the existing
install tests live. **12 tests** run under the filter
`cargo test -p terraphim_agent --lib learnings::install`:

| # | Test | Assertion |
|---|------|-----------|
| 1 | `test_agent_type_as_str` | `AgentType::Pi.as_str() == "pi"` |
| 2 | `test_agent_type_variants_distinct` | `Pi` differs from the other three |
| 3 | `test_agent_type_parses_from_clap` | `AgentType::from_str("pi", true)` yields `Pi`; the other three still parse; unknown values error |
| 4 | `test_agent_type_clap_help_mentions_pi` | clap value enum exposes `"pi"` |
| 5 | `test_pi_has_no_shell_hook_script` | `Pi.hook_script()` is `None`; the other three are `Some` |
| 6 | `test_pi_has_no_hook_path` | `Pi.hook_path()` is `None`; the other three are `Some` |
| 7 | `test_pi_config_dir_is_exactly_home_pi_agent` | `Pi.config_dir() == dirs::home_dir().join(".pi").join("agent")` (exact equality) |
| 8 | `test_pi_install_guidance_points_at_package` | guidance mentions `packages/pi-terraphim-learn`, `pi install`, the smoke helper, and **not** `PI_HOOK` / `terraphim-hook.sh` |
| 9 | `test_pi_install_guidance_matches_package_readme` | install step states `terraphim-agent >= 1.21.0`; guidance mentions `pi doctor` and "trust"; guidance contains the exact smoke JSON envelope |
| 10 | `test_hook_script_contains_agent_name` | unchanged (Claude/Codex/Opencode scripts invoke `terraphim-agent learn hook`) |
| 11 | `test_hook_script_fail_open` | unchanged (scripts contain `2>/dev/null \|\| true`) |
| 12 | `test_install_error_display` | unchanged (`InstallError` messages) |

CLI schema tests in `crates/terraphim_agent/src/cli_schema.rs` (`mod tests`).
**4 tests** run under the filter
`cargo test -p terraphim_agent --bin terraphim-agent cli_schema::tests`:

| # | Test | Assertion |
|---|------|-----------|
| 1 | `learn_install_hook_accepts_pi` | `learn install-hook pi` parses to `AgentType::Pi` |
| 2 | `learn_install_hook_still_accepts_existing_agents` | `claude`, `codex`, `opencode` still parse |
| 3 | `learn_install_hook_rejects_unknown_agent` | `learn install-hook nope` errors |
| 4 | `learn_install_hook_help_lists_pi` | `learn install-hook --help` mentions `pi` |

## Verification

Commands actually run for this change:

```
cargo fmt --check
cargo test -p terraphim_agent --lib learnings::install
cargo test -p terraphim_agent --bin terraphim-agent cli_schema::tests
cargo check -p terraphim_agent
cargo clippy -p terraphim_agent --all-targets -- -D warnings
git diff --check
```

Plus a live smoke test of the built binary in a disposable `HOME`:

```
learn install-hook --help
learn install-hook pi
```

asserting that **no hook files** are created for `pi`.

### Known pre-existing failures (not introduced by this change)

The full `terraphim_agent` package suite has **3 pre-existing cross-mode
failures**. They are **not** a pass: they fail because
`TERRAPHIM_SERVER_BIN` is absent in this workspace, so the tests that need a
prebuilt `terraphim_server` binary cannot run. They were reproduced on the
**exact base commit** (`3c9e5dc`, the merge-base for this worktree, verified in
the detached worktree `/home/alex/.cache/clients458-base`), which confirms they
are pre-existing and unrelated to the Pi change.

All 3 come from `tests/cross_mode_consistency_test.rs` and share one root
cause (`terraphim_server is not a member of this workspace, so it cannot be
built here. Set TERRAPHIM_SERVER_BIN to a prebuilt binary to run this test.
Refs #113`):

1. `test_cross_mode_consistency`
2. `test_mode_specific_verification`
3. `test_role_consistency_across_modes`

Result on base: `0 passed; 3 failed`.

The same root cause affects `tests/server_mode_tests.rs` and
`tests/kg_ranking_integration_test.rs`, which are gated the same way.

The full package suite was **not** re-run for this change; the focused filters
above cover the changed code.
