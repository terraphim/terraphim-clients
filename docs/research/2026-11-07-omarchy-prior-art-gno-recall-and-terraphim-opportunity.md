# Omarchy prior-art assessment — GNO Recall and the Terraphim opportunity

**Date:** 2026-11-07
**Author:** Kairo
**Companion to:** `2026-11-07-omarchy-launcher-search-and-terraphim-integration.md`
**Scope:** `gmickel/omarchy-gno-recall` read in full, plus the marketplace landscape (6,389 plugins)

---

## 1. Headline

The closest prior art exists, it is **very well engineered**, and it is **not a competitor for the
packaging channel**. Terraphim's differentiation on Omarchy is real but narrower than it first looks, and
there is exactly **one concrete blocker** to matching the ecosystem's privacy standard.

**The blocker:** `terraphim-grep` takes its query as a **positional argv argument**. The ecosystem's
reference implementation deliberately never puts the query on argv. Adding `--query-file` is a small,
well-scoped change and should be the first thing landed.

---

## 2. GNO Recall — the engineering reference

`gmickel/omarchy-gno-recall` v1.1.1, MIT, ★1, created 2026-08-29, pushed 2026-09-19, marketplace
`verificationStatus: verified`.

| | |
|---|---|
| Manifest | `schemaVersion: 1`, `id: gmickel.gno-recall`, `kinds: ["service","bar-widget","overlay"]`, `keepLoaded: true` |
| Entry points | `service` → `Service.qml` (63 KB), `barWidget` → `BarWidget.qml` (9 KB), `overlay` → `RecallOverlay.qml` (46 KB) |
| Backend | GNO 2.2.1 — a local-first knowledge engine (notes, documents, code) with BM25 + semantic + hybrid retrieval |
| Runtime | vendored **Bun 1.4.2** + `@gmickel/gno`, pinned and hash-locked in `runtime/trust-manifest.json` (172 KB) |
| Repo size | ~1.3 GiB unpacked runtime |

### 2.1 Architecture pattern worth copying

- **The plugin never indexes and never starts a server.** It only *queries*. Indexing and the daemon are
  the backend's job, invoked through a separate CLI the user drives.
- **Three surfaces, one service.** A quiet bar widget (history glyph only when healthy; a distinct shape
  and colour when there is backlog, staleness, or a degraded fault), an anchored index panel, and a
  keyboard-driven recall overlay summoned with
  `omarchy-shell shell toggle gmickel.gno-recall`.
- **Overlay search is a second argv-array call**: `gno search --query-file <runtime-file> --json
  --no-project-affinity -n 20` for BM25; Shift+Enter runs the hybrid `gno query …` (embeddings + expansion
  + rerank, 90 s timeout, no `--depth` flag). Collection browsing uses `gno status --json` and a paginated
  `gno ls`.

### 2.2 The privacy posture — the part Terraphim must match

> *"the query is written to a **0600 file under `XDG_RUNTIME_DIR`** and is **never on argv**"*

This is not incidental. It is the difference between a user's search query being visible to every process
on the machine via `ps` / `/proc/<pid>/cmdline` and it not being. GNO Recall also:
- never starts a server itself and never calls `gno get`;
- installs the runtime under `${XDG_DATA_HOME:-~/.local/share}/gno-recall/runtimes/` — global Bun and GNO
  are never used;
- **verifies the installed tree before every backend invocation** — missing, changed, or additional files
  **fail closed**;
- enforces exact compressed byte sizes and a 300-second deadline per archive, verifies before extraction,
  applies reviewed hash-guarded patches, and runs **no package install scripts**.

### 2.3 The install-gate pattern — converged on independently by two plugins

> *"The Omarchy plugin installer **never runs hooks**. `omarchy plugin add` only clones files and toggles
> enabled state; it never runs plugin code."*

GNO Recall therefore ships `scripts/install-keybind.sh` as a **documented post-add step the user runs
themselves**. `khephri.sia` converged on the identical posture independently:

> *"a **SETUP gate**. Open the cockpit from the SIA bar item and choose Begin… run SIA's fail-closed
> `install.sh` in it — **loading or enabling the QML never runs the installer by itself**."*

Both plugins hold the terminal open so the user can observe the installer actually start, and both refuse
to proceed on an interrupted or ambiguous installation (SIA additionally disables installation when the
checkout shows `AHEAD`). For a Terraphim plugin that must install or configure a thesaurus, this is the
pattern to follow — not a postinstall hook.

### 2.4 The security acknowledgement

> *"Plugins run as **unsandboxed code inside `omarchy-shell`**… Only install repos you are willing to run
> in the long-lived shell process."*

GNO Recall says this plainly. A Terraphim plugin should too.

---

## 3. The competitive landscape

