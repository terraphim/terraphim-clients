# Research Document: the ADF model ids and `install-hook pi` (#517)

**Issue**: #517
**Status**: Draft
**Date**: 2026-11-14
**Author**: terraphim agent

## Executive Summary

Two integrations report success without working, and both live in this
repository. The repo-local ADF agent configuration points at
`terraphim-proxy/fastest`, a provider prefix the orchestrator's allow-list
rejects outright, so **the fleet config does not load at all** - reproduced
here with the deployed orchestrator. And `terraphim-agent learn install-hook
pi` prints the installation guidance and exits 0, so a scripted install
concludes the hook is installed when nothing was installed.

Both are fixable in this repository, and both are verifiable with tools that
are already on this host: the deployed orchestrator (`adf --local --check`)
and pi's own client-side model registry.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energising? | Yes | The configuration failure stops an autonomous agent from running at all; the silent install success misleads every caller. |
| Leverages strengths? | Yes | Both are fixed and checked with the real tools - the deployed orchestrator and the real binary - not with a stand-in. |
| Meets real need? | Yes | #517 is an open issue with a verified reproduction from a post-merge review of 28 pull requests. |

**Proceed**: Yes (3/3 YES).

## Problem Statement

### Description

1. `.terraphim/adf.toml` names `terraphim-proxy/fastest` as the
   `implementation-swarm` agent's model and `terraphim-proxy/auto` as its
   fallback model. Neither prefix is on the orchestrator's allow-list, so the
   config fails to load; and neither model id is in pi's client-side registry,
   so the agent could not start even if the config loaded.
2. The fallback CLI path (`/usr/local/bin/opencode`) does not exist on this
   host.
3. `install_hook(AgentType::Pi)` prints the guidance and returns `Ok(())`, so
   `terraphim-agent learn install-hook pi` exits 0 without installing
   anything.

### Impact

- The autonomous implementer for this repository cannot start: the fleet
  config is rejected at load time, so *every* agent in the fleet that loads
  this project is affected, not only this one.
- A user or a script running `learn install-hook pi` sees exit code 0 and
  concludes the hook is installed. Nothing was installed.

### Success Criteria

- `adf --local --check` loads this project's config without a validation
  failure.
- Every agent model in the config resolves against pi's registry and passes
  the orchestrator's allow-list, and a guard keeps it that way.
- `learn install-hook pi` exits non-zero, prints the guidance, and says that
  a manual step is required.

## Current State Analysis

### Existing Implementation

| Component | Location | Purpose |
|---|---|---|
| The repo-local fleet config | `.terraphim/adf.toml` | Declares the six agents this repository contributes to the fleet. |
| The orchestrator's validation | the deployed `adf` binary (`/home/alex/.local/bin/adf`) | Checks every agent's `model` and `fallback_model` against `ALLOWED_PROVIDER_PREFIXES`. |
| pi's client-side model registry | `~/.pi/agent/models.json` | The models pi can resolve, by provider. |
| The hook installer | `crates/terraphim_agent/src/learnings/install.rs` | `install_hook(agent)` writes a shell hook, or prints the pi extension guidance. |
| The learn command | `crates/terraphim_agent/src/learn_command.rs` | `LearnSub::InstallHook` calls the installer and maps its error into the command's result. |

### Reproduction

The orchestrator rejects the config as it stands:

```text
$ adf --local --check
Discovered terraphim-clients: 6 agents from .../.terraphim/adf.toml
adf --local --check FAILED validation: banned LLM provider 'terraphim-proxy/fastest' in model for agent 'implementation-swarm' (allowed: claude-code, opencode-go, kimi-for-coding, minimax-coding-plan, zai-coding-plan)
```

and pi rejects the model id itself:

```text
$ pi-rust --provider terraphim-proxy --model fastest -p "say OK"
Model terraphim-proxy/fastest not found
```

pi's registry has exactly nine ids under `terraphim-proxy` - `glm-5.2`,
`zai:glm-5.3-flash`, `zai:glm-5.3`, `kimi-for-coding-highspeed`, `kimi:k3`,
`review`, `bailian:deepseek-v4-pro`, `bailian:qwen3.8-max`,
`bailian:qwen3.8-flash` - and no `fastest` and no `auto`.

The fallback path does not exist either:

