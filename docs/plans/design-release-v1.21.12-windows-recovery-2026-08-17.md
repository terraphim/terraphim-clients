# Phase 1/2 Plan: v1.21.12 Windows Release Recovery

**Status**: Approved  
**Approval Basis**: User instruction on 2026-08-17 to use disciplined engineering skills to complete actions.  
**Issue**: Gitea #103  
**Author**: Codex  
**Date**: 2026-08-17  
**Scope**: Phase 1 research plus Phase 2 design only. No implementation and no commit.

## Executive Summary

The `release-binaries.yml` workflow dispatch for `v1.21.12` successfully checked out the release source peeled to `e080475ac26f44ad4674a438d753f6ab185fb787` and validated release version metadata for all shipped crates. The failure is later and Windows-specific: the host validation command `cargo run -q -p terraphim_agent --bin terraphim-agent -- --version` terminates with `thread 'main' has overflowed its stack` before the Windows matrix can build and package binaries.

The prior design had a blocking semantic/pragmatic gap: dispatching the fixed workflow with `ref=v1.21.12` would load the old workflow from the tag, while dispatching the fixed workflow from the fix branch or `main` would make `actions/checkout` default to the mutable workflow ref. Recovery must therefore separate workflow execution identity from release source identity.

The corrected design is:

1. Execute the workflow from the fix branch or `main`, not from `v1.21.12`.
2. Add an explicit `source_ref` plus `expected_source_sha` contract.
3. Preflight-validate `version`, `release_tag`, `source_ref`, `target_repo`, and `expected_source_sha` before any checkout or mutation.
4. Resolve `source_ref`/`release_tag` through the GitHub API, recursively peel annotated tags, and output the exact source commit SHA.
5. Require the peeled source SHA to equal `expected_source_sha`; for this recovery it must equal `e080475ac26f44ad4674a438d753f6ab185fb787`.
6. Use only the preflight output SHA for source/script checkout steps.
7. Fail hostile inputs and source mismatch tests before build, artifact upload, release upload, or R2 publication.

For the Windows overflow itself, select H2 as the first implementation path because the log proves a runtime stack overflow and `/STACK:8388608` is a reversible build-only probe. Retain H3 only as fallback if H2 reds.

## Essential Questions

| Question | Answer | Evidence |
| --- | --- | --- |
| Does this problem energize us to solve it? | Yes | It blocks release recovery for an already prepared client release. |
| Does solving this leverage our unique capabilities? | Yes | It requires separating release metadata correctness, workflow/source identity, and platform-specific Rust startup behavior. |
| Does this meet a significant, validated need? | Yes | `/tmp/clients-windows.log` shows the Windows release job failing after version metadata validation and before binary build/package/upload. |

**Proceed**: Yes - 3/3 essential questions are satisfied.

## Exact Current-State/Data-Flow Map

### Current Workflow Entry

`.github/workflows/release-binaries.yml` is manually triggered by `workflow_dispatch` with three inputs:

| Input | Purpose |
| --- | --- |
| `version` | Release version without `v`, for example `1.21.12`. |
| `release_tag` | GitHub release tag, expected to equal `v${version}`. |
| `target_repo` | Target GitHub repo for uploaded assets, defaulting to `terraphim-ai`. |

Current risk: the workflow ref and source checkout ref are implicitly coupled. A dispatch against `ref=v1.21.12` loads the old workflow from the tag, so the recovery fix is absent. A dispatch against the fix branch or `main` loads the corrected workflow, but `actions/checkout` without an explicit immutable source SHA checks out the mutable workflow ref.

### Required Workflow Entry

The workflow must be dispatched from the fix branch or `main`. The release source must be selected only through validated inputs:

