# Research, Design, Verification and Validation: role-scoped cache flush (#460)

**Issue**: terraphim-clients#460
**Author**: opencode-cli (agent session)
**Date**: 2026-11-12
**Mode**: `remediation` (explicit defect, explicit acceptance criteria, local change)

## Research

### Defect

`terraphim-agent cache flush --role "<role>"` printed "Flushed cache for role" while removing nothing.

Three facts, read from the code rather than assumed:

| Fact | Source |
|------|--------|
| A thesaurus is persisted under the **thesaurus** name: `thesaurus_{normalize_key(name)}.json` | `terraphim_persistence::thesaurus::get_key` |
| A role with `kg.automata_path` is persisted under the name **inside that file** (e.g. `compiled_corrections`), not the role name | `terraphim_service::thesaurus::ensure_thesaurus_loaded` |
| `save()` writes to **every** configured profile; `load()` reads only the fastest | `terraphim_persistence::thesaurus::save` / `DeviceStorage` |

The old flush built `thesaurus_{role.to_lowercase()}.json` (the role name, not the thesaurus name), deleted it from `storage.fastest_op` only, and printed success unconditionally - `delete` on a missing key returns `Ok`.

### Constraints carried from research

| # | Constraint | Consequence for the design |
|---|------------|----------------------------|
| C1 | The key must match the persistence key exactly | Use `Persistable::get_key` on a `Thesaurus` with the resolved name - the same code path the writer uses |
| C2 | A thesaurus lives in every profile | Delete from every profile, not only the fastest |
| C3 | A flush that removed nothing must not claim success | Report the number of entries actually removed |
| C4 | A role that cannot be mapped to a key must be rejected | Error out instead of flushing nothing |

## Design

New library module `crates/terraphim_agent/src/cache_flush.rs`:

- `role_thesaurus_keys(config, role_query) -> Result<Vec<String>>` - resolves the role
  (exact name, then case-insensitive shortname, then case-insensitive name) and
  returns the candidate keys: the role-derived name, plus the name inside the
  role's local `kg.automata_path` file when there is one. Unknown role ->
  error.
- `delete_key_everywhere(storage, key) -> Result<usize>` - deletes the key from
  every profile, counting only keys that were actually present.

`terraphim-agent cache flush` uses both, and reports:

- `Flushed N cache entries for role 'X'` when something was removed,
- `No cached thesaurus matched role 'X' (looked for <keys>)` when nothing was,
- a non-zero exit with the role named when the role cannot be mapped.

The role-less flush now also walks every profile instead of only the fastest.

## Verification

| Check | Result |
|---|---|
| `cargo test -p terraphim_agent --test cache_flush_tests` | 7 passed (5 unit + 2 CLI) |
| `cargo test -p terraphim_agent --all-targets --no-fail-fast` | 1206 passed, 0 failed |
| `cargo clippy -p terraphim_agent --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

## Validation

| Acceptance criterion (issue #460) | Result |
|---|---|
| "Role-scoped flush resolves and removes the actual normalized/persisted key(s) used by that role" | PASS - `role_with_compiled_automata_resolves_the_thesaurus_key` resolves `thesaurus_compiled_corrections.json` from the file's `name` field |
| "or rejects the operation when it cannot map a role to a key" | PASS - `unknown_role_is_rejected_instead_of_flushing_nothing`, `cli_flush_rejects_an_unknown_role` |
| "Durable and in-memory configured persistence profiles are handled consistently" | PASS - `delete_key_everywhere` walks `storage.ops` |
| "Output reports whether entries were actually removed" | PASS - `cli_flush_reports_when_nothing_matched` asserts the old unconditional success claim is gone |
| "Tests cover spaces/case in role names" | PASS - `role_lookup_accepts_case_and_shortname` |
| "The four-step reproduction returns `pnpm add x` after a successful flush" | PARTIALLY VERIFIED - the flush now removes the key the role's thesaurus is stored under, which is the step that was missing; the end-to-end `learn compile` -> `replace` round trip needs the `learn compile` fixture and is not reproduced here |

## Limits

- The role is resolved from the loaded configuration; a role that only exists in
  a project config that is not loaded cannot be flushed by name (the CLI's
  existing `--config` / project merge rules apply).
- A remote `kg.automata_path` cannot be inspected for its thesaurus name, so
  only the role-derived key is tried for such a role.
