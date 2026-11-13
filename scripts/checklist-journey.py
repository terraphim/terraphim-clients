#!/usr/bin/env python3
"""Validate Terraphim checklists against knowledge-graph concepts, end to end.

`terraphim-agent validate --checklist <name>` reports which categories of a checklist
a text satisfies. Three things can go wrong, and nothing used to notice any of them:

  * a checklist term with no thesaurus pattern behind it makes its category
    unreachable for every text;
  * the matcher is leftmost-longest over patterns, so a text can match a *fragment*
    of a term (`check` instead of `security check`) that never credits the category;
  * the definition itself can drift - a term added, a category moved - without anyone
    measuring what it did to pass rates.

This harness closes all three, in three layers:

  contract  - every term in `data/checklists/*.toml` is classified as backed by an
              exact thesaurus pattern or not, every category is owned by exactly one
              checklist, and the category set per checklist plus the measured
              reachability matrix are pinned. An unrecorded change fails here.

  journey   - the CLI is driven the way a user drives it: validate for each checklist,
              an unknown checklist name, and --json. Structural assertions always run;
              assertions about specific texts run when a knowledge graph is present and
              are reported as *skipped* (never passed) when it is not.

  bench     - N validate invocations, p50/p95/max against a budget, so a regression in
              the matcher shows up as a slow test rather than a surprise.

Usage:
    python3 scripts/checklist-journey.py                 # contract + journey + bench
    python3 scripts/checklist-journey.py --refresh       # re-measure, rewrite the contract
    python3 scripts/checklist-journey.py --inventory     # write the coverage document
    python3 scripts/checklist-journey.py --json          # machine-readable report
    python3 scripts/checklist-journey.py --no-live --no-bench   # contract only (CI without a KG)

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
CHECKLIST_DIR = ROOT / "crates" / "terraphim_agent" / "data" / "checklists"
CONTRACT = ROOT / "tests" / "checklist-concepts.json"
AGENT = os.environ.get("TERRAPHIM_AGENT_BIN", "terraphim-agent")
GREP = os.environ.get("TERRAPHIM_GREP_BIN", "terraphim-grep")
KG_DIR = Path(os.environ.get("TERRAPHIM_KG_DIR", str(Path.home() / ".config" / "terraphim" / "kg")))
BENCH_BUDGET_MS = float(os.environ.get("CHECKLIST_BENCH_P95_MS", "2500"))
SYNONYMS = re.compile(r"^\s*synonyms\s*::?\s*(.+)$", re.I)


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
    came to report the wrong exit code: with a pipeline the status is grep's, not the
    binary's. Filtering in process keeps the real `returncode` intact."""
    kept = []
    for line in out.splitlines():
        s = line.strip()
        if s.startswith("["):
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


# --------------------------------------------------------------------------- data

def parse_checklists(directory: Path = CHECKLIST_DIR) -> dict[str, dict[str, list[str]]]:
    """Read `data/checklists/*.toml`: checklist -> category -> terms.

    Parsed from the shipped data rather than by regex over service.rs. The old approach
    broke the moment the Rust literal moved (#477) and could not tell a data change from
    a refactor."""
    try:
        import tomllib
    except ModuleNotFoundError:
        raise SystemExit("python 3.11+ required (tomllib)")

    if not directory.is_dir():
        raise SystemExit(f"checklist directory not found: {directory}")

    definitions: dict[str, dict[str, list[str]]] = {}
    for path in sorted(directory.glob("*.toml")):
        data = tomllib.loads(path.read_text())
        name = data["name"].lower()
        categories: dict[str, list[str]] = {}
        for raw in data.get("categories", []):
            cat = raw["name"].lower()
            if cat in categories:
                raise SystemExit(f"{path.name}: category '{cat}' declared twice")
            categories[cat] = [t.lower() for t in raw.get("terms", [])]
        if not categories:
            raise SystemExit(f"{path.name}: checklist '{name}' has no categories")
        definitions[name] = categories
    if not definitions:
        raise SystemExit(f"no checklist definitions in {directory}")
    return definitions


def all_terms(definitions: dict[str, dict[str, list[str]]]) -> set[str]:
    return {t for cats in definitions.values() for terms in cats.values() for t in terms}