| Input | Required Value for This Recovery | Purpose |
| --- | --- | --- |
| `version` | `1.21.12` | Release version without `v`. |
| `release_tag` | `v1.21.12` | GitHub release tag and upload target. Must equal `v${version}`. |
| `source_ref` | `v1.21.12` | Source ref to resolve and checkout. Must equal `release_tag` for this recovery. |
| `expected_source_sha` | `e080475ac26f44ad4674a438d753f6ab185fb787` | Required peeled commit SHA for the release source. |
| `target_repo` | `terraphim-clients` | Target GitHub repo for uploaded assets. Must pass allow-list validation. |

### Required Preflight Flow

1. Run a dedicated preflight job before source checkout, version mutation, build, packaging, upload, or R2 publication.
2. Validate `version` as strict semver without a leading `v`.
3. Validate `release_tag == v${version}`.
4. Validate `source_ref == release_tag` for this recovery.
5. Validate `expected_source_sha` as a 40-character lowercase hex SHA.
6. Validate `target_repo` against the intended allow-list, including `terraphim-clients`.
7. Resolve `source_ref` using the GitHub Git refs/tags and Git objects APIs.
8. If the ref is an annotated tag object, recursively peel until the object type is `commit`.
9. Fail if the peeled commit SHA does not equal `expected_source_sha`.
10. Export outputs: `version`, `release_tag`, `source_ref`, `source_sha`, and `target_repo`.
11. All source/script checkout steps must use `ref: ${{ needs.preflight.outputs.source_sha }}`.

### Required Build Matrix Flow

`build-binaries` must depend on `preflight` and run six targets with `fail-fast: false`:

| OS | Target | Cross |
| --- | --- | --- |
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | false |
| `ubuntu-22.04` | `x86_64-unknown-linux-musl` | true |
| `ubuntu-22.04` | `aarch64-unknown-linux-musl` | true |
| `macos-latest` | `x86_64-apple-darwin` | false |
| `macos-latest` | `aarch64-apple-darwin` | false |
| `windows-latest` | `x86_64-pc-windows-msvc` | false |

### Required Release Version Validation Flow

1. `actions/checkout@v4` checks out `${{ needs.preflight.outputs.source_sha }}`.
2. The job confirms `git rev-parse HEAD` equals `${{ needs.preflight.outputs.source_sha }}`.
3. Rust stable is installed for the matrix target.
4. `zig` is installed for macOS and Windows.
5. `Swatinem/rust-cache@v2` restores target/cache state except for the GNU Linux target.
6. The release version step uses only preflight outputs, validates them again locally, rewrites only `[workspace.package]` in `Cargo.toml` and `[package]` in `crates/terraphim_agent/Cargo.toml`, then checks `cargo metadata` versions for `terraphim_agent`, `terraphim-cli`, and `terraphim_grep`.
7. The host binary assertion validates `terraphim-agent --version` and compares the final output token with `${{ needs.preflight.outputs.version }}`.
8. Only after host version validation succeeds does the job build release binaries for the matrix target.
9. Windows packaging expects `target/x86_64-pc-windows-msvc/release/*.exe`, zips them, copies raw `.exe` files, and uploads the artifact.

### Downstream Release Flow

1. `create-universal-macos` depends on `build-binaries` and uses artifacts produced from the preflight source SHA.
2. `sign-and-notarize-macos` signs and notarizes the universal macOS agent and grep binaries.
3. `upload-to-target-release` depends on `preflight`, `build-binaries`, and macOS signing. It must use preflight outputs for `release_tag` and `target_repo`, upload with `gh release upload --clobber`, and publish signed `.tar.gz` archives plus manifests to R2 only after all gates pass.

### Windows Evidence Flow

The Windows log shows:

