# Research Document: Release readiness for terraphim-grep, terraphim-agent, terraphim-hooks

**Status**: Draft — awaiting approval
**Author**: Claude (disciplined-research, Phase 1)
**Date**: 2026-08-28
**Repo**: `terraphim/terraphim-clients`
**Supersedes**: the ad-hoc conclusions at the end of `session_1.md` (Grok session, 2026-08-28)

## Executive Summary

The four blockers carried over from the previous session do not survive verification. Two are
artefacts of a stale local checkout (local `main` is **31 commits behind** `gitea/main`, where the
dependency conflict is already fixed), one is simply wrong (`terraphim_grep` **is** published to
crates.io at 1.21.2, **with** the `code-search` default fix), and one is miscategorised (`grep` and
`agent` are workspace members of `terraphim-clients`, which already has a Gitea repo).

The real problem is structural and was not named in the previous session: **`terraphim-clients` and
the published `terraphim_*` 1.21.x artefacts live in two mutually incompatible API worlds.**
crates.io tops out at `terraphim_automata` **1.20.4** (owned `Thesaurus`); the borrowed `&Thesaurus`
API exists **only** on the Gitea registry as 1.21.0. `terraphim-clients` builds against the former;
the published `terraphim_agent` 1.21.2/1.21.3 and `terraphim_hooks` 1.21.0 were built against the
latter, elsewhere. No amount of version bumping in `terraphim-clients` reproduces those artefacts.

## Essential Questions Check

| Question | Answer | Evidence |
|---|---|---|
| Energizing? | Yes | Unblocks the ADF fleet reliability WIG; `terraphim-clients` is fleet infrastructure |
| Leverages strengths? | Yes | Cargo/registry provenance forensics; the `terraphim-ai` side already solved this exact migration |
| Meets real need? | Yes | Published crates currently have no reproducible source; a duplicate source of truth for `hooks` was created yesterday |

**Proceed**: Yes (3/3).

## Problem Statement

### Description

Three crates were assessed as "release ready?" and all three answered no, for reasons that turn out
to be mostly wrong. The genuine problem is that `terraphim-clients` has ceased to be the source of
truth for the crates it nominally owns:

- `terraphim_agent` 1.21.2 and 1.21.3 were published from commits that are not reachable from
  `terraphim-clients/main` (one is orphaned by a force-reset; one does not exist in the repo at all).
- `terraphim_hooks` 1.21.0 was published from a **separate Gitea repo** (`terraphim/terraphim-hooks`)
  created for that purpose, while `terraphim-clients/crates/terraphim_hooks` still sits at 1.20.2
  with the old owned-`Thesaurus` API. There are now two divergent sources for one crate name.
- Two of the last three publishes recorded `dirty: true` — uncommitted working-tree changes at
  publish time, so the published bytes correspond to no commit anywhere.

### Impact

- **No reproducible builds.** Given a published `terraphim_agent` 1.21.3, nobody can check out the
  source it was built from. Security review, bisection, and hotfixing are all blocked.
- **Silent divergence risk.** `terraphim-clients` crates declare `terraphim_automata = "1.19.2"`
  (a caret requirement) against crates.io. The day `terraphim_automata` 1.21.x lands on crates.io,
  six crates in this workspace stop compiling with no code change on our side.
- **Duplicate source of truth for `hooks`** invites a future publish from the wrong one.

### Success Criteria

1. Every published `terraphim_{grep,agent,hooks}` version maps to a reachable, clean, tagged commit.
2. `terraphim-clients` declares one coherent API family and builds green on `main`.
3. Exactly one source of truth per crate name.

## Current State Analysis

### Verified facts (all re-derived from the repos, not from the prior transcript)

| Claim from previous session | Verdict | Evidence |
|---|---|---|
| Workspace dep conflict blocks all cargo ops | **Stale — local only** | `crates/terraphim_lsp/Cargo.toml:24` pins `version = "0.1.0"` on local `main`; on `gitea/main` the same line reads `version = "1.21.1"`. Local `main` is 31 commits behind. |
| `terraphim-grep` never published | **False** | crates.io index lists `1.21.1`, `1.21.2`. Downloaded 1.21.2: `default = ["llm", "code-search"]` — the fix issue #58 tracks is already shipped. |
| `terraphim-grep`/`terraphim-agent` missing Gitea repos | **Miscategorised** | Both are members of `terraphim-clients`, which is at `git.terraphim.cloud/terraphim/terraphim-clients` (HTTP 200). Standalone repos are not required and would create a second `hooks`-style duplicate. |
| Local `agent` sources are simply "behind" the registry | **False — bidirectional fork** | `gitea/main` vs registry 1.21.3: 1049 lines registry-only, **1763 lines `gitea/main`-only**, 29 files. Not a lift; a merge. |
| `hooks` local is behind registry | **True, and trivial** | Exactly 2 lines: `self.thesaurus.clone()` → `&self.thesaurus` at `replacement.rs:98,118`. |
| `gitea/main` does not build | **False** | `cargo check --workspace --all-targets` on a clean `gitea/main` worktree: **exit 0**, zero warnings, 1m46s. Resolves `terraphim_automata v1.20.4` from crates.io. |
| `main` CI failing | **True** | `gitea/main` `58810594`: `state: failure`. `native-ci / build` = "native build passed"; `adf/build` = "build failed; see /tmp/adf-build-terraphim-clients.log on bigbox". |

