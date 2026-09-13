# Phase 2 findings — bucket 8: send-sync invariants

Run: 2026-09-11-terraphim-clients-1. Swept 2026-09-11 23:42 BST by p2-send-sync. No source modified.

**Verdict: N/A: no manual Send/Sync surface.** One dependency-inherited finding (F-801,
event-listener RUSTSEC-2026-0221), CONTRACTUAL-BUT-DEFENSIBLE. Every spawn site's `Send`
bound is satisfied by safe means; the per-site evidence table is below.

Finding numbers in this file are bucket-prefixed (F-8NN) to avoid colliding with the
per-file F-001.. scheme used by `phase2_findings_panic-safety.md`.

---

## 1. Static evidence that the bucket is empty

### 1.1 ast-grep bucket run

```
$ bash ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/ast-grep-ub-patterns.sh \
    /Users/alex/projects/terraphim/terraphim-clients send-sync
=== send-sync-manual-impl ===

```

That is the complete output (stdout and stderr captured together; exit 0, no stderr). The
script prints a `=== <rule> ===` header per pattern file matched by the bucket glob and
then the `ast-grep scan` result, so an empty body means zero matches. The single rule the
glob selects is `scripts/patterns/send-sync-manual-impl.yml`:

```yaml
id: rust-ub-send-sync-manual-impl
language: rust
severity: error
message: >-
  Manual unsafe impl Send/Sync. Verify the SAFETY comment names the external
  synchronization mechanism, the only public deref path, and why that path is
  sound.
rule:
  any:
    - pattern: 'unsafe impl Send for $T {}'
    - pattern: 'unsafe impl Sync for $T {}'
    - pattern: 'unsafe impl<$P> Send for $T {}'
    - pattern: 'unsafe impl<$P> Sync for $T {}'
```

Caveat recorded for completeness: the script wraps each scan in `|| true`, so a tool
failure would also print an empty body. It did not fail here — exit status 0 and no
diagnostic on stderr.

### 1.2 Corroborating rg sweeps

| Sweep | Command | Hits |
|---|---|---|
| Manual auto-trait impls, source | `rg -n 'unsafe impl\|\bRc<\|RefCell<\|\bCell<\|\*const \|\*mut \|thread_local!' --type rust crates` | 1, and it is prose: `crates/terraphim_agent/src/judge/mod.rs:14` is a doc comment containing the word "unsafe" |
| Manual auto-trait impls, macro expansions | `rg -n 'unsafe impl[^{]*(Send\|Sync)' /private/tmp/claude-501/ubx-expand/` | 0 — closes the "compiler-derived only" claim; phase 1 had only catalogued `TrivialClone`, which is neither Send nor Sync |
| Deliberate `!Send` markers | `rg -n 'PhantomData' --type rust crates` | 0 — no `PhantomData<*const T>` / `PhantomData<Rc<..>>` anywhere, so no type is hand-made `!Send` |
| Circumvention of the compiler's Send-future check | `rg -n 'allow\((clippy::)?(await_holding\|arc_with_non_send\|non_send\|unsafe))\|\?Send\|#!\[(forbid\|deny)\(unsafe_code' --type rust crates` | 0 — no `#[async_trait(?Send)]`, no `#[allow(clippy::await_holding_lock)]`, no `#[allow(clippy::arc_with_non_send_sync)]` |
| Alternative executors that could bypass tokio's bounds | `rg -n 'futures::executor\|pollster\|async_std\|Handle::enter\|futures_lite::future::block_on' --type rust crates` | 0 |
| Send-wrapper escape hatches in the dependency graph | `rg -n '^name = "(send_wrapper\|fragile\|unsafe-any\|static_assertions\|sendable\|ouroboros\|self_cell)"' Cargo.lock` | 2, neither a Send wrapper: `ouroboros 0.18.5` (self-referential-struct macro, pulled by opendal; version is past its unsound advisories) and `static_assertions 1.1.0` (compile-time assertion crate, pulled by compact_str via ratatui). No `send_wrapper`, `fragile`, `unsafe-any`, `self_cell` or `sendable`. |
| Positive compile-time check already in the tree | `crates/terraphim_agent/src/tui_backend.rs:155-156` | `fn assert_send_sync<T: Send + Sync>() {} assert_send_sync::<TuiBackend>();` — the workspace's only explicit auto-trait assertion, and it passes |

