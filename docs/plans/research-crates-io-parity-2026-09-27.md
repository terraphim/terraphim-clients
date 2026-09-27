# Research Document: `cargo install terraphim-agent` serves crates.io binaries 13 patch releases behind

**Status**: Draft
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Reviewers**: pending
**Phase**: 1 (disciplined-research)

## Executive Summary

terraphim.ai's headline install command is `cargo install terraphim-agent`, but crates.io serves 1.21.1 (agent and cli) and 1.21.2 (grep) while the release channel, the GitHub release and the Homebrew tap are all at 1.21.16. The gap is not an oversight in the release pipeline: the crates depend on `terraphim_config`, `terraphim_persistence`, `terraphim_settings`, `terraphim_router`, `terraphim_tracker` and `terraphim-markdown-parser` at versions that exist only on the private Gitea registry, and `publish-crates.yml` additionally refuses `terraphim_agent` outright because its dependency graph resolves only against that registry. Publishing the current binaries to crates.io therefore requires publishing the whole dependency family to crates.io first, which is a separate, larger programme.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | It is the first command a user is told to run |
| Leverages strengths? | Partially | The publish machinery exists, but the dependency family is the blocker, not the pipeline |
| Meets real need? | Yes | A 13-patch-behind install is a real user-visible defect |

**Proceed**: Yes (2/3). Note the second answer is the honest weak point: the fix is dependency work, not release work.

## Problem Statement

### Description

Three documented install paths disagree about the current version:

| Path | Version served | Verified |
|---|---|---|
| crates.io: `cargo install terraphim-agent` | 1.21.1 (2026-08-10) | Yes |
| crates.io: `cargo install terraphim-cli` | 1.21.1 (2026-08-10) | Yes |
| crates.io: `cargo install terraphim-grep` | 1.21.2 (2026-08-12) | Yes |
| R2 channel `downloads.terraphim.ai` | 1.21.16 | Yes, all 20 archives hash-verified |
| Homebrew tap | 1.21.16 | Yes |
| GitHub release v1.21.16 | 1.21.16, 21 assets | Yes |

### Impact

A user who prefers cargo - the most cargo-culted path for a Rust tool - gets code four weeks and thirteen patches old, including fixes the release notes advertise. Users then report issues already fixed, and the visible version is inconsistent with every other channel.

### Success Criteria

1. Every documented install path either delivers 1.21.16 or states plainly which version it delivers and why.
2. No documented path silently delivers an older line without an explicit, user-visible note.
3. If crates.io is to carry the binaries, the family publish is proven for one release, reproducibly, by automation.

## Current State Analysis

### Existing Implementation

- `publish-crates.yml` is `workflow_dispatch` only (no tag trigger). It refuses `terraphim_agent` with the message "terraphim_agent is private-registry-only: its install graph (terraphim_sessions >= 1.21.2 with cursor-connector) resolves only against the terraphim registry, so a crates.io publish would ship an uninstallable package (#95)".
- It then strips `registry = "terraphim"` refs from every manifest and publishes the requested crates in dependency order with `CARGO_REGISTRY_TOKEN`, skipping versions that already exist.
- `Cargo.toml` carries an explicit comment block explaining that `terraphim_config`, `terraphim_persistence`, `terraphim_settings`, `terraphim_router`, `terraphim_tracker` and `terraphim-markdown-parser` are Gitea-only, and that crates.io 1.20.4 copies of the last four drag in a second `terraphim_config`/`terraphim_types` graph producing "expected ConfigState, found ConfigState" errors. Multi-repo publish tracking is `terraphim/terraphim-core #71`; `Refs #112`.

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| Publish workflow | `.github/workflows/publish-crates.yml` | Manual, dependency-ordered crates.io publish with a refusal guard |
| Registry pins | `Cargo.toml` lines 55-85 | Which deps come from the private registry and why |
| Workspace version | `Cargo.toml` `[workspace.package] version = "1.21.16"` | Single version for the family |
| Prior research | `crates/terraphim_grep/RELEASE_RESEARCH.md`, `RELEASE_DESIGN.md` | Earlier analysis of crates.io publishing for grep |

### Data Flow

`cargo install terraphim-agent` -> crates.io index -> terraphim-agent 1.21.1 -> transitive deps from crates.io. The published 1.21.1 graph resolved at publish time, which is why it exists at all; the later pins to Gitea-only versions are what stop a repeat.

### Integration Points

- crates.io API and index (`https://crates.io/api/v1/crates/<crate>` returns `newest_version`; 403s without a User-Agent from datacenter IPs).
- Private Gitea registry, which continues to host the family (out of scope here).

## Constraints

### Technical Constraints

- `cargo publish` rejects manifests whose dependencies pin a non-default registry.
- The dependency family must be present on crates.io at compatible versions before any binary crate can be published there.
- crates.io 1.20.4 copies of four family members are incompatible with this tree; versions must be chosen deliberately, not by "latest".
- Publishing is irreversible: a version, once published, cannot be re-published or deleted (only yanked).