```text
$ ls /usr/local/bin/opencode        -> No such file or directory
$ ls /home/alex/.bun/bin/opencode    -> No such file or directory
$ which opencode                     -> /home/alex/.local/share/mise/shims/opencode
```

### What the rest of the fleet does

Two sibling fleet repositories (`terraphim-agents`, `tc-rc`) carry the pattern
that the deployed orchestrator accepts:

```toml
model = "zai-coding-plan/glm-5.1"
fallback_provider = "/home/alex/.bun/bin/opencode"
fallback_model = "kimi-for-coding/k2p6"
```

and `adf --local --check` accepts those configurations (it prints the fleet
and exits cleanly). `zai-coding-plan` is on the orchestrator's allow-list, and
pi resolves `zai-coding-plan/glm-5.1` against its registry - the probe reaches
the credential stage ("No API key found for provider zai-coding-plan"), which
is a different failure from "Model not found" and means the id itself is
valid.

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| The Pi branch of the installer | `crates/terraphim_agent/src/learnings/install.rs:235` | Prints `pi_install_guidance()` and returns `Ok(())`. |
| The install errors | `crates/terraphim_agent/src/learnings/install.rs:191` | `InstallError` - the enum the distinct outcome has to be added to. |
| The learn command's mapping | `crates/terraphim_agent/src/learn_command.rs:215` | `learnings::install_hook(agent).await.map_err(\|e\| e.into())`. |

### Integration Points

- The orchestrator reads `.terraphim/adf.toml` at fleet load and validates it.
- pi resolves `provider/model` against `~/.pi/agent/models.json` and its
  built-in subscription providers.
- `learn install-hook` is a human-facing CLI command; its exit code is the
  only signal a script sees.

## Constraints

### Technical Constraints

- The orchestrator's allow-list lives in the orchestrator's own source (the
  `terraphim-ai` repository), so this repository cannot extend it. The only
  shapes this repository can use are the prefixes already on the list.
- pi's model registry lives in the user's home directory, so a *model id*
  check is host-dependent; the *prefix* check is not.
- The exit-code taxonomy is shared with robot mode and is owned by Gitea
  #181, which lives outside this repository: this change must not mint a new
  exit code.
- The `adf` binary and pi's registry are not present on the CI runner, so a
  test that uses them has to skip with a notice rather than fail.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| The fleet config loads | `adf --local --check` reports no validation failure | FAILED - the prefix is rejected |
| The agent's model resolves in pi | pi resolves `provider/model` | FAILED - "Model terraphim-proxy/fastest not found" |
| The fallback CLI path exists | the path is a file on this host | FAILED - the path does not exist |
| `install-hook pi` reports honestly | a non-zero exit and a clear message | FAILED - exit 0, nothing installed |

## Vital Few (Essential Constraints)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|------------|----------------|----------|
| The model prefix must be on the orchestrator's allow-list | The config does not load otherwise, and the orchestrator cannot be changed from here | The reproduced `adf --local --check` failure |
| The install must not report success when it installed nothing | A script's only signal is the exit code | The issue's verified reproduction, and the code path in `install.rs` |
| A guard must exist | The defect was introduced by a merged pull request and nothing caught it | The issue's P2 finding: no config-level guard |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Adding `terraphim-proxy` to the orchestrator's allow-list | The orchestrator is a different repository and a deployed binary; the allow-listed shape works without it |
| Implementing the pi extension install (`pi install ...`) | A different design decision (the agent would spawn another tool); the issue asks for an honest outcome, and the guidance already documents the manual step |
| A live pi model call in CI | A model call in a test lane is not acceptable; the *prefix* check is portable and the id is verified by the probe recorded in this document |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|------------|--------|------|
| `.terraphim/adf.toml` | The agent declarations | Low |
| `crates/terraphim_agent/src/learnings/install.rs` | The install contract | Low |
| `crates/terraphim_agent/src/learn_command.rs` | The exit code | Low |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|------------|---------|------|-------------|
| `toml` (dev-dependency, for the config guard) | 0.8 (already in the lock file) | None - no lock refresh needed | A hand-rolled line scan |
| The deployed `adf` binary | host tool | The CI runner has none | Skip with a notice |
| pi's model registry | host file | The CI runner has none | Skip with a notice |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| The chosen model id is valid today but is retired later | Medium | Medium | The guard checks the prefix; pi's own registry check happens at run time and fails loudly |
| The fallback path is host-specific | High | Low | The guard asserts the path exists when the host has opencode, and the fleet's own repos use the same shape |
| A test that depends on host tools is flaky | Medium | Low | The host-dependent arms skip with a printed notice, and the portable arms always run |

