# Engineering conventions for `terraphim-clients`

These rules apply to every change made by any agent or contributor in this
workspace. They are enforced at code-review and CI time.

## Hard rules

- **British English.** All text output, documentation, comments, identifiers,
  and generated content use UK spelling (`colour`, `organised`, `behaviour`,
  `catalogue`, `artefact`, `centre`, `licence`, etc.).
- **No emoji.** Anywhere in code, comments, docs, commit messages, PR
  descriptions, or generated content.
- **No mocks in tests.** Tests hit real services or are removed. Use
  hermetic temp dirs, real binaries spawned with hermetic env vars, real
  fixtures under `tests/fixtures/`. Never `mockito`, `mockall`, hand-rolled
  trait doubles, or stub databases in committed test code.
- **No dead code.** No unreferenced symbols, no commented-out code, and no
  `#[allow(dead_code)]` annotations that hide a real dead-code finding.
  See [§ No-dead-code policy](#no-dead-code-policy) below.
- **No `#[ignore]` on tests.** If a test cannot be made to pass, either fix
  the underlying failure or delete the test. A skipped test is a lie.
- **No timeout flag for cargo/make on macOS.** macOS does not have GNU
  `timeout`; rely on Rust's own test harness and CI walls instead.
- **Never overwrite `.env` files.** Use `op inject` or `op run --no-masking`
  to compose secrets at runtime.
- **Never increase test timeouts to make a flaky test pass.** Diagnose the
  root cause. The only acceptable exception is integration tests against an
  LLM or similarly slow external service.
- **Use `terraphim-grep` first** for code/content search. Fall back to
  `rg`/`fd`/`fff` only when the query is purely about file names or
  `terraphim-grep` is unavailable. Never use `grep`/`find`.
- **All task tracking lives in Gitea** (`gtr` CLI / `gitea-robot` MCP). Do
  not use `br`/`bd`/`.beads/` or MCP Agent Mail for issue tracking.
- **Commit every successful change.** Tests must be green before the commit.
  Use `Refs #<id>` or `Fixes #<id>` in the subject to keep Gitea in sync.

## No-dead-code policy

`dead_code` lint findings must be resolved, not annotated away. The lint
fires when an item has no reachable user in the current compilation unit.

**Allowed without justification:** none. Every `#[allow(dead_code)]` needs a
comment explaining what is being suppressed and why.

**Allowed with a justification comment:**

- **Cross-binary shared helpers in integration-test support modules.**
  Rust integration tests compile each `tests/*.rs` file as its own
  binary, so an item that is genuinely used by one binary appears
  unused in every binary that does not import it. The function is not
  dead — only the per-binary view of it is. The annotation must say:
  - which other binaries use the item,
  - why those binaries cannot directly reach it through their `use`
    chain, and
  - what refactor would remove the annotation entirely (e.g. moving the
    helpers into a separate `*_test_support` crate consumed via
    `dev-dependency`).
- **Feature-gated public API.** Items that exist only to be reached when
  a non-default Cargo feature is enabled (`--features firecracker`,
  `--features server`, etc.). Such items are dead in the default
  `cargo check --no-default-features` build and live in the
  `cargo check --all-features` build. The annotation must say:
  - the feature flag(s) under which the item is reachable,
  - the names of the caller(s) that become reachable under that flag,
    and
  - the URL/path of the feature declaration in `Cargo.toml`.

**Forbidden:**

- `#[allow(dead_code)]` on private items reachable through a public item
  used by the same binary (the warning cascades — if A is used, B is used).
- `#[allow(dead_code)] // Will be used in Phase 2/3/...` — speculative
  future-use is not a use. Delete the item or finish the work.
- `#[allow(dead_code)] // Replaced by ...` — delete the old implementation.
- Project-wide `#![allow(dead_code)]` or `#![deny(warnings)]` blanket overrides.
- `#[allow(unused_imports)]` for hygiene; clean up the import instead.

**Verification:** before merging any PR that touches `tests/**` or
`src/**`, run `cargo check --tests --all-targets -p <changed-crate>` and
confirm zero `dead_code` warnings. Add the same command to your pre-commit
check.

## Pre-merge checks

For any PR that changes Rust code, run, in order:

1. `cargo fmt --check` (style)
2. `cargo clippy --workspace --all-targets -- -D warnings` (lints; on
   macOS use `cargo clippy` directly — do not wrap in `timeout`).
3. `cargo test -p <changed-crate> --tests --no-fail-fast` (tests).
4. `terraphim-grep "Reachable identifiers in changed files" --paths <files>`
   (sanity check that no orphan `pub` items were added).
5. `gtr comment --owner terraphim --repo terraphim-clients --index <id>`
   with a `progress update` body summarising what changed and what evidence
   confirms it works.

## Workflow

- One task per branch: `task/<gitea-index>-<short-title>`.
- One commit per logical change; the commit subject cites the issue.
- One PR per branch; squash-merge on the GitHub mirror.
- After merge: `gtr close-issue --owner terraphim --repo terraphim-clients
  --index <id>` once CI is green.
- For destructive ops (force-push, reset, drop table, etc.) confirm with
  the user before acting.