### Business Constraints

- crates.io releases are permanent and public; a mistake is visible to every `cargo install` user.
- The multi-repo publish work is already tracked (`terraphim-core #71`); duplicating it here would create conflicting plans.

### Non-Functional Requirements

| Requirement | Target | Current |
|-------------|--------|---------|
| Documented channel accuracy | version served matches release notes | 1.21.1 vs 1.21.16 |
| Publish reproducibility | one automated path per release | manual, family incomplete |

## Vital Few (Essentialism)

### Essential Constraints (Max 3)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| The user-visible link must state the true version | A wrong version claim destroys trust in every channel | `cargo install` is advertised first on the site |
| Publishing to crates.io cannot ship an uninstallable crate | An uninstallable package is worse than a stale one | The #95 refusal guard exists for this reason |
| Choosing a channel policy must be explicit | The current state is an undocumented accident | No note anywhere says crates.io lags |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|-----------------|----------------|
| Publishing the whole dependency family to crates.io in this pass | It is `terraphim-core #71`; a multi-repo, irreversible programme |
| Removing `registry = "terraphim"` pins from the tree | It changes how the product builds and needs its own research |
| Publishing a 1.21.x bump of the stale crates without the family | It would be refused by the guard or ship an unresolvable graph |
| Yanking the stale crates.io versions | Breaking existing pinned users without a migration path |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|---|---|---|
| `publish-crates.yml` | The only publish path; its refusal guard is a safety net, not a solution | Low |
| `Cargo.toml` registry pins | Determine whether a crates.io publish is even resolvable | High |
| `terraphim-core #71` | Owns the real family publish | High, external to this repo |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|---|---|---|---|
| crates.io | live | Irreversible publishes; rate limits | none |
| `CARGO_REGISTRY_TOKEN` secret | present | Missing or expired blocks publish | 1Password | 

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| User publishes a broken crate while aiming for parity | Medium | High | Keep the #95 guard; dry-run first; family-first ordering |
| Documentation note drifts from reality later | High | Medium | Derive the note from the published version at build time where possible |
| Duplicating `terraphim-core #71` work | Medium | Medium | Reference the issue; publish nothing while it is open |

### Open Questions

1. Which option for #333? (a) publish family plus binaries, tracked under `terraphim-core #71`; (b) keep crates.io deliberately lagging and document it; (c) keep lagging and steer cargo users to the installer.
2. If (b) or (c): should the stale crates carry a README note, a site footnote, or both?
3. Should `terraphim-grep`, whose dependency graph is simpler, be published first as a proof? (Owner: release owner)

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|---|---|---|---|
| The `terraphim_agent` refusal is still correct | In-tree guard with a specific rationale | Publishing would ship an uninstallable crate | No, not re-tested |
| crates.io 1.21.1/1.21.2 installs still resolve | They are published and older | Less relevant | No |
| The family publish is genuinely multi-repo | `terraphim-core #71` reference | Effort estimate wrong | Partially |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|---|---|---|
| "Publish everything to crates.io" | Correct long term, large, cross-repo, irreversible | Deferred to `terraphim-core #71` |
| "Document crates.io as intentionally lagging" | Cheap, honest, no irreversible action | Viable candidate for this release |
| "Steer cargo users to the installer" | Fixes the user outcome without crates.io work | Viable candidate, composes with the installer fix |

## Research Findings

### Key Insights

1. The blocking dependency is the family, not the publish pipeline.
2. Exactly one crate is structurally refused (`terraphim_agent`); the others are blocked only by resolver reality.
3. Four family members exist on crates.io at 1.20.4 but are known-incompatible with this tree, so "publish and let the resolver sort it out" is unsafe.
4. A cheap, honest fix (document the lag and steer users) delivers most of the user benefit with none of the irreversibility.

### Relevant Prior Art

- `crates/terraphim_grep/RELEASE_RESEARCH.md` reached the same conclusion earlier: publishing grep requires the family first.
- The existing #95 guard demonstrates the project already decided that shipping an unresolvable crate is unacceptable.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| Dry-run publish of terraphim-grep only | Establish how far the resolver gets before the family blocks it | 2 hours |
| Inventory of family versions on crates.io vs required | Size the family publish | 1 hour |

## Recommendations

### Proceed/No-Proceed

Proceed, with scope limited to accuracy of the documented channel. Do not attempt the family publish in this pass.

### Scope Recommendations

Preferred for this release: correct the documented statement of what `cargo install` delivers, and point cargo users at the self-update path, which already works. Keep the family publish as a tracked, separately-owned programme.

### Risk Mitigation Recommendations

Keep the #95 guard. Use `--dry-run` before any real publish. Never publish a version that cannot be reproduced from a tag.

