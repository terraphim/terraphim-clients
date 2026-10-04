# ADR 002: Guard Priority Order (Allowlist > Destructive > Suspicious > Default)

Status: Accepted

Date: 2026-08-30

## Context

`terraphim-agent guard` (and the `--with-guard` arm of
`terraphim-agent hook --hook-type pre-tool-use`) checks every command
against three thesaurus-backed Aho-Corasick matchers before falling
through to a default decision:

1. **Allowlist** — patterns the user has explicitly opted into
   (recursive delete in `/tmp/`, `/var/folders/`, etc.). Embedded in
   `crates/terraphim_agent/data/guard_allowlist.json`.
2. **Destructive** — patterns the guard must reject
   (`rm -rf`, `git reset --hard`, `git push --force`, `kubectl delete`,
   `DROP TABLE`, etc.). Embedded in `guard_destructive.json`.
3. **Suspicious** — patterns the guard must sandbox or warn on
   (`curl ... | sh`, `chmod 777`, etc.). Embedded in
   `guard_suspicious.json`.
4. **Default** — fall-through when nothing matched.

The question this ADR answers: **in what order should these stages
run, and which one wins when more than one matches?**

Example motivating conflict: `rm -rf /tmp/foo` matches both the
allowlist (`rm -rf /tmp/`) and the destructive pattern (`rm -rf`).
If the destructive stage ran first and blocked, every user that has
opted into recursive deletes in `/tmp/` would have to disable the
guard entirely — a usability cliff.

## Decision

The stages run in **allowlist > destructive > suspicious > default** order
and the **first stage that matches short-circuits the remaining ones**.
The implementation lives in
`crates/terraphim_agent/src/guard_patterns.rs::CommandGuard::check`
(and `check_with_trace` for the `--explain` trace).

1. **Allowlist first**. A match is `Allow` regardless of what the
   destructive or suspicious stages would say. This makes the
   allowlist a true override and the most predictable place to
   express user intent.
2. **Destructive second**. A match is `Block` and short-circuits.
3. **Suspicious third**. A match is `Sandbox`.
4. **Default last**. Fall-through `Allow`.

Each stage's thesaurus uses **fail-open** semantics: if the JSON is
malformed or fails to load, that specific stage is skipped (logged at
debug level) rather than aborting the whole check. Failures cascade
down to the next stage; the overall decision is the first
non-failure match.

The order is exposed to users via `terraphim-agent guard --explain`
(see #129). Without `--explain`, the guard is silent on `Allow` and
prints `BLOCKED: <reason>` to stderr with exit code 1 on `Block`.

## Consequences

- **Positive**: the allowlist is a true opt-in escape hatch. Users
  who need `rm -rf /tmp/foo` in their loop scripts do not need to
  disable the guard.
- **Positive**: the priority is auditable. `--explain` prints the
  per-stage trace (`stage=allowlist matched=true outcome=allow`), so
  users debugging "why did this pass?" can see which stage
  short-circuited.
- **Positive**: fail-open per stage keeps a malformed custom
  allowlist from breaking the destructive check.
- **Negative / residual risk**: a malicious or stale destructive
  pattern that overlaps an allowlist entry will be silently bypassed.
  This is acceptable because (a) the allowlist is curated and
  reviewed in PRs, (b) users can run `terraphim-agent guard --explain`
  to audit, and (c) the destructive thesaurus is embedded at compile
  time, so it is not user-mutable.
- **Negative**: adding a new stage in the middle of the priority list
  is a behaviour change that requires an ADR revision (no
  silent additions to the priority chain).

## Alternatives considered

- **Destructive > Allowlist** (block-first): rejected — would force
  every user who legitimately needs `rm -rf /tmp/foo` to disable the
  guard or to override per-call. The safety net would only ever fire
  for users who don't need it.
- **Destructive > Suspicious > Allowlist > Default**: rejected —
  makes the allowlist unreachable in practice, since any command
  with both an allowlist pattern and a destructive pattern would
  block before the allowlist is consulted.
- **Most-specific match wins**: rejected — adds an arbitrary
  specificity metric (term length? number of characters? number of
  Aho-Corasick matches?) that is hard to explain and audit.
- **Allowlist, Destructive, Suspicious all run, vote**: rejected —
  makes the decision non-deterministic from the user's perspective
  and impossible to reason about with `--explain`.

## References

- Source: `crates/terraphim_agent/src/guard_patterns.rs`
- README section: `crates/terraphim_agent/README.md` "Safety guard"
- Test: `crates/terraphim_agent/tests/guard_priority.rs` (Refs #129)
- Hook integration: `crates/terraphim_agent/src/main.rs` lines
  around the pre-tool-use `--with-guard` arm (Refs #126)
