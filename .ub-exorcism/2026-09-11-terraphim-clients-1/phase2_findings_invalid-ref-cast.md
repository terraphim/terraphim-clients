# Phase 2 findings — bucket 23: invalid-ref-cast

**Verdict: NO_SURFACE** (closed at Phase 0/2 with evidence; no subagent fan-out).

## Evidence

Signature sweep over `crates/` (excluding target and .ub-exorcism), run 2026-09-11:

```
rg -n --type rust -e '&\w+ as \*const \w+ as \*mut|cast_mut\(\)|\(&.* as \*const .*\) as \*mut' crates
```

Hit count: **0**

```
<no output>
```

Corroboration: `scripts/rustonomicon-antipatterns.sh` reported 0 hits (phase2_raw/rustonomicon_antipatterns.txt) and `scripts/ast-grep-ub-patterns.sh` reported 0 hits for every bucket-specific pattern except the deliberately broad `alignment-repr-packed-field-ref` rule, whose hits are all on non-packed structs (`#[repr(packed)]` count is 0).

## Findings

None. Bucket has no static surface in this workspace. Re-open if Phase 1 macro-expansion rows or a Phase 6 idea introduce a site.