## Next Steps

If approved:
1. Phase 2 design: how the site and crate READMEs state the channel situation, and how the cargo path is steered.
2. Confirm the policy choice with the release owner (option a, b or c).
3. If the policy is (b) or (c), implement the documentation change and validate it as part of the public-release acceptance run.

## Appendix

### Evidence Captured 2026-09-27

- crates.io `newest_version`: terraphim-agent 1.21.1 (2026-08-10), terraphim-grep 1.21.2 (2026-08-12), terraphim-cli 1.21.1 (2026-08-10).
- Family on crates.io: terraphim_types 1.22.1, terraphim_automata 1.21.1, terraphim_service 1.20.6, terraphim_config 1.20.4, terraphim_persistence 1.20.4, terraphim_settings 1.20.4, terraphim_router 1.20.4, terraphim_tracker 1.20.4, terraphim_file_search 1.20.3, terraphim_middleware 1.20.3, terraphim_rolegraph 1.20.4, terraphim_orchestrator 1.20.2, terraphim-markdown-parser 1.20.4.
- `Cargo.toml` rationale block lines 55-85, including the exact-pin comment for `terraphim_config` and `terraphim_persistence` (1.20.4 yanked on Gitea; crates.io 1.20.4 copies cause duplicate-graph type errors).
- `publish-crates.yml` refusal message and dependency-ordered publish loop.
---

## Addendum (2026-09-27): the family publish was attempted against live infrastructure

Decision 2 authorised publishing the dependency family with the existing
scripts. The publish path was executed and the blocker is now measured rather
than inferred.

### Path taken

`terraphim-ai/scripts/adf-setup/polyrepo-publish/polyrepo-publish.sh` is the
family publisher. It names one clone of `terraphim-clients` on Gitea and walks
it through Gitea CI, a registry-stripped rewrite, a GitHub mirror push, GitHub
CI, and a crates.io dispatch. It is a CI-orchestration harness rather than an
idempotent publish command, and `dispatch` covers six repositories in
topological order.

That harness duplicates a mechanism `terraphim-clients` already carries: the
repository's own `publish-crates.yml` accepts a `crate_list` and strips the
private-registry pins itself. The canonical `main` at `fb1a575` also already
contains the released `v1.21.16` tag, so the Gitea-CI gate the harness exists
to provide has already run. Dispatching the repository's workflow directly is
therefore the equivalent operation on the already-validated tree, and it is
the path that was used.

### Result

A dry run of the full client family on `main` (run 36314289075) reached the
first crate that must be built from a crates.io-resolved graph and failed:

```
error[E0432]: unresolved import `terraphim_automata::parse_markdown_directives_dir`
 --> terraphim_config-1.20.4/src/lib.rs:24:21
  note: the item is gated behind the `fs-traversal` feature
error: could not compile `terraphim_config` (lib)
error: failed to verify package tarball
```

The complete causal chain, now verified end to end:

1. `cargo install` on any client crate resolves against crates.io.
2. crates.io carries `terraphim_config` and `terraphim_persistence` at 1.20.4,
   while the client crates require 1.20.2 (pinned exactly, because 1.20.4 is
   yanked on the Gitea registry).
3. crates.io's `terraphim_automata` is 1.21.1, which gates
   `parse_markdown_directives_dir` behind `fs-traversal`, a feature
   `terraphim_config` 1.20.4 does not enable.
4. The build of `terraphim_config` 1.20.4 therefore fails, and no client crate
   can be published until that is fixed.

This is exactly the failure the `[patch.crates-io]` block in the clients
`Cargo.toml` documents, reached through the publish pipeline itself.

### What this means for the options

| Option | Verdict |
|---|---|
| Publish the client crates now | Blocked: no client crate is publishable while `terraphim_config` 1.20.4 fails to build |
| Publish the family from `terraphim-config-persistence` and `terraphim-core` first | Required, and a larger programme: it spans two other repositories, and each must first carry a green Gitea CI on its own `main` |
| Remove `[patch.crates-io]` and the registry pins in `terraphim-clients` | Not available until the family is on crates.io at matching versions; doing it first breaks the build |

The dry run also established two smaller facts worth keeping:

- `terraphim_update` 1.20.2 and `terraphim_command_runtime` 0.1.0 already exist
  on crates.io, so those two are no-ops in any future `crate_list`.
- `publish-crates.yml` correctly refuses `terraphim_agent` before any manifest
  mutation (issue #95); that guard worked as designed.

### Consequence for this release

`cargo install terraphim_agent` cannot be made to serve 1.21.16 by publishing
from `terraphim-clients` alone. The two paths that can serve 1.21.16 today are
the channel installer and Homebrew, and the website was updated to lead with
those and to state plainly what the Cargo path does. Completing the Cargo path
is a cross-repository dependency programme, not a step in this release.