| Evidence | Meaning |
| --- | --- |
| `fetch ... +e080475ac26f44ad4674a438d753f6ab185fb787:refs/tags/v1.21.12` | The previous dispatch checked out the intended release source mapping. |
| `HEAD is now at e080475 ... v1.21.12` and `git log -1 --format=%H` prints `e080475ac26f44ad4674a438d753f6ab185fb787` | The checked-out commit matches the required peeled tag commit. |
| `VERSION: 1.21.12`, `RELEASE_TAG: v1.21.12`, `TARGET_REPO: terraphim-clients` | The workflow inputs reached the Windows job correctly. |
| `Cargo.toml: [workspace.package] version -> 1.21.12` and `crates/terraphim_agent/Cargo.toml: [package] version -> 1.21.12` | CI version rewriting succeeded in the checkout. |
| `terraphim_agent 1.21.12 OK`, `terraphim-cli 1.21.12 OK`, `terraphim_grep 1.21.12 OK` | Metadata validation succeeded for all shipped crates. |
| `cargo run -q -p terraphim_agent --bin terraphim-agent -- --version` followed by `thread 'main' (7244) has overflowed its stack` and exit code `127` | The failure is the Windows host binary/version assertion, not tag checkout or metadata validation. |

### `terraphim-agent` Startup Flow

`crates/terraphim_agent/src/main.rs` declares the Clap CLI at `Cli` with `#[derive(Parser, Debug)]` and `#[command(name = "terraphim-agent", version, ...)]`. `main()` then:

1. Collects `std::env::args()`.
2. Applies `apply_forgiving_parsing(&args)`.
3. Calls `Cli::parse_from(corrected_args)`.
4. Resolves output config.
5. Creates a Tokio runtime and runs a non-blocking update check.
6. Dispatches subcommands or default TUI behavior.

Because Clap handles `--version` during parsing, a healthy `terraphim-agent --version` path should print the package version and exit before the update check, TUI startup, or command execution. The observed Windows stack overflow therefore occurs during build/run startup or early CLI parsing, before any successful version output is captured.

## Root-Cause Analysis

### What Succeeded

The release identity and release metadata path succeeded in the failing run:

- The job fetched and checked out `v1.21.12` at `e080475ac26f44ad4674a438d753f6ab185fb787`.
- `VERSION=1.21.12` and `RELEASE_TAG=v1.21.12` reached the job.
- Semver/tag/repo validation did not fail.
- CI-local version rewriting reached both workspace and `terraphim_agent` package manifests.
- `cargo metadata` proved `terraphim_agent`, `terraphim-cli`, and `terraphim_grep` all resolved to `1.21.12`.

These facts mean the prior fixes for release version propagation are working on Windows.

### What Failed

The first executable validation of the Windows host `terraphim-agent` path failed:

```bash
cargo run -q -p terraphim_agent --bin terraphim-agent -- --version
```

After about six minutes, the process reported:

```text
thread 'main' (7244) has overflowed its stack
```

No version line was captured, and the step exited `127`.

### Additional Design Gap

The original design did not make workflow execution ref and release source ref independent. That is unsafe for this recovery:

- `workflow_dispatch` with `ref=v1.21.12` loads the old workflow from the release tag, so the fixed workflow never runs.
- `workflow_dispatch` with `ref=fix-branch` or `ref=main` loads the fixed workflow, but a default `actions/checkout` would check out the mutable workflow ref rather than the immutable release source.

The release source must therefore be resolved in preflight and checked out by exact SHA in every job that reads or mutates source files.

### Working Diagnosis

The most likely Windows failure is runtime stack exhaustion in the current `terraphim-agent` startup/version path, probably while constructing or parsing the large Clap command graph. The log already proves a runtime stack overflow. The first implementation path should therefore be H2: add a reversible Windows build-only stack reserve probe using `/STACK:8388608` and run the produced executable directly.

H3, a code-level early version fast path, remains a fallback only if H2 reds.

## Constraints

