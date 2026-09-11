# CI/CD gap analysis and staged pipeline

Refs terraphim/terraphim-clients#254. Written 2026-09-11 against commit 1120180.

## Pipelines in play

| Pipeline | File | Runner | Role |
|---|---|---|---|
| native-ci | `.gitea/workflows/native-ci.yml` | `terraphim-native` (bigbox, 24 cores) | Source of truth. Full workspace gate including the terraphim_server-backed integration lane. |
| CI | `.github/workflows/ci.yml` | GitHub hosted | Public mirror for the crates.io / GitHub build of terraphim-agent. Multi-platform matrix. |
| release-binaries, publish-crates, publish-registry, r2-manifest-health | `.github/workflows/*`, `.gitea/workflows/publish-registry.yml` | mixed | Release and distribution. Out of scope here except where gates feed them. |

## Gates before and after

| V-model stage | Gate | Before | After (this change) |
|---|---|---|---|
| Implementation | fmt, clippy `-D warnings` | present, in one monolithic job | own `check` job, runs first, fails in seconds |
| Implementation | pinned toolchain | none | `rust-toolchain.toml` = 1.97.1; GitHub pins the same version |
| Verification | unit and integration tests | `cargo test --all-targets` | `cargo nextest run --all-targets --profile ci` with JUnit output; doctests via `cargo test --doc` |
| Verification | coverage threshold | none | `cargo llvm-cov --fail-under-lines N` (N set from the measured baseline, see below) |
| Verification | supply chain | none in CI; `cargo deny` failed locally | `deny.toml` with every advisory triaged; `cargo deny check` in `check` |
| Verification | Miri on unsafe or pure crates | none | `ub-gates` job on nightly, default and tree-borrows axes, crates chosen by the #252 runbook |
| Verification | benchmark regression | none | GitHub `benchmarks` job on PRs, informational, using the #253 baselines |
| Validation | multi-platform | Linux only | GitHub `test` matrix: ubuntu, macos, windows |
| Validation | release artefacts | mature | unchanged |

## Runner constraints that shaped the design

The terraphim-gitea-runner enforces a command policy on the literal first token of each step. Allowed: `cargo`, `rustup`, `bash`, `sh`, `test`, `git`, `make`, `rch` and a few coreutils. Denied: `curl`, `wget`, `docker`, `python`. Consequences:

- Tools are provisioned with `cargo install --locked` inside the job. Runners have separate `CARGO_HOME`s and uneven tool sets (nextest and miri on some, cargo-deny on runner-5 only, cargo-llvm-cov nowhere), so provisioning is idempotent and repeated per job.
- `cargo build`, `check`, `clippy` and `doc` are routed to the rch compile farm by policy; `cargo nextest`, `cargo llvm-cov`, `cargo deny` and `cargo +nightly miri` run on the host.
- Repository scripts must be invoked as `bash ./scripts/x.sh`; `./scripts/x.sh` is rejected.
- No `uses:` actions on the native runner; checkout is implicit.
- Gitea dispatches one job per task as a "SingleWorkflow" payload, so `needs:` ordering and job fan-out are handled by Gitea itself; the runner only ever sees a single job. The four-job split therefore works on native-ci and the `check` job really does fail fast.
- The runner reports a commit status per job as `native-ci / <job> (push)`. Branch protection on `main` currently lists `native-ci / build (push)` (status checks disabled at the time of writing). Because this change renames `build` to `check` and `test`, the protection rule must be updated when status checks are re-enabled.

## Advisory triage (deny.toml)

| Advisory | Crate and chain | Action |
|---|---|---|
| RUSTSEC-2026-0204 crossbeam-epoch | fff-search, ignore | `cargo update -p crossbeam-epoch` (0.9.21) |
| RUSTSEC-2026-0258 h2 | hyper, axum, rmcp, reqwest | `cargo update -p h2` (0.4.19) |
| RUSTSEC-2026-0190 anyhow (unsound downcast_mut) | opendal, terraphim_config | `cargo update -p anyhow` (1.0.104) |
| RUSTSEC-2026-0221 event-listener (!Send crossing) | sqlx via opendal | `cargo update -p event-listener` (5.4.2) |
| RUSTSEC-2026-0253 lru (UAF on panic in pop) | transitive | `cargo update -p lru` (0.18.4) |
| RUSTSEC-2026-0186 memmap2 | transitive | `cargo update -p memmap2` (0.9.11) |
| yanked spin 0.9.8 | transitive | `cargo update -p spin` |
| RUSTSEC-2026-0189 rmcp DNS rebinding | terraphim_mcp_server direct dep, 0.9.1 | major upgrade to rmcp 1.4; child issue |
| RUSTSEC-2026-0183 / 0184 git2 UB | fff-search | needs fff-search to adopt a patched git2; child issue |
| RUSTSEC-2026-0194 / 0195 quick-xml | self_update (via terraphim_update) and opendal 0.54 | upstream; child issue |
| RUSTSEC-2023-0071 rsa Marvin, quinn-proto 0.11.14 | in Cargo.lock but not in the resolved all-features graph | not reachable; cargo-audit reports them from the lockfile only |
| five unmaintained notices | bincode, instant, number_prefix, paste, proc-macro-error | recorded, warn level |

