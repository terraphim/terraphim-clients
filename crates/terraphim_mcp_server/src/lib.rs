//! MCP (Model Context Protocol) server for Terraphim AI.
//!
//! Exposes Terraphim search, autocomplete, and knowledge-graph functions as MCP
//! tools so that AI development tools (Claude, Cursor, …) can invoke them via
//! the standard MCP wire protocol.
//!
//! # Transports
//!
//! - **stdio** -- for local development and Claude Desktop integration
//! - **SSE/HTTP** -- for production deployments
//!
//! # Key Types
//!
//! - [`McpService`] -- `rmcp::ServerHandler` implementation, the main entry point
//! - [`TerraphimMcpError`] -- unified error type covering service, JSON, I/O, and MCP errors
//! - [`resource_mapper::TerraphimResourceMapper`] -- maps Terraphim documents to MCP resources

use anyhow::Result;
use base64::Engine;
use fff_search::{
    FFFMode, FilePicker, FilePickerOptions, FuzzySearchOptions, GrepMode, GrepSearchOptions,
    PaginationArgs, QueryParser, SharedFrecency, parse_grep_query,
};
use rmcp::{
    RoleServer, ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, ErrorData, ListResourcesResult,
        ListToolsResult, ReadResourceRequestParam, ReadResourceResult, ServerInfo, Tool,
    },
    service::RequestContext,
};
use std::{collections::HashMap, sync::Arc};
use terraphim_automata::builder::json_decode;
use terraphim_automata::matcher::{extract_paragraphs_from_automata, find_matches};
use terraphim_automata::{AutocompleteConfig, AutocompleteIndex, AutocompleteResult};
use terraphim_config::{Config, ConfigState, Role};
use terraphim_file_search::kg_scorer::KgPathScorer;
use terraphim_service::TerraphimService;
use terraphim_types::{
    Document, Layer, NormalizedTermValue, RelevanceFunction, RoleName, SearchQuery,
};
use thiserror::Error;
use tracing::{error, info};

mod agent_tools;
pub mod resource_mapper;

use crate::resource_mapper::TerraphimResourceMapper;

/// Merge prefix and fuzzy suggestions without letting repeated terms consume slots.
///
/// The fuzzy search already includes prefix matches. Keep distinct authored aliases
/// of one concept, but accept each normalised displayed term/concept pair only once.
fn prefix_fuzzy_suggestions(
    index: &AutocompleteIndex,
    query: &str,
    limit: usize,
) -> Vec<AutocompleteResult> {
    let mut results = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if limit == 0 {
        return results;
    }
    if let Ok(prefix) = terraphim_automata::autocomplete_search(index, query, Some(limit)) {
        for result in prefix {
            if seen.insert((NormalizedTermValue::from(result.term.as_str()), result.id)) {
                results.push(result);
            }
        }
    }
    if results.len() < limit
        && let Ok(fuzzy) =
            terraphim_automata::fuzzy_autocomplete_search(index, query, 0.6, Some(limit))
    {
        for result in fuzzy {
            if seen.insert((NormalizedTermValue::from(result.term.as_str()), result.id)) {
                results.push(result);
                if results.len() == limit {
                    break;
                }
            }
        }
    }
    results
}

/// Find the terraphim_rlm binary in common locations.
fn find_terraphim_rlm_binary() -> std::path::PathBuf {
    // Check ~/.cargo/bin first (typical cargo install location)
    if let Ok(home) = std::env::var("HOME") {
        let path = std::path::PathBuf::from(home).join(".cargo/bin/terraphim_rlm");
        if path.exists() {
            return path;
        }
    }
    // Try PATH
    if let Ok(path) = std::process::Command::new("which")
        .arg("terraphim_rlm")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        && !path.is_empty()
    {
        return std::path::PathBuf::from(path);
    }
    // Fallback to bare name and hope it's in PATH
    std::path::PathBuf::from("terraphim_rlm")
}

/// Unified error type for the Terraphim MCP server.
///
/// Variants cover the four failure domains: service-layer errors from
/// [`terraphim_service`], JSON (de)serialisation, MCP protocol errors
/// ([`ErrorData`]), and general I/O or third-party errors via [`anyhow`].
#[derive(Error, Debug)]
pub enum TerraphimMcpError {
    #[error("Service error: {0}")]
    Service(#[from] terraphim_service::ServiceError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("MCP error: {0}")]
    Mcp(#[from] ErrorData),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}

impl From<TerraphimMcpError> for ErrorData {
    fn from(err: TerraphimMcpError) -> Self {
        ErrorData::internal_error(err.to_string(), None)
    }
}

/// Upper bound on the size of a file the grep tools will read.
///
/// Shared between the content-cache budget and `GrepSearchOptions` so the two
/// cannot drift apart: a file large enough to be refused by one but not the
/// other would be a silent inconsistency.
const MAX_GREP_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// A content-cache budget that refuses fff-search's mmap content cache outright.
///
/// fff-search maps file content through `memmap2::Mmap::map` (the crate's only
/// such call, in `get_cached_content`), whose safety contract requires that the
/// mapped file is not modified concurrently. The crate delegates that guarantee
/// to a background watcher, but `FilePicker::new` never installs one, so a
/// concurrent writer yields invalid-UTF-8 match text and an in-place truncation
/// raises SIGBUS -- which killed the shipped `terraphim_mcp_server --sse` with
/// exit 135 during the audit. `max_files: 0` makes `get_cached_content` return
/// `None` at its count guard before it reaches `absolute_path` or `Mmap::map`,
/// so every read takes the slow path and lands in an owned buffer that no other
/// process can mutate. On Windows `get_cached_content` is already a stub
/// returning `None`, so this simply makes Unix behave the same way.
/// See UF-039 / #293.
///
/// `max_file_size` must stay non-zero. It also gates the *slow* path
/// (`get_content_for_search` rejects any file whose `size > max_file_size`), so
/// zeroing it -- or using `ContentCacheBudget::zero()` -- would silence grep
/// entirely rather than harden it: zero matches, looking like a working fix.
/// `ContentCacheBudget::from_overrides` cannot express this budget either, as it
/// reads `max_files: 0` as "use the library default", hence the struct literal.
///
/// Scope: this removes *fff-search's* mapping, not all mapping in the process.
/// `FrecencyTracker::open` still memory-maps an LMDB data file and libgit2 maps
/// pack files; neither is covered here.
///
/// This duplicates `terraphim_grep`'s helper deliberately: this crate does not
/// depend on `terraphim_grep`, and adding a dependency to share eight lines
/// would couple two binaries for no benefit.
fn no_mmap_budget() -> fff_search::ContentCacheBudget {
    use std::sync::atomic::{AtomicU64, AtomicUsize};

    fff_search::ContentCacheBudget {
        max_files: 0,
        max_bytes: 0,
        max_file_size: MAX_GREP_FILE_SIZE,
        cached_count: AtomicUsize::new(0),
        cached_bytes: AtomicU64::new(0),
    }
}

/// Mirrors fff-search's `PATH_BUF_SIZE` (`simd_path.rs`).
///
/// Not configurable upstream, so it is reproduced here. If a future fff-search
/// changes it, the guard below becomes conservative rather than unsound.
const PATH_BUF_SIZE: usize = 4096;

/// The number of bytes `write_absolute_path` consumes before the relative path.
///
/// Mirrors that function's own arithmetic exactly (`types.rs:343-351`) so the
/// two cannot drift: the base path's encoded bytes, plus one separator unless
/// the base already ends in `MAIN_SEPARATOR`.
///
/// This counts the root's *raw* bytes, while `terraphim_grep`'s call site hands fff-search
/// `base_path: search_path.to_string_lossy().to_string()`. The two agree only
/// because `reject_hostile_tree` has already refused a root that is not valid
/// UTF-8, which makes `to_string_lossy` a no-op on it. That is the invariant the
/// whole length rule rests on: without it a root with invalid bytes would be
/// stored up to three times longer than measured here, and the arithmetic would
/// understate what the buffer actually holds.
fn base_prefix_len(base: &std::path::Path) -> usize {
    let bytes = base.as_os_str().as_encoded_bytes();
    let base_len = bytes.len();
    let sep_len =
        usize::from(base_len > 0 && bytes[base_len - 1] != std::path::MAIN_SEPARATOR as u8);
    base_len + sep_len
}

/// Decide one directory entry, returning its length relative to the search root.
///
/// `base_end_idx` is what `base_prefix_len` returned for the search root: the
/// buffer is shared, so the length rule has to be applied to the total, not to
/// the relative portion alone.
///
/// Split out from the walk to mirror `terraphim_grep::path_guard`, where both
/// rules are unit-tested. The host filesystem will not create every geometry
/// fff-search mishandles -- macOS caps a single pathname at 1024 bytes and Linux
/// at 4096 -- so the arithmetic has to be exercised through this function.
fn check_entry(
    base: &std::path::Path,
    dir: &std::path::Path,
    name: &std::ffi::OsStr,
    base_end_idx: usize,
    parent_rel_len: usize,
) -> Result<usize, String> {
    if name.to_str().is_none() {
        return Err(format!(
            "refusing to search {}: entry {:?} under {} has a name that is not \
             valid UTF-8. fff-search stores paths lossily, which inflates each \
             invalid byte threefold, and the shared {PATH_BUF_SIZE}-byte path \
             buffer then truncates inside a replacement character and returns \
             the result through `from_utf8_unchecked` (UF-040 / #296).",
            base.display(),
            name,
            dir.display(),
        ));
    }

    // Relative length as fff-search stores it: the parent's relative length, a
    // separator once there is a parent component, and the name.
    let name_len = name.as_encoded_bytes().len();
    let rel_len = if parent_rel_len == 0 {
        name_len
    } else {
        parent_rel_len + 1 + name_len
    };

    // `write_absolute_path` fills one buffer with base, separator and relative,
    // so the budget is shared.
    //
    // Its exact truncation threshold is `base_end_idx + rel_len > PATH_BUF_SIZE`:
    // the relative portion is read into `&mut buf[base_end_idx..]`, and
    // `read_to_buf` takes `byte_len.min(buf.len())`, so a total of exactly
    // `PATH_BUF_SIZE` fits and nothing is cut. This guard uses `>=`, one byte
    // stricter, DELIBERATELY -- do not "correct" it to `>`:
    //
    // - One rule then also covers `write_relative_cstr`, whose own threshold is
    //   `rel_len >= PATH_BUF_SIZE` because it reserves its buffer's last byte for
    //   a NUL (`types.rs:366-375`). Since `base_end_idx >= 0`, this condition
    //   subsumes that one and no second branch is needed.
    // - It keeps the guard off the exact dependency boundary, which would
    //   otherwise have to be re-derived on every fff-search upgrade.
    //
    // The cost is that an absolute path summing to exactly `PATH_BUF_SIZE` bytes
    // is refused although `write_absolute_path` alone would handle it. That is
    // intended.
    if base_end_idx + rel_len >= PATH_BUF_SIZE {
        return Err(format!(
            "refusing to search {}: the absolute path to {:?} under {} would be \
             {} bytes ({base_end_idx} for the search root and its separator, plus \
             {rel_len} relative), at or over fff-search's {PATH_BUF_SIZE}-byte path \
             buffer. `write_absolute_path` writes both halves into that one buffer \
             and truncates whatever does not fit, then returns the result through \
             `from_utf8_unchecked` -- which cuts a multi-byte character in half. \
             Searching from a shorter root is usually enough to fix this \
             (UF-040 / #296).",
            base.display(),
            name,
            dir.display(),
            base_end_idx + rel_len,
        ));
    }

    Ok(rel_len)
}

/// Reject a tree that would defeat fff-search's `from_utf8_unchecked`.
///
/// fff-search stores every relative path as `to_string_lossy().into_owned()`.
/// On Linux each invalid byte in an entry name becomes a three-byte U+FFFD, so
/// a hostile name inflates threefold. `ChunkedString::read_to_buf`
/// (`simd_path.rs`) then copies `byte_len.min(buf.len())` bytes into a
/// 4096-byte buffer and returns `core::str::from_utf8_unchecked(&buf[..total])`
/// -- so once the inflated path exceeds the buffer the cut lands inside a
/// U+FFFD and the returned `&str` is not valid UTF-8. That is reached from
/// `FileItem::absolute_path`, which both `get_content_for_search` and
/// `collect_files`' git-status lookup call for every file. The `path` argument
/// of these tools is attacker-chosen, so the check has to run before it reaches
/// the library; upstream offers no path-length cap, no strict-UTF-8 mode and no
/// non-lossy storage switch. See UF-040 / #296.
///
/// Fails, naming the offending entry, when the search root's own path is not
/// valid UTF-8 (it would be stored lossily, inflating it and leaving a name that
/// no longer refers to the directory), when an entry name is not valid UTF-8,
/// when the search root alone fills `PATH_BUF_SIZE` (which makes
/// `write_absolute_path`'s `copy_from_slice` panic rather than truncate), or
/// when the root plus a relative path reaches `PATH_BUF_SIZE`. Base and relative
/// share one buffer (`types.rs:337-357`), so the budget is joint -- the third
/// rule covers ordinary trees of *valid* names reached from a long root, where
/// the truncation cuts a multi-byte character in half with no hostile name
/// involved. EXP-201 did not test that combination, so it is reasoned from
/// `write_absolute_path` and `read_to_buf` rather than observed.
///
/// Names only -- `DirEntry::file_type` uses the directory entry's type, with no
/// `stat` -- and symbolic links are not descended into, matching the
/// `follow_symlinks: false` the pickers now assert. An unreadable directory is
/// skipped rather than reported: fff-search will skip it too, and failing here
/// would turn a permission quirk into a refused search.
///
/// Residual risk: a TOCTOU window remains. An entry created between this walk
/// and fff-search's `collect_files` still reaches the inflation. That needs an
/// active concurrent writer inside the searched tree, whereas the confirmed
/// exposure is a static hostile checkout. The walk also does not honour
/// `.gitignore`, so `target/` and `node_modules/` are walked; for a per-call
/// picker that already pays a full `collect_files` scan immediately after, the
/// cost is acceptable.
///
/// This duplicates `terraphim_grep::path_guard` deliberately: this crate does
/// not depend on `terraphim_grep`, and adding a dependency to share one function
/// would couple two binaries for no benefit.
fn reject_hostile_tree(base: &std::path::Path) -> Result<(), String> {
    // fff-search receives `base_path` as a `String`. `terraphim_grep`'s call site
    // builds it with `to_string_lossy()`, which replaces each invalid byte with a
    // three-byte U+FFFD, so a root with invalid bytes would be stored up to three
    // times longer than `base_prefix_len` measures -- defeating the arithmetic
    // below, and panicking `copy_from_slice` outright once the inflated root
    // alone reaches the buffer. Refusing such a root restores the invariant.
    //
    // It is the right answer regardless of the arithmetic: the lossily stored
    // name no longer names the on-disk entry, so fff-search cannot open anything
    // under it. That is the same silent-drop defect the audit recorded for
    // hostile entry names, and a named error beats searching and finding nothing.
    //
    // Reachable through `terraphim-grep --paths` and `with_search_paths`, since
    // clap builds `Vec<PathBuf>` from `OsString` and invalid bytes survive on
    // Linux. The MCP tools are immune -- their `path` arrives as JSON and is
    // always valid UTF-8 -- but both copies carry the check so neither depends on
    // where it was called from.
    //
    // The root is never an entry the walk discovers, so the per-entry UTF-8 rule
    // in `check_entry` does not cover it.
    if base.to_str().is_none() {
        return Err(format!(
            "refusing to search {:?}: the search root's own path is not valid \
             UTF-8. fff-search stores it lossily, which both inflates each invalid \
             byte threefold past the {PATH_BUF_SIZE}-byte path buffer and leaves a \
             stored name that no longer refers to the directory on disk, so no file \
             under it could be opened (UF-040 / #296).",
            base.as_os_str(),
        ));
    }

    // `write_absolute_path` writes the base path into the same fixed buffer as
    // the relative path, so the root's own length is part of the budget -- and a
    // root at or over the buffer makes `copy_from_slice` panic outright.
    let base_end_idx = base_prefix_len(base);
    if base_end_idx >= PATH_BUF_SIZE {
        return Err(format!(
            "refusing to search {}: the search root alone is {base_end_idx} bytes \
             with its separator, at or over fff-search's {PATH_BUF_SIZE}-byte path \
             buffer, which leaves no room for any relative path. \
             `write_absolute_path` copies the root into that buffer unconditionally, \
             so a root this long panics the library rather than merely truncating \
             (UF-040 / #296).",
            base.display(),
        ));
    }

    let mut stack = vec![(base.to_path_buf(), 0usize)];

    while let Some((dir, rel_len)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let entry_rel_len = check_entry(base, &dir, &name, base_end_idx, rel_len)?;

            // `file_type` reports a symbolic link as such, so links are never
            // descended into -- no `metadata` call, and no loop to detect.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push((entry.path(), entry_rel_len));
            }
        }
    }

    Ok(())
}

// RoleGraph keys documents by ID. Use a temporary composite key during union
// ranking so equal source IDs from different haystacks cannot overwrite each other.
fn rank_expanded_documents(
    graph: &mut terraphim_rolegraph::RoleGraph,
    canonical: &str,
    documents: &mut [Document],
) -> Result<()> {
    documents.sort_by(|a, b| {
        a.source_haystack
            .cmp(&b.source_haystack)
            .then_with(|| a.id.cmp(&b.id))
    });
    let keys: Vec<_> = documents
        .iter()
        .map(|document| serde_json::to_string(&(&document.source_haystack, &document.id)))
        .collect::<Result<_, _>>()?;
    for (key, document) in keys.iter().zip(documents.iter()) {
        graph.insert_document(key, document.clone());
    }
    let ranks: HashMap<_, _> = graph
        .query_graph(canonical, None, None)?
        .into_iter()
        .map(|(id, document)| (id, document.rank))
        .collect();
    for (key, document) in keys.iter().zip(documents.iter_mut()) {
        document.rank = Some(ranks.get(key).copied().unwrap_or(0));
    }
    Ok(())
}

// A deserialised index has no role provenance. Keep it available to the explicit
// index operations without allowing it to impersonate a configured role.
#[derive(Default)]
struct AutocompleteState {
    active: Option<Arc<AutocompleteIndex>>,
    roles: HashMap<RoleName, (Role, Arc<AutocompleteIndex>)>,
}

/// The main service type for the Terraphim MCP server
#[derive(Clone)]
pub struct McpService {
    config_state: Arc<ConfigState>,
    resource_mapper: Arc<TerraphimResourceMapper>,
    autocomplete_index: Arc<tokio::sync::RwLock<AutocompleteState>>,
    /// Optional KG scorer for boosting file search results by path concept matches.
    kg_scorer: Option<Arc<KgPathScorer>>,
    /// Optional persistent frecency tracker (LMDB-backed) for access-frequency scoring.
    frecency: Option<SharedFrecency>,
    agent_store: Option<Arc<tokio::sync::Mutex<terraphim_agent::agent_store::AgentStore>>>,
}

impl McpService {
    /// Create a new service instance
    pub fn new(config_state: Arc<ConfigState>) -> Self {
        let frecency = std::env::var("FFF_FRECENCY_PATH").ok().and_then(|path| {
            fff_search::FrecencyTracker::open(&path)
                .map(|tracker| {
                    let shared = SharedFrecency::default();
                    shared.init(tracker).ok();
                    shared
                })
                .ok()
        });
        tracing::debug!(
            frecency_enabled = frecency.is_some(),
            "MCP service initialised (frecency tracking wired when FFF_FRECENCY_PATH is set)"
        );

        Self {
            config_state,
            resource_mapper: Arc::new(TerraphimResourceMapper::new()),
            autocomplete_index: Arc::new(tokio::sync::RwLock::new(AutocompleteState::default())),
            kg_scorer: None,
            frecency,
            agent_store: None,
        }
    }

