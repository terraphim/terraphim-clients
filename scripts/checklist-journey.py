#!/usr/bin/env python3
"""Validate Terraphim checklists against knowledge-graph concepts, end to end.

`terraphim-agent validate --checklist <name>` reports which categories of a checklist
the text satisfies. Whether a category *can* be satisfied depends on concepts that
exist in the knowledge graph, and nothing tested that link: the checklist term lists
and the category maps live as two hardcoded literals in service.rs, and a term with
no concept silently makes its category unsatisfiable no matter what the text says.

This harness closes that gap in three layers:

  contract  - every checklist term is classified as concept-backed or a known gap
              (tests/checklist-concepts.json), every category term belongs to a
              checklist, and each checklist's relevant-category set is pinned. A new
              term, category or checklist that nobody classified fails here.

  journey   - the CLI is driven the way a user drives it: validate for each checklist,
              an unknown checklist name, and --json. Assertions are structural
              (passed == no missing items, score adds up, error lists the available
              checklists) plus, when a KG is present, the exact category outcome for
              each sample text.

  bench     - N validate invocations, p50/p95/max, compared against a budget so a
              regression in the matcher shows up as a slow test rather than a surprise.

Usage:
    python3 scripts/checklist-journey.py                 # contract + journey + bench
    python3 scripts/checklist-journey.py --refresh       # re-probe the KG, rewrite the contract
    python3 scripts/checklist-journey.py --json          # machine-readable report
    python3 scripts/checklist-journey.py --no-bench --no-live   # contract only (CI without a KG)

Exit: 0 all executed checks passed, 1 any failed.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SERVICE_RS = ROOT / "crates" / "terraphim_agent" / "src" / "service.rs"
CONTRACT = ROOT / "tests" / "checklist-concepts.json"
AGENT = os.environ.get("TERRAPHIM_AGENT_BIN", "terraphim-agent")
GREP = os.environ.get("TERRAPHIM_GREP_BIN", "terraphim-grep")
BENCH_BUDGET_MS = float(os.environ.get("CHECKLIST_BENCH_P95_MS", "2500"))


class Report:
    def __init__(self) -> None:
        self.checks: list[dict] = []

    def check(self, name: str, passed: bool, detail: str = "", skipped: bool = False,
              known: str = "") -> None:
        self.checks.append({"name": name, "passed": passed, "detail": detail,
                            "skipped": skipped, "known": known})

    @property
    def failures(self) -> list[dict]:
        return [c for c in self.checks if not c["passed"] and not c["skipped"]]

    @property
    def skips(self) -> list[dict]:
        return [c for c in self.checks if c["skipped"]]


def clean(out: str) -> str:
    """Drop tracing noise so assertions read the command's own answer.

    This used to be done by piping through `grep -v WARN`, which is how the harness
    came to report the wrong exit code: with a pipeline, the status is grep's, not the
    binary's. Filtering in process keeps the real `returncode` intact."""
    kept = []
    for line in out.splitlines():
        s = line.strip()
        if s.startswith("["):                      # [2026-11-10T10:21:18Z INFO ...]
            continue
        if " INFO " in line or " WARN " in line:
            continue
        if s.startswith("auto-route"):
            continue
        kept.append(line)
    return "\n".join(kept)


def run(cmd: str, timeout: int = 120) -> tuple[int, str]:
    p = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=timeout)
    return p.returncode, clean(f"{p.stdout}\n{p.stderr}")


def parse_checklists(source: str) -> tuple[dict, dict]:
    """The two literals in validate_checklist, parsed rather than duplicated."""
    # The literal has lived in two places: inline in validate_checklist, and (after the
    # exit-code fix in #476) in checklist_definitions(). Match either rather than pinning
    # one shape - a refactor that moves it should not break the parser, only a refactor
    # that changes the data should.
    ck = source.split("let checklists = std::collections::HashMap::from([", 1)
    if len(ck) != 2:
        fn_start = source.find("pub fn checklist_definitions()")
        if fn_start != -1:
            ck = source[fn_start:].split("HashMap::from([", 1)
            if len(ck) == 2:
                ck = ["", ck[1]]
    if len(ck) != 2:
        raise SystemExit("service.rs: cannot find the checklist HashMap entries "
                         "(looked for an inline literal and for checklist_definitions())")
    block = ck[1].split("]);", 1)[0]
    checklists = {
        m.group(1): re.findall(r'"([^"]+)"', m.group(2))
        for m in re.finditer(r'\(\s*"([a-z_]+)",\s*vec!\[(.*?)\]\s*,?\s*\)', block, re.S)
    }
    cat = source.split("let categories = vec![", 1)
    if len(cat) != 2:
        raise SystemExit("service.rs: cannot find the categories vec literal")
    block = cat[1].split("];", 1)[0]
    categories = {
        m.group(1): re.findall(r'"([^"]+)"', m.group(2))
        for m in re.finditer(r'\(\s*"([a-z_]+)",\s*vec!\[(.*?)\]\s*,?\s*\)', block, re.S)
    }
    return checklists, categories


