//! Shared learning store implementation
//!
//! Provides markdown-backed storage with BM25-based deduplication and
//! trust-gated promotion logic. When a Terraphim `RoleGraph` is configured,
//! suggestion and similarity lookups use the existing Terraphim hybrid
//! scorer (`RoleGraph::query_graph`: weighted mean of node rank + edge rank
//! + document rank with thesaurus term expansion) instead of pure BM25.
// BM25 remains the fallback when the graph returns no matches.

use std::collections::HashMap;

use chrono::Utc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[cfg(feature = "shared-learning")]
use terraphim_types::{Document, DocumentType};

use crate::shared_learning::markdown_store::{
    MarkdownLearningStore, MarkdownStoreConfig, MarkdownStoreError,
};
use crate::shared_learning::types::{SharedLearning, TrustLevel};
pub use terraphim_types::shared_learning::StoreError;
use terraphim_types::shared_learning::SuggestionStatus;

impl From<MarkdownStoreError> for StoreError {
    fn from(e: MarkdownStoreError) -> Self {
        StoreError::Persistence(e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct StoreConfig {
    pub similarity_threshold: f64,
    pub auto_promote_l2: bool,
    pub markdown: MarkdownStoreConfig,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.8,
            auto_promote_l2: true,
            markdown: MarkdownStoreConfig::default(),
        }
    }
}

impl StoreConfig {
    pub fn with_similarity_threshold(mut self, threshold: f64) -> Self {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn with_markdown_config(mut self, config: MarkdownStoreConfig) -> Self {
        self.markdown = config;
        self
    }
}

/// BM25 scoring for text similarity
pub struct Bm25Scorer {
    avg_doc_len: f64,
    total_docs: usize,
    idf_cache: HashMap<String, f64>,
}

impl Bm25Scorer {
    pub fn new(total_docs: usize, avg_doc_len: f64) -> Self {
        Self {
            avg_doc_len,
            total_docs,
            idf_cache: HashMap::new(),
        }
    }

    fn calculate_idf(&mut self, term: &str, doc_freq: usize) -> f64 {
        if let Some(&idf) = self.idf_cache.get(term) {
            return idf;
        }

        let n = doc_freq as f64;
        let n_docs = self.total_docs as f64;

        let idf = if n_docs <= 1.0 || n >= n_docs {
            0.5
        } else {
            ((n_docs - n + 0.5) / (n + 0.5)).ln().max(0.0)
        };

        self.idf_cache.insert(term.to_string(), idf);
        idf
    }

    pub fn score(&mut self, query: &str, doc: &str, doc_freqs: &HashMap<String, usize>) -> f64 {
        const K1: f64 = 1.2;
        const B: f64 = 0.75;

        let query_terms: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let doc_terms: Vec<String> = doc
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        let doc_len = doc_terms.len() as f64;
        let mut score = 0.0;

        let mut doc_tf: HashMap<String, usize> = HashMap::new();
        for term in &doc_terms {
            *doc_tf.entry(term.clone()).or_insert(0) += 1;
        }

        for term in &query_terms {
            let f = *doc_tf.get(term).unwrap_or(&0) as f64;
            let doc_freq = *doc_freqs.get(term).unwrap_or(&1);
            let idf = self.calculate_idf(term, doc_freq);

            let numerator = f * (K1 + 1.0);
            let denominator = f + K1 * (1.0 - B + B * doc_len / self.avg_doc_len);

            score += idf * numerator / denominator;
        }

        score
    }

    pub fn normalize_score(&self, score: f64, query_len: usize) -> f64 {
        if query_len == 0 {
            return 0.0;
        }
        let normalized = (score / query_len as f64).tanh();
        normalized.clamp(0.0, 1.0)
    }
}

/// Build a Terraphim `Document` from a `SharedLearning` for ingestion into
/// the role graph. The body carries the same lowercased, keyword-tagged
/// text used by the BM25 fallback (`extract_searchable_text`), so both
/// scorers index the same surface form.
#[cfg(feature = "shared-learning")]
fn build_document_for_graph(learning: &SharedLearning) -> Document {
    let body = learning.extract_searchable_text();
    let tags = if learning.keywords.is_empty() {
        None
    } else {
        Some(learning.keywords.clone())
    };
    Document {
        id: learning.id.clone(),
        url: String::new(),
        title: learning.title.clone(),
        body,
        description: None,
        summarization: None,
        stub: None,
        tags,
        rank: None,
        source_haystack: Some("shared_learning_store".to_string()),
        doc_type: DocumentType::default(),
        synonyms: None,
        route: None,
        priority: None,
        quality_score: None,
    }
}

pub struct SharedLearningStore {
    backend: MarkdownLearningStore,
    index: RwLock<HashMap<String, SharedLearning>>,
    config: StoreConfig,
    #[cfg(feature = "shared-learning")]
    role_graph: Option<std::sync::RwLock<terraphim_rolegraph::RoleGraph>>,
}

impl SharedLearningStore {
    pub async fn open(config: StoreConfig) -> Result<Self, StoreError> {
        let backend = MarkdownLearningStore::with_config(config.markdown.clone());
        let store = Self {
            backend,
            index: RwLock::new(HashMap::new()),
            config,
            #[cfg(feature = "shared-learning")]
            role_graph: None,
        };
        store.load_all().await?;
        Ok(store)
    }

    async fn load_all(&self) -> Result<(), StoreError> {
        info!("Loading shared learnings from markdown backend");
        let all_learnings = self.backend.list_all_with_origin().await?;
        let discovered = all_learnings.len();

        let mut selected: HashMap<String, (bool, SharedLearning)> = HashMap::new();
        for (is_shared, learning) in all_learnings {
            match selected.get(&learning.id) {
                None => {
                    selected.insert(learning.id.clone(), (is_shared, learning));
                }
                Some((existing_is_shared, _)) => {
                    if *existing_is_shared && !is_shared {
                        selected.insert(learning.id.clone(), (is_shared, learning));
                    }
                }
            }
        }

        let mut index = self.index.write().await;
        for (_, learning) in selected.into_values() {
            index.insert(learning.id.clone(), learning);
        }
        let loaded = index.len();
        drop(index);

        info!(
            "Loaded {} shared learnings into index ({} discovered)",
            loaded, discovered
        );
        Ok(())
    }

    async fn persist(&self, learning: &SharedLearning) -> Result<(), StoreError> {
        self.backend.save(learning).await?;
        Ok(())
    }

    pub async fn insert(&self, learning: SharedLearning) -> Result<(), StoreError> {
        let id = learning.id.clone();
        self.persist(&learning).await?;
        self.index.write().await.insert(id.clone(), learning.clone());
        // Mirror the insert into the role graph so suggestion / similarity
        // can find this learning via Terraphim hybrid scoring immediately.
        // Best-effort: a poisoned write lock on the graph must not fail
        // the store-level insert.
        #[cfg(feature = "shared-learning")]
        self.sync_to_graph(&learning);
        Ok(())
    }