    /// Enable local agent tools with an already validated explicit storage root.
    /// Default service instances expose no agent memory/learning tools.
    pub fn with_agent_store(mut self, store: terraphim_agent::agent_store::AgentStore) -> Self {
        self.agent_store = Some(Arc::new(tokio::sync::Mutex::new(store)));
        self
    }

    /// Read-only access to the optional frecency tracker (for consumers that
    /// score file access frequency, e.g. `terraphim_find_files`).
    pub fn frecency(&self) -> Option<&SharedFrecency> {
        self.frecency.as_ref()
    }

    /// Attach a KG scorer so that `terraphim_find_files` boosts results by
    /// knowledge-graph concept matches in the file path.
    pub fn with_kg_scorer(mut self, scorer: Arc<KgPathScorer>) -> Self {
        self.kg_scorer = Some(scorer);
        self
    }

    /// Initialize autocomplete index for the currently selected role, if possible
    pub async fn init_autocomplete_default(&self) {
        if let Ok(result) = self.build_autocomplete_index(None).await {
            if result.is_error == Some(true) {
                tracing::debug!("Autocomplete init skipped: {:?}", result.content);
            } else {
                tracing::info!("Autocomplete index initialized by default");
            }
        } else {
            tracing::debug!("Autocomplete init failed");
        }
    }

