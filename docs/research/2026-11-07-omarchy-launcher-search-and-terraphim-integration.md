# Omarchy launcher & desktop-search architecture — and Terraphim integration status

**Date:** 2026-11-07
**Author:** Kairo
**Scope:** `basecamp/omarchy` v4.5.0 (released 2026-11-05) shell + search; Terraphim's presence on Omarchy
**Status:** Research report — two deep-dive agents still running (delivered separately)

---

## 1. Headline

**Omarchy 4.5.0 has no desktop content search, and its native palette cannot be extended with one from
outside the shell.** The launcher was merged into the menu; search matches *menu rows*, not files.

**Terraphim's Omarchy presence today is distribution-only and not yet landed.** There is **no launcher or
search integration** — no repository, no plugin, no menu entry, no MCP registration. What exists is a
canonical AUR/pacman `PKGBUILD` (`terraphim-clients-bin`) plus two open Gitea issues to land it in
`omacom/omarchy-pkgs`.

**The closest prior art has now been read in full** (`gmickel/omarchy-gno-recall`) and the competitive
landscape mapped (6,389 marketplace plugins). Two conclusions changed: the ecosystem's dominant
third-party pattern for a Raycast-style surface is **`overlay` (382 plugins), not `menu` (0 third-party)** —
and `relote.context-engine` proves menu presence comes from **JSONC menu rows**, which validates the
cheapest path. Terraphim also has **one concrete blocker**: `terraphim-grep` puts the query on argv.
See the companion prior-art assessment.

The primitives to build a real integration are already in place and shipped as part of the Terraphim CLI:
`terraphim-grep` (hybrid local + knowledge-graph search, JSON output) and `terraphim-mcp-server`.

---

## 2. What changed in Omarchy 4.x

| | Before (v3.x) | Now (v4.5.0 "Quattro") |
|---|---|---|
| Launcher | **Walker** (separate binary) | **Gone.** `SUPER+SPACE` opens the Omarchy menu itself; `SUPER+ALT+SPACE` is an apps-only launcher |
| Bar / panels / OSD / lock / polkit | Waybar, Mako, SwayOSD, hyprlock, swaybg, polkit-gnome — separate processes | One long-running **Quickshell** process (`omarchy-shell`); each is a *plugin* |
| Shell process model | many daemons | `omarchy-launch-shell` → `quickshell -n -p` under `systemd-cat`, respawn loop; restart via `omarchy-restart-shell` |
| Extensibility | config overlays | **Plugin system**: `omarchy plugin add/clone/enable/disable/update/remove`, `Setup › Plugins` |

Rationale (from `shell/README.md`, reported in the Omarchy 4.0 notes): shared services exist once instead of
once per process; summoning a panel is IPC into an already-running process rather than a cold start; and
third-party plugins load from disk without patching Omarchy.

---

## 3. The launcher / search architecture (verified at `v4.5.0`)

### 3.1 Plugin contract

- Third-party plugins live at `~/.config/omarchy/plugins/<id>/` — a **git repo with `manifest.json` at the
  root**. Install: `omarchy plugin add https://github.com/acme/omarchy-weather.git`; update:
  `omarchy plugin update` (fetch → show diff → fast-forward); remove: `omarchy plugin remove`.
- Manifest declares `schemaVersion`, `id`, `name`, `version`, `kinds`, `entryPoints` (`activation`,
  `keepLoaded`, `barWidget` optional).

| Kind | What it is |
|---|---|
| `bar-widget` | Component the active bar drops into a section |
| `bar` | Full bar option that can replace `omarchy.bar` |
| `panel` | Floating window (e.g. OSD) |
| `overlay` | Fullscreen overlay (e.g. background picker) |
| `menu` | Summoned menu surface |
| `service` | Headless singleton, no UI |

- Entry points are QML **`Item`s** (not `ShellRoot`), receiving shell-injected properties `omarchyPath`,
  `shell`, `manifest`, and `pluginRegistry` / `barWidgetRegistry`.
- **Panel / overlay / menu plugins must expose `open(payloadJson)` and `close()`.**
- Third-party plugins receive **capability-scoped facades**, not host objects: ordinary plugins may control
  only their own service and lifecycle; **menu plugins receive an application-library facade**; bar plugins
  get detached scalar bar state. Explicitly *not* a sandbox — *"a visual bar widget can walk its parent
  hierarchy to ordinary host objects."* Authentication services are kept out of the host's public service
  map and QML tree, and third-party registry/config views are detached snapshots.