    /// Insert a learning's text into the configured role graph, if any.
    ///
    /// Failures (poisoned lock, missing graph) are swallowed because the
    /// graph is an accelerator on top of the in-memory index, not the
    /// source of truth: subsequent BM25 fallback will still surface the
    /// learning.
    #[cfg(feature = "shared-learning")]
    fn sync_to_graph(&self, learning: &SharedLearning) {
        if let Some(ref graph_lock) = self.role_graph {
            if let Ok(mut graph) = graph_lock.write() {
                let doc = build_document_for_graph(learning);
                graph.insert_document(learning.id.as_str(), doc);
            }
        }
    }

    pub async fn store_with_dedup(
        &self,
        learning: SharedLearning,
    ) -> Result<StoreResult, StoreError> {
        let search_text = learning.extract_searchable_text();
        let query_lower = search_text.to_lowercase();

        let index = self.index.read().await;
        let all_learnings: Vec<SharedLearning> = index.values().cloned().collect();
        drop(index);

        if !all_learnings.is_empty() {
            let mut doc_freqs: HashMap<String, usize> = HashMap::new();
            let mut total_doc_len = 0;

            for doc in &all_learnings {
                let text = doc.extract_searchable_text();
                let terms: std::collections::HashSet<String> = text
                    .to_lowercase()
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect();
                total_doc_len += terms.len();
                for term in &terms {
                    *doc_freqs.entry(term.clone()).or_insert(0) += 1;
                }
            }

            let avg_doc_len = total_doc_len as f64 / all_learnings.len() as f64;
            let mut scorer = Bm25Scorer::new(all_learnings.len(), avg_doc_len);
            let query_len = query_lower.split_whitespace().count();

            let best_match = all_learnings
                .iter()
                .map(|doc| {
                    let doc_text = doc.extract_searchable_text();
                    let raw_score = scorer.score(&query_lower, &doc_text, &doc_freqs);
                    let normalized = scorer.normalize_score(raw_score, query_len);
                    (doc.id.clone(), normalized)
                })
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            if let Some((existing_id, score)) = best_match {
                if score >= self.config.similarity_threshold {
                    debug!(
                        "Merging with existing learning {} (score={:.3})",
                        existing_id, score
                    );
                    self.merge_learning(&existing_id, &learning).await?;
                    return Ok(StoreResult::Merged(existing_id));
                }
            }
        }

        let id = learning.id.clone();
        self.insert(learning).await?;
        info!("Created new learning: {}", id);
        Ok(StoreResult::Created)
    }

    async fn merge_learning(
        &self,
        existing_id: &str,
        new_learning: &SharedLearning,
    ) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let existing = index
            .get_mut(existing_id)
            .ok_or_else(|| StoreError::NotFound(existing_id.to_string()))?;

        existing.quality.applied_count += new_learning.quality.applied_count;
        existing.quality.effective_count += new_learning.quality.effective_count;

        for agent in &new_learning.quality.agent_names {
            if !existing.quality.agent_names.contains(agent) {
                existing.quality.agent_names.push(agent.clone());
            }
        }

        existing.updated_at = Utc::now();
        let merged = existing.clone();
        drop(index);

        self.persist(&merged).await?;
        Ok(())
    }

    /// Record that a graph query touched this learning
    ///
    /// Increments the applied_count quality metric and persists the update.
    pub async fn record_graph_touch(&self, learning_id: &str) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        if let Some(learning) = index.get_mut(learning_id) {
            learning.quality.applied_count += 1;
            learning.updated_at = Utc::now();
            let updated = learning.clone();
            drop(index);
            self.persist(&updated).await?;
            Ok(())
        } else {
            Err(StoreError::NotFound(learning_id.to_string()))
        }
    }

    pub async fn get(&self, id: &str) -> Result<SharedLearning, StoreError> {
        let index = self.index.read().await;
        index
            .get(id)
            .cloned()
            .ok_or_else(|| StoreError::NotFound(id.to_string()))
    }

    pub async fn list_all(&self) -> Result<Vec<SharedLearning>, StoreError> {
        let index = self.index.read().await;
        Ok(index.values().cloned().collect())
    }

    pub async fn list_by_trust_level(
        &self,
        level: TrustLevel,
    ) -> Result<Vec<SharedLearning>, StoreError> {
        let index = self.index.read().await;
        Ok(index
            .values()
            .filter(|l| l.trust_level == level)
            .cloned()
            .collect())
    }

    pub async fn promote_to_l1(&self, id: &str) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let learning = index
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        learning.promote_to_l1();
        let updated = learning.clone();
        drop(index);