    /// Create a Terraphim service instance from the current configuration
    pub async fn terraphim_service(&self) -> Result<TerraphimService, anyhow::Error> {
        // Instead of cloning the old ConfigState (which has stale roles),
        // create a fresh ConfigState from the current config to ensure roles are up-to-date
        let config = self.config_state.config.clone();
        let current_config = config.lock().await;
        let mut fresh_config = current_config.clone();
        drop(current_config);

        let fresh_config_state = terraphim_config::ConfigState::new(&mut fresh_config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create fresh ConfigState: {}", e))?;

        Ok(TerraphimService::new(fresh_config_state))
    }

    /// Update the configuration
    pub async fn update_config(&self, new_config: Config) -> Result<()> {
        let config = self.config_state.config.clone();
        let mut current_config = config.lock().await;
        *current_config = new_config;
        *self.autocomplete_index.write().await = AutocompleteState::default();
        Ok(())
    }

    /// Search for documents in the Terraphim knowledge graph
    pub async fn search(
        &self,
        query: String,
        role: Option<String>,
        limit: Option<i32>,
        skip: Option<i32>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut service = self
            .terraphim_service()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Determine which role to use. If the caller passed `role`, honour it
        // verbatim (matches CLI semantics: explicit always wins). Otherwise
        // auto-route on the query and prepend an explainability line to the
        // response so MCP clients can see the routing decision.
        let (role_name, auto) = if let Some(role_str) = role {
            (RoleName::from(role_str), None)
        } else {
            let config_snapshot = self.config_state.config.lock().await.clone();
            let selected = self.config_state.get_selected_role().await;
            let selected_normalised = if config_snapshot.roles.contains_key(&selected) {
                Some(selected)
            } else {
                None
            };
            let ctx =
                terraphim_service::auto_route::AutoRouteContext::from_env(selected_normalised);
            let result = terraphim_service::auto_route::auto_select_role(
                &query,
                &config_snapshot,
                &self.config_state,
                &ctx,
            )
            .await;
            (result.role.clone(), Some(result))
        };

        if limit.is_some_and(|value| value < 0) || skip.is_some_and(|value| value < 0) {
            return Ok(CallToolResult::error(vec![Content::text(
                "Search limit and skip must be non-negative",
            )]));
        }

        match self
            .search_role_documents(&mut service, &query, &role_name, limit, skip)
            .await
        {
            Ok(documents) => {
                let mut contents = Vec::new();
                if let Some(ref ar) = auto {
                    contents.push(Content::text(format!(
                        "[auto-route] picked role \"{}\" (score={}, candidates={}); pass role parameter to override",
                        ar.role.as_str(),
                        ar.score,
                        ar.candidates.len(),
                    )));
                }
                let summary = format!("Found {} documents matching your query.", documents.len());
                contents.push(Content::text(summary));

                let limit = limit.unwrap_or(documents.len() as i32) as usize;
                for (idx, doc) in documents.iter().enumerate() {
                    if idx >= limit {
                        break;
                    }

                    let resource_contents = self
                        .resource_mapper
                        .document_to_resource_contents(doc)
                        .unwrap();
                    contents.push(Content::resource(resource_contents));
                }

                Ok(CallToolResult::success(contents))
            }
            Err(e) => {
                error!("Search failed: {}", e);
                let error_content = Content::text(format!("Search failed: {}", e));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    // Expand only exact authored concepts, using the same normalised-term
    // identity as terraphim-grep. Free-text and compound query syntax is unchanged.
    async fn search_role_documents(
        &self,
        service: &mut TerraphimService,
        query: &str,
        role_name: &RoleName,
        limit: Option<i32>,
        skip: Option<i32>,
    ) -> Result<Vec<Document>> {
        let role =
            self.config_state.get_role(role_name).await.ok_or_else(|| {
                anyhow::anyhow!("Role '{}' not found in configuration", role_name)
            })?;
        let mut terms = vec![query.to_owned()];
        let mut expansion_graph = None;
        if role.relevance_function == RelevanceFunction::TerraphimGraph {
            let thesaurus = service.ensure_thesaurus_loaded(role_name).await?;
            let normalised_query = NormalizedTermValue::from(query.trim());
            if let Some(matched) = find_matches(query.trim(), &thesaurus, false)?
                .into_iter()
                .find(|matched| {
                    NormalizedTermValue::from(matched.term.as_str()) == normalised_query
                })
            {
                let canonical = matched.normalized_term.value;
                terms.clear();
                terms.extend(
                    (&thesaurus)
                        .into_iter()
                        .filter(|(_, term)| term.value == canonical)
                        .map(|(alias, _)| alias.to_string()),
                );
                terms.push(canonical.to_string());
                terms.sort();
                terms.dedup();
                expansion_graph = Some((
                    terraphim_rolegraph::RoleGraph::new_sync(role_name.clone(), thesaurus)?,
                    canonical,
                ));
            }
        }
        if expansion_graph.is_none() {
            // Preserve existing pagination and ordering for non-KG/free-text
            // queries instead of widening their underlying retrieval.
            return Ok(service
                .search(&SearchQuery {
                    search_term: NormalizedTermValue::from(query),
                    search_terms: None,
                    operator: None,
                    role: Some(role_name.clone()),
                    limit: limit.map(|value| value as usize),
                    skip: skip.map(|value| value as usize),
                    layer: Layer::default(),
                    include_pinned: false,
                    min_quality: None,
                })
                .await?);
        }
        let expanded = terms.len() > 1;
        let mut documents = HashMap::new();
        for term in terms {
            let search_query = SearchQuery {
                search_term: NormalizedTermValue::from(term),
                search_terms: None,
                operator: None,
                role: Some(role_name.clone()),
                // Apply pagination once to the merged, deduplicated documents.
                limit: None,
                skip: None,
                layer: Layer::default(),
                include_pinned: false,
                min_quality: None,
            };
            for document in service.search(&search_query).await? {
                let key = (document.source_haystack.clone(), document.id.clone());
                let entry = documents.entry(key).or_insert_with(|| document.clone());
                if document.rank > entry.rank {
                    *entry = document;
                }
            }
        }
        let mut documents: Vec<Document> = documents.into_values().collect();
        if let Some((mut graph, canonical)) = expansion_graph {
            // Per-query ranks describe different haystacks and cannot be merged
            // as scores. Rank the union once with the existing RoleGraph engine.
            // Insert in identity order to keep repeated calls deterministic.
            rank_expanded_documents(&mut graph, canonical.as_str(), &mut documents)?;
        }
        // A stable identity tie-breaker makes synonym and canonical pagination
        // agree, independent of the haystack/index map's iteration order.
        documents.sort_by(|a, b| {
            b.rank
                .cmp(&a.rank)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| a.source_haystack.cmp(&b.source_haystack))
        });
        tracing::debug!(
            expanded,
            documents = documents.len(),
            "Resolved MCP search documents"
        );
        Ok(documents
            .into_iter()
            .skip(skip.unwrap_or(0) as usize)
            .take(limit.map_or(usize::MAX, |value| value as usize))
            .collect())
    }

    async fn role_autocomplete_index(
        &self,
        role: Option<String>,
    ) -> Result<(RoleName, Arc<AutocompleteIndex>), CallToolResult> {
        // Lock order is config then index, also used by update/build. A request
        // receives one coherent role snapshot rather than another role's cache.
        let config = self.config_state.config.lock().await;
        let name = role
            .map(RoleName::from)
            .unwrap_or_else(|| config.selected_role.clone());
        let Some(role) = config.roles.get(&name) else {
            return Err(CallToolResult::error(vec![Content::text(format!(
                "Role '{}' not found in configuration",
                name
            ))]));
        };
        let state = self.autocomplete_index.read().await;
        match state.roles.get(&name) {
            Some((built_role, index)) if built_role == role => Ok((name, Arc::clone(index))),
            _ => Err(CallToolResult::error(vec![Content::text(format!(
                "Autocomplete index not built for role '{}'. Run 'build_autocomplete_index' for this role first.",
                name
            ))])),
        }
    }

    /// Update the Terraphim configuration
    pub async fn update_config_tool(
        &self,
        config_str: String,
    ) -> Result<CallToolResult, ErrorData> {
        match serde_json::from_str::<Config>(&config_str) {
            Ok(new_config) => match self.update_config(new_config).await {
                Ok(()) => {
                    let content = Content::text("Configuration updated successfully".to_string());
                    Ok(CallToolResult::success(vec![content]))
                }
                Err(e) => {
                    error!("Failed to update configuration: {}", e);
                    let error_content =
                        Content::text(format!("Failed to update configuration: {}", e));
                    Ok(CallToolResult::error(vec![error_content]))
                }
            },
            Err(e) => {
                error!("Failed to parse config: {}", e);
                let error_content = Content::text(format!("Invalid configuration JSON: {}", e));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Build autocomplete index from current role's thesaurus
    pub async fn build_autocomplete_index(
        &self,
        role: Option<String>,
    ) -> Result<CallToolResult, ErrorData> {
        // Determine which role to use (provided role or selected role)
        let role_name = if let Some(role_str) = role {
            RoleName::from(role_str)
        } else {
            self.config_state.get_selected_role().await
        };

        // Check if the role exists and has proper knowledge graph configuration
        let role_config = self.config_state.get_role(&role_name).await;
        if let Some(ref role_cfg) = role_config {
            // Check if role uses TerraphimGraph relevance function (required for knowledge graph)
            if role_cfg.relevance_function != terraphim_types::RelevanceFunction::TerraphimGraph {
                let error_content = Content::text(format!(
                    "Role '{}' does not use knowledge graph ranking (TerraphimGraph). Autocomplete is only available for roles with knowledge graph-based ranking. Current relevance function: {:?}",
                    role_name, role_cfg.relevance_function
                ));
                return Ok(CallToolResult::error(vec![error_content]));
            }

            // Check if role has knowledge graph configuration
            let kg_is_properly_configured = role_cfg
                .kg
                .as_ref()
                .map(|kg| kg.automata_path.is_some() || kg.knowledge_graph_local.is_some())
                .unwrap_or(false);

            if !kg_is_properly_configured {
                let error_content = Content::text(format!(
                    "Role '{}' does not have a properly configured knowledge graph. Autocomplete requires a role with defined automata_path or local knowledge graph.",
                    role_name
                ));
                return Ok(CallToolResult::error(vec![error_content]));
            }
        } else {
            let error_content = Content::text(format!(
                "Role '{}' not found in configuration. Available roles: {:?}",
                role_name,
                self.config_state.roles.keys().collect::<Vec<_>>()
            ));
            return Ok(CallToolResult::error(vec![error_content]));
        }

        let mut service = self
            .terraphim_service()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Load thesaurus for the role
        match service.ensure_thesaurus_loaded(&role_name).await {
            Ok(thesaurus_data) => {
                if thesaurus_data.is_empty() {
                    let error_content = Content::text(format!(
                        "No thesaurus data available for role '{}'. Please ensure the role has a properly configured and loaded knowledge graph.",
                        role_name
                    ));
                    return Ok(CallToolResult::error(vec![error_content]));
                }

                info!(
                    "Building autocomplete index from {} thesaurus entries for role: {}",
                    thesaurus_data.len(),
                    role_name
                );

                let config = AutocompleteConfig::default();
                match terraphim_automata::build_autocomplete_index(
                    thesaurus_data.clone(),
                    Some(config),
                ) {
                    Ok(index) => {
                        let config = self.config_state.config.lock().await;
                        if config.roles.get(&role_name) != role_config.as_ref() {
                            return Ok(CallToolResult::error(vec![Content::text(
                                "Role configuration changed while building the index; rebuild it",
                            )]));
                        }
                        let index = Arc::new(index);
                        let mut state = self.autocomplete_index.write().await;
                        state.roles.insert(
                            role_name.clone(),
                            (role_config.unwrap(), Arc::clone(&index)),
                        );
                        state.active = Some(index);

                        let content = Content::text(format!(
                            "Autocomplete index built successfully with {} terms for role '{}'",
                            thesaurus_data.len(),
                            role_name
                        ));
                        Ok(CallToolResult::success(vec![content]))
                    }
                    Err(e) => {
                        error!("Failed to build autocomplete index: {}", e);
                        let error_content =
                            Content::text(format!("Failed to build autocomplete index: {}", e));
                        Ok(CallToolResult::error(vec![error_content]))
                    }
                }
            }
            Err(e) => {
                error!("Failed to load thesaurus for role '{}': {}", role_name, e);
                let error_content = Content::text(format!(
                    "Failed to load thesaurus for role '{}': {}. Please ensure the role has a valid knowledge graph configuration with accessible automata_path.",
                    role_name, e
                ));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Autocomplete terms (prefix + fuzzy) returning structured results
    pub async fn autocomplete_terms(
        &self,
        query: String,
        limit: Option<usize>,
        role: Option<String>,
    ) -> Result<CallToolResult, ErrorData> {
        let (_, index) = match self.role_autocomplete_index(role).await {
            Ok(value) => value,
            Err(result) => return Ok(result),
        };
        let max_results = limit.unwrap_or(10);
        let mut combined = prefix_fuzzy_suggestions(&index, &query, max_results);
        // Graph embeddings-style expansion: include additional synonyms sharing the same concept id
        if combined.len() < max_results {
            use std::collections::HashSet;
            let mut seen_terms: HashSet<_> = combined
                .iter()
                .map(|r| (NormalizedTermValue::from(r.term.as_str()), r.id))
                .collect();
            let concept_ids: HashSet<u64> = combined.iter().map(|r| r.id).collect();
            let mut candidates: Vec<AutocompleteResult> = Vec::new();
            // Iterate over all metadata and pick terms with same concept id
            for (_, meta) in terraphim_automata::autocomplete_helpers::iter_metadata(&index) {
                let identity = (
                    NormalizedTermValue::from(meta.original_term.as_str()),
                    meta.id,
                );
                if concept_ids.contains(&meta.id) && !seen_terms.contains(&identity) {
                    candidates.push(AutocompleteResult {
                        term: meta.original_term.clone(),
                        normalized_term: meta.normalized_term.clone(),
                        id: meta.id,
                        url: meta.url.clone(),
                        score: 0.0,
                    });
                }
            }
            // Prefer shorter terms, stable order
            candidates.sort_by(|a, b| {
                a.term
                    .len()
                    .cmp(&b.term.len())
                    .then_with(|| a.term.cmp(&b.term))
            });
            for cand in candidates {
                if combined.len() >= max_results {
                    break;
                }
                if seen_terms.insert((NormalizedTermValue::from(cand.term.as_str()), cand.id)) {
                    combined.push(cand);
                }
            }
        }
        let mut contents = Vec::new();
        contents.push(Content::text(format!(
            "Found {} suggestions",
            combined.len()
        )));
        for r in combined.into_iter().take(max_results) {
            let line = r.term.to_string();
            contents.push(Content::text(line));
        }
        Ok(CallToolResult::success(contents))
    }

    /// Autocomplete with snippets: returns term and a short snippet (stub/description) when available
    pub async fn autocomplete_with_snippets(
        &self,
        query: String,
        limit: Option<usize>,
        role: Option<String>,
    ) -> Result<CallToolResult, ErrorData> {
        let (role_name, index) = match self.role_autocomplete_index(role).await {
            Ok(value) => value,
            Err(result) => return Ok(result),
        };
        let max_results = limit.unwrap_or(10);
        let results = prefix_fuzzy_suggestions(&index, &query, max_results);

        // Build snippets by searching for each term to fetch a related document and use its stub/description/body excerpt
        let mut service = self
            .terraphim_service()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let mut contents = Vec::new();
        contents.push(Content::text(format!(
            "Found {} suggestions",
            results.len()
        )));
        for r in results.into_iter().take(max_results) {
            let snippet = match self
                .search_role_documents(&mut service, &r.term, &role_name, Some(1), Some(0))
                .await
            {
                Ok(documents) if !documents.is_empty() => {
                    let d = &documents[0];
                    if let Some(stub) = &d.stub {
                        stub.clone()
                    } else if let Some(desc) = &d.description {
                        desc.clone()
                    } else {
                        let body = d.body.as_str();
                        body.chars().take(160).collect::<String>()
                    }
                }
                _ => String::new(),
            };
            let line = if snippet.is_empty() {
                r.term
            } else {
                format!("{} — {}", r.term, snippet)
            };
            contents.push(Content::text(line));
        }
        Ok(CallToolResult::success(contents))
    }

    /// Fuzzy autocomplete search using Jaro-Winkler similarity (default, faster)
    pub async fn fuzzy_autocomplete_search(
        &self,
        query: String,
        similarity: Option<f64>,
        limit: Option<usize>,
    ) -> Result<CallToolResult, ErrorData> {
        let autocomplete_lock = self.autocomplete_index.read().await;

        if let Some(index) = autocomplete_lock.active.as_deref() {
            let min_similarity = similarity.unwrap_or(0.6);
            let max_results = limit.unwrap_or(10);

            match terraphim_automata::fuzzy_autocomplete_search(
                index,
                &query,
                min_similarity,
                Some(max_results),
            ) {
                Ok(results) => {
                    let mut contents = Vec::new();
                    let summary = format!(
                        "Found {} autocomplete suggestions for '{}'",
                        results.len(),
                        query
                    );
                    contents.push(Content::text(summary));

                    for result in results {
                        let suggestion = format!("• {} (score: {:.3})", result.term, result.score);
                        contents.push(Content::text(suggestion));
                    }

                    Ok(CallToolResult::success(contents))
                }
                Err(e) => {
                    error!("Autocomplete search failed: {}", e);
                    let error_content = Content::text(format!("Autocomplete search failed: {}", e));
                    Ok(CallToolResult::error(vec![error_content]))
                }
            }
        } else {
            let error_content = Content::text(
                "Autocomplete index not built. Please run 'build_autocomplete_index' first."
                    .to_string(),
            );
            Ok(CallToolResult::error(vec![error_content]))
        }
    }

    /// Fuzzy autocomplete search using Levenshtein distance (baseline comparison)
    pub async fn fuzzy_autocomplete_search_levenshtein(
        &self,
        query: String,
        max_edit_distance: Option<usize>,
        limit: Option<usize>,
    ) -> Result<CallToolResult, ErrorData> {
        let autocomplete_lock = self.autocomplete_index.read().await;

        if let Some(index) = autocomplete_lock.active.as_deref() {
            let max_distance = max_edit_distance.unwrap_or(2);
            let max_results = limit.unwrap_or(10);

            match terraphim_automata::fuzzy_autocomplete_search_levenshtein(
                index,
                &query,
                max_distance,
                Some(max_results),
            ) {
                Ok(results) => {
                    let mut contents = Vec::new();
                    let summary = format!(
                        "Found {} Levenshtein autocomplete suggestions for '{}'",
                        results.len(),
                        query
                    );
                    contents.push(Content::text(summary));

                    for result in results {
                        let suggestion = format!("• {} (score: {:.3})", result.term, result.score);
                        contents.push(Content::text(suggestion));
                    }

                    Ok(CallToolResult::success(contents))
                }
                Err(e) => {
                    error!("Levenshtein autocomplete search failed: {}", e);
                    let error_content =
                        Content::text(format!("Levenshtein autocomplete search failed: {}", e));
                    Ok(CallToolResult::error(vec![error_content]))
                }
            }
        } else {
            let error_content = Content::text(
                "Autocomplete index not built. Please run 'build_autocomplete_index' first."
                    .to_string(),
            );
            Ok(CallToolResult::error(vec![error_content]))
        }
    }

    /// Fuzzy autocomplete search using Jaro-Winkler similarity (explicit)
    pub async fn fuzzy_autocomplete_search_jaro_winkler(
        &self,
        query: String,
        similarity: Option<f64>,
        limit: Option<usize>,
    ) -> Result<CallToolResult, ErrorData> {
        let autocomplete_lock = self.autocomplete_index.read().await;

        if let Some(index) = autocomplete_lock.active.as_deref() {
            let min_similarity = similarity.unwrap_or(0.6);
            let max_results = limit.unwrap_or(10);

            match terraphim_automata::fuzzy_autocomplete_search(
                index,
                &query,
                min_similarity,
                Some(max_results),
            ) {
                Ok(results) => {
                    let mut contents = Vec::new();
                    let summary = format!(
                        "Found {} Jaro-Winkler autocomplete suggestions for '{}'",
                        results.len(),
                        query
                    );
                    contents.push(Content::text(summary));

                    for result in results {
                        let suggestion = format!("• {} (score: {:.3})", result.term, result.score);
                        contents.push(Content::text(suggestion));
                    }

                    Ok(CallToolResult::success(contents))
                }
                Err(e) => {
                    error!("Jaro-Winkler autocomplete search failed: {}", e);
                    let error_content =
                        Content::text(format!("Jaro-Winkler autocomplete search failed: {}", e));
                    Ok(CallToolResult::error(vec![error_content]))
                }
            }
        } else {
            let error_content = Content::text(
                "Autocomplete index not built. Please run 'build_autocomplete_index' first."
                    .to_string(),
            );
            Ok(CallToolResult::error(vec![error_content]))
        }
    }

    /// Serialize autocomplete index to bytes for storage/transmission
    pub async fn serialize_autocomplete_index(&self) -> Result<CallToolResult, ErrorData> {
        let autocomplete_lock = self.autocomplete_index.read().await;

        if let Some(index) = autocomplete_lock.active.as_deref() {
            match terraphim_automata::serialize_autocomplete_index(index) {
                Ok(bytes) => {
                    let mut contents = Vec::new();
                    contents.push(Content::text(format!(
                        "Successfully serialized autocomplete index to {} bytes",
                        bytes.len()
                    )));

                    // Convert bytes to base64 for text representation
                    let base64_data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    contents.push(Content::text("Base64 encoded data:".to_string()));
                    contents.push(Content::text(base64_data));

                    Ok(CallToolResult::success(contents))
                }
                Err(e) => {
                    error!("Serialize autocomplete index failed: {}", e);
                    let error_content =
                        Content::text(format!("Serialize autocomplete index failed: {}", e));
                    Ok(CallToolResult::error(vec![error_content]))
                }
            }
        } else {
            let error_content = Content::text(
                "Autocomplete index not built. Please run 'build_autocomplete_index' first."
                    .to_string(),
            );
            Ok(CallToolResult::error(vec![error_content]))
        }
    }

    /// Deserialize autocomplete index from bytes
    pub async fn deserialize_autocomplete_index(
        &self,
        base64_data: String,
    ) -> Result<CallToolResult, ErrorData> {
        // Decode base64 data
        let bytes = match base64::engine::general_purpose::STANDARD.decode(&base64_data) {
            Ok(data) => data,
            Err(e) => {
                let error_content = Content::text(format!("Invalid base64 data: {}", e));
                return Ok(CallToolResult::error(vec![error_content]));
            }
        };

        match terraphim_automata::deserialize_autocomplete_index(&bytes) {
            Ok(index) => {
                // Store the deserialized index
                let mut autocomplete_lock = self.autocomplete_index.write().await;
                let count = index.len();
                autocomplete_lock.active = Some(Arc::new(index));

                let mut contents = Vec::new();
                contents.push(Content::text(format!(
                    "Successfully deserialized autocomplete index with {} terms",
                    count
                )));

                Ok(CallToolResult::success(contents))
            }
            Err(e) => {
                error!("Deserialize autocomplete index failed: {}", e);
                let error_content =
                    Content::text(format!("Deserialize autocomplete index failed: {}", e));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Find all term matches in text using Aho-Corasick algorithm
    pub async fn find_matches(
        &self,
        text: String,
        role: Option<String>,
        return_positions: Option<bool>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut service = self
            .terraphim_service()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Determine which role to use (provided role or selected role)
        let role_name = if let Some(role_str) = role {
            RoleName::from(role_str)
        } else {
            self.config_state.get_selected_role().await
        };

        // Load thesaurus for the role
        match service.ensure_thesaurus_loaded(&role_name).await {
            Ok(thesaurus_data) => {
                if thesaurus_data.is_empty() {
                    let error_content = Content::text(format!(
                        "No thesaurus data available for role '{}'. Please ensure the role has a properly configured and loaded knowledge graph.",
                        role_name
                    ));
                    return Ok(CallToolResult::error(vec![error_content]));
                }

                let return_pos = return_positions.unwrap_or(false);

                match find_matches(&text, &thesaurus_data, return_pos) {
                    Ok(matches) => {
                        let mut contents = Vec::new();
                        let summary = format!(
                            "Found {} term matches in text for role '{}'",
                            matches.len(),
                            role_name
                        );
                        contents.push(Content::text(summary));

                        for matched in matches.iter() {
                            let match_info = if return_pos {
                                if let Some((start, end)) = matched.pos {
                                    format!("• {} (pos: {}-{})", matched.term, start, end)
                                } else {
                                    format!("• {} (no position)", matched.term)
                                }
                            } else {
                                format!("• {}", matched.term)
                            };
                            contents.push(Content::text(match_info));
                        }

                        Ok(CallToolResult::success(contents))
                    }
                    Err(e) => {
                        error!("Find matches failed: {}", e);
                        let error_content = Content::text(format!("Find matches failed: {}", e));
                        Ok(CallToolResult::error(vec![error_content]))
                    }
                }
            }
            Err(e) => {
                error!("Failed to load thesaurus for role '{}': {}", role_name, e);
                let error_content = Content::text(format!(
                    "Failed to load thesaurus for role '{}': {}. Please ensure the role has a valid knowledge graph configuration.",
                    role_name, e
                ));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Replace matched terms in text with links using specified format
    pub async fn replace_matches(
        &self,
        text: String,
        role: Option<String>,
        link_type: String,
    ) -> Result<CallToolResult, ErrorData> {
        let mut service = self
            .terraphim_service()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        let role_name = if let Some(role_str) = role {
            RoleName::from(role_str)
        } else {
            self.config_state.get_selected_role().await
        };

        let link_type_enum = match link_type.to_lowercase().as_str() {
            "wiki" | "wikilinks" => terraphim_hooks::LinkType::WikiLinks,
            "html" | "htmllinks" => terraphim_hooks::LinkType::HTMLLinks,
            "markdown" | "md" => terraphim_hooks::LinkType::MarkdownLinks,
            "plain" | "plaintext" => terraphim_hooks::LinkType::PlainText,
            _ => {
                let error_content = Content::text(format!(
                    "Invalid link type '{}'. Supported types: wiki, html, markdown, plain",
                    link_type
                ));
                return Ok(CallToolResult::error(vec![error_content]));
            }
        };

        match service.ensure_thesaurus_loaded(&role_name).await {
            Ok(thesaurus_data) => {
                if thesaurus_data.is_empty() {
                    let error_content = Content::text(format!(
                        "No thesaurus data available for role '{}'. Please ensure the role has a properly configured and loaded knowledge graph.",
                        role_name
                    ));
                    return Ok(CallToolResult::error(vec![error_content]));
                }

                let replacement_service = terraphim_hooks::ReplacementService::new(thesaurus_data)
                    .with_link_type(link_type_enum);

                match replacement_service.replace(&text) {
                    Ok(hook_result) => {
                        let mut contents = Vec::new();
                        contents.push(Content::text(format!(
                            "Replaced {} term(s) for role '{}' using {} format",
                            hook_result.replacements, role_name, link_type
                        )));
                        contents.push(Content::text(hook_result.result));
                        Ok(CallToolResult::success(contents))
                    }
                    Err(e) => {
                        error!("Replace matches failed: {}", e);
                        let error_content = Content::text(format!("Replace matches failed: {}", e));
                        Ok(CallToolResult::error(vec![error_content]))
                    }
                }
            }
            Err(e) => {
                error!("Failed to load thesaurus for role '{}': {}", role_name, e);
                let error_content = Content::text(format!(
                    "Failed to load thesaurus for role '{}': {}. Please ensure the role has a valid knowledge graph configuration.",
                    role_name, e
                ));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Extract paragraphs containing matched terms from text
    pub async fn extract_paragraphs_from_automata(
        &self,
        text: String,
        role: Option<String>,
        include_term: Option<bool>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut service = self
            .terraphim_service()
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

        // Determine which role to use (provided role or selected role)
        let role_name = if let Some(role_str) = role {
            RoleName::from(role_str)
        } else {
            self.config_state.get_selected_role().await
        };

        // Load thesaurus for the role
        match service.ensure_thesaurus_loaded(&role_name).await {
            Ok(thesaurus_data) => {
                if thesaurus_data.is_empty() {
                    let error_content = Content::text(format!(
                        "No thesaurus data available for role '{}'. Please ensure the role has a properly configured and loaded knowledge graph.",
                        role_name
                    ));
                    return Ok(CallToolResult::error(vec![error_content]));
                }

                let include_term_bool = include_term.unwrap_or(true);

                match extract_paragraphs_from_automata(&text, &thesaurus_data, include_term_bool) {
                    Ok(paragraphs) => {
                        let mut contents = Vec::new();
                        let summary = format!(
                            "Extracted {} paragraphs containing matched terms for role '{}'",
                            paragraphs.len(),
                            role_name
                        );
                        contents.push(Content::text(summary));

                        for (idx, (matched, paragraph)) in paragraphs.iter().enumerate() {
                            let match_info = format!("Match {}: {}", idx + 1, matched.term);
                            contents.push(Content::text(match_info));
                            contents.push(Content::text(format!("Paragraph: {}", paragraph)));
                            contents.push(Content::text("---".to_string()));
                        }

                        Ok(CallToolResult::success(contents))
                    }
                    Err(e) => {
                        error!("Extract paragraphs failed: {}", e);
                        let error_content =
                            Content::text(format!("Extract paragraphs failed: {}", e));
                        Ok(CallToolResult::error(vec![error_content]))
                    }
                }
            }
            Err(e) => {
                error!("Failed to load thesaurus for role '{}': {}", role_name, e);
                let error_content = Content::text(format!(
                    "Failed to load thesaurus for role '{}': {}. Please ensure the role has a valid knowledge graph configuration.",
                    role_name, e
                ));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Parse Logseq JSON output using terraphim_automata
    pub async fn json_decode(&self, jsonlines: String) -> Result<CallToolResult, ErrorData> {
        match json_decode(&jsonlines) {
            Ok(messages) => {
                let mut contents = Vec::new();
                let summary = format!("Successfully parsed {} Logseq messages", messages.len());
                contents.push(Content::text(summary));

                for (idx, message) in messages.iter().enumerate() {
                    let message_info = format!("Message {}: {:?}", idx + 1, message);
                    contents.push(Content::text(message_info));
                }

                Ok(CallToolResult::success(contents))
            }
            Err(e) => {
                error!("JSON decode failed: {}", e);
                let error_content = Content::text(format!("JSON decode failed: {}", e));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Load thesaurus from automata path (local file or remote URL)
    pub async fn load_thesaurus(&self, automata_path: String) -> Result<CallToolResult, ErrorData> {
        // Parse the automata path
        let path = if automata_path.starts_with("http://") || automata_path.starts_with("https://")
        {
            terraphim_automata::AutomataPath::from_remote(&automata_path)
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?
        } else {
            terraphim_automata::AutomataPath::from_local(&automata_path)
        };

        match terraphim_automata::load_thesaurus(&path).await {
            Ok(thesaurus) => {
                let mut contents = Vec::new();
                let summary = format!(
                    "Successfully loaded thesaurus from '{}' with {} terms",
                    automata_path,
                    thesaurus.len()
                );
                contents.push(Content::text(summary));

                // Show first few terms as preview
                let preview_terms: Vec<_> = thesaurus.keys().take(10).collect();
                if !preview_terms.is_empty() {
                    contents.push(Content::text("Preview of terms:".to_string()));
                    for term in preview_terms {
                        let normalized = thesaurus.get(term).unwrap();
                        let term_info = format!("• {} -> {}", term, normalized.value);
                        contents.push(Content::text(term_info));
                    }
                    if thesaurus.len() > 10 {
                        contents.push(Content::text(format!(
                            "... and {} more terms",
                            thesaurus.len() - 10
                        )));
                    }
                }

                Ok(CallToolResult::success(contents))
            }
            Err(e) => {
                error!("Load thesaurus failed: {}", e);
                let error_content = Content::text(format!("Load thesaurus failed: {}", e));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Load thesaurus from JSON string
    pub async fn load_thesaurus_from_json(
        &self,
        json_str: String,
    ) -> Result<CallToolResult, ErrorData> {
        match terraphim_automata::load_thesaurus_from_json(&json_str) {
            Ok(thesaurus) => {
                let mut contents = Vec::new();
                let summary = format!(
                    "Successfully loaded thesaurus from JSON with {} terms",
                    thesaurus.len()
                );
                contents.push(Content::text(summary));

                // Show first few terms as preview
                let preview_terms: Vec<_> = thesaurus.keys().take(10).collect();
                if !preview_terms.is_empty() {
                    contents.push(Content::text("Preview of terms:".to_string()));
                    for term in preview_terms {
                        let normalized = thesaurus.get(term).unwrap();
                        let term_info = format!("• {} -> {}", term, normalized.value);
                        contents.push(Content::text(term_info));
                    }
                    if thesaurus.len() > 10 {
                        contents.push(Content::text(format!(
                            "... and {} more terms",
                            thesaurus.len() - 10
                        )));
                    }
                }

                Ok(CallToolResult::success(contents))
            }
            Err(e) => {
                error!("Load thesaurus from JSON failed: {}", e);
                let error_content =
                    Content::text(format!("Load thesaurus from JSON failed: {}", e));
                Ok(CallToolResult::error(vec![error_content]))
            }
        }
    }

    /// Check if all matched terms in text can be connected by a single path in the knowledge graph
    pub async fn is_all_terms_connected_by_path(
        &self,
        text: String,
        role: Option<String>,
    ) -> Result<CallToolResult, ErrorData> {
        // Determine which role to use (provided role or selected role)
        let role_name = if let Some(role_str) = role {
            RoleName::from(role_str)
        } else {
            self.config_state.get_selected_role().await
        };

        // Check if the role exists and has proper knowledge graph configuration
        let role_config = self.config_state.get_role(&role_name).await;
        if let Some(role_cfg) = role_config {
            // Check if role uses TerraphimGraph relevance function (required for knowledge graph)
            if role_cfg.relevance_function != terraphim_types::RelevanceFunction::TerraphimGraph {
                let error_content = Content::text(format!(
                    "Role '{}' does not use knowledge graph ranking (TerraphimGraph). Graph connectivity check is only available for roles with knowledge graph-based ranking. Current relevance function: {:?}",
                    role_name, role_cfg.relevance_function
                ));
                return Ok(CallToolResult::error(vec![error_content]));
            }

            // Check if role has knowledge graph configuration
            let kg_is_properly_configured = role_cfg
                .kg
                .as_ref()
                .map(|kg| kg.automata_path.is_some() || kg.knowledge_graph_local.is_some())
                .unwrap_or(false);

            if !kg_is_properly_configured {
                let error_content = Content::text(format!(
                    "Role '{}' does not have a properly configured knowledge graph. Graph connectivity check requires a role with defined automata_path or local knowledge graph.",
                    role_name
                ));
                return Ok(CallToolResult::error(vec![error_content]));
            }
        } else {
            let error_content = Content::text(format!(
                "Role '{}' not found in configuration. Available roles: {:?}",
                role_name,
                self.config_state.roles.keys().collect::<Vec<_>>()
            ));
            return Ok(CallToolResult::error(vec![error_content]));
        }

        // Get the RoleGraphSync from config_state.roles
        let rolegraph_sync = match self.config_state.roles.get(&role_name) {
            Some(rg) => rg,
            None => {
                let error_content = Content::text(format!(
                    "RoleGraph not loaded for role '{}'. The role may not have been initialized with a knowledge graph. Available loaded roles: {:?}",
                    role_name,
                    self.config_state.roles.keys().collect::<Vec<_>>()
                ));
                return Ok(CallToolResult::error(vec![error_content]));
            }
        };

        // Lock the RoleGraph and check connectivity
        let rolegraph = rolegraph_sync.lock().await;

        // First, find matched terms for reporting
        let matched_terms = rolegraph.find_matching_node_ids(&text);

        if matched_terms.is_empty() {
            let content = Content::text(format!(
                "No terms from role '{}' knowledge graph found in the provided text. Cannot check graph connectivity.",
                role_name
            ));
            return Ok(CallToolResult::success(vec![content]));
        }

        // Check actual graph connectivity using the real implementation
        let is_connected = rolegraph.is_all_terms_connected_by_path(&text);

        // Build response with detailed information
        let mut contents = Vec::new();

        // Get term names for the matched node IDs
        let term_names: Vec<String> = matched_terms
            .iter()
            .filter_map(|node_id| {
                rolegraph
                    .ac_reverse_nterm
                    .get(node_id)
                    .map(|nterm| nterm.to_string())
            })
            .collect();

        contents.push(Content::text(format!(
            "Graph Connectivity Result for role '{}':\n\
             - Connected: {}\n\
             - Matched terms count: {}\n\
             - Matched terms: {:?}",
            role_name,
            is_connected,
            matched_terms.len(),
            term_names
        )));

        if is_connected {
            contents.push(Content::text(
                "All matched terms are connected by a single path in the knowledge graph, indicating semantic coherence."
            ));
        } else {
            contents.push(Content::text(
                "The matched terms are NOT all connected by a single path. This may indicate:\n\
                 - The text spans multiple unrelated concepts\n\
                 - Some terms are isolated in the knowledge graph\n\
                 - The knowledge graph may need additional edges",
            ));
        }

        Ok(CallToolResult::success(contents))
    }

    /// Find files in a directory using fff-search fuzzy matching with optional
    /// KG-scored path boosting.
    ///
    /// - `query`: fuzzy search string
    /// - `path`: directory to search (defaults to current directory)
    /// - `limit`: maximum number of results (defaults to 20)
    pub async fn find_files(
        &self,
        query: String,
        path: Option<String>,
        limit: Option<usize>,
    ) -> Result<CallToolResult, ErrorData> {
        let base_path = path.unwrap_or_else(|| ".".to_string());

        // Refuse the tree before its base path reaches fff-search: a non-UTF-8
        // or over-long entry name defeats the library's `from_utf8_unchecked`
        // on the stored relative path. UF-040 / #296.
        reject_hostile_tree(std::path::Path::new(&base_path))
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let max_results = limit.unwrap_or(20);

        // Build index synchronously (no background threads needed for MCP tool use)
        let mut picker = FilePicker::new(FilePickerOptions {
            base_path: base_path.clone(),
            mode: FFFMode::Ai,
            watch: false,
            // See `no_mmap_budget` -- UF-039 / #293.
            cache_budget: Some(no_mmap_budget()),
            // Asserted rather than inherited from `FilePickerOptions::default()`
            // so that reasoning about which entries fff-search will index rests
            // on a value this crate states, not on an upstream default that a
            // minor release could change. Refs #293.
            follow_symlinks: false,
            ..FilePickerOptions::default()
        })
        .map_err(|e| ErrorData::internal_error(format!("FilePicker init failed: {e}"), None))?;

        picker
            .collect_files()
            .map_err(|e| ErrorData::internal_error(format!("File scan failed: {e}"), None))?;

        let files = picker.get_files();
        if files.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No files found under '{base_path}'"
            ))]));
        }

        let parser = QueryParser::default();
        let fff_query = parser.parse(&query);

        let result = picker.fuzzy_search(
            &fff_query,
            None,
            FuzzySearchOptions {
                max_threads: 0,
                pagination: PaginationArgs {
                    offset: 0,
                    limit: max_results * 4, // over-fetch so KG re-ranking has material to work with
                },
                ..Default::default()
            },
        );

        // Apply KG boost: add path concept score to the fuzzy score, re-sort.
        let mut scored: Vec<(i32, String)> = result
            .items
            .iter()
            .zip(result.scores.iter())
            .map(|(file, score)| {
                let base = score.total;
                let relative_path = file.relative_path(&picker);
                let kg_boost = self
                    .kg_scorer
                    .as_ref()
                    .map(|s| s.score_path(&relative_path))
                    .unwrap_or(0);
                (base + kg_boost, relative_path)
            })
            .collect();
        #[allow(clippy::unnecessary_sort_by)]
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        let mut contents = Vec::new();
        contents.push(Content::text(format!(
            "Found {} files matching '{}' (searched {} files under '{}')",
            result.total_matched.min(max_results),
            query,
            result.total_files,
            base_path
        )));

        for (score, path) in scored.into_iter().take(max_results) {
            contents.push(Content::text(format!("{score:>5}  {path}")));
        }

        Ok(CallToolResult::success(contents))
    }

    /// Grep file contents with KG-aware file ordering and optional cursor pagination.
    ///
    /// Files are sorted by KG path score before searching so that conceptually
    /// relevant files appear first in paginated results.
    pub async fn grep_files(
        &self,
        query: String,
        path: Option<String>,
        limit: Option<usize>,
        output_mode: Option<String>,
        cursor: Option<String>,
    ) -> Result<CallToolResult, ErrorData> {
        let base_path = path.unwrap_or_else(|| ".".to_string());

        // Refuse the tree before its base path reaches fff-search: a non-UTF-8
        // or over-long entry name defeats the library's `from_utf8_unchecked`
        // on the stored relative path. UF-040 / #296.
        reject_hostile_tree(std::path::Path::new(&base_path))
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let max_results = limit.unwrap_or(50);
        let files_only = output_mode.as_deref() == Some("files");

        let file_offset = cursor
            .as_deref()
            .and_then(|c| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(c)
                    .ok()
            })
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let mut picker = FilePicker::new(FilePickerOptions {
            base_path: base_path.clone(),
            mode: FFFMode::Ai,
            watch: false,
            // See `no_mmap_budget` -- UF-039 / #293.
            cache_budget: Some(no_mmap_budget()),
            // Asserted rather than inherited from `FilePickerOptions::default()`
            // so that reasoning about which entries fff-search will index rests
            // on a value this crate states, not on an upstream default that a
            // minor release could change. Refs #293.
            follow_symlinks: false,
            ..FilePickerOptions::default()
        })
        .map_err(|e| ErrorData::internal_error(format!("FilePicker init failed: {e}"), None))?;

        picker
            .collect_files()
            .map_err(|e| ErrorData::internal_error(format!("File scan failed: {e}"), None))?;

        if picker.get_files().is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No files found under '{base_path}'"
            ))]));
        }

        let fff_query = parse_grep_query(&query);
        let options = GrepSearchOptions {
            max_file_size: MAX_GREP_FILE_SIZE,
            max_matches_per_file: 200,
            smart_case: true,
            file_offset,
            page_limit: max_results,
            mode: GrepMode::PlainText,
            time_budget_ms: 0,
            before_context: 0,
            after_context: 0,
            classify_definitions: false,
            ..GrepSearchOptions::default()
        };

        let result = picker.grep(&fff_query, &options);

        let mut contents = Vec::new();
        let header = format!(
            "Found {} matches across {} files (searched {} files under '{}')",
            result.matches.len(),
            result.files_with_matches,
            result.total_files_searched,
            base_path
        );
        contents.push(Content::text(header));

        if files_only {
            let mut seen = std::collections::HashSet::new();
            for m in &result.matches {
                if let Some(file) = result.files.get(m.file_index) {
                    let relative_path = file.relative_path(&picker);
                    if seen.insert(relative_path.clone()) {
                        contents.push(Content::text(relative_path));
                    }
                }
            }
        } else {
            for m in result.matches.iter().take(max_results) {
                if let Some(file) = result.files.get(m.file_index) {
                    let relative_path = file.relative_path(&picker);
                    let line = format!(
                        "{}:{}:{}",
                        relative_path,
                        m.line_number,
                        m.line_content.trim_end()
                    );
                    contents.push(Content::text(line));
                }
            }
        }

        if result.next_file_offset > 0 {
            let token = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(result.next_file_offset.to_string());
            contents.push(Content::text(format!("next_cursor: {token}")));
        }

        Ok(CallToolResult::success(contents))
    }

    /// Multi-pattern grep: search file contents for lines matching ANY of
    /// multiple patterns (OR logic) using Aho-Corasick. Files are ordered by
    /// KG path score. Supports cursor-based pagination.
    pub async fn multi_grep_files(
        &self,
        patterns: Vec<String>,
        path: Option<String>,
        constraints: Option<String>,
        limit: Option<usize>,
        cursor: Option<String>,
        output_mode: Option<String>,
    ) -> Result<CallToolResult, ErrorData> {
        let base_path = path.unwrap_or_else(|| ".".to_string());

        // Refuse the tree before its base path reaches fff-search: a non-UTF-8
        // or over-long entry name defeats the library's `from_utf8_unchecked`
        // on the stored relative path. UF-040 / #296.
        reject_hostile_tree(std::path::Path::new(&base_path))
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let max_results = limit.unwrap_or(50);
        let files_only = output_mode.as_deref() == Some("files");

        if patterns.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "At least one pattern is required".to_string(),
            )]));
        }

        let file_offset = cursor
            .as_deref()
            .and_then(|c| {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(c)
                    .ok()
            })
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);

        let mut picker = FilePicker::new(FilePickerOptions {
            base_path: base_path.clone(),
            mode: FFFMode::Ai,
            watch: false,
            // See `no_mmap_budget` -- UF-039 / #293.
            cache_budget: Some(no_mmap_budget()),
            // Asserted rather than inherited from `FilePickerOptions::default()`
            // so that reasoning about which entries fff-search will index rests
            // on a value this crate states, not on an upstream default that a
            // minor release could change. Refs #293.
            follow_symlinks: false,
            ..FilePickerOptions::default()
        })
        .map_err(|e| ErrorData::internal_error(format!("FilePicker init failed: {e}"), None))?;

        picker
            .collect_files()
            .map_err(|e| ErrorData::internal_error(format!("File scan failed: {e}"), None))?;

        if picker.get_files().is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(format!(
                "No files found under '{base_path}'"
            ))]));
        }

        let patterns_refs: Vec<&str> = patterns.iter().map(|s| s.as_str()).collect();

        let parser = QueryParser::new(fff_search::AiGrepConfig);
        let constraint_str = constraints.as_deref().unwrap_or("");
        let parsed = parser.parse(constraint_str);
        let parsed_constraints = parsed.constraints.as_slice();

        let options = GrepSearchOptions {
            max_file_size: MAX_GREP_FILE_SIZE,
            max_matches_per_file: 200,
            smart_case: true,
            file_offset,
            page_limit: max_results,
            mode: GrepMode::PlainText,
            time_budget_ms: 0,
            before_context: 0,
            after_context: 0,
            classify_definitions: false,
            ..GrepSearchOptions::default()
        };

        let result = picker.multi_grep(&patterns_refs, parsed_constraints, &options);

        let mut contents = Vec::new();
        let header = format!(
            "Found {} matches across {} files for patterns {:?} (searched {} files under '{}')",
            result.matches.len(),
            result.files_with_matches,
            patterns,
            result.total_files_searched,
            base_path
        );
        contents.push(Content::text(header));

        if files_only {
            let mut seen = std::collections::HashSet::new();
            for m in &result.matches {
                if let Some(file) = result.files.get(m.file_index) {
                    let relative_path = file.relative_path(&picker);
                    if seen.insert(relative_path.clone()) {
                        contents.push(Content::text(relative_path));
                    }
                }
            }
        } else {
            for m in result.matches.iter().take(max_results) {
                if let Some(file) = result.files.get(m.file_index) {
                    let relative_path = file.relative_path(&picker);
                    let line = format!(
                        "{}:{}:{}",
                        relative_path,
                        m.line_number,
                        m.line_content.trim_end()
                    );
                    contents.push(Content::text(line));
                }
            }
        }

        if result.next_file_offset > 0 {
            let token = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(result.next_file_offset.to_string());
            contents.push(Content::text(format!("next_cursor: {token}")));
        }

        Ok(CallToolResult::success(contents))
    }

    /// Spawn the terraphim_rlm CLI and return the result.
    async fn spawn_rlm_cli(
        &self,
        command: &str,
        arguments: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Result<CallToolResult, ErrorData> {
        let args = arguments.unwrap_or_default();

        // Extract session_id or use "auto"
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();

        // Build the JSON args for stdin (excluding session_id)
        let mut stdin_args = args.clone();
        stdin_args.remove("session_id");
        let stdin_json = serde_json::to_string(&stdin_args).map_err(|e| {
            ErrorData::internal_error(format!("Failed to serialize args: {}", e), None)
        })?;

        // Find the terraphim_rlm binary
        let binary_path = find_terraphim_rlm_binary();

        tracing::info!(
            "Spawning terraphim_rlm {} --session-id {}",
            command,
            session_id
        );

        let mut child = tokio::process::Command::new(&binary_path)
            .arg(command)
            .arg("--session-id")
            .arg(&session_id)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                ErrorData::internal_error(format!("Failed to spawn terraphim_rlm: {}", e), None)
            })?;

        // Write JSON args to stdin
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(stdin_json.as_bytes()).await.map_err(|e| {
                ErrorData::internal_error(
                    format!("Failed to write to terraphim_rlm stdin: {}", e),
                    None,
                )
            })?;
            stdin.shutdown().await.map_err(|e| {
                ErrorData::internal_error(
                    format!("Failed to close terraphim_rlm stdin: {}", e),
                    None,
                )
            })?;
        }

        let output = child.wait_with_output().await.map_err(|e| {
            ErrorData::internal_error(format!("terraphim_rlm process failed: {}", e), None)
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            tracing::error!(
                "terraphim_rlm exited with code {:?}: stderr={}",
                output.status.code(),
                stderr
            );
            return Ok(CallToolResult::error(vec![Content::text(format!(
                "RLM execution failed (exit code {:?}):\nstdout: {}\nstderr: {}",
                output.status.code(),
                stdout,
                stderr
            ))]));
        }

        // Parse the JSON response
        match serde_json::from_str::<serde_json::Value>(&stdout) {
            Ok(response) => {
                if response
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    let data = response
                        .get("data")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string_pretty(&data).unwrap_or_else(|_| data.to_string()),
                    )]))
                } else {
                    let error = response
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("Unknown RLM error");
                    Ok(CallToolResult::error(vec![Content::text(
                        error.to_string(),
                    )]))
                }
            }
            Err(e) => {
                tracing::error!(
                    "Failed to parse terraphim_rlm output: {}. stdout: {}",
                    e,
                    stdout
                );
                Ok(CallToolResult::error(vec![Content::text(format!(
                    "Failed to parse RLM output: {}\nRaw output: {}",
                    e, stdout
                ))]))
            }
        }
    }
}