### 3.2 IPC

`bin/omarchy-shell` is the canonical entry point (forwards to the running shell; `omarchy-shell-ipc`
auto-starts it). The `shell` target exposes:

```
ping | summon <id> <payloadJson> | hide <id> | toggle <id> <payloadJson>
call <id> <method> <arg> | rescanPlugins | reloadConfig | setPluginEnabled | listPlugins
```

Individual plugins register their own targets, named for the plugin (e.g. `omarchy.clock`,
`omarchy.indicators`). There is no `bar` target.

### 3.3 The menu — and where search happens

The menu is the `omarchy.menu` plugin (`shell/plugins/menu/Menu.qml`); the pure logic is
`shell/plugins/menu/MenuModel.js` (plain JS, Node-testable). It is defined by JSONC
(`default/omarchy/omarchy-menu.jsonc`).

- Ids are object keys; **dotted ids imply hierarchy** (`trigger.share.file` lives under `trigger.share`).
- Kind is inferred: `action` → action, `target` → link, otherwise submenu.
- Optional fields: `aliases` (alternate `omarchy menu summon <name>` routes, also searchable), `iconFont`,
  `title`, `when` (shell condition → hide row), `checked` (append ✓), `disabled` (row stays, dimmed),
  `description` (subtitle + extra whole-word search text).
- **Search matches labels, the last id segment, and descriptions.** App rows are additionally searchable by
  their desktop `Keywords`; provider rows by an optional 4th, search-only field.

**The matching is substring, not fuzzy** (verified against the implementation, not the naming):

- *Menu-tree rows* — `MenuModel.matchesQuery` / `MenuModel.searchScore` in
  `shell/plugins/menu/MenuModel.js`. `nameSearchText()` builds a lowercased haystack of
  `label + leaf-id-of-dotted-id + aliases + searchTerms`; each whitespace-separated query term must appear as
  a **substring** (`nameText.indexOf(term) >= 0`) or match a whole word in the description. Scoring is a
  hand-rolled tier ladder: exact label (0/2) → label prefix (10) → label substring (30) → name-text substring
  (40) → description word match (60), then `score*1000 + depth*25 + order`.
- *App rows* — additionally through `shell/services/AppSearch.js`, whose `fuzzyScore` is a **prefix-weighted
  substring scorer with an acronym fallback** for terms ≤ 5 characters. Weights: name-prefix 10000, id-prefix
  9500, name-substring 8000, id-substring 7600, haystack 6000, acronym-prefix 5000, acronym-substring 4600,
  else 4000.
  **Despite the function name it is not subsequence-fuzzy** — `zrb` does *not* match `Firefox`. No external
  fuzzy library (no fzf, no Sublime-style scorer) is used by the shell at all.

#### The provider constraint — the key architectural limit

> *"A submenu with `provider: "name"` gets its rows at runtime instead of from JSONC. The names are defined
> by the shell, not the menu file — an extension can point a submenu at an existing provider but **cannot
> declare a new one**."*
> — `docs/menu.md`, v4.5.0

> *"Adding a provider means adding an entry to the `providers` map in `Menu.qml`."*

Built-in providers: `apps` (QML-native, from the shared `AppLibrary` — desktop entries, image icons, launch
feedback, uninstall support), plus bash one-liners in the `providers` map: `fonts`, `power-profiles`,
`kernels`, `languages`. Provider contract: one tab-delimited line per row, `label\tvalue\tcurrent`, with an
optional 4th field as a search-only term (the languages provider passes each language's English name, so
"danish" finds the row labelled "Dansk" without "danish" being routable). Row ids are
`<menuId>.<slugify(value)>`, with `-` appended on collision. A `volatile` provider re-runs every time its
submenu is entered — but **not on search keystrokes**, which would restart the enumeration per key.

⇒ **Consequence for Terraphim:** a third party cannot add rows to the native `SUPER+SPACE` search. The
provider map is shell-internal. Any Terraphim search must therefore be its own summoned surface, or a
menu *action* that shells out.

### 3.4 The menu doubles as the system dmenu — the most useful seam