From the marketplace registry (6,389 plugins; kinds: Bar widget 5,373 · Service 383 · Overlay 382 ·
Panel 182 · Bar 42 · Plugin 23 · Suite 2 · **Service + Bar widget 1** · **Menu + Bar widget 1**).

| Plugin | ★ | Kind | What it is | Marketplace |
|---|---:|---|---|---|
| `khephri.sia` | 31 | `bar-widget` + `overlay` | **"The Omarchy Brain"** — persistent local corpus, semantic index, typed knowledge graph with local embeddings, deterministic nightly compaction, falsifiable predictions with an audited judge, **MCP for every agent** | unverified, `installAvailable: false` |
| `relote.context-engine` | 4 | `bar-widget` | "One memory for every agent" — bar icon, hotkey box, **Omarchy menu entries**, one skill + one MCP server bridging Claude Code / Codex / OpenCode / Hermes | **verified** |
| `gmickel.gno-recall` | 1 | `bar-widget` (+ service, overlay) | GNO local search engine surfaced from the bar | **verified** |
| `io.github.5h3rd1l.omasearch` | 11 | `overlay` | "Quick AI search" — Enter asks Claude (or your default agent), Ctrl+Enter searches the web | **verified** |

### 3.1 The taxonomy point — and how it reconciles

`omarchy.menu` is the **only** entry with kind `Menu + Bar widget`, and it is the **first-party built-in**.
So no *third-party* plugin declares a `menu` kind. That looked like a shell limitation; it is not.

`relote.context-engine` proves the mechanism. Its README lists, among the files it writes:

> `~/.config/omarchy/extensions/omarchy-menu.jsonc` — **Menu rows between marker comments**

⇒ **Menu presence comes from the user-extension JSONC, not from a `menu`-kind plugin.** The catalogue's
`kind` field reflects the manifest's declared kinds only. This validates the cheapest integration path
(menu rows in JSONC) as the **proven, shipped pattern**, used by two independent plugins.

### 3.2 The closest competitor is SIA — and it is Terraphim's thesis, already built

SIA's own description: *"a persistent, associative, self-consolidating memory for your Linux desktop.
**Local embeddings, typed knowledge graph, deterministic cognitive core, outcome learning with an audited
judge**, mission-control cockpit, MCP for every agent."*