        self.persist(&updated).await?;
        info!("Promoted learning {} to L1", id);
        Ok(())
    }

    pub async fn promote_to_l2(&self, id: &str) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let learning = index
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        learning.promote_to_l2();
        let updated = learning.clone();
        drop(index);

        self.persist(&updated).await?;
        info!("Promoted learning {} to L2", id);
        Ok(())
    }

    pub async fn promote_to_l3(&self, id: &str) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let learning = index
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        learning.promote_to_l3();
        let updated = learning.clone();
        drop(index);

        self.persist(&updated).await?;
        info!("Promoted learning {} to L3", id);
        Ok(())
    }

    pub async fn list_pending(&self) -> Result<Vec<SharedLearning>, StoreError> {
        self.list_by_status(SuggestionStatus::Pending).await
    }

    pub async fn list_by_status(
        &self,
        status: SuggestionStatus,
    ) -> Result<Vec<SharedLearning>, StoreError> {
        let index = self.index.read().await;
        Ok(index
            .values()
            .filter(|l| l.suggestion_status == status)
            .cloned()
            .collect())
    }

    pub async fn approve(&self, id: &str) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let learning = index
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        learning.suggestion_status = SuggestionStatus::Approved;
        learning.promote_to_l3();
        let updated = learning.clone();
        drop(index);

        self.persist(&updated).await?;
        info!("Approved suggestion {}", id);
        Ok(())
    }

    pub async fn reject(&self, id: &str, reason: Option<&str>) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let learning = index
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;
        learning.suggestion_status = SuggestionStatus::Rejected;
        learning.rejection_reason = reason.map(|r| r.to_string());
        learning.updated_at = Utc::now();
        let updated = learning.clone();
        drop(index);

        self.persist(&updated).await?;
        info!("Rejected suggestion {}", id);
        Ok(())
    }

    pub async fn record_application(
        &self,
        id: &str,
        agent_name: &str,
        effective: bool,
    ) -> Result<(), StoreError> {
        let mut index = self.index.write().await;
        let learning = index
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(id.to_string()))?;

        learning.quality.record_application(agent_name, effective);
        learning.updated_at = Utc::now();

        let should_auto_promote = self.config.auto_promote_l2
            && learning.trust_level == TrustLevel::L1
            && learning.quality.meets_l2_criteria();

        if should_auto_promote {
            learning.promote_to_l2();
            info!("Auto-promoted learning {} to L2", id);
        }

        let updated = learning.clone();
        drop(index);

        self.persist(&updated).await?;
        Ok(())
    }

    pub async fn find_similar(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(f64, SharedLearning)>, StoreError> {
        let index = self.index.read().await;
        let all_learnings: Vec<SharedLearning> = index.values().cloned().collect();
        drop(index);

        if all_learnings.is_empty() {
            return Ok(Vec::new());
        }

        // Hybrid path: when a role graph is configured, prefer its
        // weighted-mean-of-node-edge-doc rank with thesaurus term
        // expansion over pure BM25. The substring fallback within the
        // same call keeps candidates that match the literal query but
        // are not yet covered by any thesaurus node.
        if let Some(hybrid) = self.hybrid_rank(query, &all_learnings) {
            let mut scored = hybrid;
            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            if scored.len() > limit {
                scored.truncate(limit);
            }
            return Ok(scored);
        }

        // Fallback: pure BM25.
        let mut doc_freqs: HashMap<String, usize> = HashMap::new();
        let mut total_doc_len = 0;

        for doc in &all_learnings {
            let text = doc.extract_searchable_text();
            let terms: std::collections::HashSet<String> = text
                .to_lowercase()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            total_doc_len += terms.len();
            for term in &terms {
                *doc_freqs.entry(term.clone()).or_insert(0) += 1;
            }
        }

        let avg_doc_len = total_doc_len as f64 / all_learnings.len() as f64;
        let mut scorer = Bm25Scorer::new(all_learnings.len(), avg_doc_len);

        let query_lower = query.to_lowercase();
        let query_len = query_lower.split_whitespace().count();

        let mut scored: Vec<(f64, SharedLearning)> = all_learnings
            .into_iter()
            .map(|doc| {
                let doc_text = doc.extract_searchable_text();
                let raw_score = scorer.score(&query_lower, &doc_text, &doc_freqs);
                let normalized = scorer.normalize_score(raw_score, query_len);
                let weighted = normalized * doc.trust_level.weight() as f64;
                (weighted, doc)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        if scored.len() > limit {
            scored.truncate(limit);
        }

        Ok(scored)
    }

    /// Run the configured role graph against `query` and produce a
    /// `(score, SharedLearning)` list ranked by Terraphim hybrid scoring.
    ///
    /// Returns `None` when no graph is configured, the graph read lock is
    /// poisoned, the graph query itself fails, or the graph returns an
    /// empty result set (no thesaurus node matched the query). In every
    /// such case the caller is expected to fall back to pure BM25.
    ///
    /// Scoring: each `IndexedDocument.rank` is normalised against the
    /// best rank returned by the graph so the score sits in `[0, 1]`,
    /// then multiplied by the trust-level weight (`0..=3`) to keep
    /// parity with the BM25 scoring shape.
    #[cfg(feature = "shared-learning")]
    fn hybrid_rank(
        &self,
        query: &str,
        candidates: &[SharedLearning],
    ) -> Option<Vec<(f64, SharedLearning)>> {
        let graph_lock = self.role_graph.as_ref()?;
        let graph = graph_lock.read().ok()?;
        let graph_results = graph.query_graph(query, None, None).ok()?;
        if graph_results.is_empty() {
            return None;
        }
        let graph_id_rank: HashMap<String, u64> = graph_results
            .into_iter()
            .map(|(id, doc)| (id, doc.rank))
            .collect();
        let max_rank = graph_id_rank.values().copied().max().unwrap_or(1).max(1);
        let query_lower = query.to_lowercase();
        let scored: Vec<(f64, SharedLearning)> = candidates
            .iter()
            .filter(|l| {
                graph_id_rank.contains_key(&l.id)
                    || l.extract_searchable_text().contains(&query_lower)
            })
            .map(|l| {
                let rank = graph_id_rank.get(&l.id).copied().unwrap_or(0);
                let normalised = if rank == 0 {
                    0.0
                } else {
                    rank as f64 / max_rank as f64
                };
                let weighted = normalised * l.trust_level.weight() as f64;
                (weighted, l.clone())
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();
        if scored.is_empty() {
            return None;
        }
        Some(scored)
    }

    /// Shared-learning variant of `hybrid_rank` for the non-`shared-learning`
    /// feature build: always returns `None`, so callers fall back to BM25.
    #[cfg(not(feature = "shared-learning"))]
    fn hybrid_rank(
        &self,
        _query: &str,
        _candidates: &[SharedLearning],
    ) -> Option<Vec<(f64, SharedLearning)>> {
        None
    }

    pub async fn suggest(
        &self,
        context: &str,
        agent_name: &str,
        limit: usize,
    ) -> Result<Vec<SharedLearning>, StoreError> {
        let index = self.index.read().await;
        let applicable: Vec<SharedLearning> = index
            .values()
            .filter(|doc| {
                doc.applicable_agents.is_empty()
                    || doc.applicable_agents.contains(&agent_name.to_string())
            })
            .cloned()
            .collect();
        drop(index);

        if applicable.is_empty() {
            return Ok(Vec::new());
        }

        // Hybrid path: same gate-on-graph-then-fallback as `find_similar`,
        // applied after the `applicable_agents` filter so per-agent
        // scoping is preserved on both code paths.
        if let Some(mut hybrid) = self.hybrid_rank(context, &applicable) {
            hybrid.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            let mut out: Vec<SharedLearning> =
                hybrid.into_iter().map(|(_, l)| l).collect();
            if out.len() > limit {
                out.truncate(limit);
            }
            return Ok(out);
        }

        // Fallback: pure BM25.
        let mut doc_freqs: HashMap<String, usize> = HashMap::new();
        let mut total_doc_len = 0;

        for doc in &applicable {
            let text = doc.extract_searchable_text();
            let terms: std::collections::HashSet<String> = text
                .to_lowercase()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            total_doc_len += terms.len();
            for term in &terms {
                *doc_freqs.entry(term.clone()).or_insert(0) += 1;
            }
        }

        let avg_doc_len = total_doc_len as f64 / applicable.len() as f64;
        let mut scorer = Bm25Scorer::new(applicable.len(), avg_doc_len);

        let query_lower = context.to_lowercase();
        let query_len = query_lower.split_whitespace().count();

        let mut scored: Vec<SharedLearning> = applicable
            .into_iter()
            .map(|doc| {
                let doc_text = doc.extract_searchable_text();
                let raw_score = scorer.score(&query_lower, &doc_text, &doc_freqs);
                let normalized = scorer.normalize_score(raw_score, query_len);
                let weighted = normalized * doc.trust_level.weight() as f64;
                (doc, weighted)
            })
            .filter(|(_, score)| *score > 0.0)
            .map(|(doc, _)| doc)
            .collect();

        scored.truncate(limit);
        Ok(scored)
    }

    /// Suggest relevant entries from the legacy local `LearningEntry` corpus
    /// alongside BM25-scored `SharedLearning` results.
    ///
    /// The shared corpus is ranked with BM25 via `Self::suggest`. The local
    /// corpus is ranked outside this method by the caller (e.g.
    /// `learnings::capture::suggest_learnings` in `main.rs`) and passed in
    /// as `local_candidates` together with per-candidate weights. Results are
    /// merged, sorted by weight descending, and truncated to `limit`.
    ///
    /// Why decouple: the legacy learning module lives in the binary
    /// (`mod learnings` in `main.rs`) and the library crate cannot depend
    /// on it. Splitting the orchestration this way keeps the library free
    /// of binary-only paths while still letting callers (like
    /// `SuggestSub::SessionEnd`) rank across both corpora.
    ///
    /// `local_candidates` carries `(weight, SharedLearning)` pairs already
    /// converted by the caller (typically via
    /// `learnings::capture::shared_learning_from_entry`). Callers that have
    /// no local corpus to merge can pass an empty Vec.
    pub async fn suggest_with_local_scored(
        &self,
        context: &str,
        agent_name: &str,
        local_candidates: Vec<(f64, SharedLearning)>,
        limit: usize,
    ) -> Result<Vec<SharedLearning>, StoreError> {
        // 1. Rank shared corpus.
        let shared_results = self.suggest(context, agent_name, limit * 2).await?;

        // 2. Seed merged vec with shared results at default weight 1.0.
        let mut merged: Vec<(f64, SharedLearning)> = shared_results
            .into_iter()
            .map(|l| (1.0, l))
            .collect();

        // 3. Append pre-scored local candidates at their caller-supplied weight.
        merged.extend(local_candidates);

        // 4. Sort by weighted score descending and truncate.
        merged.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        merged.truncate(limit);

        Ok(merged.into_iter().map(|(_, l)| l).collect())
    }

    pub async fn close(&self) {
        info!("Shared learning store closed");
    }

    #[cfg(feature = "shared-learning")]
    pub fn set_role_graph(&mut self, graph: terraphim_rolegraph::RoleGraph) {
        // Populate the graph from the current in-memory index so callers
        // do not have to pre-load documents. Without this initial sync,
        // the graph would have no documents and `query_graph` would
        // always return empty, defeating the purpose of hybrid scoring.
        let existing: Vec<SharedLearning> = self
            .index
            .try_read()
            .map(|guard| guard.values().cloned().collect())
            .unwrap_or_default();

        let graph_lock = std::sync::RwLock::new(graph);
        {
            if let Ok(mut g) = graph_lock.write() {
                for learning in &existing {
                    let doc = build_document_for_graph(learning);
                    g.insert_document(learning.id.as_str(), doc);
                }
            }
        }
        self.role_graph = Some(graph_lock);
    }

    #[cfg(feature = "shared-learning")]
    pub fn role_graph(&self) -> Option<&std::sync::RwLock<terraphim_rolegraph::RoleGraph>> {
        self.role_graph.as_ref()
    }
}

#[cfg(feature = "shared-learning")]
impl terraphim_middleware::feedback_loop::GraphTouchStore for SharedLearningStore {
    fn record_graph_touch<'a>(
        &'a self,
        learning_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), StoreError>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut index = self.index.write().await;
            if let Some(learning) = index.get_mut(learning_id) {
                learning.quality.applied_count += 1;
                learning.updated_at = chrono::Utc::now();
                let updated = learning.clone();
                drop(index);
                self.persist(&updated).await?;
                Ok(())
            } else {
                Err(StoreError::NotFound(learning_id.to_string()))
            }
        })
    }
}

