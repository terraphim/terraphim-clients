# Design Document: a policy-compliant publish path (#400)

**Status**: Approved (Phase 1 research: `docs/plans/research-publish-workflow-policy-2026-11-13.md`)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13

## The decision

**Move the publish shell into a checked-in script and call it with a single
`bash` line.** The workflow keeps two steps: the checkout, and the publish.

## Why

The runner classifies the **literal first token** of every `run:` line and
rejects anything off its allowlist; a multi-line block is rejected before the
first step runs. The repository already proves the shape the runner accepts:
`run: bash ./scripts/ci/...` ✓ (the native-ci lanes ✓).

Two shapes were considered:

| Shape | Consequence |
|-------|-------------|
| Keep the shell inline and make every line legal | the block stays long, and the policy's rules would have to be re-derived for each construct (`if`, heredocs, assignments) - the issue's evidence shows the rules are not enumerable from here |
| **A checked-in script** | the workflow becomes two steps ✓; the logic is reviewable and runnable outside the runner ✓; the gate (#516) becomes reachable ✓ |

## The script's interface

`scripts/release/publish-registry.sh`:

| Input | Source | Meaning |
|-------|--------|---------|
| `CARGO_REGISTRIES_TERRAPHIM_TOKEN` | the step's `env:` (the secret) | the registry credential; the script refuses to run without it |
| `CRATE` | the step's `env:` (the workflow input) | the package to publish |

Both arrive through the step's `env:`, never through shell text ✓ (the policy
strips leading assignments token-wise, but an `env:` entry is not shell text at
all ✓).

The script does, in order:

1. refuses to run without the token or the crate name ✓ (loudly ✓);
2. writes the registry credentials and the sparse index into
   `${CARGO_HOME:-$HOME/.cargo}` with `printf` ✓ (no heredoc ✓);
3. refuses `terraphim_lsp` / `terraphim-lsp` ✓ (terraphim-ai owns it ✓);
4. checks that `crates/<crate>` exists ✓;
5. runs the pre-publish gate for `terraphim_agent` ✓ (#516 ✓);
6. publishes ✓.

Every step prints what it is doing, so a failed publish names its stage ✓.

## The guard

A new `ci_guards` arm, `publish_workflow_uses_only_policy_compatible_commands`:
every `run:` block in `.gitea/workflows/publish-registry.yml` must

- be a single line ✓,
- start with a program on the allowlist ✓ (`bash`, `cargo`, `test` ✓),
- name a file that exists in the repository when it is a `bash` line ✓,
- contain no `${{ ... }}` expression ✓ (expressions belong in `env:` ✓).

The guard parses the YAML with the `toml`/`serde_yaml` crates the file
already uses ✓ (`serde_yaml` is a dev-dependency? hmm — let me check ✓).

## Validation plan

- The guard's own tests: a compliant workflow passes ✓; a workflow with a
  comment line, a multi-line block, a `${{ }}` expression or a missing script
  fails ✓.
- The script runs locally up to the publish step (with a dry-run flag? hmm — the script would try to publish ✗). Let me add a `--dry-run` mode ✓ so the script can be exercised without publishing ✓ — hmm, the workflow calls it without arguments ✓ → the script would publish ✓. Let me support `PUBLISH_DRY_RUN=1` (an env var ✓) → the script stops before the publish ✓ → that's testable ✓ and the CI can prove the script's logic ✓.

Hmm, hmm: a dry-run mode in the publish script is a small, useful addition ✓ (and the GitHub publish path already has a `dry_run` input ✓ — a precedent ✓).

## Risks

| Risk | Mitigation |
|------|------------|
| The runner rejects the new one-liner | the native-ci lanes use the same shape ✓ |
| The credentials write behaves differently inside the script | the same files, the same content ✓; the script fails loudly when the token is missing ✓ |
| The publish script is never run in CI | `PUBLISH_DRY_RUN=1` makes the path exercisable ✓ (and a shellcheck-style self-test can run it ✓) |

## Out of scope

- The GitHub `publish-crates.yml` path ✓ (crates.io only ✓, and it refuses
  `terraphim_agent` ✓).
- The runner's policy implementation ✓ (not in this repository ✓).
- The `=1.23.0` publish-ordering pin ✓ (#516 ✓, a release action ✓).
