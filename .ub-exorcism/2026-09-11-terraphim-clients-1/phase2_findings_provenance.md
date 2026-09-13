# Phase 2 findings — bucket 2: provenance

**Verdict: NO_SURFACE** (closed at Phase 0/2 with evidence; no subagent fan-out).

## Evidence

Signature sweep over `crates/` (excluding target and .ub-exorcism), run 2026-09-11:

```
rg -n --type rust -e 'as usize as \*|as \*const|as \*mut|ptr::from_exposed_addr|with_addr|expose_provenance|from_raw_parts' crates
```

Hit count: **0**

```
<no output>
```

Corroboration: `scripts/rustonomicon-antipatterns.sh` reported 0 hits (phase2_raw/rustonomicon_antipatterns.txt) and `scripts/ast-grep-ub-patterns.sh` reported 0 hits for every bucket-specific pattern except the deliberately broad `alignment-repr-packed-field-ref` rule, whose hits are all on non-packed structs (`#[repr(packed)]` count is 0).

## Findings

None. Bucket has no static surface in this workspace. Re-open if Phase 1 macro-expansion rows or a Phase 6 idea introduce a site.