| Constraint | Source | Impact |
| --- | --- | --- |
| Workflow must execute from fix branch or `main` | Blocking KLS semantic/pragmatic finding | The fixed workflow cannot be dispatched with `ref=v1.21.12`, because that would load the old workflow. |
| Preserve immutable release source | User request and log evidence | Do not move, recreate, or replace `v1.21.12`; recovery must use the peeled source commit `e080475ac26f44ad4674a438d753f6ab185fb787`. |
| Require explicit `expected_source_sha` | Source identity contract | Prevents mutable branch checkout, tag retargeting, wrong tag, or hostile `source_ref` from reaching build/upload. |
| Resolve annotated tags recursively through GitHub API | Source identity contract | Lightweight and annotated tags must both peel to a commit before checkout. |
| All source/script checkout steps use preflight SHA | Source identity contract | No job may implicitly checkout the mutable workflow ref after preflight. |
| Preflight before checkout/mutation | Release integrity | Hostile inputs and mismatch tests fail before source mutation, build, artifact upload, release upload, or R2 publication. |
| Preserve existing release workflow shape | User request | Use `.github/workflows/release-binaries.yml` with `workflow_dispatch`; do not invent a parallel release pipeline. |
| Do not skip Windows validation | Release integrity | Windows assets must be built, version-validated, packaged, and included in the fail-closed release gate. |
| Select H2 first | User instruction and log evidence | `/STACK:8388608` is a reversible build-only probe for a proven runtime stack overflow. |
| Keep H3 fallback only | Risk control | Avoid source-level CLI behavior changes unless H2 fails. |
| No implementation in Phase 1/2 | User request and disciplined process | This document defines work only; no code changes now. |
| Only this document changes in this turn | User request | No implementation files, no commits. |

## Vital Few

| Vital Item | Why It Matters | Evidence |
| --- | --- | --- |
| Separate workflow ref from release source ref | The fixed workflow must run without accidentally building mutable branch source. | KLS blocking semantic/pragmatic finding. |
| Preserve tag/commit identity | Release recovery must not mutate historical release state. | Windows log checked out `v1.21.12` at `e080475ac26f44ad4674a438d753f6ab185fb787`. |
| Recover Windows `terraphim-agent --version` validation | This is the immediate blocker and protects #67/#95 acceptance. | Failure occurs at the host version assertion step. |
| Keep Windows matrix mandatory | Skipping Windows would ship an unvalidated platform and hide the failure. | Existing matrix includes `x86_64-pc-windows-msvc`; downstream upload waits for full build success. |

## Explicit Assumptions/Unknowns

### Assumptions

1. Gitea #103 acceptance criteria include successful recovery of `v1.21.12` client binaries using the existing release workflow and immutable release source.
2. The fixed workflow can be dispatched from the fix branch first, then from `main` after merge, while `source_ref=v1.21.12` and `expected_source_sha=e080475ac26f44ad4674a438d753f6ab185fb787`.
3. The Windows stack overflow is reproducible on GitHub-hosted `windows-latest` with Rust stable.
4. `terraphim-cli` and `terraphim-grep` do not share the same runtime startup stack issue, because the failure occurs before their build/package steps.
5. `cargo metadata` success is sufficient proof that CI-local version propagation is correct before executable validation.

### Unknowns

1. Whether the overflow occurs while launching through `cargo run`, loading the executable, constructing Clap parser state, or parsing `--version`.
2. Whether a Windows linker stack-size increase alone fixes the issue.
3. Whether a code-level early `--version` fast path is needed to avoid constructing the full Clap graph.
4. Whether the same stack behavior appears in release builds without `/STACK:8388608`.

## Falsifiable Hypotheses and Smallest CI Probes