def kg_patterns(kg: Path) -> set[str]:
    """Every thesaurus pattern the graph can produce: file stems plus `synonyms::` entries.

    This is the sound coverage measure. The previous probe fed a term to `extract` as
    text, which reports any pattern contained in the probe - so `security check` looked
    concept-backed because `check` matched inside it."""
    patterns: set[str] = set()
    skipped: list[str] = []
    try:
        candidates = list(kg.rglob("*.md"))
    except OSError:
        return patterns
    for f in candidates:
        # A graph directory can exist and still hold nothing usable: dangling symlinks
        # (a synced or partially-copied graph is the usual cause), permission-denied
        # files, or a file removed mid-scan. Crashing here fails the whole gate for a
        # reason that has nothing to do with checklists.
        if not f.is_file():
            skipped.append(str(f))
            continue
        try:
            text = f.read_text(errors="replace")
        except OSError:
            skipped.append(str(f))
            continue
        patterns.add(f.stem.strip().lower())
        for line in text.splitlines():
            m = SYNONYMS.match(line)
            if m:
                for item in m.group(1).split(","):
                    item = item.strip().lower()
                    if item:
                        patterns.add(item)
    if skipped:
        print(f"warning: skipped {len(skipped)} unreadable concept file(s) under {kg} "
              f"(first: {skipped[0]})", file=sys.stderr)
    return patterns


def unbacked_terms(definitions, patterns: set[str]) -> list[str]:
    return sorted(t for t in all_terms(definitions) if t not in patterns)


def unreachable_categories(definitions, patterns: set[str]) -> dict[str, list[str]]:
    out: dict[str, list[str]] = {}
    for name, categories in definitions.items():
        dead = [c for c, terms in categories.items() if not any(t in patterns for t in terms)]
        if dead:
            out[name] = dead
    return out


# ----------------------------------------------------------------------- commands

def has_kg() -> bool:
    return KG_DIR.is_dir() and bool(kg_patterns(KG_DIR))


def is_reference_graph(patterns: set[str], contract: dict) -> bool:
    """Is this host's graph the one the contract was measured against?

    Coverage and reachability numbers are properties of a specific graph. A CI runner
    with no graph, or with a different one, cannot verify them - and must not pretend
    the numbers drifted. Only the contract's own digest may decide this."""
    recorded = contract.get("coverage", {})
    if not patterns or recorded.get("digest") is None:
        return False
    return pattern_digest(patterns) == recorded["digest"]


def pattern_digest(patterns: set[str]) -> str:
    import hashlib

    joined = "\n".join(sorted(patterns))
    return hashlib.sha256(joined.encode()).hexdigest()[:16]


def validate(checklist: str, text: str, as_json: bool = False) -> tuple[int, str]:
    flag = " --json" if as_json else ""
    return run(f'{AGENT} validate --checklist {checklist}{flag} "{text}"')


def parse_human(out: str) -> dict:
    m = re.search(r"Passed:\s*(true|false)", out)
    score = re.search(r"Score:\s*(\d+)/(\d+)", out)
    return {
        "passed": (m.group(1) == "true") if m else None,
        "score": (int(score.group(1)), int(score.group(2))) if score else None,
        "satisfied": re.findall(r"✓\s*([a-z_]+)", out),
        "missing": re.findall(r"✗\s*([a-z_]+)", out),
    }


def measure_reachability(definitions, patterns: set[str]) -> dict[str, dict[str, bool]]:
    """Can each category be credited at all when it is the only thing addressed?

    Measured by feeding the category's own terms to the CLI: a category that cannot be
    credited for its own words cannot be credited for anything. Term coverage is not
    evidence - matching is leftmost-longest, so a category can have a pattern and still
    lose to a longer one."""
    out: dict[str, dict[str, bool]] = {}
    for name, categories in definitions.items():
        out[name] = {}
        for cat, terms in categories.items():
            # Always ask the CLI, even when no pattern exists for any term: since the
            # phrase fallback landed, a category with no graph pattern can still be
            # credited by its own words. Skipping the call hid exactly that.
            _, text = validate(name, " ".join(terms))
            out[name][cat] = cat in parse_human(text)["satisfied"]
    return out


# ------------------------------------------------------------------------- checks

def _recorded_reach_skip(report: Report, contract: dict, early_return: bool = True) -> None:
    """Reachability is measured by running the CLI against a graph; without the reference
    graph there is nothing to compare, so say so instead of reporting drift."""
    if early_return:
        report.check("reachability matches the contract", True,
                     "not the reference graph on this host", skipped=True)


