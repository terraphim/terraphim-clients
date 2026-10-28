# Unit + Integration Test Traceability Matrix

**Change**: fix-coverage-pinning-guard
**Phase 2 Doc**: `docs/plans/design-fix-coverage-pinning-guard.md`
**Phase 2.5 Doc**: N/A (design declared no specification interview needed;
no behaviour specification beyond the guard contract)
**Implementation**: PR #345, commit `6c4b4e2`

## Coverage Summary

- New/modified functions: 1 modified test, 1 new helper
  (`native_ci_install_pin`), 1 new test module
- Helper branches: 9/9 covered by dedicated unit tests
- Guard end-to-end: verified against the real `.gitea/workflows/native-ci.yml`
  with local tools at the pins

## Unit Test Traceability (parser)

| Function | Test | Design Ref | Edge Case | Status |
|----------|------|------------|-----------|--------|
| `native_ci_install_pin` | `install_pin_happy_path` | Design "API Design" | Happy path, both tools, indented YAML | PASS |
| `native_ci_install_pin` | `install_pin_missing_tool` | Design test table | Zero pins → fail-closed panic | PASS |
| `native_ci_install_pin` | `install_pin_requires_version` | Design test table (deviation +1, see report D002) | `--version` absent → panic | PASS |
| `native_ci_install_pin` | `install_pin_requires_locked` | Design test table | `--locked` absent → panic | PASS |
| `native_ci_install_pin` | `install_pin_rejects_non_numeric_version` | Design "API Design" | Non-numeric version → panic | PASS |
| `native_ci_install_pin` | `install_pin_rejects_divergent_duplicates` | Design test table | Two distinct pins → panic | PASS |
| `native_ci_install_pin` | `install_pin_allows_identical_duplicates` | Design test table | Identical duplicates → single pin | PASS |
| `native_ci_install_pin` | `install_pin_token_boundary` | Design "API Design" | `cargo-llvm-coverage` must not satisfy `cargo-llvm-cov` | PASS |
| `native_ci_install_pin` | `install_pin_accepts_block_scalar_line` | Deviation D003 (defect fix) | Bare `cargo install` line inside `run: \|` block | PASS |

Branch-coverage notes:
- Non-matching lines skipped (`continue`) — exercised by every fixture's
  `- name:` lines.
- Optional `run:` prefix strip — exercised by happy-path fixtures and
  block-scalar fixture.
- Boundary reject (`rest` not whitespace-start) — exercised by
  `install_pin_token_boundary`.

## Integration Test Traceability

| Source | Target | Contract | Test | Data Flow Verified | Status |
|--------|--------|----------|------|-------------------|--------|
| `coverage_tool_pinning_matches_local_toolchain` | `.gitea/workflows/native-ci.yml` (real file, workspace root) | `cargo install <tool> --version <semver> --locked` line shape | `coverage_tool_pinning_matches_local_toolchain` (integration) | File read → pin parse → local `--version` resolution → equality assert | PASS |
| ci_guards (other four) | `native-ci.yml` token-alias lanes, `Cargo.lock` dupes, publish-gate script | Unchanged contracts | Same test binary, 4 tests | No regression vs `origin/main` behaviour | PASS |

Local versions at verification time: cargo-llvm-cov 0.8.5, cargo-nextest
0.9.144 — equal to the pins, so the equality asserts exercise the
match path (the drift-failure path is exercised by the unit-level
`should_panic` tests' symmetry and by the pre-fix failure mode itself,
which is documented history: run 36153).

## Gaps Identified

| Gap | Severity | Action | Status |
|-----|----------|--------|--------|
| Drift-failure path of the top-level assert not exercised locally (would require installing a wrong tool version) | Low | Accept: fail path proven by the original red-main incident (run 36153) and by unit-level panic symmetry | Closed (justified) |

## Requirements → Design → Code → Test (Summary)

| Research/Design Requirement | Design Section | Code | Test | Status |
|-----------------------------|----------------|------|------|--------|
| Single source of truth for coverage pins | Key Design Decisions | `native_ci_install_pin` reads `native-ci.yml` only | happy_path + integration guard | PASS |
| Fail-closed on missing/unpinned | Key Design Decisions | panics in 4 missing-shape branches | missing_tool, requires_version, requires_locked, rejects_non_numeric | PASS |
| Consistent pins (no divergence) | Key Design Decisions | HashSet dedup + divergent-panic | rejects_divergent, allows_identical | PASS |
| Versions unchanged (0.8.5 / 0.9.144) | Scope | Pins untouched in `native-ci.yml`; fixtures use same values | happy_path, integration guard vs real file | PASS |
| Other guards unaffected | Scope | No edits outside the coverage test block | 4 pre-existing guards pass | PASS |