This matches the phase 1 inventory header (6 `unsafe` blocks workspace-wide, all
`std::env::set_var`/`remove_var` in test code, section A) and the per-section notes:
B "0 `unsafe impl Send/Sync`", D "no explicit `unsafe impl Send/Sync`", H "no manual
`Send`/`Sync`", G "no `Hash`, `Borrow`, `Iterator`, `Deref`, `Send`, or `Sync` impls,
hand-written or otherwise".

### 1.3 Sub-bucket (b): Rc/RefCell/Cell/raw pointers smuggled across tasks

Confirmed absent, as expected. There are zero `Rc`, `RefCell`, `Cell`, raw-pointer and
`thread_local!` tokens in `crates/`, and zero `unsafe` blocks outside the six env setters,
so there is no mechanism by which a `!Sync` cell could reach a second thread. Nothing to
experiment on.

### 1.4 Sub-bucket (d): non-Send guards held across `.await` in a `Send` future

Not a UB bucket, and nothing circumvents the check: zero `?Send` async-trait
relaxations, zero `await_holding_*` allows, zero alternative executors (table above).

One clarification on the framing, because it matters for what the absence of compile
errors actually proves. The `Send` bound on a future comes from `tokio::spawn`, not from
async itself: `Runtime::block_on` and `#[tokio::main]` impose no `Send` requirement, so a
non-Send guard held across `.await` compiles fine in a future that is only ever driven by
`block_on`. `terraphim_agent` has zero `tokio::spawn` in `src`, so nothing in that crate
is ever *required* to be `Send`, and "it compiles" is therefore not by itself evidence
that no guard crosses an await there.

Checked directly instead:

```
$ ast-grep scan --rule ~/.claude/skills/rust-undefined-behavior-exorcist/scripts/patterns/await-holding-mutex.yml crates
```

Zero hits across the workspace. And the one std-lock-in-async site the audit knows
about — `kg_validation.rs:87-116`, a `std::sync::RwLock` guard held across filesystem
I/O, reached from `handle_hook_command` on a tokio worker — contains no async at all:
`rg -n 'async fn|\.await' crates/terraphim_agent/src/kg_validation.rs` returns zero, so
the guard provably cannot span an await point. It is a worker-blocking and
fail-open-on-poison concern (phase 1 note B, "Panic/poison surface", and F-004 in
`phase2_findings_panic-safety.md`), not a Send/Sync one. The tokio-guard-across-await
sites in `service.rs`, `repl/handler.rs` and `commands/registry.rs` are `tokio::sync`
guards, which are `Send`; they are a lock-ordering question already catalogued in note B.

---

## 2. Spawn-site evidence table (sub-bucket c)

Enumerated with:

```
rg -n 'tokio::spawn|spawn_blocking|block_in_place|std::thread::spawn|thread::spawn|thread::scope|rayon|par_iter|spawn_local|LocalSet|Handle::current' --type rust crates
rg -n '\.spawn\(|tokio::join!|try_join!|join_all|JoinSet|futures::join|select!' --type rust crates
```

The question for each row is whether the `Send` bound is met by safe means, and in
particular whether any `Arc<Mutex<T>>` / `Arc<RwLock<T>>` is carrying a `!Sync` inner.
None is: every inner type is a plain collection or plain data (`HashMap`, `String`,
`i32`, `Option<AutocompleteIndex>`, `Thesaurus`, `RoleGraph`, `Config`). Note that
`Mutex<T>: Sync` requires `T: Send` and `RwLock<T>: Sync` additionally requires
`T: Sync`, both of which the compiler checks here with no `unsafe` in the path, so the
lock wrappers are not laundering anything.

### Production code

