# Research Document: Documented Homebrew command names a formula that does not exist

**Status**: Draft
**Author**: Release orchestrator (autonomous session)
**Date**: 2026-09-27
**Reviewers**: pending
**Phase**: 1 (disciplined-research)

## Executive Summary

terraphim.ai documents `brew tap terraphim/terraphim && brew install terraphim-ai`. No repository named `terraphim/homebrew-terraphim` formula called `terraphim-ai` exists, and the tap repository itself is `terraphim/homebrew-terraphim`, which the command's `terraphim/terraphim` tap shorthand would not resolve to. The tap does contain correct, checksum-verified formulae for `terraphim-agent` and `terraphim-grep` at 1.21.16. So the tap is healthy and the documented command is wrong: the user-facing defect is one line of copy plus a tap-name mismatch.

## Essential Questions Check

| Question | Answer | Evidence |
|----------|--------|----------|
| Energizing? | Yes | It is a documented command that cannot succeed |
| Leverages strengths? | Yes | The formulae already exist and verify against the manifest |
| Meets real need? | Yes | Homebrew is a primary macOS and Linuxbrew path |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

The site documents `brew tap terraphim/terraphim && brew install terraphim-ai`. Reality, verified 2026-09-27:

- Tap repository: `terraphim/homebrew-terraphim` (tap shorthand `terraphim/terraphim`).
- Formulae present: `terraphim-agent.rb`, `terraphim-grep.rb`, `terraphim-server.rb`.
- No formula named `terraphim-ai`.
- `terraphim-agent.rb` and `terraphim-grep.rb` are at 1.21.16, with checksums matching the published manifest for `universal-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-musl`.

### Impact

A user following the documented Homebrew command fails at the second step (`brew install terraphim-ai`: no such formula). If the tap shorthand is also wrong, the failure happens at the first step. Either way the documented path is broken while a working one exists under a different name.

### Success Criteria

1. The documented Homebrew command succeeds verbatim and installs the current version.
2. The documented tap name resolves to the real tap.
3. The tap carries the current release for every binary the site advertises.

## Current State Analysis

### Existing Implementation

- Tap `terraphim/homebrew-terraphim`: `terraphim-agent.rb` and `terraphim-grep.rb` at v1.21.16; `terraphim-server.rb` pins terraphim-ai v1.20.5 assets.
- `terraphim-agent.rb` selects `universal-apple-darwin` on macOS and musl/gnu on Linux, verifies SHA-256, has a `test do` block that runs `--version`, `learn --help`, `memory --help` and `sessions expand --help`, and on macOS runs `codesign --verify --all-architectures --deep --strict`.
- The tap's latest commit is a merge of PR #3 on 2026-09-25 ("Bump terraphim-agent and terraphim-grep to v1.21.16").

### Code Locations

| Component | Location | Purpose |
|---|---|---|
| Agent formula | `terraphim/homebrew-terraphim` `Formula/terraphim-agent.rb` | Installs terraphim-agent 1.21.16 |
| Grep formula | `terraphim/homebrew-terraphim` `Formula/terraphim-grep.rb` | Installs terraphim-grep 1.21.16 |
| Server formula | `terraphim/homebrew-terraphim` `Formula/terraphim-server.rb` | Pins terraphim-ai v1.20.5 |
| Site copy | terraphim.ai install section | Documents `brew install terraphim-ai` |

### Data Flow

Site command -> tap resolution -> formula -> `downloads.terraphim.ai` archive (with a GitHub release mirror) -> SHA-256 check -> `bin.install`.

### Integration Points

- Homebrew tap naming convention: repository `homebrew-<name>` maps to tap `<owner>/<name>`.
- The formulae reference both the R2 channel and the GitHub release as a mirror, so both must stay valid.

## Constraints

### Technical Constraints

- Homebrew requires the repository to be named `homebrew-<tap>` for `brew tap` to resolve the shorthand.
- Formulae must be valid Ruby and pass `brew audit` where practical.
- A macOS-specific codesign test only executes on macOS; Linuxbrew skips it.

### Business Constraints

- The tap is public; a formula that fails `brew install` is user-visible immediately.
- The site copy may live in a repository separate from this project.

### Non-Functional Requirements

| Requirement | Target | Current |
|---|---|---|
| Documented command succeeds | yes | no |
| Formula version | 1.21.16 | 1.21.16 for agent and grep |
| Checksum parity with manifest | exact | exact (verified) |