def contract_checks(report: Report, definitions, patterns: set[str], contract: dict,
                    reference: bool) -> None:
    terms = all_terms(definitions)
    declared = contract.get("concepts", {})

    unclassified = sorted(terms - set(declared))
    report.check("every checklist term is classified in the contract", not unclassified,
                 f"unclassified: {unclassified}" if unclassified else f"{len(terms)} terms classified")

    stale = sorted(set(declared) - terms)
    report.check("contract has no terms the definitions dropped", not stale,
                 f"stale: {stale}" if stale else "no stale entries")

    empty = {n: [c for c, ts in cats.items() if not ts] for n, cats in definitions.items()}
    empty = {n: v for n, v in empty.items() if v}
    report.check("every category has at least one term", not empty, str(empty) if empty else "ok")

    expected = contract.get("relevant_categories", {})
    actual = {name: list(cats) for name, cats in definitions.items()}
    mismatch = {k: (actual.get(k), expected.get(k)) for k in set(actual) | set(expected)
                if actual.get(k) != expected.get(k)}
    report.check("category set per checklist is unchanged", not mismatch,
                 f"drift: {mismatch}" if mismatch else str(actual))

    if not reference:
        for name in ("recorded concept coverage matches the graph",
                     "unreachable categories unchanged since the contract was measured",
                     "unbacked terms are recorded in the contract"):
            report.check(name, True,
                         "this host's graph is not the reference graph the contract was "
                         "measured against", skipped=True)
        _recorded_reach_skip(report, contract)
        return

    covered = {t: (t in patterns) for t in sorted(terms)}
    drift = {t: (covered[t], declared.get(t, {}).get("covered")) for t in covered
             if covered[t] != declared.get(t, {}).get("covered")}
    report.check("recorded concept coverage matches the graph", not drift,
                 f"drift (measured, contract): {drift}" if drift else
                 f"{sum(covered.values())}/{len(covered)} terms have an exact pattern")

    dead = unreachable_categories(definitions, patterns)
    recorded_dead = contract.get("defects", {}).get("unsatisfiable_by_concept")
    if recorded_dead is None:
        report.check("unreachable categories are recorded", False,
                     "add defects.unsatisfiable_by_concept (run --refresh)")
    else:
        report.check(
            "unreachable categories unchanged since the contract was measured",
            recorded_dead == dead,
            f"now {dead}, contract says {recorded_dead} - re-measure deliberately"
            if recorded_dead != dead else f"{dead or 'none'}",
            known=("no thesaurus pattern expresses these categories, so only the phrase fallback can credit them: "
                   + ", ".join(f"{k}:{'/'.join(v)}" for k, v in dead.items())) if dead else "",
        )

    recorded_unbacked = contract.get("coverage", {}).get("unbacked")
    measured_unbacked = unbacked_terms(definitions, patterns)
    report.check(
        "unbacked terms are recorded in the contract",
        recorded_unbacked is not None and sorted(recorded_unbacked) == measured_unbacked,
        f"measured {len(measured_unbacked)} unbacked, contract says "
        f"{'none recorded' if recorded_unbacked is None else len(recorded_unbacked)} - "
        f"run --refresh and review the diff",
    )

    _recorded_reach_skip(report, contract, early_return=False)

    recorded_reach = contract.get("reachable_categories")
    if recorded_reach is None:
        report.check("reachability is recorded", False, "run --refresh")
    elif not has_kg():
        report.check("reachability matches the contract", True, "no knowledge graph on this host",
                     skipped=True)
    else:
        measured = measure_reachability(definitions, patterns)
        reach_drift = {n: {c: (measured.get(n, {}).get(c), recorded_reach.get(n, {}).get(c))
                           for c in set(measured.get(n, {})) | set(recorded_reach.get(n, {}))}
                       for n in set(measured) | set(recorded_reach)}
        reach_drift = {n: {c: v for c, v in d.items() if v[0] != v[1]} for n, d in reach_drift.items()}
        reach_drift = {n: d for n, d in reach_drift.items() if d}
        report.check("reachability matches the contract", not reach_drift,
                     f"drift (measured, contract): {reach_drift}" if reach_drift else "unchanged")