`omarchy-menu-select` and `omarchy-menu-input` summon the menu plugin with `mode: select` / `mode: input`
and block on a tempfile handshake (the shell writes the selection to `selectionFile` and touches
`doneFile`; cancellation exits 1). A select option is `label`, `glyph\tlabel`, or `glyph\tlabel\tsubtext`
— the glyph shows but never returns; the subtext renders under the label, filters with it, and returns as
`label\tsubtext` so same-named rows keep a stable key. `-- --multiple` enables multi-select;
`--selected <value>` preselects.

`bin/omarchy-menu` wraps the plugin IPC: `omarchy menu` (toggle root), `toggle <route>`, `summon <route>`,
`close`, `refresh`, `ping`. A route is an item id or alias, case-insensitive, underscores normalised to
dashes.

⇒ **Any dmenu-speaking CLI can be the search backend for a menu action.** This is the cheapest integration
path and needs no Omarchy patch.

### 3.5 What Omarchy does **not** have

- **No file or content search.** Nothing in the shell indexes file contents. `apps` searches desktop
  entries; the palette searches menu rows. `SUPER+SPACE` finds apps and commands, not files.
- **No MCP client.** No `mcp`-named path exists in `shell/`, `bin/` or `default/` at v4.5.0. Omarchy's
  agent integration is a *launcher* for a coding agent (Claude Code, Codex, OpenCode, Pi, Gemini, Grok,
  Copilot, Crush), not an MCP host. ⇒ `terraphim-mcp-server` cannot register itself with Omarchy; an MCP
  consumer would have to be a plugin we ship.
- **No content index service** of any kind.
- **No shipped plugin→native-palette seam.** `providers` is a **`readonly`** property hardcoded in the
  first-party `Menu.qml`; `grep -E 'registerProvider|addProvider|menuProvider'` across `shell/` returns
  **zero** hits. The seam that would deliver it exists only in **open PR #8537**
  (`~/.config/omarchy/query-plugins.json`, hot-reloaded, plus
  `~/.config/omarchy/extensions/query-plugins-prefs.jsonc`) — *unmerged* as of 2026-11-07. Companion open
  PRs: #13971 (typed addresses + a "Search the web" row) and #10294 (inline calculator in search). None are
  merged; `docs/query-plugins.md` and `bin/omarchy-query-*` do not exist upstream.

### 3.6 Corrections to the common account of Omarchy 4

- **There is no "Omacalculate".** Upstream ships **Omacalc** (`omacom/omacalc`). Omawrite, Omacut and
  Omacalc are all **C++/Qt Quick**, and all are base-install defaults since v4.0.0. **None of them does
  file or content search** — Omawrite's `Ctrl+F` is in-document only.