| file:line | Construct | State moved in | Send satisfied by |
|---|---|---|---|
| `crates/terraphim_update/src/lib.rs:263` | `spawn_blocking` | `ManifestConfig` clone, two `String`s | owned plain data |
| `crates/terraphim_update/src/lib.rs:307` | `spawn_blocking` | `ManifestConfig`, `String` × 2, `bool` | owned plain data |
| `crates/terraphim_update/src/lib.rs:414` | `spawn_blocking` | `String` × 4, `bool`, `Option<String>` auth token | owned plain data |
| `crates/terraphim_update/src/lib.rs:574` | `spawn_blocking` | as above plus `Vec<[u8; 32]>` verifying keys | owned plain data |
| `crates/terraphim_update/src/lib.rs:692` | `spawn_blocking` | `String` × 4, `bool` | owned plain data |
| `crates/terraphim_update/src/lib.rs:1283` | `spawn_blocking` | two `String`s | owned plain data |
| `crates/terraphim_update/src/lib.rs:1434` | `tokio::spawn` | `mpsc::UnboundedReceiver<UpdateNotification>`, `Arc<Box<dyn Fn(UpdateAvailableInfo) + Send + Sync>>` | the declared `Send + Sync` bound on the callback (`lib.rs:1391`); tokio channel halves are `Send` when the item is |
| `crates/terraphim_update/src/scheduler.rs:160` | `tokio::spawn` | `Arc<UpdateConfig>`, `Arc<dyn Fn() -> Result<UpdateCheckResult> + Send + Sync>` (`scheduler.rs:34`), `mpsc::UnboundedSender` | the explicit trait-object bound |
| `crates/terraphim_update/src/scheduler.rs:250` | `spawn_blocking` | clone of the same `Arc<dyn Fn .. + Send + Sync>` | same |
| `crates/terraphim_update/src/lib.rs:1407` | `block_in_place` + nested `Runtime::new` | two `String`s | Runtime-nesting question, not Send/Sync. Cross-ref phase 1 note E lines 53-74, which resolves it: the closure is reachable only via `perform_check` -> `spawn_blocking`, where `current_enter_context()` is `NotEntered`, so neither `block_in_place` nor the nested runtime panics. |
| `crates/terraphim_mcp_server/src/main.rs:289` | `tokio::spawn` | only `server`, the axum `Serve` future (owns the `TcpListener`, the router and a `CancellationToken` child) | axum and tokio types are `Send` |
| `crates/terraphim_mcp_server/src/main.rs:294` | `sse_server.with_service(move \|\| service.clone())` | the `McpService` | rmcp's own `Send + Sync + 'static` bound on the service factory. Satisfied structurally: `Arc<ConfigState>`, `Arc<TerraphimResourceMapper>` (unit struct), `Arc<tokio::sync::RwLock<Option<AutocompleteIndex>>>`, `Option<Arc<KgPathScorer>>` (`RwLock<Thesaurus>` inner), `Option<SharedFrecency>` (`lib.rs:96-104`) |
| `crates/terraphim_lsp/src/server.rs:51` | `tokio::spawn` | `Arc<tokio::sync::Mutex<HashMap<Url, String>>>`, `Arc<tokio::sync::Mutex<HashMap<Url, AbortHandle>>>`, `NegativeContributionScanner` (a `Thesaurus` by value), tower-lsp `Client`, `u64`, two `Url`s | plain inners plus tower-lsp's own `Client: Clone + Send + Sync`; `LanguageServer` already requires `Send + Sync + 'static` (phase 1 note H line 49) |
| `crates/terraphim_sessions/src/connector/native.rs:161` | `spawn_blocking` (detached) | `Arc<PathBuf>`, `tokio::mpsc::Sender<Session>`, `oneshot::Sender<Result<()>>` | plain data; `Session` is `Send` |
| `crates/terraphim_sessions/src/connector/native.rs:163` | `notify` watcher callback, runs on notify's own thread | `std::sync::mpsc::Sender<notify::Result<Event>>` | `notify::EventHandler` requires `Send + 'static`; the std `Sender` is `Send` |
| `crates/terraphim_sessions/src/connector/cursor.rs:129` | `spawn_blocking`, awaited | `Vec<PathBuf>`; the `HashSet<String>` and each `rusqlite::Connection` are created **inside** the closure (`cursor.rs:131,163`) | nothing shared; `rusqlite::Connection` (which is `!Sync`) never crosses the boundary |
| `crates/terraphim_sessions/src/cla/connector.rs:62,122` | `spawn_blocking`, awaited | owned `cla_options`; a fresh stateless connector is built inside the closure | owned plain data |
| `crates/terraphim_agent/src/shared_learning/store.rs:609` | `block_in_place` + `Handle::current().block_on` | nothing moved (same thread) | Runtime-flavour question, not Send/Sync. Cross-ref phase 1 note B "Runtime-nesting sites" and its open question 1 (the production caller is `terraphim_middleware::feedback_loop`, whose runtime flavour is out of section scope) |
| `crates/terraphim-session-analyzer/src/parser.rs:177,242,284` | `rayon` `par_iter` | `&SessionParser` (`Vec<SessionEntry>`, two `String`s) and, at 284, `&dyn PatternMatcher` | rayon requires `Sync` on the borrowed data. Satisfied structurally, and for the trait object by the declared `pub trait PatternMatcher: Send + Sync` (`patterns/matcher.rs:19`) |
| `crates/terraphim-session-analyzer/src/analyzer.rs:72` | `rayon` `par_iter` over `self.parsers` | `&SessionAnalyzer` | same; all fields are plain data |
| `crates/terraphim_grep/src/lib.rs:4` | none | — | Doc drift: the crate-level doc claims the pipelines run "concurrently via `tokio::spawn`", but `rg -n 'spawn' crates/terraphim_grep/src` returns only that doc line. No spawn exists. Cosmetic, recorded so a later reader does not go looking for a site. |

### Test code

| file:line | Construct | State moved in | Send satisfied by |
|---|---|---|---|
| `crates/terraphim_agent/tests/error_handling_test.rs:257` | `tokio::spawn` × 10 | `ApiClient` clone, `usize` | plain data |
| `crates/terraphim_agent/tests/persistence_tests.rs:386` | `tokio::spawn` per role | `Arc<ConfigState>`, `String` | `ConfigState` is `{ Arc<tokio::Mutex<Config>>, AHashMap<RoleName, RoleGraphSync> }` (registry `terraphim_config-1.20.2/src/lib.rs:1109-1114`); plain inners |
| `crates/terraphim_agent/tests/update_functionality_tests.rs:253-256` | `JoinSet` | `&'static str` | plain data |
| `crates/terraphim_agent/tests/extract_functionality_validation.rs:17` | `std::thread::spawn` | `std::process::Child`, `std::sync::mpsc::Sender` | plain data; the child is waited on inside the thread |
| `crates/terraphim_update/tests/r2_update.rs:26,30` | `std::thread::spawn` × 2 (accept loop + per-connection) | `TcpListener`, `HashMap<String,(u16,Vec<u8>,String)>` clone, `TcpStream` | plain data, cloned per connection |
| `crates/terraphim_update/tests/manifest.rs:31` | `std::thread::spawn` | `TcpListener`, `OneShotConfig` | plain data |
| `crates/terraphim_update/tests/manifest.rs:80` | `std::thread::spawn` | two `Arc<AtomicUsize>`, `TcpListener` | atomics are `Sync`; cross-ref phase 1 note E line 37 |
| `crates/terraphim_update/tests/integration_test.rs:418` | `std::thread::spawn` × 5 | `Arc<TempDir>`, `Arc<Mutex<i32>>` counter | `i32` inner; cross-ref phase 1 note E line 142 |
| `crates/terraphim_mcp_server/tests/test_tools_list.rs:47` | `std::thread::spawn` | `Arc<std::sync::Mutex<String>>`, `ChildStderr` | `String` inner; cross-ref phase 1 note G line 41 |