| Hypothesis | Smallest Probe | Falsifies If |
| --- | --- | --- |
| H2: Increasing the Windows binary stack reserve fixes `terraphim-agent --version`. | First implementation: build only `terraphim-agent` on Windows with `RUSTFLAGS="-C link-arg=/STACK:8388608"` and run the produced executable `--version`. | The same stack overflow occurs with the larger stack, or output final token does not equal `VERSION`. |
| H1: Runtime startup/Clap parsing overflows Windows main-thread stack. | On Windows after version rewrite, run `cargo build -p terraphim_agent --bin terraphim-agent`, then run the produced debug executable directly with `--version` under `RUST_BACKTRACE=full`. | Build fails before executable launch, or direct executable succeeds while `cargo run` fails. |
| H3: A code-level early version fast path avoids the stack-heavy Clap path and preserves reported version. | Fallback only if H2 reds: add a minimal branch before `Cli::parse_from` in `crates/terraphim_agent/src/main.rs`, run `terraphim-agent --version`, and compare output token to `VERSION`. | The overflow still occurs before or inside the fast path, or the output no longer matches `VERSION`. |
| H4: The issue is debug-only because the workflow uses `cargo run` without `--release`. | Build release first and run the release executable directly, with the same source SHA and version rewrite. | Release validation also overflows. |

The implementation phase must convert only the smallest successful probe into the permanent fix.

## Rejected Alternatives

| Alternative | Rejection Reason |
| --- | --- |
| Dispatch the fixed workflow with `ref=v1.21.12` | This loads the old workflow from the tag, so the recovery fix does not execute. |
| Dispatch from fix branch or `main` and rely on default `actions/checkout` | This checks out a mutable workflow ref, not the immutable release source. |
| Accept `source_ref` without `expected_source_sha` | Allows tag retargeting, typoed refs, or hostile refs to reach build/upload. |
| Resolve tags with local checkout state only | A checkout is exactly what must be delayed until preflight validates the source identity. |
| Skip Windows validation or remove the Windows matrix target | Violates release integrity and would produce or omit Windows assets without proving the shipped binary reports the release version. |
| Move, delete, or recreate tag `v1.21.12` | Violates immutable tag preservation and risks invalidating already-audited release evidence. |
| Create a new release tag such as `v1.21.13` for this recovery | Does not recover issue #103's requested `v1.21.12` release and broadens scope. |
| Disable the host binary `--version` assertion globally | Regresses #67/#95 protection that binaries must report the tag version. |
| Replace the release workflow with a new pipeline | Larger blast radius than needed; existing dispatch, metadata validation, packaging, signing, and R2 publication already encode the desired release flow. |
| Treat metadata validation as a substitute for executable validation | Metadata can be correct while the actual executable fails to start or report a version. |
| Start with H3 source-level CLI changes | Higher risk than H2; H3 remains fallback only if the reversible build-only stack probe fails. |

## Exact File Changes

### New Files

None for implementation. This plan document already exists as the approved Phase 1/2 record.

### Modified Files for Phase 3

| File | Exact Intended Change |
| --- | --- |
| `.github/workflows/release-binaries.yml` | Add `workflow_dispatch` inputs `source_ref` and `expected_source_sha`. Add a required `preflight` job that validates `version`, `release_tag`, `source_ref`, `target_repo`, and `expected_source_sha`; resolves and recursively peels `source_ref`/`release_tag` through the GitHub API; fails on hostile input or SHA mismatch; emits immutable outputs. Make all jobs that read source depend on `preflight`. Set every source/script `actions/checkout@v4` to `ref: ${{ needs.preflight.outputs.source_sha }}` and assert `git rev-parse HEAD` matches that SHA. Replace raw input usage in build/upload jobs with preflight outputs. Add Windows H2 validation: build `terraphim-agent` with `RUSTFLAGS="-C link-arg=/STACK:8388608"` and run the produced `.exe --version`, asserting final token equals the preflight `version`. Retain non-Windows host assertion equivalently. Keep upload/R2 fail-closed behind successful preflight, build matrix, and macOS signing. |
| `crates/terraphim_agent/src/main.rs` | Fallback only if H2 reds. Add a minimal early `--version`/`-V` path before `Cli::parse_from` that prints Clap-compatible version output using `env!("CARGO_PKG_VERSION")`, then exits `0`. Do not alter command behavior for other args. |

### Deleted Files

None.

### Public API Changes

None planned. This is release CI/startup behavior only.

### New Dependencies

None planned. Use GitHub API via existing workflow shell/`gh api`/`curl` capabilities available in GitHub Actions.