- **"Deep search" is not a feature name.** It is a phrase from DHH's commit `7a9947a` (2026-07-23):
  *"Combine the Omarchy menu and the launcher — Now that we can deep search, they don't need to be
  different"*. It means **menu-tree drilldown search**, not file search. That commit **deleted** the separate
  `omarchy.launcher` overlay (its `Launcher.qml`, 665 lines, and `LauncherSearch.js`) and replaced it with
  `shell/services/AppLibrary.qml` + `shell/services/AppSearch.js`, giving the menu a native `apps` provider.
  The `omarchy-mac` fork and its Gitea mirror merely *carry* that upstream commit. A separate v3-era
  "Deep Search" PR (#2253, bash `show_deep_search()` with fuzzy matching over `full_menu_list`) was
  **closed unmerged**.
- **Repo identity.** `basecamp/omarchy` **301-redirects to `omacom/omarchy`**; the default branch is
  **`quattro`**, not `main` (`main` 404s on raw.githubusercontent). The in-tree `version` file reads
  `4.0.0.alpha` on *every* release tag from v4.0.0 to v4.5.0 — **git tags are the only trustworthy
  version signal**.

---

## 4. Terraphim integration status — what actually exists

### 4.1 There is no launcher/search integration

- **No repository.** `terraphim` org: 69 repos, **0** matching `omarchy`; targeted searches
  (`org:terraphim omarchy`, `user:AlexMikhalev omarchy`, `in:name`) all return `total=0`. Gitea: no
  omarchy repo in the `terraphim` org.
  *A repo named `Omarchy-Scripts` appears in Alex's GitHub repo list — that is an unrelated third-party
  project, not Terraphim.*
- **No code references.** `terraphim/terraphim-skills` → 0 hits for "omarchy";
  `terraphim/terraphim-cursor-plugin` → 0 hits.
- **No plugin, no menu entry, no MCP registration, no Omarchy plugin marketplace submission.**
- **The ecosystem is not yet exercising the seam we would want.** The marketplace registry holds ~6,350
  plugins, but **none currently declares `kind: "menu"`** — even though the shell's validator explicitly
  accepts `menu:menu`. That is a catalogue-taxonomy gap rather than a shell limitation, but it does mean a
  `menu`-kind third-party plugin would be **untested in the wild**.

### 4.2 What does exist: packaging (designed, tracked, not landed)

**Canonical PKGBUILD** — `terraphim/terraphim-clients` → `pkgbuilds/terraphim-clients-bin/PKGBUILD`:

| | |
|---|---|
| pkgbase | `terraphim-clients-bin` |
| outputs | `terraphim-agent-bin` (Apache-2.0), `terraphim-grep-bin` (MIT) |
| arch | `x86_64`, `aarch64` — static MUSL, no runtime library deps |
| pkgver | 1.21.22 |
| install | `/usr/bin/terraphim-agent`, `/usr/bin/terraphim-grep` |
| licences | `/usr/share/licenses/<pkg>/LICENSE-{Apache-2.0,MIT}` |
| receipts | `/usr/share/terraphim/package-manager.d/terraphim-{agent,grep}` = `pacman` |

The receipt is what makes the packaged clients treat themselves as package-managed: **no startup update
request and no self-update writing a shadow binary into `~/.local/bin`**.

The PKGBUILD header names its intended downstream explicitly:

> *"The downstream Omarchy directory `omacom/omarchy-pkgs/pkgbuilds/terraphim-clients-bin/` consumes the
> same split-package semantics (Gitea terraphim-clients#249)."*

**That directory does not exist.** `omacom/omarchy-pkgs` (149 ★, MIT, pushed 2026-11-07) has **zero**
terraphim pkgbuilds across every page of `pkgbuilds/`, and a code search for "terraphim" in that repo
returns `total=0`.

**The tracking issues (both open, `terraphim/terraphim-clients`):**

| Issue | Title | State | Created → Updated |
|---|---|---|---|
| **#249** | `[Omarchy O4] Package, validate, and submit Terraphim CLIs to omarchy-pkgs` | open (`distribution`, `enhancement`) | 2026-09-11 → 2026-10-03 |
| **#316** | `[MP6] Publish terraphim-clients-bin for Arch/AUR and feed Omarchy` | open | 2026-09-12 → 2026-10-04 |

#249 is parented by #245 and gated on the O3 release-integrity issue publishing a fresh release with all
four MUSL assets — with an explicit warning: *"do not submit a PKGBUILD permanently pinned to `v1.21.11`
or another mislabeled release."* #316 defines the contract: AUR submission to
`ssh://aur@aur.archlinux.org/terraphim-clients-bin.git`, `.SRCINFO` regenerated and committed alongside the
PKGBUILD, and **one** Omarchy directory.

Supporting work: `terraphim-clients/docs/plans/design-omarchy-metadata-licenses-2026-09-11.md`
(Gitea #246) fixes the crate `repository` URL, license identifiers and deb `license-file` pointers for the
two crates that actually ship as packages — `terraphim_agent` and `terraphim_grep` are the only two with a
`[package.metadata.deb]` section. The release-manifest schema's channel enum includes
`aur_terraphim_clients_bin` and `omarchy_terraphim_clients_bin`.

### 4.3 The primitives a search integration would use — already shipped

**`terraphim-grep`** (MIT, `terraphim-clients`) — "intelligent hybrid grep with RLM fallback and KG
curation": local code search via the `fff-search` backend, KG concept boosting, **exact KG term searches
every alias of the same normalized concept**, a sufficiency judge for optional LLM synthesis, graceful
degradation to search-only when no LLM is configured, and optional KG curation. Ships a standalone binary
**with JSON output support**. Path relevance is scored by `terraphim_file_search::kg_scorer::KgPathScorer`.

**`terraphim-mcp-server`** — exposes Terraphim search/autocomplete/KG as MCP tools:

```
terraphim_find_files   terraphim_grep      terraphim_multi_grep   search
build_autocomplete_index                  autocomplete_terms       autocomplete_with_snippets
fuzzy_autocomplete_search                 fuzzy_autocomplete_search_levenshtein
fuzzy_autocomplete_search_jaro_winkler    find_matches            replace_matches
load_thesaurus        load_thesaurus_from_json
is_all_terms_connected_by_path             extract_paragraphs_from_automata
update_config_tool   serialize_autocomplete_index  deserialize_autocomplete_index  json_decode
```

**`terraphim_mcp_search`** (in `terraphim-ai`) — a persistent Aho-Corasick index over MCP *tool* metadata
(name + description + tags), with save/load; <70 ms for 100 tools. It indexes **tools, not content**.

**`terraphim-cursor-plugin` v0.2.7** — the existing **host-integration precedent**, and the template to
copy. It ships four skills (`terraphim-grep`, `terraphim-editor`, `terraphim-agent-learn`,
`terraphim-agent-memory`) plus `integrations/cursor/` (`hooks.json.example`, `mcp.json.example`,
`policy.json.example`, `hook_adapter.py`, `commands/`, `examples/`, `release-inputs.json`) and
`integrations/kilo/`. Its host table covers Cursor, Kimi Code, AutoClaw, ZCode, Hermes Agent, skills.sh,
Claude Code and Grok Build — **Omarchy is not among them**.

---

## 5. Integration options

Three constraints shape every option: **(a)** there is no shipped seam for a third party to contribute
search results to the native palette; **(b)** the menu plugin *is* the system dmenu, so any dmenu-speaking
CLI can back a menu action; **(c)** no marketplace plugin currently uses `kind: "menu"`, so that path is
untested in the wild.

| # | Approach | Effort | In native `SUPER+SPACE`? | Omarchy patch? | Wildness |
|---|---|---|---|---|---|
| **1** | **Menu action** in `~/.config/omarchy/extensions/omarchy-menu.jsonc` → runs a Terraphim CLI | hours | No — it is a *row you select*, not rows it returns | **No** | Well-trodden (the menu is the documented action surface) |
| **2** | **Quickshell plugin, `kind: "overlay"`** — own summoned searchable surface (the ecosystem's dominant pattern: **382 overlay plugins**, vs **0 third-party `menu`-kind**) | days | No — must be summoned | No | Well-populated kind; GNO Recall and SIA both use `overlay` |
| **3** | **dmenu-compatible `terraphim-search` CLI** speaking `glyph\tlabel\tsubtext` | hours–1 day | No — but reusable by *any* dmenu caller, and usable as the backend of option 1 | No | Standard protocol |
| **4** | Wait for / contribute to **PR #8537** (`query-plugins.json`) so plugins *can* feed the native palette | weeks | **Yes, eventually** | Yes (upstream) | Unmerged; companion PRs #13971, #10294 also open |
| **5** | Patch Omarchy to add a provider to `Menu.qml`'s `providers` map | days + upstream PR | Yes | **Yes** | The facade/capability model is deliberately narrow; 4.x churn is high |

Option 1 and 3 compose: a dmenu-compatible `terraphim-search` CLI is the backend; a menu action is the
entry point. Option 2 is the "real launcher" answer and mirrors what the community already does.

### What the ecosystem already does (so we do not have to invent it)

| Plugin | Ranking / indexing | fzf? | Extensible with a custom source? |
|---|---|---|---|
| `omarchySearch` (marketplace #921) | Apps via Omarchy's own `AppLibrary.sortedEntries(query)`; files via `fd … \| fzf --filter <query> \| head -n N` — *"No custom fuzzy matcher written for this — fzf does the ranking."* Calculator is `awk` behind a digits/operators whitelist | **Yes** (files) | Own surface |
| `OmaLauncher` (`mirashif/omalauncher`, v1.3.1) | Apps/menu/CLI via its **own semantic tiered ranker** (`shared/core/src/SearchEngine.js`) with a genuinely fuzzy subsequence fallback; files via `find` + its own tier ranker | **No** (zero repo-wide hits) | Own surface, but it **does expose a manifest opt-in contract for other plugins to appear in its results** |
| `relote.context-engine` (marketplace) | A knowledge engine in the bar — near-identical use case to Terraphim. Passes typed text via **environment variable, not argv**, so it is not visible in `ps` | — | Own surface |

**Closest published prior art to a Terraphim-style KG search in an Omarchy plugin:**
`gmickel/omarchy-gno-recall` — a GNO knowledge-engine plugin with a "deep search" mode. Its source was **not
fetched** (UNVERIFIED), but it is the most relevant existing thing to read before building the same thing.

### The ordering constraint

**Finish #249/#316 first.** A plugin or menu action that shells out to `terraphim-grep` assumes the binary
exists on the machine. Today, on a stock Omarchy install, it does not — the pacman package has not landed
in `omacom/omarchy-pkgs`, and #249 is explicitly blocked on the O3 release-integrity gate. Building the
search surface before the packaging lands means shipping something that cannot run on a clean Omarchy box.

### What is *not* worth doing

- **Option 5** — patching Omarchy for a provider. The facade model is deliberately narrow, the provider map
  is shell-internal and `readonly`, and the ecosystem answered this question by building standalone plugins.
  Option 4 (contribute to #8537) is the *correct* upstream route if native-palette presence matters.
- **An MCP-based integration.** Omarchy has **no shell-side MCP server** (the only `mcpServers` anywhere are
  a Claude-account carry-over) and **no LSP**. MCP would only be consumed by a plugin we ship — adding a
  runtime dependency without adding capability over calling `terraphim-grep` directly.
- **A bar widget** for search. A search surface belongs in a summoned panel/menu, not the bar. (The
  `relote.context-engine` precedent shows a *knowledge* widget in the bar is viable — but that is a status
  surface, not a search surface.)

---

## 6. Recommendation

1. **Close #249 and #316** so `terraphim-grep` and `terraphim-agent` are actually installable on Omarchy via
   `pacman` (`omacom/omarchy-pkgs/pkgbuilds/terraphim-clients-bin/`). This is the prerequisite for
   everything else and is already scoped.
2. **Add `integrations/omarchy/`** to `terraphim-cursor-plugin`, following the `integrations/cursor/`
   pattern: a `README.md` documenting the host, a `commands/` example, and a `release-inputs.json`. Keep it
   thin — the Terraphim-specific logic stays in the Rust crates.
3. **Ship a dmenu-compatible `terraphim-search` CLI** (option 3) as the reusable primitive: it speaks the
   `glyph\tlabel\tsubtext` protocol, calls `terraphim-grep` internally, and can be used by *any* dmenu
   caller, not just Omarchy.
4. **Ship the one-row menu action** (option 1) as the zero-plugin proof — needs no plugin infrastructure and
   validates the UX. Because the JSONC `action` matching `omarchy-shell shell summon <id> '<payload>'` takes
   an **in-process fast path** (`MenuModel.summonAction`) that skips bash + IPC, a menu-driven surface is
   cheap.
5. **Then** build the Quickshell plugin in the shape the ecosystem actually uses —
   `kinds: ["service","bar-widget","overlay"]`, `keepLoaded: true`, a **quiet bar widget** that only turns
   loud when the index is unhealthy, and an overlay that queries `terraphim-grep --query-file … --json`.
   Prefer **`overlay`**, not `menu`: 382 overlay plugins vs 0 third-party menu-kind plugins.
6. **Do not** patch Omarchy for a provider. If native-palette presence is a requirement, contribute to
   **PR #8537** (`query-plugins.json`) instead and make Terraphim the first `query-plugins` entry.

**One prerequisite above all:** `terraphim-grep` currently takes its query as a **positional argv
argument** — no `--query-file`, no stdin. The ecosystem's reference implementation writes the query to a
`0600` file under `XDG_RUNTIME_DIR` precisely so it is **never on argv**. Add `--query-file` (and stdin
support) before shipping anything that shells out to it, or Terraphim's searches will be visible to every
process on the machine via `ps`. See the companion prior-art assessment.

**Also note the competitive context:** `khephri.sia` (★31) is already shipping "a persistent, associative,
self-consolidating memory… typed knowledge graph… MCP for every agent" as an Omarchy plugin, and
`relote.context-engine` (★4) already ships the bar + menu + per-agent MCP shape Terraphim would want.
Terraphim's genuine differentiators are **roles / role-scoped retrieval via a configured thesaurus** and
an **installable pacman channel** — not the knowledge engine itself.

Estimated: step 1 is already-scoped work; step 2 ~1 day; step 3 ~1–2 days; step 4 ~half a day; step 5
~3–5 days. The `--query-file` change is ~half a day and should precede step 3.

**Companion:** `2026-11-07-omarchy-prior-art-gno-recall-and-terraphim-opportunity.md` — the prior-art
assessment (GNO Recall read in full, the marketplace landscape, and the concrete gap in `terraphim-grep`).

## 7. Evidence appendix

**Omarchy (read firsthand at `basecamp/omarchy` v4.5.0, released 2026-11-05 — the current upstream tag):**
`agents/skills/shell-dev.md` (plugin contract, IPC, state), `docs/omarchy-shell.md` (manifest schema, plugin
kinds, install flow, facades), `docs/menu.md` (menu schema, providers, dmenu/select/input protocol, CLI),
`default/omarchy/omarchy-menu.jsonc` (the provider note and field list), plus the v4.0.0 release notes and
the `v4.5.0`/`v4.0.4` release bodies. `shell/plugins/` enumerated via the GitHub trees API.

**Terraphim:** GitHub code search `omarchy+org:terraphim` (30 hits, all distribution/release plumbing);
`terraphim-clients` `pkgbuilds/terraphim-clients-bin/{PKGBUILD,README.md}`; `terraphim-ai`
`.release/release-manifest.schema.json`; `terraphim-clients` Gitea issues #249 and #316;
`terraphim-clients` `docs/plans/design-omarchy-metadata-licenses-2026-09-11.md`;
`terraphim-clients` `crates/terraphim_grep/{Cargo.toml,README.md}` and
`crates/terraphim_mcp_server/src/lib.rs` (tool list); `terraphim-ai` `crates/terraphim_mcp_search/src/lib.rs`;
`terraphim-cursor-plugin` v0.2.7 tree + `docs/distribution.md`; `omacom/omarchy-pkgs` tree + code search.

**Absence evidence:** `terraphim` org = 69 repos, 0 with "omarchy"; `search/repositories` for
`org:terraphim omarchy`, `user:AlexMikhalev omarchy`, `org:terraphim omarchy in:name` → `total=0` each;
Gitea org repo list → no omarchy repo; code search in `terraphim-skills` and `terraphim-cursor-plugin` → 0;
`omacom/omarchy-pkgs` `pkgbuilds/` → 0 terraphim entries; code search in that repo → `total=0`; no `mcp`
path in omarchy's `shell/`, `bin/`, `default/` trees.

**Deep-dive reports (two agents, both verified against a full clone of `omacom/omarchy`):**
- `omarchy-research/omarchy-quattro-extension-report.md` — 1,298 lines. Shell architecture, plugin
  contract, JSONC schema, extension-surface table, 12 explicit UNVERIFIED items.
- `reports/omarchy-4-desktop-search-report.md` — 210 lines, 26 sources, citation ledger passing
  `--strict --min-coverage 0.6` (83 % coverage).
- Supporting artefacts: `omarchy-research/catalog.json` (marketplace registry, ~6,350 plugins),
  `omarchy-research/ce/` (the `relote.context-engine` reference plugin).
- New skill `research/upstream-capability-audit` captures the reusable method (repo canonicalisation,
  pinning by tag not by in-tree version file, reading the algorithm body instead of trusting the function
  name, proving negatives by scope, distinguishing upstream from fork-carrying, and labelling unmerged PRs
  as proposals).

**Still not verified:** the upstream intent behind the phrase "deep search" beyond that one commit line
(it is an interpretation, though the diff supports it); the fate of v3-era PR #2258; the source of
`gmickel/omarchy-gno-recall` (surfaced in search results only — the closest prior art and worth a dedicated
look); the live registry status of two third-party plugins; runtime ranking behaviour (no live Omarchy 4
install was available on this machine); whether `barWidget.defaults` / `barWidget.schema` are consumed by
any code (they appear in the docs, but no consumer was found); the `query-plugins.json` field names beyond
the four documented paths (PR #8537 is unmerged).