def starter_concept_check(report: Report, contract: dict) -> None:
    """The shipped starter concepts must cover every term the reference graph cannot express.

    `docs/checklists/concepts/` is documentation, not a runtime dependency: copying those
    articles into a knowledge graph is what makes the graph-side crediting work. Without this
    check the files would rot the first time a term was added to a checklist."""
    directory = ROOT / "docs" / "checklists" / "concepts"
    unbacked = contract.get("coverage", {}).get("unbacked", [])
    if not unbacked:
        report.check("starter concepts cover unbacked terms", True, "nothing unbacked")
        return
    if not directory.is_dir():
        report.check("starter concepts cover unbacked terms", False, f"{directory} is missing")
        return
    patterns = kg_patterns(directory)
    missing = [t for t in unbacked if t not in patterns]
    report.check(
        "starter concepts cover unbacked terms",
        not missing,
        f"not covered by docs/checklists/concepts: {missing}" if missing else
        f"{len(unbacked)} unbacked terms covered by "
        f"{len(list(directory.glob('*.md')))} concept files",
    )


def journey_checks(report: Report, definitions, contract: dict, live: bool) -> None:
    for name, categories in definitions.items():
        rc, out = validate(name, "some text mentioning nothing in particular")
        parsed = parse_human(out)
        ok = (rc == 0 and parsed["passed"] is False and parsed["score"] is not None
              and parsed["score"][0] == len(parsed["satisfied"])
              and parsed["score"][1] == len(parsed["satisfied"]) + len(parsed["missing"])
              and parsed["passed"] == (not parsed["missing"])
              and parsed["score"][1] == len(categories))
        report.check(f"validate --checklist {name}: report adds up", ok,
                     out.strip()[-300:] or "no output")

    rc, out = validate("no_such_checklist", "text")
    named = {n for n in re.findall(r"[\"']([a-z_]+)[\"']", out) if n != "no_such_checklist"}
    report.check("unknown checklist names the available ones", set(definitions).issubset(named),
                 f"available checklists named: {sorted(named)}")

    expected_rc = contract.get("expectations", {}).get("unknown_checklist_exit_code")
    report.check("unknown checklist exit code matches the contract",
                 expected_rc is not None and rc == expected_rc,
                 f"exit={rc}, contract says {expected_rc} - re-measure deliberately if this changed")

    rc, out = validate(list(definitions)[0], "unit tests and documentation", as_json=True)
    payload = None
    for line in out.splitlines():
        if line.strip().startswith("{"):
            try:
                payload = json.loads(line.strip())
                break
            except json.JSONDecodeError:
                continue
    report.check("--json emits parseable JSON", payload is not None,
                 "parsed" if payload else out.strip()[-200:] or "no JSON in output")

    if not live:
        report.check("live assertions", True, "no knowledge graph on this host", skipped=True)
        return

    for sample in contract.get("samples", []):
        rc, out = validate(sample["checklist"], sample["text"])
        parsed = parse_human(out)
        ok = (rc == 0 and parsed["passed"] == sample["passed"]
              and sorted(parsed["missing"]) == sorted(sample["missing"]))
        report.check(f"live: {sample['label']}", ok,
                     f"expected passed={sample['passed']} missing={sample['missing']}, got "
                     f"passed={parsed['passed']} missing={parsed['missing']}")

    rc, out = run(f"{GREP} checklist -n 5", timeout=180)
    report.check("terraphim-grep returns hits for a concept the KG knows",
                 rc == 0 and bool(out.strip()), f"exit={rc} lines={len(out.splitlines())}")


def bench_checks(report: Report, definitions) -> None:
    name = list(definitions)[0]
    text = "unit tests documentation error handling security check performance optimization"
    samples: list[float] = []
    for _ in range(int(os.environ.get("CHECKLIST_BENCH_RUNS", "15"))):
        t0 = time.perf_counter()
        validate(name, text)
        samples.append((time.perf_counter() - t0) * 1000)
    samples.sort()
    p50 = statistics.median(samples)
    p95 = samples[max(0, int(len(samples) * 0.95) - 1)]
    report.check(f"bench: validate p95 within {BENCH_BUDGET_MS:.0f} ms", p95 <= BENCH_BUDGET_MS,
                 f"p50={p50:.0f} ms p95={p95:.0f} ms max={samples[-1]:.0f} ms over {len(samples)} runs")


# ------------------------------------------------------------------------ contract