def relevant_categories(checklist_terms: list[str], categories: dict[str, list[str]]) -> list[str]:
    """Mirror of the filter in validate_checklist: a category is relevant when any of
    its terms is listed in the checklist."""
    return [c for c, terms in categories.items() if any(t in checklist_terms for t in terms)]


def probe_kg(term: str) -> bool:
    """Does any KG concept match this term? `extract` reports the KG terms it finds in
    the given text, so it doubles as a concept probe.

    Match the result line, not the substring "Found" anywhere in the output: the binary
    logs messages such as "Found role_config in settings.toml", which made every term
    look concept-backed (28/28) on the first run of this check."""
    rc, out = run(f'{AGENT} extract "{term}"')
    return rc == 0 and re.search(r"^\s*Found \d+ paragraph", out, re.M) is not None


def validate(checklist: str, text: str, as_json: bool = False) -> tuple[int, str]:
    flag = " --json" if as_json else ""
    return run(f'{AGENT} validate --checklist {checklist}{flag} "{text}"')


def parse_human(out: str) -> dict:
    """Read the human report: passed, score, and the satisfied/missing category names."""
    m = re.search(r"Passed:\s*(true|false)", out)
    score = re.search(r"Score:\s*(\d+)/(\d+)", out)
    satisfied = re.findall(r"✓\s*([a-z_]+)", out)
    missing = re.findall(r"✗\s*([a-z_]+)", out)
    return {
        "passed": (m.group(1) == "true") if m else None,
        "score": (int(score.group(1)), int(score.group(2))) if score else None,
        "satisfied": satisfied,
        "missing": missing,
    }


def contract_checks(report: Report, checklists: dict, categories: dict, contract: dict) -> None:
    terms = {t for ts in checklists.values() for t in ts}
    declared = contract.get("concepts", {})

    unclassified = sorted(terms - set(declared))
    report.check(
        "every checklist term is classified in the contract",
        not unclassified,
        f"unclassified: {unclassified}" if unclassified else f"{len(terms)} terms classified",
    )

    stale = sorted(set(declared) - terms)
    report.check(
        "contract has no terms the code dropped",
        not stale,
        f"stale contract entries: {stale}" if stale else "no stale entries",
    )

    orphans = {c: [t for t in ts if t not in terms] for c, ts in categories.items()}
    orphans = {c: v for c, v in orphans.items() if v}
    report.check(
        "every category term belongs to a checklist",
        not orphans,
        f"orphan category terms: {orphans}" if orphans else f"{len(categories)} categories consistent",
    )

    expected = contract.get("relevant_categories", {})
    actual = {name: relevant_categories(ts, categories) for name, ts in checklists.items()}
    mismatch = {k: (actual.get(k), expected.get(k)) for k in set(actual) | set(expected) if actual.get(k) != expected.get(k)}
    report.check(
        "relevant-category set per checklist is unchanged",
        not mismatch,
        f"drift: {mismatch}" if mismatch else str(actual),
    )

    # a category with no concept behind any of its terms can never be satisfied
    unreachable = {
        name: [c for c in cats if not any(declared.get(t, {}).get("covered") for t in categories[c])]
        for name, cats in actual.items()
    }
    unreachable = {k: v for k, v in unreachable.items() if v}
    recorded_unreachable = contract.get("defects", {}).get("unsatisfiable_by_concept")
    if recorded_unreachable is None:
        report.check("unsatisfiable categories are recorded", False,
                     "add defects.unsatisfiable_by_concept to the contract (run --refresh)")
    else:
        same = recorded_unreachable == unreachable
        report.check(
            "unsatisfiable categories unchanged since the contract was measured",
            same,
            f"now {unreachable}, contract says {recorded_unreachable} - re-measure deliberately"
            if not same else f"{unreachable or 'none'}",
            known=("no concept credits these categories, so a text-only pass is impossible: "
                   + ", ".join(f"{k}:{'/'.join(v)}" for k, v in unreachable.items())) if unreachable else "",
        )

    recorded = contract.get("reachable_categories")
    if recorded is None:
        report.check("per-category reachability is recorded", False, "run --refresh to measure it")
    elif not probe_kg("unit test"):
        report.check("per-category reachability (measured live)", True,
                     "no knowledge graph on this host", skipped=True)
    else:
        measured = measure_reachability(checklists, categories)
        drift = {n: {c: (measured.get(n, {}).get(c), recorded.get(n, {}).get(c))
                     for c in set(measured.get(n, {})) | set(recorded.get(n, {}))}
                 for n in set(measured) | set(recorded)}
        drift = {n: {c: v for c, v in d.items() if v[0] != v[1]} for n, d in drift.items()}
        drift = {n: d for n, d in drift.items() if d}
        dead = {n: sorted(c for c, ok in cats.items() if not ok) for n, cats in measured.items()}
        dead = {n: v for n, v in dead.items() if v}
        report.check(
            "reachable categories match the contract",
            not drift,
            f"drift (measured, contract): {drift}; categories no text can satisfy: {dead}"
            if drift else f"unchanged; still unsatisfiable: {dead or 'none'}",
        )