Read against Terraphim's positioning, SIA overlaps on: typed knowledge graph, local embeddings, MCP for
every agent, auditability (*"You can audit every stored word because the corpus is markdown in git and the
daemon signs its own acts"*), and a cockpit/overlay UI.

Where Terraphim is genuinely different:
- **Roles.** Terraphim's thesaurus and rolegraph are a first-class, *configured* concept
  (`terraphim-grep --role`, `--thesaurus`, `--role-config`). SIA has a graph but no role model.
- **Role-scoped retrieval, not just retrieval.** Terraphim's hybrid search boosts on *your configured
  thesaurus*, expands synonyms to every alias of the same normalised concept, and only calls an LLM when
  a sufficiency judge says the evidence is insufficient.
- **An installable distribution channel.** SIA is `installAvailable: false` and marketplace-`unverified`.
  Terraphim already has the AUR/pacman PKGBUILD and the `terraphim-clients-bin` split contract — Terraphim
  can be `pacman -S terraphim-grep-bin` on a clean Omarchy box, and SIA cannot.
- **An agent skill and MCP server already shipped.** `terraphim-cursor-plugin` v0.2.7 ships four skills
  and `terraphim-mcp-server` ships the tools.

### 3.3 The reference implementation for a "Terraphim in the bar" plugin

`relote.context-engine` is the closest **integration** reference (a memory engine surfaced in the bar, in
the menu, and in every coding agent). Its shape maps onto Terraphim almost one-to-one:

| Context Engine | Terraphim equivalent |
|---|---|
| bar icon · `Super+Ctrl+M` · menu entries | bar widget · hotkey · JSONC menu rows |
| one skill + one MCP server per agent | `terraphim-cursor-plugin` skills + `terraphim-mcp-server` |
| `~/.claude.json`, `~/.codex/config.toml`, `~/.config/opencode/opencode.json`, `~/.cursor/mcp.json`, `~/.gemini/settings.json`, `~/.hermes/config.yaml` | same six hosts — Terraphim already writes several |
| `ce mcp-headers` helper so agent config holds no key copy | the same trick would apply to Terraphim's MCP auth |

---

## 4. What Terraphim should build, and the one blocker

### 4.1 The blocker: the query must not be on argv

`terraphim-grep`'s full CLI surface today:

```
query (positional, Option<String>)   --context   --max-results   --haystack
--force-rlm   --answer   --search-only   --json   --role   --thesaurus
--role-config   --kg-path
subcommands: check-update, update
```

and the query is resolved as:

```rust
let query = args.query.as_deref()
    .context("missing search query; run `terraphim-grep --help` for usage")?;
```

There is **no `--query-file` and no stdin path**. The query is therefore always visible in `ps` output,
`/proc/<pid>/cmdline`, and shell history. The ecosystem's reference implementation goes out of its way to
avoid exactly this.

**Required change (small, well-scoped):** add `--query-file <path>` to `terraphim-grep` — and, ideally,
read the query from stdin when `-` is passed — so a launcher can write the query to a `0600` file under
`XDG_RUNTIME_DIR` and pass only the path. This is a prerequisite for matching GNO Recall's posture and is
independently worth having for any launcher integration.

### 4.2 The build order, revised

1. **Add `--query-file` / stdin to `terraphim-grep`.** Small; unblocks everything; independently valuable.
2. **Close #249 / #316** so `terraphim-grep` is actually `pacman -S`-installable on Omarchy. This is the
   differentiator against SIA, which has no installable route.
3. **Ship the JSONC menu rows** (`~/.config/omarchy/extensions/omarchy-menu.jsonc`) — the proven pattern,
   zero plugin infrastructure, and it uses the in-process `summon` fast path when the action matches
   `omarchy-shell shell summon <id> '<payload>'`.
4. **Then** a Quickshell plugin in the GNO Recall shape: `kinds: ["service","bar-widget","overlay"]`,
   `keepLoaded: true`, a quiet bar widget that only turns loud when the index is unhealthy, and an overlay
   that queries `terraphim-grep --query-file … --json`. Note the ecosystem's dominant pattern for a
   Raycast-style surface is **`overlay` (382 plugins)**, not `menu` (0 third-party) — so prefer `overlay`.
5. **Follow the install-gate pattern**: the plugin never installs or configures anything by itself; the
   user explicitly triggers it from a visible terminal.
6. **Mirror the agent-integration surface** that Context Engine demonstrates — one skill + one MCP server
   per host, with the credential helper pattern so agent config never holds a secret.

### 4.3 What not to build

- **A `menu`-kind plugin.** No third-party precedent; the JSONC route is simpler and proven.
- **An auto-running installer.** Both reference plugins refuse this, and the shell's installer never runs
  plugin code by design.
- **A second knowledge engine.** Terraphim's graph, thesaurus and roles already exist; SIA's differentiator
  is that it *is* the engine. Terraphim's is that it is a *role-aware* one with a distribution channel.

---

## 5. Evidence appendix

**Read in full:** `gmickel/omarchy-gno-recall` — `manifest.json`, `README.md` (20 KB), `runtime/package.json`
(`@gmickel/gno` 2.2.1, `@oven/bun-linux-x64-baseline` 1.4.2), the full git tree, and the repository
metadata (`pushed 2026-09-19`, ★1, MIT, `verificationStatus: verified` in the marketplace).

**Read:** `AnubisQuantumCipher/sia` v1.8.4 (`manifest.json`, README — `kinds: ["bar-widget","overlay"]`,
`keepLoaded: true`, entryPoints `barWidget`→Panel.qml, `overlay`→Cockpit.qml; ★31, MIT, pushed 2026-10-31,
marketplace `unverified`, `installAvailable: false`) and `relote-com/omarchy-context-engine` v0.2.5 (README
— the `~/.config/omarchy/extensions/omarchy-menu.jsonc` "menu rows between marker comments" mechanism and
the six-agent MCP config table; ★4, MIT, pushed 2026-11-03, marketplace `verified`).

**Marketplace registry:** `omarchy-research/catalog.json` — 6,389 plugins; kind distribution as tabulated;
per-plugin metadata (`kind`, `stars`, `license`, `verificationStatus`, `installAvailable`,
`repositoryLayout`, `repo`, `addedAt`, `repositoryUpdatedAt`).

**Terraphim side:** `terraphim-clients` `crates/terraphim_grep/src/main.rs` — full clap argument
definitions and the query-resolution site (positional only, no `--query-file`, no stdin);
`terraphim-cursor-plugin` v0.2.7 skills + `integrations/`; `terraphim_mcp_server` tool list.

**Not verified:** SIA's source internals (README and manifest only — its indexing, embedding model and
judge implementation were not read, so the comparison in §3.2 is of stated scope, not implementation);
GNO Recall's `Service.qml` / `RecallOverlay.qml` bodies (README-documented behaviour only);
whether `omarchy plugin add` has changed since v4.5.0 (the "never runs hooks" guarantee is taken from the
v4.5.0 docs and GNO Recall's README, not from a code read of the installer); the runtime-side comparison of
SIA's local-embedding model against Terraphim's role/thesaurus retrieval.
