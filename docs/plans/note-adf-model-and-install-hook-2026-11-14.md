# Implementation, Verification and Validation: the ADF model ids and `install-hook pi` (#517)

**Issue**: terraphim-clients#517
**Research**: `docs/plans/research-adf-model-and-install-hook-2026-11-14.md`
**Design**: `docs/plans/design-adf-model-and-install-hook-2026-11-14.md`
**Author**: opencode-cli (agent session)
**Date**: 2026-11-14

## What was implemented

### 1. The fleet configuration loads again

`.terraphim/adf.toml` declared the `implementation-swarm` agent's model as
`terraphim-proxy/fastest`, with `terraphim-proxy/auto` as the fallback model.
Neither prefix is on the orchestrator's allow-list, so the **fleet refused to
load this project at all** - every agent that loads it was affected, not just
that one. Neither model id exists in pi's client-side registry either.

The agent now carries the pair the sibling fleet repositories use and the
deployed orchestrator accepts:

```toml
model = "zai-coding-plan/glm-5.1"
fallback_provider = "/home/alex/.local/share/mise/shims/opencode"
fallback_model = "kimi-for-coding/k2p6"
```

The fallback CLI path is the mise shim because that is what `which opencode`
resolves to on this host; neither the fleet's older
`/home/alex/.bun/bin/opencode` nor the `/usr/local/bin/opencode` this replaced
exists here.

### 2. `install-hook pi` no longer reports a success it did not achieve

`install_hook(AgentType::Pi)` printed the extension-package guidance and
returned `Ok(())`, so `terraphim-agent learn install-hook pi` exited 0 with
nothing installed. A scripted install that sees 0 concludes the hook is
installed.

`crates/terraphim_agent/src/learnings/install.rs` gained a distinct outcome:

```rust
/// The agent has no shell hook to install: its hook is an extension
/// package the user installs with the agent's own tool, so this command
/// printed the instructions and installed nothing.
#[error(
    "the {agent} agent's hook was not installed: it is an extension package, \
     not a shell hook - the instructions above are the installation"
)]
ManualInstallRequired { agent: &'static str },
```

The Pi branch prints the guidance and returns that error, so the command exits
with the taxonomy's general error code (1) and says what a caller has to do.
The exit-code taxonomy belongs to Gitea #181, outside this repository, so no
new code was minted here. The other three agents are unchanged: they write a
real hook file and exit 0.

### 3. The guard that keeps both fixed

`crates/terraphim_agent/tests/adf_config_guard.rs` is the test neither defect
had. It reads `.terraphim/adf.toml` and asserts:

| Arm | What it checks | Host-dependent? |
|---|---|---|
| The prefixes | Every `model` and `fallback_model` provider prefix is on the orchestrator's allow-list | No |
| The fallback path | Every `fallback_provider` is an absolute path | No |
| The fallback exists | When the host has `opencode` on `PATH`, the `fallback_provider` is a file | Conditionally |
| The orchestrator | When the host has the `adf` binary, `adf --local --check` reports no validation failure | Conditionally |

The portable arms always run, so the guard is never a no-op on the CI runner.
The host-dependent arms skip with a printed notice when the tool is absent -
the pattern `tests/support/binary.rs::skip_without_server` established.

## Verification

### The configuration

Before:

```text
$ adf --local --check
Discovered terraphim-clients: 6 agents from .../.terraphim/adf.toml
adf --local --check FAILED validation: banned LLM provider 'terraphim-proxy/fastest' in model for agent 'implementation-swarm' (allowed: claude-code, opencode-go, kimi-for-coding, minimax-coding-plan, zai-coding-plan)
```

After:

```text
$ adf --local --check
terraphim-clients  implementation-swarm            zai-coding-plan/glm-5.1  Core
```

and no validation failure. The model id resolves in pi as well - the probe
reaches the credential stage ("No API key found for provider zai-coding-plan"),
which is a different failure from the one the old id produced:

```text
$ pi-rust --provider terraphim-proxy --model fastest -p "say OK"
Model terraphim-proxy/fastest not found
```

The fallback CLI path is real and runnable:

```text
$ /home/alex/.local/share/mise/shims/opencode --version
opencode v2.3.0
```

### The install contract

```text
$ HOME=$FAKEHOME terraphim-agent learn install-hook pi; echo "exit=$?"
Pi agent (pi_agent_rust) uses an extension package, not a shell hook.
  1. Install the extension package (requires terraphim-agent >= 1.21.0 on PATH):
       pi install <terraphim-clients>/packages/pi-terraphim-learn
  ...
Error: the pi agent's hook was not installed: it is an extension package, not a shell hook - the instructions above are the installation
exit=1
```

and nothing was written under the fake home.

### The suite

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --all-targets --no-fail-fast` | 2061 passed / 0 failed (2055 before this change, plus 6 new tests) |
| `cargo test -p terraphim_agent --test adf_config_guard` | 3 passed |
| `cargo test -p terraphim_agent --test learn_install_hook_contract` | 2 passed |
| `adf --local --check` | the configuration loads |

## Validation

| Finding (issue #517) | Result |
|---|---|
| P1 - the model is not in pi's registry and the orchestrator bans the prefix, so the agent cannot start | PASS - `zai-coding-plan/glm-5.1` passes the orchestrator's allow-list and resolves in pi; `adf --local --check` loads the project |
| P1 - `fallback_model` is unregistered and the fallback CLI path does not exist | PASS - `kimi-for-coding/k2p6` with the mise shim path; the path exists and runs (opencode v2.3.0) |
| P1 - `install-hook pi` exits 0 without installing anything | PASS - it exits 1, says that nothing was installed, prints the guidance, and writes nothing |
| P2 - no config-level guard catches an unregistered or disallowed model id | PASS - `tests/adf_config_guard.rs` fails on a prefix that is not allow-listed, on a relative fallback path, on a fallback path that does not exist, and on the orchestrator's own verdict when it is on the host |

## Limits

- The provider allow-list is quoted from the deployed orchestrator's message
  into the guard, so the two can drift. The guard also asks the orchestrator
  itself whenever the host has the binary, which is the authoritative check;
  the quoted list is the portable fallback.
- The `adf` binary and pi's model registry are not present on the CI runner,
  so the host-dependent arms of the guard skip there with a printed notice.
  The prefix arm - the one that catches the defect that was merged - always
  runs.
- The fallback's *model id* (`kimi-for-coding/k2p6`) belongs to opencode's
  registry, not pi's, because the fallback CLI is opencode. It is the id the
  sibling fleet repositories use; this repository cannot check it against
  opencode's registry from a test.
- The pi extension is still installed by hand. The alternative - the command
  running `pi install` itself - is a different design decision (it spawns
  another tool and needs the package's path); the issue's first option, an
  honest outcome, is what this change implements.
- The `digital-twins` fleet gate asserts that agents must not carry a
  per-agent provider fallback at all, while the sibling repositories keep
  one. This change keeps the fleet's shape and points it at a path that
  exists; removing the fallback is a fleet-wide decision, not one
  repository's.