def journey_checks(report: Report, checklists: dict, categories: dict, contract: dict, live: bool) -> None:
    for name in checklists:
        # structural invariants hold whatever the graph contains
        rc, out = validate(name, "some text mentioning nothing in particular")
        parsed = parse_human(out)
        ok = (
            rc == 0
            and parsed["passed"] is False
            and parsed["score"] is not None
            and parsed["score"][0] == len(parsed["satisfied"])
            and parsed["score"][1] == len(parsed["satisfied"]) + len(parsed["missing"])
            and parsed["passed"] == (not parsed["missing"])
        )
        report.check(f"validate --checklist {name}: report adds up", ok, out.strip()[-300:] or "no output")

    rc, out = validate("no_such_checklist", "text")
    named = {n for n in re.findall(r"[\"']([a-z_]+)[\"']", out) if n != "no_such_checklist"}
    report.check(
        "unknown checklist names the available ones",
        set(checklists).issubset(named),
        f"available checklists named: {sorted(named)}",
    )
    expected_rc = contract.get("expectations", {}).get("unknown_checklist_exit_code")
    report.check(
        "unknown checklist exit code matches the contract",
        expected_rc is not None and rc == expected_rc,
        f"exit={rc}, contract says {expected_rc} - re-measure deliberately if this changed",
    )

    rc, out = validate(list(checklists)[0], "unit tests and documentation", as_json=True)
    payload = None
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                payload = json.loads(line)
                break
            except json.JSONDecodeError:
                continue
    report.check(
        "--json emits parseable JSON",
        payload is not None,
        "parsed" if payload else out.strip()[-200:] or "no JSON in output",
    )

    if not live:
        report.check("live KG assertions", True, "no knowledge graph on this host", skipped=True)
        return

    for sample in contract.get("samples", []):
        rc, out = validate(sample["checklist"], sample["text"])
        parsed = parse_human(out)
        ok = (
            rc == 0
            and parsed["passed"] == sample["passed"]
            and sorted(parsed["missing"]) == sorted(sample["missing"])
        )
        report.check(
            f"live KG: {sample['label']}",
            ok,
            f"expected passed={sample['passed']} missing={sample['missing']}, got "
            f"passed={parsed['passed']} missing={parsed['missing']}",
        )

    # the second half of the journey: same concepts, searched rather than validated.
    # Live-only: without a knowledge graph there are no haystacks to search.
    rc, out = run(f'{GREP} checklist -n 5 2>&1 | tail -20', timeout=180)
    hits = len([l for l in out.splitlines() if l.strip()])
    report.check(
        "terraphim-grep returns hits for a concept the KG knows",
        rc == 0 and hits > 0,
        f"exit={rc} lines={hits}",
    )


def bench_checks(report: Report, checklists: dict) -> None:
    name = list(checklists)[0]
    text = "unit tests documentation error handling security check performance optimization"
    samples: list[float] = []
    for _ in range(int(os.environ.get("CHECKLIST_BENCH_RUNS", "15"))):
        t0 = time.perf_counter()
        validate(name, text)
        samples.append((time.perf_counter() - t0) * 1000)
    samples.sort()
    p50 = statistics.median(samples)
    p95 = samples[max(0, int(len(samples) * 0.95) - 1)]
    report.check(
        f"bench: validate p95 within {BENCH_BUDGET_MS:.0f} ms",
        p95 <= BENCH_BUDGET_MS,
        f"p50={p50:.0f} ms p95={p95:.0f} ms max={samples[-1]:.0f} ms over {len(samples)} runs",
    )


