# Checklists

A checklist is a named set of categories a piece of text is expected to address — a pull
request description, a review comment, a commit message. `terraphim-agent validate --checklist`
reports which categories the text covers:

```
$ terraphim-agent validate --checklist code_review "Added unit tests and updated documentation. \
    Handled error handling paths. Ran a security review."
Checklist 'code_review' Validation for role 'AI Engineer':
  Passed: false
  Score: 4/5
  Satisfied items:
    ✓ tests (concept:unit test)
    ✓ documentation (concept:documentation)
    ✓ error_handling (concept:error handling)
    ✓ security (phrase:security)
  Missing items:
    ✗ performance
```

## Where definitions live

`crates/terraphim_agent/data/checklists/*.toml`, embedded into the binary at build time
(`include_str!`), so an installed `terraphim-agent` needs no data directory:

```toml
name = "code_review"
title = "Code review"

[[categories]]
name = "tests"
terms = ["tests", "test", "testing", "unit test", "integration test"]
```

Each category belongs to exactly one checklist. The checklist is the standard; the knowledge
graph only helps *recognise* it.

## How a category is credited

A category is satisfied when **either** proof holds:

1. **Concept** — a thesaurus pattern equal to one of the category's terms matched in the text.
   The report shows `(concept:<pattern>)`.
2. **Phrase** — one of the category's terms appears in the text as a whole-word phrase.
   The report shows `(phrase:<term>)`. Word boundaries are alphanumeric, so `security` matches
   "a security-check endpoint" and "SECURITY", but not "insecurity"; `error handling` matches
   "error-handling".

Both proofs exist because neither is sufficient alone. Matching runs leftmost-longest over
thesaurus patterns, so a text can match a *fragment* of a term — "Ran a security check" matches
`check`, a synonym of an unrelated concept — and a category whose terms have no pattern at all
can never be credited by the graph. In a typical graph only a minority of terms have patterns;
the phrase proof is what keeps the checklist usable, and `(concept:…)` versus `(phrase:…)` tells
you which one you are relying on.

### What this does and does not promise

A checklist is a **presence** check, not a truth check. "We did not test this" credits `tests`,
exactly as a keyword search would. It answers "did the author address this area", which is the
question a reviewer's checklist can honestly automate; it does not read the sentence.

## Unreachable categories

When no term of a category has a thesaurus pattern, the category is reported as unreachable:

```
  Missing items:
    ✗ performance (no knowledge-graph concept can credit this)
```

That is a statement about the graph, not the text: editing the prose cannot fix it. Two ways out:

- mention the term literally — the phrase proof credits it regardless of the graph;
- add a concept article to the graph with the terms on its `synonyms::` line.

## Adding or changing a checklist

1. Edit `data/checklists/<name>.toml`.
2. Run the gate: `python3 scripts/checklist-journey.py --refresh`.
3. Review the contract diff in `tests/checklist-concepts.json` — every term must be classified,
   and a term with no pattern is recorded as unbacked. Commit both.
4. `python3 scripts/checklist-journey.py` must be green; it runs in CI (`native-ci`, last step).

The refresh is deliberate friction: changing a standard should show up as a reviewed diff, not
as a silent shift in what the gate demands.

## Starter concepts

`docs/checklists/concepts/` contains nine concept articles — security, performance,
authentication, authorization, input validation, encryption, tests, documentation, error
handling — covering the 21 shipped checklist terms that a typical knowledge graph has no
pattern for. They are **documentation**: nothing installs them. Copy the files you need into
your graph directory (`~/.config/terraphim/kg/` by default) and the concept proof starts
working for those terms.

The gate fails if a checklist term is neither backed by the reference graph nor covered by a
starter concept, so these files cannot rot silently when a term is added.

## Command reference

```bash
terraphim-agent validate --checklist code_review "text"      # human report
terraphim-agent validate --checklist code_review "text" --json   # JSON: satisfied, missing,
                                                                 # evidence, unreachable
terraphim-agent validate --checklist nope "text"            # exit 2, lists available checklists
```

Exit codes: `0` the checklist ran (whether or not the text passed it — read `Passed`),
`2` unknown checklist name, `1` runtime error.