### The two API worlds

| | crates.io | Gitea registry |
|---|---|---|
| `terraphim_automata` max | **1.20.4** | **1.21.0** |
| `Thesaurus` in `find_matches`/`replace_matches` | owned (`Thesaurus`) | borrowed (`&Thesaurus`) |
| Who lives here | **`terraphim-clients`** (all deps are bare crates.io caret reqs; no `[patch.crates-io]` for automata/types) | **`terraphim-ai`** (24-line `[patch.crates-io]` block pinning the whole 1.21.x family), and the published `agent`/`hooks` 1.21.x |

`terraphim-clients/Cargo.toml` `[patch.crates-io]` contains only `terraphim_service` and
`rustls-webpki`. Nothing redirects `terraphim_automata`, so `^1.19.2` resolves to crates.io 1.20.4
and the workspace compiles against the owned API today.

### Owned-`Thesaurus` call sites in `terraphim-clients` (the migration surface)

| Crate | Location |
|---|---|
| `terraphim_hooks` | `src/replacement.rs:98`, `:118` |
| `terraphim_negative_contribution` | `src/scanner.rs:35` |
| `terraphim_sessions` | `src/enrichment/enricher.rs:103`, `src/search.rs:211` |
| `terraphim_mcp_server` | `src/lib.rs:831` |
| `terraphim-session-analyzer` | `src/kg/search.rs:129` + 6 sites in `tests/terraphim_integration_tests.rs` |
| `terraphim_agent` | `src/mcp_tool_index.rs:149` |

Six crates, not one. This is the true cost of joining the 1.21.x family.

### Publish provenance

