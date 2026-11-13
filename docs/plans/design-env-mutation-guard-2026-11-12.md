# Design Document: A ci_guards arm that rejects new environment-mutation sites

**Status**: Approved (Phase 2 gate passed)
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Research input**: `docs/plans/research-env-mutation-guard-2026-11-12.md`
**Issue**: terraphim-clients#375

## Objective

Add one `ci_guards` test that fails when a new environment-mutation site
appears under `crates/`, in either form:

1. the direct `std::env::set_var` / `std::env::remove_var` calls (each one
   needs an `unsafe` block today), and
2. the safe wrapper `terraphim_test_utils::set_env_var` /
   `remove_env_var` / `EnvVarGuard`, which `forbid(unsafe_code)` (#310)
   cannot see.

## Constraints Carried From Research

| # | Constraint | Consequence |
|---|------------|-------------|
| C1 | The guard file quotes the forbidden literals | Scan with `workspace_rust_sources()`, which excludes the guard file |
| C2 | The allow-list must not rot | The arm fails both for new sites and for stale entries |
| C3 | No process spawning | Pure `std::fs` walking; manifests parsed with the existing `toml` dev-dependency |
| C4 | The arm must pass on `main` today | Seeded with the measured 23-site inventory |

## API Design

No public API. Inside `crates/terraphim_agent/tests/ci_guards.rs`:

```rust
/// A known environment-mutation site: file (workspace-relative), the
/// call-shaped needle, and how many times it appears.
struct EnvMutationSite {
    file: &'static str,
    needle: &'static str,
    count: usize,
}

/// The call-shaped needles that indicate an environment mutation.
const ENV_MUTATION_NEEDLES: &[&str] = &[
    "env::set_var(",
    "env::remove_var(",
    "set_env_var(",
    "remove_env_var(",
    "EnvVarGuard",
];

/// Every environment-mutation site that is still allowed to exist.
const KNOWN_ENV_MUTATION_SITES: &[EnvMutationSite] = &[ ... ];

/// Collect (file, needle, count) for every environment-mutation site in `files`.
fn collect_env_mutation_sites(files: &[PathBuf]) -> Vec<(String, &'static str, usize)>;

/// Fail when a new environment-mutation site appears, or when an allow-list
/// entry names a site that no longer exists.
#[test]
fn no_new_environment_mutation_sites() { ... }
```

### New values introduced

| Value | Where produced | Where consumed | Traces to |
|-------|----------------|----------------|-----------|
| `ENV_MUTATION_NEEDLES` | constant | `collect_env_mutation_sites` | C1, C4 |
| `KNOWN_ENV_MUTATION_SITES` | constant | the test's comparison | C2, C4 |
| `collect_env_mutation_sites` | the scanner | the test, and a unit test | C1 |
| `EnvMutationSite` | the allow-list constant | the test | C2 |

Every value has a producer, a consumer and an acceptance criterion.

## Behaviour

1. `workspace_rust_sources()` gives the `.rs` files under `crates/`.
2. For each file, count the occurrences of each needle (call-shaped, so a
   mention in prose does not match).
3. Compare with the allow-list:
   - a (file, needle) pair with a **higher** count than listed -> **new
     site**: fail, naming file, needle and the extra occurrences;
   - a listed entry whose count is **lower** than listed -> **stale entry**:
     fail, naming file, needle and the expected/actual counts (the list must
     shrink in the same commit that removes a site);
   - a listed entry whose file no longer exists -> also a stale entry.
4. Manifest rule: while the allow-list still contains wrapper sites, the arm
   reports that `terraphim_test_utils` is still in use and does **not** fail;
   once no wrapper site remains, no member manifest may list
   `terraphim_test_utils` as a dependency, and the arm fails if one does
   (that is the re-entry path #375 calls load-bearing).

## Implementation Steps

1. Add `EnvMutationSite`, `ENV_MUTATION_NEEDLES`,
   `KNOWN_ENV_MUTATION_SITES` and `collect_env_mutation_sites` to
   `ci_guards.rs`.
2. Add the `no_new_environment_mutation_sites` test.
3. Add the manifest rule to the same test.
4. Add a unit test over a synthetic file list proving the scanner reports an
   unlisted site (the detector's own proof).
5. Seed the allow-list with the measured inventory (5 files, 23 sites).

## Test Strategy

| Level | What it proves |
|-------|----------------|
| Unit (`env_mutation_scanner_reports_an_unlisted_site`) | The detector finds a site that is not allow-listed |
| Guard (`no_new_environment_mutation_sites`) | The allow-list matches the tree exactly today |
| Manual proof | Adding one `unsafe { std::env::set_var(...) }` to a file under `crates/` makes the guard fail, naming file and line (reverted afterwards) |
| CI | The arm runs in `cargo test -p terraphim_agent --test ci_guards` and under `--all-targets` |

## Avoid At All Cost

| Item | Marker |
|------|--------|
| Making the guard fail while wrapper sites remain | Out-of-scope: the sweep has not happened; the arm would block the remediation train (#375 sequencing) |
| Shelling out to `grep`/`rg` for the scan | General: the native runner's command policy allows only `cargo` and repo scripts |
| Listing the `terraphim_test_utils` *definition* (it is another crate) | Out-of-scope: this repository can only guard call sites; the manifest rule covers the re-entry path |
| Replacing the remaining 23 sites in this PR | Out-of-scope: #374 and the UF-014 sweep own them |

## Risks

| Risk | Mitigation |
|------|------------|
| The allow-list becomes a rubber stamp | Stale entries fail, so the list must shrink with the remediation |
| A false positive on prose | The needles are call-shaped |
| The manifest rule surprises a contributor | The message names the dependency and the issue to use instead |

## Rollback

Test-only change; reverting the branch removes the arm.

## Gate Record: Phase 2 (Design) Quality Evaluation

| Dimension | Score | Justification |
|-----------|-------|---------------|
| Physical | 4/5 | Tables for constraints, API and behaviour; the allow-list shape is explicit. |
| Empirical | 4/5 | Counts and file names come from a scan of the tree. |
| Syntactic | 4/5 | Every constraint appears in the behaviour section. |
| Semantic | 4/5 | The domain (source scanning, manifest parsing, CI policy) is represented accurately. |
| Pragmatic | 5/5 | Five ordered steps, per-level test strategy, a manual proof of the detector, and a rollback statement. |
| Social | 3/5 | The conditional manifest rule is the author's interpretation of the acceptance list. |

**Average**: 4.0/5 | **Minimum**: 3/5 (Social)

### Essentialism

| Check | Status | Evidence |
|-------|--------|----------|
| Vital Few (<=5) | Pass | One test, one scanner, one allow-list, one manifest rule, one unit test. |
| Eliminated Noise | Pass | Four rejected alternatives with markers. |
| Effortless Path | Pass | Reuses `workspace_rust_sources()` and the existing `toml` dev-dependency. |
| 90% Rule | Pass | Every element maps to an acceptance criterion of #375. |

### Traceability

**Check 1 - Avoid At All Cost markers**: all four carry `Out-of-scope:` or
`General:` markers. **Pass.**

**Check 2 - new fields have references**:

| New value | In API Design | In Implementation Steps | Traces to |
|-----------|---------------|-------------------------|-----------|
| `ENV_MUTATION_NEEDLES` | Yes | 1, 2 | C1, C4 |
| `KNOWN_ENV_MUTATION_SITES` | Yes | 2, 5 | C2, C4 |
| `collect_env_mutation_sites` | Yes | 1, 4 | C1 |
| `EnvMutationSite` | Yes | 1 | C2 |

**Pass.**

### Decision

**CONDITIONAL PASS** - all dimensions >= 3, average 4.0 >= 3.5, both
traceability checks pass, essentialism satisfied. The conditional manifest rule
was published on #375 for confirmation before implementation.