## Test-First Workflow Strategy

1. Add preflight hostile-input tests before the build matrix:
   - `version` with leading `v` fails.
   - `release_tag` not equal to `v${version}` fails.
   - `source_ref` not equal to `release_tag` fails for this recovery.
   - `target_repo` outside the allow-list fails.
   - malformed `expected_source_sha` fails.
   - resolved peeled SHA not equal to `expected_source_sha` fails.
2. Add a positive preflight test for:
   - workflow dispatch ref: fix branch or `main`;
   - `version=1.21.12`;
   - `release_tag=v1.21.12`;
   - `source_ref=v1.21.12`;
   - `target_repo=terraphim-clients`;
   - `expected_source_sha=e080475ac26f44ad4674a438d753f6ab185fb787`.
3. Preserve the existing successful metadata validation, but feed it only from preflight outputs.
4. Implement H2 first on Windows:
   - build `terraphim-agent` with `/STACK:8388608`;
   - run the produced `.exe --version` directly;
   - assert exit code `0`;
   - assert final token equals `1.21.12`.
5. Run the full matrix only after preflight and H2 pass.
6. Use H3 only if H2 reds with the same stack overflow or fails to produce the expected version output.
7. Require the full release workflow to remain fail-closed before upload/R2 publication.

## Rollback

1. If preflight rejects the intended recovery inputs, do not weaken validation. Fix the resolver or input contract and rerun before any build/upload.
2. If a source SHA mismatch occurs, stop the recovery. Do not checkout, mutate manifests, build, upload artifacts, or modify `v1.21.12`.
3. If the workflow-only H2 stack mitigation causes unrelated CI failures, revert only the `.github/workflows/release-binaries.yml` stack-specific change and keep the source-ref preflight contract.
4. If H2 reds, retain preflight and proceed to the approved H3 fallback only after documenting H2 evidence.
5. If an H3 early version fast path is added and causes CLI regressions, revert only the `crates/terraphim_agent/src/main.rs` change and keep the independent preflight contract.
6. Do not change or roll back tag `v1.21.12`.
7. Do not delete uploaded artifacts unless a later approved release operation determines that invalid assets were actually published.

## Traceability from Issue Acceptance Criteria

Because the local worktree does not contain the full Gitea #103 text, the acceptance criteria below are inferred from the user request, KLS gate feedback, and release evidence.

| Acceptance Criterion | Design Coverage | Verification Evidence |
| --- | --- | --- |
| Fixed workflow executes while release source remains immutable | Required workflow entry, required preflight flow, constraints | Workflow dispatch uses fix branch or `main`; checkout uses preflight `source_sha`. |
| Preserve immutable `v1.21.12` at `e080475ac26f44ad4674a438d753f6ab185fb787` | Constraints, vital few, rollback, implementation steps | Preflight recursively peels `source_ref`/`release_tag` and requires exact `expected_source_sha`. |
| Hostile inputs fail before build/upload | Required preflight flow, test-first strategy | Negative preflight tests fail before checkout, mutation, build, artifact upload, release upload, and R2 publication. |
| Use existing release workflow shape | Current-state map, constraints, file changes | `workflow_dispatch` remains in `.github/workflows/release-binaries.yml`; no parallel release pipeline. |
| Recover Windows release validation | Root-cause analysis, hypotheses, test-first strategy | Windows H2 `terraphim-agent --version` exits `0` and reports `1.21.12`; H3 fallback only if H2 reds. |
| Distinguish metadata/version success from Windows stack overflow | Root-cause analysis | CI output keeps metadata validation lines separate from executable validation lines. |
| Keep scope surgical | Vital few, rejected alternatives, exact file changes | Primary implementation file is `.github/workflows/release-binaries.yml`; `main.rs` only if H2 fails. |
| Do not skip Windows validation | Rejected alternatives, constraints | Windows matrix remains mandatory and upload remains gated on `build-binaries == success`. |