def refresh_contract(definitions, patterns: set[str], contract: dict) -> dict:
    contract["concepts"] = {t: {"covered": t in patterns} for t in sorted(all_terms(definitions))}
    contract["relevant_categories"] = {n: list(c) for n, c in definitions.items()}
    contract["coverage"] = {
        "unbacked": unbacked_terms(definitions, patterns),
        "patterns": len(patterns),
        "digest": pattern_digest(patterns),
        "kg_dir": str(KG_DIR),
    }
    contract["reachable_categories"] = measure_reachability(definitions, patterns)
    contract.setdefault("defects", {})["unsatisfiable_by_concept"] = unreachable_categories(definitions, patterns)
    contract.setdefault("expectations", {})["unknown_checklist_exit_code"] = validate("__nope__", "text")[0]
    contract.get("defects", {}).pop("unknown_checklist_exit_code", None)
    return contract


def write_inventory(definitions, patterns: set[str], contract: dict) -> str:
    reach = contract.get("reachable_categories", {})
    lines = [
        "# Checklist terms against knowledge-graph concepts",
        "",
        f"Generated by `scripts/checklist-journey.py --inventory` from `{KG_DIR}` "
        f"({len(patterns)} patterns). Do not edit by hand.",
        "",
        "**Exact pattern** means the graph can produce this term as a thesaurus pattern "
        "(a concept file name or a `synonyms::` entry). **Reachable** is measured by the "
        "CLI: can this category be credited when a text addresses only it?",
        "",
    ]
    for name, categories in definitions.items():
        lines += [f"## `{name}`", "", "| category | term | exact pattern | category reachable |",
                  "|---|---|---|---|"]
        for cat, terms in categories.items():
            for term in terms:
                lines.append(f"| {cat} | `{term}` | {'yes' if term in patterns else '_no_'} | "
                             f"{'yes' if reach.get(name, {}).get(cat) else '**no**'} |")
        lines.append("")
    unbacked = unbacked_terms(definitions, patterns)
    if unbacked:
        lines += ["## Terms with no exact pattern", "",
                  "Add a concept article whose `synonyms::` line carries the term (see "
                  "`docs/checklists.md`), or drop the term from the checklist. Until then the "
                  "term is recognised only by the phrase fallback, not by the graph.", ""]
        lines += [f"- `{t}`" for t in unbacked]
        lines.append("")
    out = ROOT / "docs" / "checklist-concept-coverage.md"
    out.parent.mkdir(exist_ok=True)
    out.write_text("\n".join(lines) + "\n")
    return str(out)


def main() -> int:
    ap = argparse.ArgumentParser(description="Validate Terraphim checklists against KG concepts")
    ap.add_argument("--refresh", action="store_true", help="re-measure and rewrite the contract")
    ap.add_argument("--inventory", action="store_true", help="write docs/checklist-concept-coverage.md")
    ap.add_argument("--json", action="store_true", help="emit a JSON report")
    ap.add_argument("--no-bench", action="store_true", help="skip the latency bench")
    ap.add_argument("--no-live", action="store_true", help="skip assertions that need a knowledge graph")
    args = ap.parse_args()

    if shutil.which(AGENT) is None:
        raise SystemExit(f"{AGENT} not found on PATH")

    definitions = parse_checklists()
    patterns = kg_patterns(KG_DIR) if KG_DIR.is_dir() else set()
    contract = json.loads(CONTRACT.read_text()) if CONTRACT.is_file() else {}

    if args.refresh:
        contract = refresh_contract(definitions, patterns, contract)
        CONTRACT.write_text(json.dumps(contract, indent=2, sort_keys=True) + "\n")
        covered = sum(1 for v in contract["concepts"].values() if v["covered"])
        print(f"contract refreshed: {CONTRACT} ({covered}/{len(contract['concepts'])} terms "
              f"have an exact pattern, {len(patterns)} patterns in {KG_DIR})")
        return 0

    if args.inventory:
        print(f"inventory written: {write_inventory(definitions, patterns, contract)}")
        return 0

    live = not args.no_live and has_kg()
    report = Report()
    reference = is_reference_graph(patterns, contract)
    contract_checks(report, definitions, patterns, contract, reference)
    starter_concept_check(report, contract)
    journey_checks(report, definitions, contract, live)
    if not args.no_bench:
        bench_checks(report, definitions)

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
              f"{'' if live else ' (no KG on this host: live assertions skipped)'}")
        if report.failures:
            print("\nchecklist validation is not trustworthy until these pass:", file=sys.stderr)
            for c in report.failures:
                print(f"  {c['name']}: {c['detail']}", file=sys.stderr)

    return 1 if report.failures else 0


if __name__ == "__main__":
    sys.exit(main())