### Open Questions

1. Should the fallback exist at all? The `digital-twins` fleet gate asserts
   that agents must not carry a per-agent fallback ("the proxy owns provider
   fallback"), while the sibling repositories keep one. This change keeps the
   fleet's shape and points it at a path that exists; the decision belongs to
   the fleet, not to one repository.
2. Does the orchestrator validate the fallback's model id against the *fallback
   CLI's* registry? It validates the prefix; the id `kimi-for-coding/k2p6` is
   opencode's, and it is the id the sibling repositories use.

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|------------|-------|---------------|-----------|
| The allow-listed prefixes are stable | The orchestrator's message enumerates them | The guard's list drifts and rejects a valid config | Yes - the deployed binary's message |
| `zai-coding-plan/glm-5.1` is the fleet's working id | Two sibling fleet repositories use it and the orchestrator accepts their configs | The agent resolves but the model is unavailable | Yes - pi resolves the id (it reaches the credential stage) |
| The mise shim is a usable opencode path | `opencode --version` through the shim prints v2.3.0 | The fallback cannot start | Yes - the shim ran |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|----------------|--------------|---------------------|
| Add `terraphim-proxy` to the orchestrator's allow-list | Keeps the proxy as the agent's provider | Rejected - the orchestrator is another repository and a deployed binary; the allow-listed shape needs no redeploy |
| Keep the proxy prefix and drop the agent's model | The agent has no model | Rejected - the agent cannot run |
| Make `install-hook pi` run `pi install` itself | The command installs what it claims | Deferred - it spawns another tool and needs the package's path; the issue's first option (an honest outcome) is smaller and safer |
| Mint a new exit code for "manual action required" | Automation gets a precise signal | Rejected - the taxonomy is owned by Gitea #181, outside this repository |

## Research Findings

### Key Insights

1. The failure is **at config load**, not at agent start: one bad prefix fails
   the whole fleet's load of this project.
2. The orchestrator's allow-list and pi's registry are two *different* lists;
   a valid id has to satisfy both.
3. The sibling fleet repositories carry the accepted shape, and the deployed
   orchestrator accepts their configurations - so the fix is a known-good
   configuration, not a new invention.
4. `install_hook` has exactly one caller, so the contract change is local.
5. A distinct error variant plus a non-zero exit is enough for automation;
   the exit-code taxonomy belongs to another repository.

### Relevant Prior Art

- The fleet's `digital-twins` `adf_proxy_routing.rs` gate: agents must not
  carry a per-agent provider fallback.
- `tests/skip_without_server`: the repository's pattern for a host-dependent
  test arm that skips with a notice rather than failing.

### Technical Spikes Needed

None. The behaviour is reproduced with tools already on this host.

## Recommendations

### Proceed/No-Proceed

Proceed. Both halves are small, both are verifiable with real tools, and the
guard that keeps them fixed is the durable part.

### Scope Recommendations

- `.terraphim/adf.toml`: the allow-listed model, the registered fallback
  model, and a fallback CLI path that exists on this host.
- `install.rs`: a distinct `InstallError::ManualInstallRequired`, returned by
  the Pi branch after the guidance is printed.
- A new guard test for the fleet configuration: the allow-listed prefixes, an
  absolute fallback path, and - when the host has the tools - the deployed
  orchestrator's own verdict.

### Risk Mitigation Recommendations

- Keep the portable arms of the guard (the prefix list) unconditional, and the
  host-dependent arms (the orchestrator binary, pi's registry) skippable with
  a printed notice.
- Publish the before/after evidence on the issue: the reproduced validation
  failure and the accepted configuration.

## Next Steps

1. Write the design document.
2. Implement the configuration fix, the install-outcome fix and the guard.
3. Verify: the workspace suite, clippy, fmt, `adf --local --check`, the pi
   probes, and the CLI contract test.
4. Publish the numbers and the evidence on the pull request.

## Appendix

### Reference Materials

- Issue #517 and the report it quotes (`.docs/review-merged-prs-2026-11-12.md`).
- The deployed orchestrator's validation message (quoted above).
- `~/.pi/agent/models.json` (the nine `terraphim-proxy` model ids).