No `LocalSet`, no `spawn_local`, no `thread::scope`, no `futures::executor` anywhere, so
there is no context in which a `!Send` future or value is legal and could later be moved.

---

## F-801 — event-listener 5.4.1 unsound `Send`/`Sync` for `StackSlot` reaches the five terraphim_config-dependent crates through sqlx

**File:line**: `Cargo.lock:1752-1755` (`event-listener 5.4.1`); consumer
`~/.cargo/registry/src/index.crates.io-*/sqlx-core-0.8.6/src/pool/inner.rs:33,62,119` and
`.../pool/mod.rs:65,266`.

**Kind**: dependency-inherited unsound auto-trait impl (`unsafe impl<T> Send for
StackSlot<'_, T> {}` and the matching `Sync`, `event-listener-5.4.1/src/lib.rs:1500-1501`).

**Bucket(s)**: 8 (send-sync invariants); overlaps the phase 1 section J advisory table.

**Severity**: CONTRACTUAL-BUT-DEFENSIBLE.

**Static evidence**

Two facts, deliberately kept apart, because the task's "compiled but never used" escape
hatch applies to only one of them.

*sqlx is compiled AND driven at runtime by default.* `cargo tree --offline --workspace -e
normal -i event-listener` gives the single chain `event-listener 5.4.1 -> sqlx-core 0.8.6
-> sqlx 0.8.6 -> opendal 0.54.1 -> {terraphim_config 1.20.2, terraphim_persistence 1.20.2,
terraphim_orchestrator 1.21.0, terraphim_service 1.21.1, terraphim_usage 1.20.3}`, which
reaches five of the eleven workspace members: `terraphim-cli`, `terraphim_agent`,
`terraphim_command_runtime`, `terraphim_mcp_server` and `terraphim_grep`. It does not
reach `terraphim_lsp`, `terraphim_hooks`, `terraphim_sessions`, `terraphim_update`,
`terraphim_negative_contribution` or `terraphim-session-analyzer`. The
`services-sqlite` opendal feature is on in the resolved graph (`cargo tree -p
terraphim_agent -e features -i opendal` lists `opendal feature "services-sqlite"`,
enabled through `terraphim_persistence feature "sqlite"` via
`terraphim_persistence-1.20.2/Cargo.toml:62-64`, itself pulled in by
`terraphim_orchestrator` and by persistence's own `default`). It is also exercised, not
merely linked: `terraphim_settings-1.20.2/src/lib.rs:170-180` writes a `sqlite` profile
into the embedded defaults ("Use only SQLite for persistent storage; DashMap disabled"),
`terraphim_persistence-1.20.2/src/settings.rs:277,314` turns that profile into
`Operator::from_iter::<services::Sqlite>(..)`, and opendal's backend lazily builds a real
`sqlx::SqlitePool` on first use (`opendal-0.54.1/src/services/sqlite/backend.rs:176,185-188`).
Workspace code reaches it through `DeviceStorage::instance()`
(`crates/terraphim_agent/src/main.rs:2340`) and the `DeviceSettings::load_from_env_and_file`
paths at `crates/terraphim_agent/src/service.rs:78` and
`crates/terraphim_cli/src/service.rs:58`. So: sqlx is live in production for those five
crates, and the "compiled but never used" answer would be wrong.

*The unsound path is nevertheless not exercised.* The advisory
(`~/.cargo/advisory-db/crates/event-listener/RUSTSEC-2026-0221.md`, `informational =
"unsound"`, patched `>= 5.4.2`) is specific: it requires a `!Send` **tag** installed via
`Event::with_tag` (`event-listener-5.4.1/src/lib.rs:247`) reaching another thread through
the stack-allocated `StackSlot` that the `listener!` macro creates. sqlx-core uses neither
half. It constructs `event_listener::Event::new()` — the default tag is `()`, which is
`Send` — at `pool/inner.rs:33,62`, and it takes heap listeners through `.listen()` into an
owned `EventListener` field (`pool/inner.rs:119`, `pool/mod.rs:65,266`), never the
stack-slot macro. `rg 'with_tag\(|listener!' ` over `sqlx-core-0.8.6/src` and
`opendal-0.54.1/src`: zero hits. And sqlx-core is the sole dependent of event-listener in
this graph, so no other crate can supply a tag either.

Conclusion: the unsound `unsafe impl` is compiled into those five crates, but no reachable
call constructs the affected type with a `!Send` tag. Nothing in this workspace's own code
can widen that, because the workspace has no `unsafe`, no manual auto-trait impl and no
`PhantomData` marker (section 1 above).

**Draft experiment**

1. `cargo update -p event-listener --precise 5.4.2` (semver-compatible within the 5.4.x
   line; no API change is involved, only the removal of the two blanket impls).
2. `cargo tree --offline --workspace -e normal -i event-listener` to confirm the single
   chain now resolves to 5.4.2.
3. `cargo audit` and `cargo deny check advisories` to confirm RUSTSEC-2026-0221 drops off
   the report.
4. `cargo check --workspace --all-targets` — expected to be a no-op change, since nothing
   in the graph names `StackSlot`.

A Miri or loom experiment is not warranted: there is no reachable construction of the
affected type to drive, and the defect is a missing negative bound rather than a race the
workspace can trigger.

**Cross-refs**

- Phase 1 note J, advisory table row RUSTSEC-2026-0221 and the "Send/Sync-relevant
  dependency notes" paragraph, which reached the same CONTRACTUAL-BUT-DEFENSIBLE verdict
  pending the bump. This block adds the reachability proof in both directions (sqlx is
  live; the tag path is not).
- Bundle with the lockfile bumps already tracked as #254 (h2) and #257/#258, since they
  are the same one-line `Cargo.lock` change class. Note J.md ties only h2 to #254; the
  anyhow bump (RUSTSEC-2026-0190) carries no issue number there.
- Other unsound advisories in the same graph (crossbeam-epoch RUSTSEC-2026-0204, anyhow
  RUSTSEC-2026-0190, lru RUSTSEC-2026-0253, memmap2 RUSTSEC-2026-0186) belong to other
  buckets; only event-listener is a Send/Sync defect.

---

## 3. Open questions carried forward

1. Phase 1 note B open question 1 is unresolved and is a runtime-flavour question rather
   than a Send/Sync one: `shared_learning/store.rs:609` panics if
   `terraphim_middleware::feedback_loop` ever drives it from a `current_thread` runtime or
   inside a `LocalSet`. Owned by whoever sweeps runtime nesting, not this bucket.
2. Whether the event-listener bump lands as part of #254 or separately. No technical
   dependency either way.