## Vital Few (Essential Constraints)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| Documented command must resolve | A command that cannot run is worse than no instruction | No `terraphim-ai` formula exists |
| Formulae must match the manifest | Guarantees the tap cannot serve mismatched bytes | Checksums match today |
| Tap name must be the real one | Otherwise the first command fails | Repository is `homebrew-terraphim` |

### Eliminated from Scope

| Eliminated Item | Why Eliminated |
|---|---|
| Creating a `terraphim-ai` formula | Inventing a formula to satisfy wrong copy |
| Renaming the tap | Breaks existing users and the merge history |
| Publishing to homebrew-core | Out of vital few; needs notability and review |
| Rewriting the formula structure | It already verifies and tests correctly |

## Dependencies

### Internal Dependencies

| Dependency | Impact | Risk |
|---|---|---|
| Release promotion to R2 and GitHub | Formulae point at both | Low |

### External Dependencies

| Dependency | Version | Risk | Alternative |
|---|---|---|---|
| Homebrew | current | Formula DSL changes | pin the test to documented interfaces |
| GitHub releases | v3 | Asset renames break the mirror | R2 primary, already present |

## Risks and Unknowns

### Known Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Copy fixed in one place and left stale elsewhere | High | Medium | Single checklist item in the release acceptance run |
| Formula fails on real macOS | Unknown | High | This is the separate macOS evidence item |
| `terraphim-server.rb` pins an old terraphim-ai release | Certain | Low | Out of scope for the clients release; note it |

### Open Questions

1. Is the site's Homebrew line meant to install the agent, the cli, or a bundle? (Owner: release owner)
2. Should `terraphim-cli` gain a formula, since the site lists `cargo install terraphim-cli`? (Owner: release owner)
3. Where is the site source? (Owner: release owner)

### Assumptions Explicitly Stated

| Assumption | Basis | Risk if Wrong | Verified? |
|---|---|---|---|
| The site intends the clients binaries | It also lists cargo installs for them | Wrong copy fixed | Partially |
| Tap shorthand in the copy is wrong | Repository naming convention | Unnecessary change | Yes, by naming rule |
| Checksums will keep matching | They match today and promotion writes both | Silent mismatch | Yes, today |

### Multiple Interpretations Considered

| Interpretation | Implications | Why Chosen/Rejected |
|---|---|---|
| Fix the copy to the real formula names | Smallest, honest change | Chosen |
| Add a `terraphim-ai` formula | Satisfies wrong copy, invents a product name | Rejected |
| Point the copy at the installer instead | Removes Homebrew entirely | Rejected; Homebrew is a real, working channel |

## Research Findings

### Key Insights

1. The tap is correct and verified; only the documented command is wrong.
2. The defect is copy plus a tap-name mismatch, not formula work.
3. The tap's own test block already provides install-time validation, including codesign on macOS.
4. An inconsistency exists between channels: the site advertises cli, but the tap has no cli formula.

### Relevant Prior Art

- Tap PR #3 (merged 2026-09-25) demonstrates the bump procedure for this release.
- The formula mirrors the R2 primary with a GitHub fallback, matching the plan of record for distribution.

### Technical Spikes Needed

| Spike | Purpose | Estimated Effort |
|-------|---------|------------------|
| `brew install` on Linuxbrew for both formulae | First real install evidence on a Linux host | 1 hour |
| `brew audit` on the two formulae | Catch formula smells before macOS evidence | 30 minutes |

## Recommendations

### Proceed/No-Proceed

Proceed. It is a small, low-risk fix with immediate user benefit.

### Scope Recommendations

Correct the documented command to the real tap and real formula names, decide whether cli should also have a formula, and record the `terraphim-server.rb` staleness as a separate observation.

### Risk Mitigation Recommendations

Add the Homebrew command to the public-release acceptance run so copy and formulae cannot drift apart again.

## Next Steps

If approved:
1. Phase 2 design: exact copy change, optional cli formula, and the validation step.
2. Confirm the intended set of Homebrew-installable binaries.

## Appendix

### Evidence Captured 2026-09-27

- `Formula/` contents: `terraphim-agent.rb`, `terraphim-grep.rb`, `terraphim-server.rb`.
- `terraphim-agent.rb` url `https://downloads.terraphim.ai/terraphim-agent/terraphim-agent-1.21.16-#{target}.tar.gz` with mirror to the GitHub release and SHA-256 `on_system_conditional`.
- Tap checksums match the manifest for `universal-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-musl` for both agent and grep.
- Site copy verbatim: `brew tap terraphim/terraphim && brew install terraphim-ai`.