impl ServerHandler for McpService {
    async fn initialize(
        &self,
        request: rmcp::model::InitializeRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::InitializeResult, ErrorData> {
        if context.peer.peer_info().is_none() {
            context.peer.set_peer_info(request);
        }
        Ok(self.get_info())
    }

    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        tracing::debug!("list_tools function called!");

        // Convert JSON values to Arc<Map<String, Value>> for input_schema
        let search_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "role": {
                    "type": "string",
                    "description": "Optional role to filter by"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of deduplicated results to return",
                    "minimum": 0,
                    "maximum": 2147483647
                },
                "skip": {
                    "type": "integer",
                    "description": "Number of deduplicated results to skip",
                    "minimum": 0,
                    "maximum": 2147483647
                }
            },
            "required": ["query"]
        });
        let search_map = search_schema.as_object().unwrap().clone();

        let config_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "config_str": {
                    "type": "string",
                    "description": "JSON configuration string"
                }
            },
            "required": ["config_str"]
        });
        let config_map = config_schema.as_object().unwrap().clone();

        let build_autocomplete_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "role": {
                    "type": "string",
                    "description": "Optional role name to build autocomplete index for. If not provided, uses the currently selected role."
                }
            },
            "required": []
        });
        let build_autocomplete_map = build_autocomplete_schema.as_object().unwrap().clone();

        let autocomplete_terms_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Prefix or term for suggestions" },
                "limit": { "type": "integer", "description": "Max suggestions (default 10)" },
                "role": { "type": "string", "description": "Optional role name to use for autocomplete. If not provided, uses the currently selected role." }
            },
            "required": ["query"]
        });
        let autocomplete_terms_map = autocomplete_terms_schema.as_object().unwrap().clone();

        let autocomplete_snippets_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Prefix or term for suggestions with snippets" },
                "limit": { "type": "integer", "description": "Max suggestions (default 10)" },
                "role": { "type": "string", "description": "Optional role name to use for autocomplete. If not provided, uses the currently selected role." }
            },
            "required": ["query"]
        });
        let autocomplete_snippets_map = autocomplete_snippets_schema.as_object().unwrap().clone();

        let fuzzy_autocomplete_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The text to get autocomplete suggestions for"
                },
                "similarity": {
                    "type": "number",
                    "description": "Minimum Jaro-Winkler similarity threshold (0.0-1.0, default: 0.6)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of suggestions to return (default: 10)"
                }
            },
            "required": ["query"]
        });
        let fuzzy_autocomplete_map = fuzzy_autocomplete_schema.as_object().unwrap().clone();

        let levenshtein_autocomplete_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The text to get autocomplete suggestions for"
                },
                "max_edit_distance": {
                    "type": "integer",
                    "description": "Maximum Levenshtein edit distance allowed (default: 2)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of suggestions to return (default: 10)"
                }
            },
            "required": ["query"]
        });
        let levenshtein_autocomplete_map =
            levenshtein_autocomplete_schema.as_object().unwrap().clone();

        let find_matches_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to search in" },
                "role": { "type": "string", "description": "Optional role to filter by" },
                "return_positions": { "type": "boolean", "description": "Whether to return positions (default: false)" }
            },
            "required": ["text"]
        });
        let find_matches_map = find_matches_schema.as_object().unwrap().clone();

        let replace_matches_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to replace terms in" },
                "role": { "type": "string", "description": "Optional role to filter by" },
                "link_type": { "type": "string", "description": "The type of link to use (wiki, html, markdown)" }
            },
            "required": ["text", "link_type"]
        });
        let replace_matches_map = replace_matches_schema.as_object().unwrap().clone();

        let extract_paragraphs_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to extract paragraphs from" },
                "role": { "type": "string", "description": "Optional role to filter by" },
                "include_term": { "type": "boolean", "description": "Whether to include the matched term in the paragraph (default: true)" }
            },
            "required": ["text"]
        });
        let extract_paragraphs_map = extract_paragraphs_schema.as_object().unwrap().clone();

        let json_decode_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "jsonlines": { "type": "string", "description": "The JSON lines string to decode" }
            },
            "required": ["jsonlines"]
        });
        let json_decode_map = json_decode_schema.as_object().unwrap().clone();

        let load_thesaurus_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "automata_path": { "type": "string", "description": "The path to the automata file (local or remote URL)" }
            },
            "required": ["automata_path"]
        });
        let load_thesaurus_map = load_thesaurus_schema.as_object().unwrap().clone();

        let load_thesaurus_json_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "json_str": { "type": "string", "description": "The JSON string to load thesaurus from" }
            },
            "required": ["json_str"]
        });
        let load_thesaurus_json_map = load_thesaurus_json_schema.as_object().unwrap().clone();

        let is_all_terms_connected_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string", "description": "The text to check for term connectivity" },
                "role": { "type": "string", "description": "Optional role to use for thesaurus and graph" }
            },
            "required": ["text"]
        });
        let is_all_terms_connected_map = is_all_terms_connected_schema.as_object().unwrap().clone();

        let mut tools = vec![
            Tool {
                name: "search".into(),
                title: Some("Search Knowledge Graph".into()),
                description: Some("Search documents in the selected role. Exact authored KG concepts expand to their synonyms; compound queries retain their existing semantics. Pagination applies after deduplication.".into()),
                input_schema: Arc::new(search_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "update_config_tool".into(),
                title: Some("Update Configuration".into()),
                description: Some("Update the Terraphim configuration".into()),
                input_schema: Arc::new(config_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "build_autocomplete_index".into(),
                title: Some("Build Autocomplete Index".into()),
                description: Some("Build FST-based autocomplete index from role's knowledge graph. Only available for roles with TerraphimGraph relevance function and configured knowledge graph.".into()),
                input_schema: Arc::new(build_autocomplete_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "fuzzy_autocomplete_search".into(),
                title: Some("Fuzzy Autocomplete Search".into()),
                description: Some("Perform fuzzy autocomplete search using Jaro-Winkler similarity (default, faster and higher quality)".into()),
                input_schema: Arc::new(fuzzy_autocomplete_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "autocomplete_terms".into(),
                title: Some("Autocomplete Terms".into()),
                description: Some("Autocomplete terms using FST prefix + fuzzy fallback".into()),
                input_schema: Arc::new(autocomplete_terms_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "autocomplete_with_snippets".into(),
                title: Some("Autocomplete With Snippets".into()),
                description: Some("Autocomplete and return short snippets from matching documents".into()),
                input_schema: Arc::new(autocomplete_snippets_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "fuzzy_autocomplete_search_levenshtein".into(),
                title: Some("Fuzzy Search (Levenshtein)".into()),
                description: Some("Perform fuzzy autocomplete search using Levenshtein distance (baseline comparison algorithm)".into()),
                input_schema: Arc::new(levenshtein_autocomplete_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "fuzzy_autocomplete_search_jaro_winkler".into(),
                title: Some("Fuzzy Search (Jaro-Winkler)".into()),
                description: Some("Perform fuzzy autocomplete search using Jaro-Winkler similarity (explicit)".into()),
                input_schema: Arc::new(fuzzy_autocomplete_schema.as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "serialize_autocomplete_index".into(),
                title: Some("Serialize Index".into()),
                description: Some("Serialize the current autocomplete index to a base64-encoded string for storage/transmission".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "deserialize_autocomplete_index".into(),
                title: Some("Deserialize Index".into()),
                description: Some("Deserialize an autocomplete index from a base64-encoded string".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "base64_data": { "type": "string", "description": "The base64-encoded string of the serialized index" }
                    },
                    "required": ["base64_data"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "find_matches".into(),
                title: Some("Find Matches".into()),
                description: Some("Find all term matches in text using Aho-Corasick algorithm".into()),
                input_schema: Arc::new(find_matches_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "replace_matches".into(),
                title: Some("Replace Matches".into()),
                description: Some("Replace matched terms in text with links using specified format".into()),
                input_schema: Arc::new(replace_matches_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "extract_paragraphs_from_automata".into(),
                title: Some("Extract Paragraphs".into()),
                description: Some("Extract paragraphs containing matched terms from text".into()),
                input_schema: Arc::new(extract_paragraphs_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "json_decode".into(),
                title: Some("JSON Decode".into()),
                description: Some("Parse Logseq JSON output using terraphim_automata".into()),
                input_schema: Arc::new(json_decode_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "load_thesaurus".into(),
                title: Some("Load Thesaurus".into()),
                description: Some("Load thesaurus from a local file or remote URL".into()),
                input_schema: Arc::new(load_thesaurus_map.clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "load_thesaurus_from_json".into(),
                title: Some("Load Thesaurus from JSON".into()),
                description: Some("Load thesaurus from a JSON string".into()),
                input_schema: Arc::new(load_thesaurus_json_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "is_all_terms_connected_by_path".into(),
                title: Some("Check Terms Connectivity".into()),
                description: Some("Check if all matched terms in text can be connected by a single path in the knowledge graph".into()),
                input_schema: Arc::new(is_all_terms_connected_map),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "terraphim_find_files".into(),
                title: Some("Find Files (KG-boosted)".into()),
                description: Some("Fuzzy file search using fff-search with optional knowledge-graph path scoring. Results are ranked by fuzzy match score plus KG concept boost.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Fuzzy search string"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory to search (defaults to current directory)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of results (default 20)"
                        }
                    },
                    "required": ["query"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "terraphim_grep".into(),
                title: Some("Grep (KG-ordered, paginated)".into()),
                description: Some("Search file contents with ripgrep-style matching. Files are searched in KG path-score order so conceptually relevant files appear first. Supports cursor-based pagination for large result sets.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search pattern (plain text)"
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory to search (defaults to current directory)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of matches to return (default 50)"
                        },
                        "output_mode": {
                            "type": "string",
                            "enum": ["content", "files"],
                            "description": "Return 'content' (file:line:text, default) or 'files' (unique file paths only)"
                        },
                        "cursor": {
                            "type": "string",
                            "description": "Pagination cursor from a previous result's next_cursor field"
                        }
                    },
                    "required": ["query"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "terraphim_multi_grep".into(),
                title: Some("Multi-Grep (OR patterns, KG-ordered, paginated)".into()),
                description: Some("Search file contents for lines matching ANY of multiple patterns (OR logic) using Aho-Corasick. Faster than running separate grep calls. Files are KG path-score ordered. Supports cursor pagination.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "patterns": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Patterns to match (OR logic). Include all naming conventions: snake_case, PascalCase, camelCase."
                        },
                        "path": {
                            "type": "string",
                            "description": "Directory to search (defaults to current directory)"
                        },
                        "constraints": {
                            "type": "string",
                            "description": "File constraints (e.g. '*.rs !test/')"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of matches to return (default 50)"
                        },
                        "output_mode": {
                            "type": "string",
                            "enum": ["content", "files"],
                            "description": "Return 'content' (file:line:text, default) or 'files' (unique file paths only)"
                        },
                        "cursor": {
                            "type": "string",
                            "description": "Pagination cursor from a previous result's next_cursor field"
                        }
                    },
                    "required": ["patterns"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            // RLM tools
            Tool {
                name: "rlm_code".into(),
                title: Some("Execute Python Code".into()),
                description: Some("Execute Python code in an isolated execution environment. Returns stdout, stderr, and exit status.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "code": {
                            "type": "string",
                            "description": "Python code to execute"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Optional session ID (auto-created if not provided)"
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Optional execution timeout in milliseconds"
                        }
                    },
                    "required": ["code"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "rlm_bash".into(),
                title: Some("Execute Bash Command".into()),
                description: Some("Execute a bash command in an isolated execution environment.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "Bash command to execute"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Optional session ID (auto-created if not provided)"
                        },
                        "timeout_ms": {
                            "type": "integer",
                            "description": "Optional execution timeout in milliseconds"
                        }
                    },
                    "required": ["command"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "rlm_query".into(),
                title: Some("Query LLM".into()),
                description: Some("Query the LLM from within an RLM session.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "prompt": {
                            "type": "string",
                            "description": "The prompt/query to send to the LLM"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Optional session ID (auto-created if not provided)"
                        }
                    },
                    "required": ["prompt"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "rlm_context".into(),
                title: Some("Manage Context Variables".into()),
                description: Some("Manage context variables within an RLM session.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["get", "set", "list", "delete"],
                            "description": "The action to perform"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Optional session ID (auto-created if not provided)"
                        },
                        "key": {
                            "type": "string",
                            "description": "Variable key"
                        },
                        "value": {
                            "type": "string",
                            "description": "Variable value (for set)"
                        }
                    },
                    "required": ["action"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "rlm_snapshot".into(),
                title: Some("Manage VM Snapshots".into()),
                description: Some("Manage execution environment snapshots for rollback support.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["create", "restore", "list", "delete"],
                            "description": "The snapshot action"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Optional session ID (auto-created if not provided)"
                        },
                        "snapshot_name": {
                            "type": "string",
                            "description": "Name for the snapshot"
                        }
                    },
                    "required": ["action"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            },
            Tool {
                name: "rlm_status".into(),
                title: Some("Get Session Status".into()),
                description: Some("Get the status of an RLM session.".into()),
                input_schema: Arc::new(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "session_id": {
                            "type": "string",
                            "description": "Optional session ID (auto-created if not provided)"
                        },
                        "include_history": {
                            "type": "boolean",
                            "description": "Whether to include command history"
                        }
                    },
                    "required": []
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
                meta: None,
            }
        ];

        if self.agent_store.is_some() {
            tools.extend(agent_tools::tools());
        }
        tracing::debug!("Created {} tools", tools.len());
        tracing::debug!("First tool name: {:?}", tools.first().map(|t| &t.name));

        let result = ListToolsResult {
            tools,
            next_cursor: None,
        };

        tracing::debug!(
            "Returning ListToolsResult with {} tools",
            result.tools.len()
        );

        Ok(result)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if request.name.starts_with("agent_") {
            let store = self.agent_store.as_ref().ok_or_else(|| ErrorData::invalid_params(
                "Agent tools are disabled; start the local stdio server with an explicit --agent-data-dir", None))?;
            return agent_tools::call_guarded(
                store,
                &_context.ct,
                &request.name,
                serde_json::Value::Object(request.arguments.unwrap_or_default()),
            )
            .await;
        }
        match request.name.as_ref() {
            "search" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();

                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let parse_page = |name: &str| -> Result<Option<i32>, ErrorData> {
                    arguments
                        .get(name)
                        .map(|value| {
                            value
                                .as_i64()
                                .and_then(|number| i32::try_from(number).ok())
                                .filter(|number| *number >= 0)
                                .ok_or_else(|| {
                                    ErrorData::invalid_params(
                                        format!(
                                            "'{name}' must be an integer from 0 to {}",
                                            i32::MAX
                                        ),
                                        None,
                                    )
                                })
                        })
                        .transpose()
                };
                let limit = parse_page("limit")?;
                let skip = parse_page("skip")?;

                self.search(query, role, limit, skip)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "update_config_tool" => {
                let arguments = request.arguments.unwrap_or_default();
                let config_str = arguments
                    .get("config_str")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Missing 'config_str' parameter".to_string(),
                            None,
                        )
                    })?
                    .to_string();

                self.update_config_tool(config_str)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "build_autocomplete_index" => {
                let arguments = request.arguments.unwrap_or_default();
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                self.build_autocomplete_index(role)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "fuzzy_autocomplete_search" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();

                let similarity = arguments.get("similarity").and_then(|v| v.as_f64());

                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);

                self.fuzzy_autocomplete_search(query, similarity, limit)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "autocomplete_terms" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                self.autocomplete_terms(query, limit, role)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "autocomplete_with_snippets" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                self.autocomplete_with_snippets(query, limit, role)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "fuzzy_autocomplete_search_levenshtein" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();

                let max_edit_distance = arguments
                    .get("max_edit_distance")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);

                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);

                self.fuzzy_autocomplete_search_levenshtein(query, max_edit_distance, limit)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "fuzzy_autocomplete_search_jaro_winkler" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();

                let similarity = arguments.get("similarity").and_then(|v| v.as_f64());

                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);

                self.fuzzy_autocomplete_search_jaro_winkler(query, similarity, limit)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "serialize_autocomplete_index" => self
                .serialize_autocomplete_index()
                .await
                .map_err(TerraphimMcpError::Mcp)
                .map_err(ErrorData::from),
            "deserialize_autocomplete_index" => {
                let arguments = request.arguments.unwrap_or_default();
                let base64_data = arguments
                    .get("base64_data")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Missing 'base64_data' parameter".to_string(),
                            None,
                        )
                    })?
                    .to_string();

                self.deserialize_autocomplete_index(base64_data)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "find_matches" => {
                let arguments = request.arguments.unwrap_or_default();
                let text = arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'text' parameter".to_string(), None)
                    })?
                    .to_string();
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let return_positions = arguments
                    .get("return_positions")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                self.find_matches(text, role, Some(return_positions))
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "replace_matches" => {
                let arguments = request.arguments.unwrap_or_default();
                let text = arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'text' parameter".to_string(), None)
                    })?
                    .to_string();
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let link_type = arguments
                    .get("link_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'link_type' parameter".to_string(), None)
                    })?
                    .to_string();

                self.replace_matches(text, role, link_type)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "extract_paragraphs_from_automata" => {
                let arguments = request.arguments.unwrap_or_default();
                let text = arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'text' parameter".to_string(), None)
                    })?
                    .to_string();
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let include_term = arguments
                    .get("include_term")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);

                self.extract_paragraphs_from_automata(text, role, Some(include_term))
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "json_decode" => {
                let arguments = request.arguments.unwrap_or_default();
                let jsonlines = arguments
                    .get("jsonlines")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'jsonlines' parameter".to_string(), None)
                    })?
                    .to_string();

                self.json_decode(jsonlines)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "load_thesaurus" => {
                let arguments = request.arguments.unwrap_or_default();
                let automata_path = arguments
                    .get("automata_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params(
                            "Missing 'automata_path' parameter".to_string(),
                            None,
                        )
                    })?
                    .to_string();

                self.load_thesaurus(automata_path)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "load_thesaurus_from_json" => {
                let arguments = request.arguments.unwrap_or_default();
                let json_str = arguments
                    .get("json_str")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'json_str' parameter".to_string(), None)
                    })?
                    .to_string();

                self.load_thesaurus_from_json(json_str)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "is_all_terms_connected_by_path" => {
                let arguments = request.arguments.unwrap_or_default();
                let text = arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'text' parameter".to_string(), None)
                    })?
                    .to_string();
                let role = arguments
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                self.is_all_terms_connected_by_path(text, role)
                    .await
                    .map_err(TerraphimMcpError::Mcp)
                    .map_err(ErrorData::from)
            }
            "terraphim_find_files" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);

                self.find_files(query, path, limit).await
            }
            "terraphim_grep" => {
                let arguments = request.arguments.unwrap_or_default();
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ErrorData::invalid_params("Missing 'query' parameter".to_string(), None)
                    })?
                    .to_string();
                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);
                let output_mode = arguments
                    .get("output_mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let cursor = arguments
                    .get("cursor")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                self.grep_files(query, path, limit, output_mode, cursor)
                    .await
            }
            "terraphim_multi_grep" => {
                let arguments = request.arguments.unwrap_or_default();

                let patterns: Vec<String> = arguments
                    .get("patterns")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                if patterns.is_empty() {
                    return Err(ErrorData::invalid_params(
                        "Missing or empty 'patterns' parameter".to_string(),
                        None,
                    ));
                }

                let path = arguments
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let constraints = arguments
                    .get("constraints")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .map(|i| i as usize);
                let cursor = arguments
                    .get("cursor")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let output_mode = arguments
                    .get("output_mode")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                self.multi_grep_files(patterns, path, constraints, limit, cursor, output_mode)
                    .await
            }
            // RLM tools - spawn terraphim_rlm CLI
            "rlm_code" => self.spawn_rlm_cli("code", request.arguments).await,
            "rlm_bash" => self.spawn_rlm_cli("bash", request.arguments).await,
            "rlm_query" => self.spawn_rlm_cli("query", request.arguments).await,
            "rlm_context" => self.spawn_rlm_cli("context", request.arguments).await,
            "rlm_snapshot" => self.spawn_rlm_cli("snapshot", request.arguments).await,
            "rlm_status" => self.spawn_rlm_cli("status", request.arguments).await,
            _ => Err(ErrorData::method_not_found::<
                rmcp::model::CallToolRequestMethod,
            >()),
        }
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut service = self
            .terraphim_service()
            .await
            .map_err(TerraphimMcpError::Anyhow)?;

        // Use a broad search term to find documents instead of empty search
        // We'll try common terms that should match documents in our KG
        let search_terms = vec!["terraphim", "graph", "service", "haystack"];
        let mut all_documents = std::collections::HashSet::new();

        // Perform multiple searches to gather available documents
        for term in search_terms {
            let search_query = terraphim_types::SearchQuery {
                search_term: terraphim_types::NormalizedTermValue::new(term.to_string()),
                search_terms: None,
                operator: None,
                limit: Some(50), // Reasonable limit per search
                skip: None,
                role: None,
                layer: terraphim_types::Layer::default(),
                include_pinned: false,
                min_quality: None,
            };

            match service.search(&search_query).await {
                Ok(documents) => {
                    for doc in documents {
                        all_documents.insert(doc.id.clone());
                    }
                }
                Err(_) => {
                    // Continue with other terms if one fails
                    continue;
                }
            }
        }

        // If we still have no documents, try a final broad search
        if all_documents.is_empty() {
            let fallback_query = terraphim_types::SearchQuery {
                search_term: terraphim_types::NormalizedTermValue::new("*".to_string()),
                search_terms: None,
                operator: None,
                limit: Some(100),
                skip: None,
                role: None,
                layer: terraphim_types::Layer::default(),
                include_pinned: false,
                min_quality: None,
            };

            let documents = service
                .search(&fallback_query)
                .await
                .map_err(TerraphimMcpError::Service)?;

            let resources = self
                .resource_mapper
                .documents_to_resources(&documents)
                .map_err(TerraphimMcpError::Anyhow)?;

            return Ok(ListResourcesResult {
                resources,
                next_cursor: None,
            });
        }

        // Convert unique document IDs back to documents for resource mapping
        // For now, we'll do individual searches to get full document objects
        let mut final_documents = Vec::new();
        for doc_id in all_documents.iter().take(50) {
            // Limit to 50 resources
            if let Ok(Some(doc)) = service.get_document_by_id(doc_id).await {
                final_documents.push(doc);
            }
        }

        let resources = self
            .resource_mapper
            .documents_to_resources(&final_documents)
            .map_err(TerraphimMcpError::Anyhow)?;

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        let doc_id = self
            .resource_mapper
            .uri_to_id(&request.uri)
            .map_err(TerraphimMcpError::Anyhow)?;
        let mut service = self
            .terraphim_service()
            .await
            .map_err(TerraphimMcpError::Anyhow)?;
        let document = service
            .get_document_by_id(&doc_id)
            .await
            .map_err(TerraphimMcpError::Service)?;
        if let Some(doc) = document {
            let contents = self
                .resource_mapper
                .document_to_resource_contents(&doc)
                .map_err(TerraphimMcpError::Anyhow)?;
            Ok(ReadResourceResult {
                contents: vec![contents],
            })
        } else {
            Err(ErrorData::resource_not_found(
                format!("Document not found: {}", doc_id),
                None,
            ))
        }
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            // Declare exactly what is implemented: tools (call_tool/list_tools)
            // and resources (list_resources/read_resource). Clients that gate
            // on capabilities otherwise show no tools.
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
            server_info: rmcp::model::Implementation {
                name: "terraphim-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: Some("Terraphim MCP Server".to_string()),
                icons: None,
                website_url: None,
            },
            instructions: Some("This server provides Terraphim knowledge graph search capabilities through the Model Context Protocol. You can search for documents using the search tool and access resources that represent Terraphim documents.".to_string()),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod synonym_ranking_tests {
    use super::*;
    use terraphim_types::{NormalizedTerm, Thesaurus};

    #[test]
    fn same_document_id_in_different_haystacks_keeps_distinct_graph_ranks() -> Result<()> {
        let mut thesaurus = Thesaurus::new("concepts".into());
        for term in ["risk", "concern"] {
            thesaurus.insert(
                NormalizedTermValue::from(term),
                NormalizedTerm::new(1, NormalizedTermValue::from("risk")),
            );
        }
        let mut graph =
            terraphim_rolegraph::RoleGraph::new_sync(RoleName::from("project"), thesaurus)?;
        let mut documents = vec![
            Document {
                id: "same-id".into(),
                source_haystack: Some("one".into()),
                body: "Risk Concern Risk".into(),
                ..Default::default()
            },
            Document {
                id: "same-id".into(),
                source_haystack: Some("two".into()),
                body: "Risk".into(),
                ..Default::default()
            },
        ];
        rank_expanded_documents(&mut graph, "risk", &mut documents)?;
        assert_eq!(graph.get_document_count(), 2);
        assert!(documents[0].rank > documents[1].rank);
        assert_eq!(documents[1].rank, Some(0));
        assert_eq!(documents[0].id, "same-id");
        assert_eq!(documents[1].id, "same-id");
        Ok(())
    }
}