#[cfg(feature = "shared-learning")]
fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::task::block_in_place(|| {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(fut)
    })
}

#[cfg(feature = "shared-learning")]
impl terraphim_types::shared_learning::LearningStore for SharedLearningStore {
    fn insert(
        &self,
        learning: terraphim_types::shared_learning::SharedLearning,
    ) -> Result<String, terraphim_types::shared_learning::StoreError> {
        let id = learning.id.clone();
        block_on(Self::insert(self, learning))?;
        Ok(id)
    }

    fn get(
        &self,
        id: &str,
    ) -> Result<
        terraphim_types::shared_learning::SharedLearning,
        terraphim_types::shared_learning::StoreError,
    > {
        block_on(Self::get(self, id))
    }

    fn query_relevant(
        &self,
        agent: &str,
        context: &str,
        min_trust: terraphim_types::shared_learning::TrustLevel,
        limit: usize,
    ) -> Result<
        Vec<terraphim_types::shared_learning::SharedLearning>,
        terraphim_types::shared_learning::StoreError,
    > {
        let index = block_on(self.index.read());
        let mut candidates: Vec<terraphim_types::shared_learning::SharedLearning> = index
            .values()
            .filter(|l| l.trust_level >= min_trust)
            .filter(|l| {
                l.applicable_agents.is_empty()
                    || l.applicable_agents
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(agent))
            })
            .cloned()
            .collect();
        drop(index);

        if candidates.is_empty() {
            return Ok(Vec::new());
        }

        if !context.is_empty() {
            let context_lower = context.to_lowercase();
            if let Some(ref graph_lock) = self.role_graph {
                if let Ok(graph) = graph_lock.read() {
                    if let Ok(graph_results) = graph.query_graph(context, None, None) {
                        if !graph_results.is_empty() {
                            let graph_id_rank: std::collections::HashMap<String, u64> =
                                graph_results
                                    .into_iter()
                                    .map(|(id, doc)| (id, doc.rank))
                                    .collect();
                            candidates.retain(|l| {
                                graph_id_rank.contains_key(&l.id)
                                    || l.extract_searchable_text().contains(&context_lower)
                            });
                            candidates.sort_by(|a, b| {
                                let a_rank = graph_id_rank.get(&a.id).copied().unwrap_or(0);
                                let b_rank = graph_id_rank.get(&b.id).copied().unwrap_or(0);
                                b_rank.cmp(&a_rank)
                            });
                            candidates.truncate(limit);
                            return Ok(candidates);
                        }
                    }
                }
            }

            candidates.retain(|l| l.extract_searchable_text().contains(&context_lower));
        }