def measure_reachability(checklists: dict, categories: dict) -> dict:
    """Can a category actually be satisfied? Probe it the only way that cannot lie: feed
    the category's own terms to the CLI and see whether it credits the category. Term
    coverage is not evidence - a concept can match a term and still fail to credit the
    category (observed: 'security check')."""
    out = {}
    for name, terms in checklists.items():
        out[name] = {}
        for cat in relevant_categories(terms, categories):
            _, text = validate(name, " ".join(categories[cat]))
            out[name][cat] = cat in parse_human(text)["satisfied"]
    return out


def refresh_contract(checklists: dict, categories: dict, contract: dict) -> dict:
    concepts = {}
    for term in sorted({t for ts in checklists.values() for t in ts}):
        concepts[term] = {"covered": probe_kg(term)}
    contract["concepts"] = concepts
    contract["relevant_categories"] = {n: relevant_categories(ts, categories) for n, ts in checklists.items()}
    contract["reachable_categories"] = measure_reachability(checklists, categories)
    unreachable = {}
    for name, terms in checklists.items():
        dead = [c for c in relevant_categories(terms, categories)
                if not any(concepts[t]["covered"] for t in categories[c] if t in concepts)]
        if dead:
            unreachable[name] = dead
    contract.setdefault("defects", {})["unsatisfiable_by_concept"] = unreachable
    contract.setdefault("expectations", {})["unknown_checklist_exit_code"] = validate("__nope__", "text")[0]
    # the exit code is an expectation, not a defect; drop the pre-rename key if present
    contract.get("defects", {}).pop("unknown_checklist_exit_code", None)
    return contract



def load_concepts(kg: Path) -> dict[str, list[str]]:
    """Every concept article in the graph: file stem plus its `synonyms::` list."""
    concepts = {}
    for f in sorted(kg.glob("*.md")):
        syn: list[str] = []
        for line in f.read_text(errors="replace").splitlines():
            if line.lower().startswith("synonyms::"):
                syn = [s.strip() for s in line.split("::", 1)[1].split(",") if s.strip()]
        concepts[f.stem] = syn
    return concepts


def write_inventory(kg: Path, checklists: dict, categories: dict, contract: dict) -> str:
    """Which concept backs each checklist term, and which terms have none.

    This is the actionable half: a term with no concept cannot credit its category,
    so the list of unbacked terms is the list of articles someone has to write."""
    concepts = load_concepts(kg)
    reachable = contract.get("reachable_categories", {})
    lines = [
        "# Checklist terms against knowledge-graph concepts",
        "",
        f"Generated by `scripts/checklist-journey.py --inventory` from `{kg}` "
        f"({len(concepts)} concept articles). Do not edit by hand.",
        "",
        "A category is satisfied only when a *concept* credits one of its terms. A term "
        "with no backing concept makes its category unsatisfiable, whoever writes the text.",
        "",
        "Two different columns, deliberately: **literal match** is a static search of concept "
        "names and `synonyms::` lines, while **category reachable** is measured by the CLI "
        "itself. Where they disagree the measurement is authoritative - the matcher resolves "
        "more than literal substrings, and guessing at that would be worse than saying so.",
        "",
    ]
    gaps: list[tuple[str, str, str]] = []
    for name, terms in checklists.items():
        lines += [f"## `{name}`", "", "| category | term | literal match in KB | category reachable (measured) |", "|---|---|---|---|"]
        for cat in relevant_categories(terms, categories):
            for term in categories[cat]:
                if term not in terms:
                    continue
                backing = sorted(
                    c for c, syn in concepts.items()
                    if term in c.lower() or any(term in s.lower() for s in syn)
                )
                if not backing:
                    gaps.append((name, cat, term))
                lines.append(
                    f"| {cat} | `{term}` | {', '.join(f'`{b}`' for b in backing) or '_none_'} | "
                    f"{'yes' if reachable.get(name, {}).get(cat) else '**no**'} |")
        lines.append("")

    disagreements = []
    for name, terms in checklists.items():
        for cat in relevant_categories(terms, categories):
            backed = any(
                term in c.lower() or any(term in s.lower() for s in syn)
                for term in categories[cat] if term in terms
                for c, syn in concepts.items()
            )
            if not backed and reachable.get(name, {}).get(cat):
                disagreements.append((name, cat))

    if disagreements:
        lines += ["## Measured reachable but no literal match", "",
                  "The CLI credits these categories although no concept name or synonym "
                  "contains the checklist term literally. Whoever fixes the gap in #474 needs "
                  "to know the matcher is not doing plain substring matching:", ""]
        lines += [f"- `{n}` / `{c}`" for n, c in disagreements]
        lines.append("")

    # terms the CLI matches even though nothing names them literally: the asymmetry
    # behind #475 - a match that never reaches the category check
    invisible = []
    for term in sorted({t for ts in checklists.values() for t in ts}):
        literal = any(term in c.lower() or any(term in s.lower() for s in syn)
                      for c, syn in concepts.items())
        if not literal and probe_kg(term):
            invisible.append(term)
    if invisible:
        lines += ["## Matched by the CLI but named by no concept", "",
                  "`extract` reports a hit for these terms while no concept name or synonym "
                  "contains them. That is the mechanism behind unmatched-but-present concepts: "
                  "the matcher resolves more than literal text, and the category check then "
                  "looks up the canonical term and finds nothing (#475).", ""]
        lines += [f"- `{term}`" for term in invisible]
        lines.append("")

    if gaps:
        lines += ["## Terms with no backing concept", "",
                  "Write a concept article whose `synonyms::` line carries the term, or drop the "
                  "term from the checklist. Until one of those happens the category cannot be "
                  "credited.", "", "| checklist | category | term |", "|---|---|---|"]
        lines += [f"| {n} | {c} | `{t}` |" for n, c, t in gaps]
        lines.append("")

    md = "\n".join(lines) + "\n"
    out = ROOT / "docs" / "checklist-concept-coverage.md"
    out.parent.mkdir(exist_ok=True)
    out.write_text(md)
    return str(out)