The seven lockfile bumps are one commit. They are held until the UB audit (#252) finishes its Phase 3 dynamic runs so Miri and TSan results are attributed to a single dependency set. Until then `deny.toml` carries dated ignore entries and `yanked = "warn"`; that commit removes the entries and restores `yanked = "deny"`.

### Licence finding

`html2md` (GPL-3.0+) reaches `terraphim_agent` and `terraphim-cli` (Apache-2.0) through `terraphim_middleware` from the terraphim registry. This is an upstream licence-compatibility problem in terraphim-ai; `deny.toml` carries a named exception so the gate is green, and the exception must go when terraphim_middleware drops or feature-gates html2md.

## Why GitHub CI has been red since 2026-09-01

Every run fails within twenty seconds at `cargo fmt --check`. Locally the tree is rustfmt-clean on 1.97.1. The GitHub workflow used `dtolnay/rust-toolchain@stable`, which resolves to the newest stable and its newer rustfmt, whose output differs from 1.97.1 on this tree. Pinning the toolchain in the workflow (and `rust-toolchain.toml`) is the fix; no source formatting change is needed.

## A test that only fails under workspace feature unification

`terraphim-session-analyzer::connectors::codex::tests::test_parse_response_item` passes when run with `-p terraphim-session-analyzer` and fails under `cargo test --workspace --lib` (the GitHub CI command) because feature unification across the workspace changes the `ResponseItem` payload parsing. This is a pre-existing red, independent of this change; it is tracked in its own issue and the coverage baseline was measured with `--ignore-run-fail`.

## Coverage threshold

The threshold in both workflows is set from the first measured baseline, rounded down to the nearest 5. It only ever increases.

Baseline measured 2026-09-11 on commit 1120180, macOS aarch64, `cargo llvm-cov --workspace --lib --ignore-run-fail`:

| Metric | Covered | Total | Percent |
|---|---|---|---|
| Lines | 23753 | 35183 | 67.51 |
| Functions | 2425 | 3615 | 67.08 |
| Regions | 37948 | 54538 | 69.58 |

Gate: `--fail-under-lines 65`. Per-crate targets from the skill's table (80 percent for library crates, 70 for binaries, 90 for any module that carries `unsafe`) are the direction of travel, not the gate today.

## Miri crate selection

Miri cannot execute tokio, mio, reqwest or process spawning. The `ub-gates` job therefore runs the pure-computation crates only: terraphim_negative_contribution, terraphim_command_runtime and terraphim_hooks. The hooks `discovery` tests spawn a subprocess (`posix_spawnattr_init` is unsupported by Miri) and are skipped with `-- --skip discovery`, and the 1000-iteration latency test in the same crate is skipped with `--skip latency` because wall-clock assertions are meaningless under Miri. The #252 runbook widens this list as Phase 3 establishes which other test modules are Miri-clean (session-analyzer parsing and sessions redaction are the next candidates).

## Two lineages, not one repository

GitHub `main` is not a mirror of Gitea `main`. At the time of writing GitHub is 17 commits ahead of and 253 behind the Gitea lineage, and the two do not merge cleanly. Consequences:

1. A branch cut from Gitea `main` cannot be a GitHub pull request (it conflicts, and GitHub Actions skips conflicting PRs). The CI change therefore exists twice: `task/254-ci-pipeline` on the Gitea lineage and `task/254-ci-pipeline-gh` on the GitHub lineage, with identical CI files.
2. The GitHub lineage carried unformatted code in terraphim-session-analyzer (43 rustfmt diffs under 1.97.1); a whitespace-only commit fixes it there.
3. The GitHub mirror cannot resolve the private registry: the first real run failed with `failed to load source for dependency terraphim_service` because `https://git.terraphim.cloud/api/packages/terraphim/cargo/config.json` answered "Not available" to the GitHub runner. Until the 1.21.x family is on crates.io (#210) or the registry token secret is confirmed to work in pull-request runs, the GitHub `test`, `coverage` and `ub-gates` jobs cannot go green. This is a pre-existing condition that the old single-job workflow never reached because it failed at rustfmt first.

## First-run observations on native-ci

- `check` (fmt, clippy, cargo-deny including its install) took about 2.5 minutes; cargo-deny's `cargo install` alone was 84 seconds. A host-level install would remove that.
- `coverage` failed on the first run because `cargo llvm-cov` aborts when a test fails (#264); `--ignore-run-fail` now separates the two concerns.
- Gitea marked the `test` job of run 449 and the `check` job of run 451 as skipped at creation, with no runner having fetched the task (verified in the runner journal). No per-job rerun API exists on Gitea 1.26 (`jobs/{id}/rerun` and `runs/{id}/rerun` both 404), so a skipped job costs a full `workflow_dispatch`. Cause unknown; tracked with #244 and terraphim-ai#3375.

## Sync rule

`native-ci.yml` is the source of truth. Any change to a `cargo` invocation in a gate is made there first and mirrored into `ci.yml` in the same commit. Allowed divergence between the two:

1. Provisioning: `cargo install` on the native runner versus `taiki-e/install-action` on GitHub.
2. The terraphim_server-backed integration lane exists only on native-ci (private registry).
3. The multi-platform matrix and the benchmark job exist only on GitHub.

A PR that changes one file and not the other is rejected in review unless the diff falls into one of those three categories.

## Staged rollout

1. Land `rust-toolchain.toml`, `deny.toml`, `.config/nextest.toml` and the restructured workflows (this branch). Coverage threshold at the measured floor.
2. After #252 Phase 3: the lockfile bump commit; remove the six ignores; `yanked = "deny"`.
3. After #252 Phase 12: extend the Miri crate list from the runbook; add the TSan lane if the runner can build-std.
4. After #251 lands feature flags: add `--features otel,prometheus` to the clippy and test lanes.
5. After #253 commits baselines: point the benchmark job at `docs/perf/` baselines and raise the alert threshold from informational to blocking on main.

## Open items

- `cargo-llvm-cov` on the native runner needs `rustup component add llvm-tools` for the pinned 1.97.1 toolchain; the job does this each run.
- The Windows leg of the GitHub matrix has never run this workspace's tests; expect path or `zipsign` related failures on first run and gate them with `if: runner.os != 'Windows'` only with a recorded reason.