## Implementation Steps

### Step 1: Add Preflight Source Contract

**Files**: `.github/workflows/release-binaries.yml`  
**Description**: Add `source_ref` and `expected_source_sha` inputs. Add a `preflight` job that validates `version`, `release_tag`, `source_ref`, `target_repo`, and `expected_source_sha`; resolves `source_ref`/`release_tag`; recursively peels annotated tags via the GitHub API; requires the peeled commit to match `expected_source_sha`; emits immutable outputs.  
**Tests**: Run hostile-input preflight cases and the approved positive recovery case.  
**Expected Result**: Wrong tags, mutable refs, malformed SHAs, unauthorized target repos, and SHA mismatches fail before checkout or mutation.

### Step 2: Pin All Source Checkouts to Preflight SHA

**Files**: `.github/workflows/release-binaries.yml`  
**Description**: Make source-reading jobs depend on `preflight`. Set each source/script checkout to `ref: ${{ needs.preflight.outputs.source_sha }}` and assert `git rev-parse HEAD` equals that output. Replace raw dispatch input usage with preflight outputs where jobs mutate manifests, build artifacts, upload release assets, or publish to R2.  
**Tests**: Positive recovery dispatch from fix branch or `main` confirms every checkout is at `e080475ac26f44ad4674a438d753f6ab185fb787`.  
**Expected Result**: The fixed workflow executes from fix branch or `main`, but all release source operations run against the immutable release commit.

### Step 3: Use the Proven Windows Release Profile (H4)

**Files**: `.github/workflows/release-binaries.yml`  
**Description**: Build `terraphim-agent` in the same unmodified `--release` profile shipped to users and execute that exact target binary with `--version`; require the final output token to equal the preflight version. Recovery run `32060761712` isolated the original debug-profile failure: the unmodified release build returned `build_status=0`, `run_status=0`, and `terraphim-agent 1.21.12`. A second `/STACK:8388608` build also passed, proving the linker override was unnecessary rather than causal. Remove the one-shot diagnostic and do not ship an unevidenced stack override.
**Tests**: Windows `x86_64-pc-windows-msvc` release job exits `0`, reports `1.21.12`, contains no debug assertion or `/STACK:8388608`, and packages the same release-target executable.
**Expected Result**: H4 greens using the actual shipped profile with no source or linker behavior change.

### Step 4: Preserve and Re-run Full Matrix

**Files**: `.github/workflows/release-binaries.yml`  
**Description**: Ensure non-Windows validation remains equivalent, Windows still builds all three binaries, and packaging still produces `.zip` plus raw `.exe` artifacts from the preflight source SHA.  
**Tests**: Full `release-binaries.yml` workflow dispatch for all matrix targets.  
**Expected Result**: All matrix targets build from the same immutable source SHA.

### Step 5: Verify Fail-Closed Release Publication

**Files**: `.github/workflows/release-binaries.yml`  
**Description**: Confirm `upload-to-target-release` still requires successful `preflight`, `build-binaries`, and `sign-and-notarize-macos`, and uses preflight `release_tag`/`target_repo` outputs.  
**Tests**: Successful full workflow reaches upload/R2 only after all build targets and macOS signing pass. Negative preflight cases never reach upload/R2.  
**Expected Result**: Publication remains gated and source identity is auditable.

### Step 6: Use H3 Fallback Only if the Release Profile Reds

**Files**: `crates/terraphim_agent/src/main.rs`; `.github/workflows/release-binaries.yml`  
**Description**: If the unmodified release profile fails with the same stack overflow or cannot produce the expected version output, add a minimal early `--version`/`-V` path before `Cli::parse_from`, then rerun Windows validation. Recovery run `32060761712` passed H4, so this fallback is not implemented.
**Tests**: Unit or integration coverage for `--version` output only if the source fallback becomes necessary, plus full workflow validation.
**Expected Result**: H3 remains an unimplemented contingency because the actual release artifact is proven healthy.
