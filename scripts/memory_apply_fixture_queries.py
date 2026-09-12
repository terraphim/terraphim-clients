#!/usr/bin/env python3
"""Run `terraphim-agent memory apply --format json` over every query in the
committed memory benchmark fixture and print injected_bytes and
estimated_tokens per query plus the mean and max (docs/memory-benchmark.md).

The binary runs against a hermetic HOME under a temporary directory, so the
developer's own evolution store is never read or written. The role config is
the committed Terraphim Engineer fixture config with its knowledge graph
pointed at `crates/terraphim_agent/docs/src/kg`, the directory the committed
`tests/fixtures/memory_bench/thesaurus.json` was built from, so the binary
ranks with the same concepts as the retrieval quality test and the latency
bench. The store is created through the real `memory capture` and then seeded
with the committed fixture items (all Medium importance, so all in
`short_term`).

Usage:
    cargo build -p terraphim_agent --bin terraphim-agent
    scripts/memory_apply_fixture_queries.py [workspace_root]

Set TERRAPHIM_AGENT_BIN to point at a different binary.
"""
import json
import os
import pathlib
import statistics
import subprocess
import sys
import tempfile


def main() -> int:
    here = pathlib.Path(__file__).resolve()
    ws = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else here.parent.parent
    binary = pathlib.Path(
        os.environ.get("TERRAPHIM_AGENT_BIN", ws / "target/debug/terraphim-agent")
    )
    if not binary.exists():
        sys.exit(f"binary not found: {binary} (run: cargo build -p terraphim_agent --bin terraphim-agent)")
    fixture = ws / "crates/terraphim_agent/tests/fixtures/memory_bench"

    root = pathlib.Path(tempfile.mkdtemp(prefix="memory-apply-fixture-"))
    home = root / "home"
    config_dir = home / ".config" / "terraphim"
    mac_config_dir = home / "Library" / "Application Support" / "com.aks.terraphim"
    data = root / "data"
    dashmap = root / "dashmap"
    sqlite = root / "sqlite"
    for d in (config_dir, mac_config_dir, data, dashmap, sqlite):
        d.mkdir(parents=True, exist_ok=True)

    cfg = json.load(open(ws / "crates/terraphim_agent/tests/fixtures/terraphim_engineer_config.json"))
    cfg["roles"]["Terraphim Engineer"]["kg"]["knowledge_graph_local"]["path"] = (
        "crates/terraphim_agent/docs/src/kg"
    )
    role_config = root / "role_config.json"
    role_config.write_text(json.dumps(cfg, indent=2))

    settings = f"""
server_hostname = "127.0.0.1:8000"
api_endpoint = "http://localhost:8000/api"
initialized = "false"
default_data_path = "{data}"
role_config = "{role_config}"

[profiles.dashmap]
type = "dashmap"
root = "{dashmap}"

[profiles.sqlite]
type = "sqlite"
datadir = "{sqlite}"
connection_string = "{sqlite / 'terraphim.db'}"
table = "terraphim_kv"
"""
    for d in (config_dir, mac_config_dir):
        (d / "settings.toml").write_text(settings)

    env = dict(
        os.environ,
        HOME=str(home),
        XDG_CONFIG_HOME=str(home / ".config"),
        TERRAPHIM_SETTINGS_PATH=str(config_dir),
        TERRAPHIM_DEFAULT_DATA_PATH=str(data),
    )

    def run(*args):
        proc = subprocess.run(
            [str(binary), "--format", "json", *args],
            cwd=ws,
            env=env,
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            sys.exit(f"command failed: {args}\n{proc.stdout}\n{proc.stderr}")
        return json.loads(proc.stdout.strip().splitlines()[-1])

    # Create the store through the real CLI, then seed it with the fixture corpus.
    run("memory", "capture", "--provenance-tag", "memory-benchmark-doc")
    store = next(root.rglob("cli-agent.json"))
    envelope = json.load(open(store))
    items = [json.loads(line) for line in open(fixture / "corpus.jsonl") if line.strip()]
    envelope["memory"]["short_term"] = items
    envelope["memory"]["long_term"] = {}
    store.write_text(json.dumps(envelope, indent=2))

    queries = [json.loads(line) for line in open(fixture / "queries.jsonl") if line.strip()]
    rows = []
    for q in queries:
        v = run("memory", "apply", "--role", "Terraphim Engineer", "--prompt", q["query"])
        rows.append((v["retrieved_items"], v["injected_bytes"], v["estimated_tokens"]))
        print(
            f"retrieved={v['retrieved_items']}  bytes={v['injected_bytes']:>6}  "
            f"tokens={v['estimated_tokens']:>5}  {q['query'][:70]!r}"
        )

    bytes_ = [r[1] for r in rows]
    tokens = [r[2] for r in rows]
    non_zero = [r for r in rows if r[1] > 0]
    print(
        f"queries={len(rows)} non_zero={len(non_zero)} "
        f"mean_bytes={statistics.mean(bytes_):.1f} max_bytes={max(bytes_)} "
        f"mean_estimated_tokens={statistics.mean(tokens):.1f} max_estimated_tokens={max(tokens)}"
    )
    if non_zero:
        print(
            f"non_zero_only mean_bytes={statistics.mean(r[1] for r in non_zero):.1f} "
            f"mean_estimated_tokens={statistics.mean(r[2] for r in non_zero):.1f}"
        )
    print(f"hermetic root: {root}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
