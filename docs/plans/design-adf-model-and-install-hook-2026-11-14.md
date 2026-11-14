# Design Document: the ADF model ids and `install-hook pi` (#517)

**Issue**: #517
**Status**: Draft
**Date**: 2026-11-14
**Research**: `docs/plans/research-adf-model-and-install-hook-2026-11-14.md`

## Summary

Two changes, both in this repository, plus the guard that keeps them fixed:

- **The fleet configuration** (`.terraphim/adf.toml`): the agent's model moves
  onto a provider prefix the orchestrator's allow-list accepts, the fallback
  model moves onto an id the fallback CLI's registry knows, and the fallback
  CLI path points at a file that exists on this host.
- **The install contract** (`crates/terraphim_agent/src/learnings/install.rs`):
  `install_hook(Pi)` prints the guidance and returns a **distinct error**,
  `InstallError::ManualInstallRequired`, so the command exits non-zero. A
  command that installed nothing must not report success.
- **The guard** (`crates/terraphim_agent/tests/adf_config_guard.rs`): a test
  that fails when an agent's model prefix is not on the orchestrator's
  allow-list, when a fallback path is not absolute, or when the host has
  opencode and the fallback path does not exist. When the deployed
  orchestrator is on the host, the guard also runs its own
  `adf --local --check` and fails on a validation error.

## Motivation

The first half of #517 is not a subtle misconfiguration: the fleet config does
not load, so the autonomous implementer for this repository cannot start. The
second half is a lie in an exit code: a script that runs
`terraphim-agent learn install-hook pi` sees 0 and concludes the hook is
installed.

Neither defect had a guard, which is why both survived a merged pull request.

## Architecture

```text
.terraphim/adf.toml                 the fleet configuration
        |
        | loaded and validated by the orchestrator at fleet load
        v
adf --local --check                  the verdict: a validation failure names the
                                     agent and the rejected prefix
        |
        v
tests/adf_config_guard.rs            the guard that keeps the prefixes
                                     allow-listed (portable) and asks the
                                     orchestrator itself when it is on the host

crates/terraphim_agent/src/learnings/install.rs
        |
        | install_hook(Pi) -> prints the guidance, returns ManualInstallRequired
        v
crates/terraphim_agent/src/learn_command.rs
        |
        | the error propagates; main's Result termination prints it
        v
exit code 1 + "Error: the pi agent's hook was not installed: ..."
```

### The configuration

```toml
model = "zai-coding-plan/glm-5.1"
fallback_provider = "/home/alex/.local/share/mise/shims/opencode"
fallback_model = "kimi-for-coding/k2p6"
```

- `zai-coding-plan` is on the orchestrator's allow-list; `glm-5.1` is the id
  the sibling fleet repositories use, and pi resolves it.
- `kimi-for-coding/k2p6` is the fallback model the sibling fleet
  repositories use; the fallback CLI is opencode, so its registry is the one
  that has to know the id.
- The fallback CLI path is the mise shim, which is what `which opencode`
  resolves to on this host. The fleet's older value
  (`/home/alex/.bun/bin/opencode`) does not exist here.

### The install outcome

```rust
/// The agent has no shell hook: the hook is an extension package the user
/// installs with the agent's own tool, so this command printed the
/// instructions and installed nothing. Refs terraphim-clients#517.
#[error(
    "the {agent} agent's hook was not installed: it is an extension package, \
     not a shell hook - the instructions above are the installation"
)]
ManualInstallRequired { agent: &'static str },
```

The Pi branch prints the guidance and then returns that error. The command
therefore exits with the taxonomy's general error code (1) - the taxonomy is
owned by Gitea #181, outside this repository, so no new code is minted here -
and the message says what a caller has to do. The other three agents are
unchanged: they write a real hook file and exit 0.

### The guard

| Arm | What it checks | Host-dependent? |
|---|---|---|
| The prefixes | Every `model` and `fallback_model` provider prefix is on the orchestrator's allow-list | No |
| The fallback path | Every `fallback_provider` is an absolute path | No |
| The fallback exists | When the host has `opencode` on PATH, the `fallback_provider` is a file on disk | Yes, conditionally |
| The orchestrator | When the host has the `adf` binary, `adf --local --check` reports no validation failure | Yes, conditionally |

The host-dependent arms skip with a printed notice when the tool is absent -
the pattern `tests/support/binary.rs::skip_without_server` established. The
portable arms always run, so the guard is never a no-op on the CI runner.

## File changes

| File | Change |
|---|---|
| `.terraphim/adf.toml` | The model, the fallback model and the fallback CLI path. |
| `crates/terraphim_agent/src/learnings/install.rs` | The new error variant, the Pi branch returning it, a unit test. |
| `crates/terraphim_agent/tests/learn_install_hook_contract.rs` | New: the CLI contract test for `learn install-hook`. |
| `crates/terraphim_agent/tests/adf_config_guard.rs` | New: the fleet configuration guard. |
| `crates/terraphim_agent/Cargo.toml` | New: `toml` as a dev-dependency (already in the lock file). |
| `docs/plans/research-adf-model-and-install-hook-2026-11-14.md` | New: the research. |
| `docs/plans/design-adf-model-and-install-hook-2026-11-14.md` | New: this document. |
| `docs/plans/note-adf-model-and-install-hook-2026-11-14.md` | New: the implementation note and the evidence. |

## Testing strategy

| Level | What it covers |
|---|---|
| Unit (`install.rs`) | The Pi branch returns the distinct error; the other agents still install and succeed. |
| Integration (`learn_install_hook_contract.rs`) | The real binary: `install-hook pi` exits non-zero, prints the guidance, names the manual step and installs nothing; `install-hook claude` exits 0 and writes the hook under a hermetic HOME. |
| Integration (`adf_config_guard.rs`) | The config's prefixes, the fallback path, and - when the host has the tools - the deployed orchestrator's own verdict. |

No mocks: the guard runs the real orchestrator and the real agent binary.

## Risks

| Risk | Mitigation |
|---|---|
| The guard's allow-list drifts from the orchestrator's | The guard also asks the orchestrator itself, when it is on the host; the list is a comment in the test that names its source |
| A host-dependent arm hides a real defect | The portable arms always run; the skipped arm prints a notice |
| The chosen model id is retired | pi fails loudly at run time; the guard checks the prefix, which is what the orchestrator validates |

## Verification plan

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace --all-targets --no-fail-fast`
4. `adf --local --check` - the config loads
5. `pi-rust --provider zai-coding-plan --model glm-5.1` - the id resolves
6. `terraphim-agent learn install-hook pi` - a non-zero exit and the guidance