| Artefact | SHA | `dirty` | Reachable from `terraphim-clients/main`? |
|---|---|---|---|
| `terraphim_agent` 1.21.2 | `776f8fc3` | no | **No** — commit exists (2026-08-15, PR #96) but was orphaned by three `Reset to gitea/main` operations on `main` |
| `terraphim_agent` 1.21.3 | `abe79c3f` | **yes** | **No** — SHA exists in no local repo at all |
| `terraphim_hooks` 1.21.0 | `610b5365` | no | **No** — belongs to the separate `terraphim/terraphim-hooks` repo (`path_in_vcs: ""`) |
| `terraphim_grep` 1.21.2 (crates.io) | `2d1ea8ae` | **yes** | **No** — SHA not found |

Four artefacts, zero reproducible from `terraphim-clients`.

### Where the `agent` fork actually diverges

Churn from `gitea/main` → published 1.21.3, by file:

| Lines | File | Nature |
|---|---|---|
| 716 | `learnings/hook.rs` | substantive |
| 433 | `main.rs` | substantive |
| 394 | `mcp_tool_index.rs` | **upstream deleted it** — 1.21.3 is a 5-line `#[deprecated]` re-export of `terraphim_mcp_search::McpToolIndex`; `gitea/main` still has the full implementation |
| 366 | `learnings/capture.rs` | substantive |
| 170 | `shared_learning/wiki_sync.rs` | substantive |

`gitea/main` carries 2026-08-08 → 2026-08-19 `learnings/` work (pi-rust hooks, recursive KG walk,
Claude `tool_response` envelopes) that the published crate does not have. The published crate carries
the `mcp_tool_index` deprecation and the `&Thesaurus` migration that `gitea/main` does not.
**Both sides have unique work.**

## Constraints

### Technical
- crates.io cannot host the borrowed API until `terraphim_automata` 1.21.x is published there; that
  decision belongs to `terraphim-core`, not this repo.
- `terraphim-clients` deps are caret reqs against crates.io — inherently exposed to the automata bump.
- Cargo resolves the whole workspace even for `cargo check -p <one-crate>`, so any single bad manifest
  blocks every crate (as local `main` demonstrates).

### Business
- `terraphim-ai` (the downstream consumer) is **green today** against the published artefacts. There is
  no production outage. This is debt repayment, not firefighting — it should not preempt the ADF
  reliability WIG or the #3115 security hotfix.

## Vital Few (max 3)

| Constraint | Why It's Vital | Evidence |
|---|---|---|
| **One source of truth per crate name** | Two live sources for `terraphim_hooks` will produce a wrong publish | `terraphim/terraphim-hooks` @ 1.21.0 borrowed vs `terraphim-clients/crates/terraphim_hooks` @ 1.20.2 owned |
| **Pick one API family for `terraphim-clients`** | Six crates break silently when automata 1.21 reaches crates.io | Call-site table above; all caret reqs |
| **Publishes must be clean, tagged, reachable** | 4/4 recent artefacts are unreproducible | Provenance table above |

## Eliminated from Scope (5/25 rule)

| Eliminated | Why |
|---|---|
| Publishing `terraphim_grep` | Already done — crates.io 1.21.2 has the fix. Issue #58 should be **closed**, not worked. |
| Creating Gitea repos for `grep`/`agent` | They are workspace members of a repo that already exists. Doing this would replicate the `hooks` duplicate-source bug. |
| Fixing `adf/build` CI | Separate bigbox infra failure (`native-ci` passes). Belongs with issue #106 (missing `zipsign` on PATH). |
| Recovering `terraphim_agent` 1.21.3's origin | SHA exists nowhere; forensics are exhausted. Yank-and-republish is cheaper than archaeology. |
| Issue #108 (35 stale `mergeable=false` PRs) | Known Gitea 1.26.3 bug, unrelated |

## Risks and Unknowns

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Merging the `agent` fork silently drops the 1763 lines of `learnings/` work | **High** | **High** | Three-way merge with the 1.21.2 base, file-by-file review of the 5 high-churn files; never a wholesale copy |
| `terraphim_automata` 1.21.x lands on crates.io before migration | Medium | High | Pin `terraphim-clients` deps to `=1.20.4` as an immediate, cheap guard |
| Republishing `agent` breaks `terraphim-ai` | Medium | Medium | `terraphim-ai` pins `=1.21.3` via `[patch.crates-io]`; publish a new version, do not yank in place |
| Force-resets on `main` orphan more publish commits | Medium | Medium | Tag every publish; the repo already tags `v1.21.0`–`v1.21.11` but the publishes did not use them |

### Assumptions

| Assumption | Basis | Risk if wrong | Verified? |
|---|---|---|---|
| `gitea/main` is authoritative over local `main` | Local is 0 ahead / 31 behind | Would discard local work | **Yes** — `git rev-list --left-right --count` = `0  31` |
| Published 1.21.3 is what `terraphim-ai` actually needs | `terraphim-ai/Cargo.toml:135` pins `terraphim_agent = { version = "1.21.3", registry = "terraphim" }` | Migration target wrong | **Yes** |
| crates.io automata will not jump to 1.21 imminently | 1.20.4 is current; 1.21.0 is Gitea-only and 1.20.4 is *yanked* on Gitea | Silent breakage | **No** — owned by `terraphim-core` |

### Open Questions

1. **Is `terraphim-clients` meant to join the 1.21.x borrowed-`Thesaurus` family, or stay on crates.io 1.20.4?**
   This single decision determines whether the work is ~6 crates of migration or a 1-line version pin.
   *Answerable only by Alex.*
2. **Should `terraphim/terraphim-hooks` (created 2026-08-28) be deleted, or should `terraphim_hooks` be removed from the `terraphim-clients` workspace?** One of the two must go.
3. Who published `terraphim_agent` 1.21.3 and `terraphim_grep` 1.21.2 with dirty trees — a CI runner, or a local workspace since deleted?

## Recommendations

**Proceed to design — but with the scope re-cut.** Options (a)/(b)/(c) from the previous session were
built on the four unverified blockers and should be discarded. The decision that actually matters is
Open Question 1.

Recommended shape, cheapest-first:

1. **Immediate, no-decision-needed (minutes):** `git pull` local `main` to `gitea/main` (kills the
   phantom dep-conflict blocker); close issue #58 as already-shipped; rotate the invalid `GITEA_TOKEN`
   in the environment (it 401s; `tea`'s token works).
2. **Cheap guard (hours):** pin `terraphim_automata`/`terraphim_types` to `=1.20.4` across
   `terraphim-clients` so the crates.io bump cannot break six crates by surprise.
3. **The real decision (Open Question 1):** if joining 1.21.x, that is a six-crate migration mirroring
   what `terraphim-ai` already did — a single branch with the `[patch.crates-io]` family block plus the
   call-site changes, then republish `agent` and `hooks` from tagged clean commits and delete the
   duplicate `terraphim-hooks` repo.

## Next Steps (if approved)

1. Resolve Open Question 1 and 2 with Alex.
2. Proceed to `disciplined-design` for the chosen branch of Q1.
3. File a Gitea issue in `terraphim-clients` tracking publish provenance (clean tree + tag + reachable SHA) as a release gate.

## Appendix: reproduction commands

```bash
# staleness
git -C terraphim-clients rev-list --left-right --count main...gitea/main   # 0  31

# the phantom blocker (local only)
git -C terraphim-clients show gitea/main:crates/terraphim_lsp/Cargo.toml | rg negative_contribution

# the two API worlds
curl -s https://index.crates.io/te/rr/terraphim_automata | tail -1   # 1.20.4

# agent fork, both directions
diff -ru <gitea-main>/crates/terraphim_agent/src \
         ~/.cargo/registry/src/git.terraphim.cloud-*/terraphim_agent-1.21.3/src \
  | awk '/^\+[^+]/{a++} /^-[^-]/{r++} END{print a, r}'   # 1049 1763

# provenance
cat ~/.cargo/registry/src/git.terraphim.cloud-*/terraphim_agent-1.21.3/.cargo_vcs_info.json
```