        candidates.sort_by_key(|l| std::cmp::Reverse(l.trust_level.weight()));
        candidates.truncate(limit);
        Ok(candidates)
    }

    fn record_applied(
        &self,
        id: &str,
        applied_by: &str,
    ) -> Result<(), terraphim_types::shared_learning::StoreError> {
        block_on(self.record_application(id, applied_by, false))
    }

    fn record_effective(
        &self,
        id: &str,
        applied_by: &str,
    ) -> Result<(), terraphim_types::shared_learning::StoreError> {
        block_on(self.record_application(id, applied_by, true))
    }

    fn list_by_trust(
        &self,
        min_trust: terraphim_types::shared_learning::TrustLevel,
    ) -> Result<
        Vec<terraphim_types::shared_learning::SharedLearning>,
        terraphim_types::shared_learning::StoreError,
    > {
        let all = block_on(self.list_all())?;
        Ok(all
            .into_iter()
            .filter(|l| l.trust_level >= min_trust)
            .collect())
    }

    fn archive_stale(
        &self,
        max_age_days: u32,
    ) -> Result<usize, terraphim_types::shared_learning::StoreError> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days as i64);
        let mut index = block_on(self.index.write());
        let before = index.len();
        let stale: Vec<(String, String)> = index
            .iter()
            .filter(|(_, l)| {
                l.trust_level <= terraphim_types::shared_learning::TrustLevel::L0
                    && l.updated_at <= cutoff
            })
            .map(|(id, l)| (id.clone(), l.source_agent.clone()))
            .collect();
        for (id, _) in &stale {
            index.remove(id.as_str());
        }
        drop(index);
        for (id, agent) in &stale {
            if let Err(e) = block_on(self.backend.delete(agent, id)) {
                warn!("Failed to delete markdown for stale learning {}: {e}", id);
            }
        }
        let removed = before - stale.len();
        Ok(removed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreResult {
    Created,
    Merged(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared_learning::types::LearningSource;
    use tempfile::TempDir;

    async fn create_test_store() -> SharedLearningStore {
        let temp_dir = TempDir::new().unwrap();
        let markdown_config = MarkdownStoreConfig {
            learnings_dir: temp_dir.path().to_path_buf(),
            shared_dir_name: "shared".to_string(),
        };
        let config = StoreConfig::default()
            .with_similarity_threshold(0.3)
            .with_markdown_config(markdown_config);
        SharedLearningStore::open(config).await.unwrap()
    }

    #[tokio::test]
    async fn test_store_open() {
        let store = create_test_store().await;
        let learnings = store.list_all().await.unwrap();
        assert!(learnings.is_empty());
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let store = create_test_store().await;

        let learning = SharedLearning::new(
            "Test Learning".to_string(),
            "Test content".to_string(),
            LearningSource::Manual,
            "test-agent".to_string(),
        );

        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.id, id);
        assert_eq!(retrieved.title, "Test Learning");
        assert_eq!(retrieved.trust_level, TrustLevel::L0);
    }

    #[tokio::test]
    async fn test_list_by_trust_level() {
        let store = create_test_store().await;

        let mut learning = SharedLearning::new(
            "L2 Learning".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        learning.promote_to_l1();
        learning.promote_to_l2();

        store.insert(learning).await.unwrap();

        let l2_learnings = store.list_by_trust_level(TrustLevel::L2).await.unwrap();
        assert_eq!(l2_learnings.len(), 1);

        let l1_learnings = store.list_by_trust_level(TrustLevel::L1).await.unwrap();
        assert!(l1_learnings.is_empty());
    }

    #[tokio::test]
    async fn test_record_application() {
        let store = create_test_store().await;

        let learning = SharedLearning::new(
            "Test".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent1".to_string(),
        );
        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        store.record_application(&id, "agent1", true).await.unwrap();
        store.record_application(&id, "agent2", true).await.unwrap();
        store.record_application(&id, "agent2", true).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.quality.applied_count, 3);
        assert_eq!(retrieved.quality.effective_count, 3);
        assert_eq!(retrieved.quality.agent_count, 2);
    }

    #[tokio::test]
    async fn test_promote_to_l2() {
        let store = create_test_store().await;

        let learning = SharedLearning::new(
            "Test".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        store.promote_to_l1(&id).await.unwrap();
        store.promote_to_l2(&id).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.trust_level, TrustLevel::L2);
        assert!(retrieved.promoted_at.is_some());
    }

    #[tokio::test]
    async fn test_suggest() {
        let store = create_test_store().await;

        let learning = SharedLearning::new(
            "Git Push Error".to_string(),
            "How to fix git push errors".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        )
        .with_keywords(vec!["git".to_string(), "push".to_string()]);

        let mut learning = learning;
        learning.promote_to_l1();
        store.insert(learning).await.unwrap();

        let suggestions = store
            .suggest("git push problems", "test-agent", 5)
            .await
            .unwrap();
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].title, "Git Push Error");
    }

    #[tokio::test]
    async fn test_store_with_dedup() {
        let store = create_test_store().await;

        let learning1 = SharedLearning::new(
            "Git Push Error".to_string(),
            "How to fix git push errors".to_string(),
            LearningSource::Manual,
            "agent1".to_string(),
        );

        let result1 = store.store_with_dedup(learning1).await.unwrap();
        assert_eq!(result1, StoreResult::Created);

        let learning2 = SharedLearning::new(
            "Git Push Issues".to_string(),
            "How to fix git push errors and issues".to_string(),
            LearningSource::Manual,
            "agent2".to_string(),
        );

        let result2 = store.store_with_dedup(learning2).await.unwrap();
        assert!(matches!(result2, StoreResult::Merged(_)));
    }

    #[tokio::test]
    async fn test_auto_promotion() {
        let store = create_test_store().await;

        let learning = SharedLearning::new(
            "Test".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent1".to_string(),
        );
        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        store.promote_to_l1(&id).await.unwrap();
        store.record_application(&id, "agent1", true).await.unwrap();
        store.record_application(&id, "agent1", true).await.unwrap();
        store.record_application(&id, "agent2", true).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.trust_level, TrustLevel::L2);
    }

    #[tokio::test]
    async fn test_open_loads_existing_markdown_learnings() {
        // Create a temp dir and directly save learnings via the markdown backend
        let temp_dir = TempDir::new().unwrap();
        let markdown_config = MarkdownStoreConfig {
            learnings_dir: temp_dir.path().to_path_buf(),
            shared_dir_name: "shared".to_string(),
        };
        let backend = MarkdownLearningStore::with_config(markdown_config.clone());

        let learning1 = SharedLearning::new(
            "Pre-existing Learning".to_string(),
            "This learning was saved before the store opened.".to_string(),
            LearningSource::AutoExtract,
            "test-agent".to_string(),
        );
        let id1 = learning1.id.clone();
        backend.save(&learning1).await.unwrap();

        let learning2 = SharedLearning::new(
            "Another Pre-existing".to_string(),
            "Also saved before open.".to_string(),
            LearningSource::Manual,
            "other-agent".to_string(),
        );
        let id2 = learning2.id.clone();
        backend.save(&learning2).await.unwrap();

        // Now open the store - it should load existing learnings
        let config = StoreConfig::default()
            .with_similarity_threshold(0.3)
            .with_markdown_config(markdown_config);
        let store = SharedLearningStore::open(config).await.unwrap();

        let all = store.list_all().await.unwrap();
        assert_eq!(all.len(), 2);

        let retrieved1 = store.get(&id1).await.unwrap();
        assert_eq!(retrieved1.title, "Pre-existing Learning");

        let retrieved2 = store.get(&id2).await.unwrap();
        assert_eq!(retrieved2.title, "Another Pre-existing");
    }

    #[tokio::test]
    async fn test_open_dedups_shared_and_canonical_copies() {
        // Create a temp dir and save the same learning to both agent dir and shared dir
        let temp_dir = TempDir::new().unwrap();
        let markdown_config = MarkdownStoreConfig {
            learnings_dir: temp_dir.path().to_path_buf(),
            shared_dir_name: "shared".to_string(),
        };
        let backend = MarkdownLearningStore::with_config(markdown_config.clone());

        let mut learning = SharedLearning::new(
            "Shared Dedup Test".to_string(),
            "Testing deduplication.".to_string(),
            LearningSource::AutoExtract,
            "agent-x".to_string(),
        );
        learning.id = "dedup-test-id".to_string();

        // Save to agent directory (canonical)
        backend.save(&learning).await.unwrap();

        // Save a stale variant to shared directory with same ID.
        // Canonical should win after hydration.
        let mut stale_shared_copy = learning.clone();
        stale_shared_copy.title = "Stale Shared Copy".to_string();
        stale_shared_copy.trust_level = TrustLevel::L1;
        backend.save_to_shared(&stale_shared_copy).await.unwrap();

        // Now open the store - it should deduplicate
        let config = StoreConfig::default()
            .with_similarity_threshold(0.3)
            .with_markdown_config(markdown_config);
        let store = SharedLearningStore::open(config).await.unwrap();

        let all = store.list_all().await.unwrap();
        // Should only have 1 entry despite 2 files on disk
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "dedup-test-id");
        assert_eq!(all[0].title, "Shared Dedup Test");
    }

    #[tokio::test]
    async fn test_persist_after_promotion_and_application() {
        let temp_dir = TempDir::new().unwrap();
        let markdown_config = MarkdownStoreConfig {
            learnings_dir: temp_dir.path().to_path_buf(),
            shared_dir_name: "shared".to_string(),
        };
        let config = StoreConfig::default()
            .with_similarity_threshold(0.3)
            .with_markdown_config(markdown_config.clone());

        let store = SharedLearningStore::open(config).await.unwrap();

        let learning = SharedLearning::new(
            "Persist Test".to_string(),
            "Testing persistence.".to_string(),
            LearningSource::Manual,
            "test-agent".to_string(),
        );
        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        // Promote to L2
        store.promote_to_l1(&id).await.unwrap();
        store.promote_to_l2(&id).await.unwrap();

        // Record applications
        store.record_application(&id, "agent1", true).await.unwrap();
        store.record_application(&id, "agent2", true).await.unwrap();

        // Close and reopen the store
        store.close().await;

        let config2 = StoreConfig::default()
            .with_similarity_threshold(0.3)
            .with_markdown_config(markdown_config);
        let reopened = SharedLearningStore::open(config2).await.unwrap();

        let retrieved = reopened.get(&id).await.unwrap();
        assert_eq!(retrieved.trust_level, TrustLevel::L2);
        assert_eq!(retrieved.quality.applied_count, 2);
        assert_eq!(retrieved.quality.effective_count, 2);
        assert_eq!(retrieved.quality.agent_count, 2);
        assert!(retrieved.promoted_at.is_some());
    }

    #[tokio::test]
    async fn test_approve_promotes_to_l3() {
        let store = create_test_store().await;
        let learning = SharedLearning::new(
            "Approve Test".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        store.approve(&id).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.trust_level, TrustLevel::L3);
        assert_eq!(retrieved.suggestion_status, SuggestionStatus::Approved);
    }

    #[tokio::test]
    async fn test_reject_sets_status() {
        let store = create_test_store().await;
        let learning = SharedLearning::new(
            "Reject Test".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        let id = learning.id.clone();
        store.insert(learning).await.unwrap();

        store.reject(&id, Some("not applicable")).await.unwrap();

        let retrieved = store.get(&id).await.unwrap();
        assert_eq!(retrieved.suggestion_status, SuggestionStatus::Rejected);
        assert_eq!(
            retrieved.rejection_reason.as_deref(),
            Some("not applicable")
        );
        assert_eq!(retrieved.trust_level, TrustLevel::L0);
    }

    #[tokio::test]
    async fn test_list_pending_filters() {
        let store = create_test_store().await;

        let pending = SharedLearning::new(
            "Pending".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        let pending_id = pending.id.clone();
        store.insert(pending).await.unwrap();

        let mut approved = SharedLearning::new(
            "Approved".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        approved.suggestion_status = SuggestionStatus::Approved;
        store.insert(approved).await.unwrap();

        let result = store.list_pending().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, pending_id);
    }

    #[tokio::test]
    async fn test_list_by_status() {
        let store = create_test_store().await;

        let mut rejected = SharedLearning::new(
            "Rejected".to_string(),
            "Content".to_string(),
            LearningSource::Manual,
            "agent".to_string(),
        );
        rejected.suggestion_status = SuggestionStatus::Rejected;
        let rejected_id = rejected.id.clone();
        store.insert(rejected).await.unwrap();

        let result = store
            .list_by_status(SuggestionStatus::Rejected)
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, rejected_id);
    }

    #[cfg(feature = "shared-learning")]
    mod learning_store_trait_tests {
        use super::*;
        use terraphim_types::shared_learning::{LearningStore, TrustLevel as Tl};

        async fn create_trait_test_store() -> SharedLearningStore {
            create_test_store().await
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_insert_and_get() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let learning = SharedLearning::new(
                "Trait Test".to_string(),
                "Testing trait insert and get".to_string(),
                LearningSource::Manual,
                "test-agent".to_string(),
            );
            let id = dyn_store.insert(learning).unwrap();
            assert!(!id.is_empty());

            let retrieved = dyn_store.get(&id).unwrap();
            assert_eq!(retrieved.title, "Trait Test");
            assert_eq!(retrieved.source_agent, "test-agent");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_get_not_found() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;
            assert!(dyn_store.get("nonexistent-id").is_err());
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_record_applied_and_effective() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let learning = SharedLearning::new(
                "App Test".to_string(),
                "Content".to_string(),
                LearningSource::Manual,
                "agent".to_string(),
            );
            let id = dyn_store.insert(learning).unwrap();

            dyn_store.record_applied(&id, "agent-a").unwrap();
            let l = dyn_store.get(&id).unwrap();
            assert_eq!(l.quality.applied_count, 1);

            dyn_store.record_effective(&id, "agent-b").unwrap();
            let l = dyn_store.get(&id).unwrap();
            assert_eq!(l.quality.applied_count, 2);
            assert_eq!(l.quality.effective_count, 1);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_auto_promote_on_effective() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let learning = SharedLearning::new(
                "Promote Test".to_string(),
                "Content".to_string(),
                LearningSource::Manual,
                "agent".to_string(),
            );
            let id = dyn_store.insert(learning).unwrap();

            assert_eq!(dyn_store.get(&id).unwrap().trust_level, Tl::L0);

            dyn_store.record_effective(&id, "agent-a").unwrap();
            dyn_store.record_effective(&id, "agent-b").unwrap();
            dyn_store.record_effective(&id, "agent-a").unwrap();
            dyn_store.record_effective(&id, "agent-b").unwrap();

            let l = dyn_store.get(&id).unwrap();
            assert_eq!(l.quality.effective_count, 4);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_list_by_trust() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let mut l1 = SharedLearning::new(
                "L1".to_string(),
                "c".to_string(),
                LearningSource::Manual,
                "a".to_string(),
            );
            l1.promote_to_l1();
            let mut l2 = SharedLearning::new(
                "L2".to_string(),
                "c".to_string(),
                LearningSource::Manual,
                "a".to_string(),
            );
            l2.promote_to_l1();
            l2.promote_to_l2();
            dyn_store.insert(l1).unwrap();
            dyn_store.insert(l2).unwrap();

            let l1_plus = dyn_store.list_by_trust(Tl::L1).unwrap();
            assert_eq!(l1_plus.len(), 2);

            let l2_only = dyn_store.list_by_trust(Tl::L2).unwrap();
            assert_eq!(l2_only.len(), 1);
            assert_eq!(l2_only[0].title, "L2");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_query_relevant_respects_trust() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let mut l0 = SharedLearning::new(
                "L0 Test".to_string(),
                "l0 content".to_string(),
                LearningSource::Manual,
                "agent".to_string(),
            );
            l0.trust_level = Tl::L0;
            dyn_store.insert(l0).unwrap();

            let results = dyn_store
                .query_relevant("agent", "l0 content", Tl::L1, 10)
                .unwrap();
            assert!(results.is_empty(), "L0 should be filtered out at L1 min");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_query_relevant_respects_agents() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let mut learning = SharedLearning::new(
                "Agent Specific".to_string(),
                "Only for security".to_string(),
                LearningSource::Manual,
                "sec".to_string(),
            )
            .with_applicable_agents(vec!["security-audit".to_string()]);
            learning.promote_to_l1();
            dyn_store.insert(learning).unwrap();

            let for_sec = dyn_store
                .query_relevant("security-audit", "security", Tl::L1, 10)
                .unwrap();
            assert_eq!(for_sec.len(), 1);

            let for_other = dyn_store
                .query_relevant("other-agent", "security", Tl::L1, 10)
                .unwrap();
            assert!(for_other.is_empty());
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_archive_stale() {
            let temp_dir = TempDir::new().unwrap();
            let markdown_config = MarkdownStoreConfig {
                learnings_dir: temp_dir.path().to_path_buf(),
                shared_dir_name: "shared".to_string(),
            };
            let config = StoreConfig::default().with_markdown_config(markdown_config);
            let store = SharedLearningStore::open(config).await.unwrap();

            let mut l0_stale = SharedLearning::new(
                "stale".to_string(),
                "c".to_string(),
                LearningSource::Manual,
                "a".to_string(),
            );
            l0_stale.trust_level = Tl::L0;
            l0_stale.updated_at = chrono::Utc::now() - chrono::Duration::days(60);
            let mut l1_old = SharedLearning::new(
                "old but L1".to_string(),
                "c".to_string(),
                LearningSource::Manual,
                "a".to_string(),
            );
            l1_old.trust_level = Tl::L1;
            l1_old.updated_at = chrono::Utc::now() - chrono::Duration::days(60);

            let dyn_store: &dyn LearningStore = &store;
            dyn_store.insert(l0_stale).unwrap();
            dyn_store.insert(l1_old).unwrap();

            let archived = dyn_store.archive_stale(30).unwrap();
            assert_eq!(archived, 1);

            let remaining = dyn_store.list_by_trust(Tl::L0).unwrap();
            assert_eq!(remaining.len(), 1);
            assert_eq!(remaining[0].trust_level, Tl::L1);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_query_relevant_with_role_graph() {
            use terraphim_rolegraph::RoleGraph;
            use terraphim_types::{Document, NormalizedTerm, NormalizedTermValue, Thesaurus};

            let mut store = create_test_store().await;

            let mut thesaurus = Thesaurus::new("test".to_string());
            thesaurus.insert(
                NormalizedTermValue::from("git"),
                NormalizedTerm::new(1, NormalizedTermValue::from("git")),
            );
            thesaurus.insert(
                NormalizedTermValue::from("push"),
                NormalizedTerm::new(2, NormalizedTermValue::from("push")),
            );

            let mut graph =
                RoleGraph::new_sync(terraphim_types::RoleName::new("test-role"), thesaurus)
                    .unwrap();

            let doc = Document {
                id: "doc-1".to_string(),
                url: String::new(),
                title: "Git Push".to_string(),
                body: "Git push force error fix".to_string(),
                description: None,
                summarization: None,
                stub: None,
                tags: None,
                rank: None,
                source_haystack: None,
                doc_type: terraphim_types::DocumentType::default(),
                synonyms: None,
                route: None,
                priority: None,
                quality_score: None,
            };
            let learning_id = "learning-graph-test";
            graph.insert_document(learning_id, doc);

            store.set_role_graph(graph);

            let learning = SharedLearning::new(
                "Graph Test Learning".to_string(),
                "Git push force error fix".to_string(),
                LearningSource::Manual,
                "agent".to_string(),
            );
            let mut l = learning;
            l.id = learning_id.to_string();
            l.trust_level = Tl::L2;
            let dyn_store: &dyn LearningStore = &store;
            dyn_store.insert(l).unwrap();

            let results = dyn_store
                .query_relevant("agent", "git push", Tl::L1, 10)
                .unwrap();
            assert!(!results.is_empty());
            assert_eq!(results[0].id, learning_id);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn test_trait_query_relevant_without_graph() {
            let store = create_trait_test_store().await;
            let dyn_store: &dyn LearningStore = &store;

            let mut learning = SharedLearning::new(
                "Rust Error".to_string(),
                "Use cargo clippy for rust errors".to_string(),
                LearningSource::Manual,
                "agent".to_string(),
            )
            .with_keywords(vec!["rust".to_string(), "clippy".to_string()]);
            learning.promote_to_l1();
            dyn_store.insert(learning).unwrap();

            let results = dyn_store
                .query_relevant("agent", "rust clippy", Tl::L1, 10)
                .unwrap();
            assert!(!results.is_empty());
        }
    }

    #[cfg(feature = "shared-learning")]
    mod hybrid_tests {
        //! Tests covering the Terraphim hybrid-scoring path used by
        //! `find_similar` and `suggest` when a role graph is configured.
        //!
        //! Every test seeds a `RoleGraph` with a thesaurus that contains
        //! at least one matching term for the query so the graph returns
        //! a non-empty ranked set. The store is then asked to surface
        //! learnings; the assertions verify that the graph-derived
        //! ordering (normalised `IndexedDocument.rank` * trust weight) is
        //! used in preference to pure BM25.

        use super::*;
        use crate::shared_learning::types::LearningSource;
        use terraphim_rolegraph::RoleGraph;
        use terraphim_types::{
            Document, DocumentType, NormalizedTerm, NormalizedTermValue, RoleName, Thesaurus,
        };

        fn empty_thesaurus() -> Thesaurus {
            Thesaurus::new("hybrid-test".to_string())
        }

        fn thesaurus_with(terms: &[&str]) -> Thesaurus {
            let mut thesaurus = Thesaurus::new("hybrid-test".to_string());
            for (i, term) in terms.iter().enumerate() {
                thesaurus.insert(
                    NormalizedTermValue::from(*term),
                    NormalizedTerm::new(i as u64 + 1, NormalizedTermValue::from(*term)),
                );
            }
            thesaurus
        }

        fn build_doc(id: &str, title: &str, body: &str) -> Document {
            Document {
                id: id.to_string(),
                url: String::new(),
                title: title.to_string(),
                body: body.to_string(),
                description: None,
                summarization: None,
                stub: None,
                tags: None,
                rank: None,
                source_haystack: Some("test".to_string()),
                doc_type: DocumentType::default(),
                synonyms: None,
                route: None,
                priority: None,
                quality_score: None,
            }
        }

        fn seed_graph(terms: &[&str]) -> RoleGraph {
            RoleGraph::new_sync(RoleName::new("hybrid-test"), thesaurus_with(terms)).unwrap()
        }

        async fn store_with_graph(graph: RoleGraph) -> SharedLearningStore {
            let store = create_test_store().await;
            // set_role_graph auto-syncs the in-memory index (empty here)
            // so subsequent inserts hook into the same graph via
            // `sync_to_graph`.
            let mut s = store;
            s.set_role_graph(graph);
            s
        }

        fn make_learning(
            id: &str,
            title: &str,
            content: &str,
            keywords: Vec<&str>,
            trust: TrustLevel,
        ) -> SharedLearning {
            let mut l = SharedLearning::new(
                title.to_string(),
                content.to_string(),
                LearningSource::Manual,
                "agent".to_string(),
            )
            .with_keywords(keywords.into_iter().map(String::from).collect());
            l.id = id.to_string();
            l.trust_level = trust;
            l
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn find_similar_uses_role_graph_when_available() {
            // Two learnings: "git push" matches the thesaurus term and
            // should be surfaced first; "random" does not match any
            // thesaurus term and must not appear.
            let mut graph = seed_graph(&["git", "push"]);
            let doc = build_doc("git-doc", "Git Push", "git push force error fix");
            graph.insert_document("git-doc", doc);

            let store = store_with_graph(graph).await;
            store
                .insert(make_learning(
                    "git-doc",
                    "Git Push",
                    "git push force error fix",
                    vec!["git"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();
            store
                .insert(make_learning(
                    "unrelated",
                    "Unrelated",
                    "completely different topic",
                    vec!["misc"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            let results = store.find_similar("git push", 5).await.unwrap();
            assert!(!results.is_empty(), "graph path should produce results");
            assert_eq!(results[0].1.id, "git-doc");
            // Hybrid scores are normalised to [0, trust_weight], not the
            // tanh-compressed [0, 1] range BM25 returns. We only assert
            // the relative ordering here.
            let ids: Vec<&str> = results.iter().map(|(_, l)| l.id.as_str()).collect();
            assert!(
                ids.contains(&"git-doc"),
                "git-doc must be surfaced, got {:?}",
                ids
            );
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn find_similar_falls_back_to_bm25_without_graph() {
            let store = create_test_store().await;
            store
                .insert(make_learning(
                    "git-doc",
                    "Git Push",
                    "git push force error fix",
                    vec!["git"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            let results = store.find_similar("git push", 5).await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].1.id, "git-doc");
            // BM25 path score sits in [0, trust_weight]; just assert it
            // is positive.
            assert!(results[0].0 > 0.0);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn find_similar_falls_back_to_bm25_when_graph_has_no_match() {
            // Thesaurus terms are unrelated to the query, so the graph
            // returns empty and we must drop to the BM25 fallback.
            let graph = seed_graph(&["unrelated", "noise"]);
            let store = store_with_graph(graph).await;
            store
                .insert(make_learning(
                    "git-doc",
                    "Git Push",
                    "git push force error fix",
                    vec!["git"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            let results = store.find_similar("git push", 5).await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].1.id, "git-doc");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn suggest_uses_role_graph_when_available() {
            let mut graph = seed_graph(&["rust", "clippy"]);
            graph.insert_document(
                "rust-doc",
                build_doc(
                    "rust-doc",
                    "Rust Clippy",
                    "use cargo clippy to find rust errors",
                ),
            );

            let store = store_with_graph(graph).await;
            store
                .insert(make_learning(
                    "rust-doc",
                    "Rust Clippy",
                    "use cargo clippy to find rust errors",
                    vec!["rust"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            let results = store.suggest("rust clippy", "agent", 5).await.unwrap();
            assert!(!results.is_empty());
            assert_eq!(results[0].id, "rust-doc");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn suggest_respects_applicable_agents_with_graph() {
            let mut graph = seed_graph(&["shared"]);
            graph.insert_document(
                "shared-doc",
                build_doc(
                    "shared-doc",
                    "Shared Topic",
                    "shared topic for everyone",
                ),
            );

            let store = store_with_graph(graph).await;

            // shared-doc: applicable to all agents (empty list).
            store
                .insert(make_learning(
                    "shared-doc",
                    "Shared Topic",
                    "shared topic for everyone",
                    vec![],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            // scoped-doc: applicable only to security-audit.
            let scoped = make_learning(
                "scoped-doc",
                "Scoped Topic",
                "scoped to security-audit agent only",
                vec!["shared"],
                TrustLevel::L1,
            )
            .with_applicable_agents(vec!["security-audit".to_string()]);
            store.insert(scoped).await.unwrap();

            let results = store.suggest("shared", "agent", 5).await.unwrap();
            assert!(
                results.iter().any(|l| l.id == "shared-doc"),
                "shared-doc should be visible to agent"
            );
            assert!(
                !results.iter().any(|l| l.id == "scoped-doc"),
                "scoped-doc must be filtered out for non-security agent"
            );
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn suggest_falls_back_to_bm25_without_graph() {
            let store = create_test_store().await;
            store
                .insert(make_learning(
                    "rust-doc",
                    "Rust Clippy",
                    "use cargo clippy to find rust errors",
                    vec!["rust"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            let results = store.suggest("rust clippy", "agent", 5).await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].id, "rust-doc");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn insert_syncs_learning_into_role_graph() {
            // Empty thesaurus: only the substring fallback inside
            // `query_graph` can match. We verify that after `insert`,
            // the graph contains a document whose body matches the
            // inserted learning's searchable text by using a query
            // that the substring fallback can resolve.
            let graph = seed_graph(&["marker"]);
            let store = store_with_graph(graph).await;
            store
                .insert(make_learning(
                    "synced-doc",
                    "Substring Only",
                    "this body has the word marker in it",
                    vec![],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            // Force the graph path: a query that *also* contains the
            // thesaurus term "marker" so `query_graph` returns non-empty
            // results.
            let results = store.find_similar("marker substring", 5).await.unwrap();
            assert!(
                results.iter().any(|(_, l)| l.id == "synced-doc"),
                "synced-doc must surface via the role graph after insert, got {:?}",
                results.iter().map(|(_, l)| l.id.clone()).collect::<Vec<_>>()
            );
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn hybrid_rank_respects_trust_weighting() {
            // Two learnings about git push: one at L1, one at L3. Both
            // match the graph thesaurus term. L3 must rank higher
            // because the hybrid score is multiplied by trust weight.
            let mut graph = seed_graph(&["git"]);
            graph.insert_document(
                "l1-doc",
                build_doc("l1-doc", "Git Push L1", "git push notes"),
            );
            graph.insert_document(
                "l3-doc",
                build_doc("l3-doc", "Git Push L3", "git push notes"),
            );

            let store = store_with_graph(graph).await;
            store
                .insert(make_learning(
                    "l1-doc",
                    "Git Push L1",
                    "git push notes",
                    vec!["git"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();
            store
                .insert(make_learning(
                    "l3-doc",
                    "Git Push L3",
                    "git push notes",
                    vec!["git"],
                    TrustLevel::L3,
                ))
                .await
                .unwrap();

            let results = store.find_similar("git", 5).await.unwrap();
            assert!(results.len() >= 2);
            let l1_pos = results.iter().position(|(_, l)| l.id == "l1-doc");
            let l3_pos = results.iter().position(|(_, l)| l.id == "l3-doc");
            if let (Some(a), Some(b)) = (l3_pos, l1_pos) {
                assert!(
                    a < b,
                    "L3 (trust_weight=3) must rank above L1 (trust_weight=1); positions l3={}, l1={}",
                    a,
                    b
                );
            } else {
                panic!("both docs should be present, got {:?}", results);
            }
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn hybrid_rank_with_empty_thesaurus_falls_back() {
            // Empty thesaurus means query_graph returns empty for any
            // query. The store must transparently fall back to BM25 so
            // callers still receive a sensible ranking.
            let graph = RoleGraph::new_sync(
                RoleName::new("hybrid-test"),
                empty_thesaurus(),
            )
            .unwrap();
            let store = store_with_graph(graph).await;
            store
                .insert(make_learning(
                    "git-doc",
                    "Git Push",
                    "git push force error fix",
                    vec!["git"],
                    TrustLevel::L1,
                ))
                .await
                .unwrap();

            let results = store.find_similar("git push", 5).await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].1.id, "git-doc");
        }
    }
}