def main() -> int:
    ap = argparse.ArgumentParser(description="Validate Terraphim checklists against KG concepts")
    ap.add_argument("--refresh", action="store_true", help="re-probe the knowledge graph and rewrite the contract")
    ap.add_argument("--json", action="store_true", help="emit a JSON report")
    ap.add_argument("--no-bench", action="store_true", help="skip the latency bench")
    ap.add_argument("--no-live", action="store_true", help="skip assertions that need a knowledge graph")
    ap.add_argument("--inventory", action="store_true",
                    help="write docs/checklist-concept-coverage.md: every checklist term and the concept behind it")
    ap.add_argument("--kg", default=str(Path.home() / ".config" / "terraphim" / "kg"),
                    help="knowledge-graph directory to read concepts from")
    args = ap.parse_args()

    for tool in (AGENT,):
        if shutil.which(tool) is None:
            raise SystemExit(f"{tool} not found on PATH")
    if not SERVICE_RS.is_file():
        raise SystemExit(f"cannot read {SERVICE_RS}")

    checklists, categories = parse_checklists(SERVICE_RS.read_text())
    if not checklists:
        raise SystemExit("parsed no checklists from service.rs")

    contract = json.loads(CONTRACT.read_text()) if CONTRACT.is_file() else {}

    if args.refresh:
        contract = refresh_contract(checklists, categories, contract)
        CONTRACT.write_text(json.dumps(contract, indent=2, sort_keys=True) + "\n")
        print(f"contract refreshed: {CONTRACT} ({sum(1 for v in contract['concepts'].values() if v['covered'])}"
              f"/{len(contract['concepts'])} terms concept-backed)")
        return 0

    if args.inventory:
        print(f"inventory written: {write_inventory(Path(args.kg), checklists, categories, contract)}")
        return 0

    live = not args.no_live and probe_kg("unit test")

    report = Report()
    contract_checks(report, checklists, categories, contract)
    journey_checks(report, checklists, categories, contract, live)
    if not args.no_bench:
        bench_checks(report, checklists)

    if args.json:
        print(json.dumps({"checks": report.checks, "failures": len(report.failures),
                          "skipped": len(report.skips), "live_kg": live}, indent=1))
    else:
        for c in report.checks:
            mark = "SKIP" if c["skipped"] else ("KNOWN" if c["passed"] and c.get("known") else
                ("ok  " if c["passed"] else "FAIL"))
            print(f"[{mark}] {c['name']}")
            if not c["passed"] and not c["skipped"]:
                print(f"       {c['detail']}")
            elif c.get("known"):
                print(f"       known defect: {c['known']}")
        print(f"\n{len(report.checks) - len(report.failures) - len(report.skips)} passed, "
              f"{len(report.failures)} failed, {len(report.skips)} skipped"
              f"{' (no KG on this host: live assertions skipped)' if not live else ''}")
        if report.failures:
            print("\nchecklist validation is not trustworthy until these fail:", file=sys.stderr)
            for c in report.failures:
                print(f"  {c['name']}: {c['detail']}", file=sys.stderr)

    return 1 if report.failures else 0


if __name__ == "__main__":
    sys.exit(main())
