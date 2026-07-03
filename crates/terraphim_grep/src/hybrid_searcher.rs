use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use terraphim_types::Document;

#[derive(Debug, Clone)]
pub struct GrepOptions {
    pub haystack: Haystack,
    pub context_lines: usize,
    pub max_results: usize,
    pub force_rlm: bool,
    pub include_answer: bool,
}

impl Default for GrepOptions {
    fn default() -> Self {
        Self {
            haystack: Haystack::All,
            context_lines: 0,
            max_results: 50,
            force_rlm: false,
            include_answer: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Haystack {
    #[default]
    Code,
    Docs,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievedChunk {
    pub content: String,
    pub source: String,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub relevance_score: f64,
    pub haystack: &'static str,
}

impl From<Document> for RetrievedChunk {
    fn from(doc: Document) -> Self {
        Self {
            content: doc.body,
            source: doc.url,
            line_start: None,
            line_end: None,
            relevance_score: doc.rank.unwrap_or(0) as f64,
            haystack: "code",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KgConcept {
    pub id: u64,
    pub name: String,
    pub display_value: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub struct HybridResults {
    pub code_results: Vec<RetrievedChunk>,
    pub doc_results: Vec<RetrievedChunk>,
    pub kg_concepts: Vec<KgConcept>,
}

impl HybridResults {
    pub fn to_chunks(&self) -> Vec<RetrievedChunk> {
        let mut chunks = Vec::with_capacity(self.code_results.len() + self.doc_results.len());
        chunks.extend(self.code_results.clone());
        chunks.extend(self.doc_results.clone());
        chunks
    }

    pub fn total_results(&self) -> usize {
        self.code_results.len() + self.doc_results.len() + self.kg_concepts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.code_results.is_empty() && self.doc_results.is_empty() && self.kg_concepts.is_empty()
    }
}

/// Default weight applied to KG matches when boosting a chunk's relevance score.
/// A weight of 1.0 means a chunk whose path and content fully match the top KG concept
/// can roughly double its rank vs an unmatched chunk with the same base score.
pub const DEFAULT_KG_BOOST_WEIGHT: f64 = 1.0;

/// Compute the KG boost for a single chunk against a set of matched concepts.
///
/// For each concept whose `name` (or `display_value`, if set) is matched by
/// `terraphim_automata` in the chunk's source path or content, the concept's normalised
/// score contributes to the boost. Matches embedded inside a larger alphanumeric word are
/// ignored, so a concept like `auth` does not boost `Author`.
///
/// Why path-and-content: matching only paths misses content-defined concepts (a struct
/// `RetryPolicy` declared in `src/network.rs`); matching only content over-rewards files
/// that mention a concept in passing. Combining the two is a sensible default.
pub fn score_kg_boost(chunk: &RetrievedChunk, concepts: &[KgConcept], weight: f64) -> f64 {
    if concepts.is_empty() || weight <= 0.0 {
        return 0.0;
    }
    let max_concept_score: f64 = concepts.iter().map(|c| c.score).fold(0.0, f64::max);
    if max_concept_score <= 0.0 {
        return 0.0;
    }
    let mut boost = 0.0;
    for c in concepts {
        let needle = c.display_value.as_deref().unwrap_or(c.name.as_str()).trim();
        if needle.is_empty() {
            continue;
        }
        if automata_concept_matches(&chunk.source, needle)
            || automata_concept_matches(&chunk.content, needle)
        {
            boost += c.score / max_concept_score;
        }
    }
    (boost * weight).min(weight * concepts.len() as f64)
}

fn automata_concept_matches(text: &str, concept: &str) -> bool {
    let role = terraphim_types::RoleName::new("terraphim-grep-kg-boost");
    let thesaurus = terraphim_automata::thesaurus_from_terms(&role, std::iter::once(concept));
    match terraphim_automata::find_matches(text, thesaurus, true) {
        Ok(matches) => matches.iter().any(|matched| {
            matched
                .pos
                .is_some_and(|pos| has_concept_boundaries(text, pos))
        }),
        Err(error) => {
            tracing::debug!("KG boost automata match failed for concept {concept:?}: {error}");
            false
        }
    }
}

fn has_concept_boundaries(text: &str, (start, end): (usize, usize)) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

fn thesaurus_query_concepts(
    query: &str,
    thesaurus: &terraphim_types::Thesaurus,
    limit: usize,
) -> Vec<KgConcept> {
    match terraphim_automata::find_matches(query, thesaurus.clone(), false) {
        Ok(matches) => {
            let mut seen = std::collections::HashSet::new();
            let mut matched_values = std::collections::HashSet::new();
            let mut concepts: Vec<KgConcept> = matches
                .into_iter()
                .filter_map(|matched| {
                    matched_values.insert(matched.normalized_term.value.clone());
                    if !seen.insert(matched.term.clone()) {
                        return None;
                    }
                    Some(KgConcept {
                        id: 0,
                        name: matched.term,
                        display_value: None,
                        score: 1.0,
                    })
                })
                .take(limit)
                .collect();

            for (key, value) in thesaurus.clone().into_iter() {
                if matched_values.contains(&value.value) && seen.insert(key.to_string()) {
                    concepts.push(KgConcept {
                        id: 0,
                        name: key.to_string(),
                        display_value: None,
                        score: 1.0,
                    });
                }
            }

            concepts.sort_by(|a, b| a.name.cmp(&b.name));
            concepts.truncate(limit);
            concepts
        }
        Err(error) => {
            tracing::debug!("Thesaurus query automata match failed for {query:?}: {error}");
            Vec::new()
        }
    }
}

/// Apply KG boost to a batch of chunks and sort by boosted score (descending).
/// Mutates `relevance_score` in place so downstream consumers can see the boost reflected
/// in the JSON output -- otherwise the ordering would be inexplicable.
pub fn boost_chunks_with_kg(
    mut chunks: Vec<RetrievedChunk>,
    concepts: &[KgConcept],
) -> Vec<RetrievedChunk> {
    for chunk in chunks.iter_mut() {
        let boost = score_kg_boost(chunk, concepts, DEFAULT_KG_BOOST_WEIGHT);
        chunk.relevance_score += boost;
    }
    chunks.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    chunks
}

pub struct HybridSearcher {
    role_graph: Arc<tokio::sync::RwLock<terraphim_rolegraph::RoleGraph>>,
    /// Kept alongside the rolegraph so KG-style boosting still works when no documents
    /// have been indexed into the graph. The rolegraph requires indexed documents to
    /// return meaningful query results; the raw thesaurus is enough to identify which
    /// of the user's known concepts touch the query.
    thesaurus: terraphim_types::Thesaurus,
    search_path: PathBuf,
}

impl HybridSearcher {
    pub fn new(
        role_name: String,
        thesaurus: terraphim_types::Thesaurus,
    ) -> Result<Self, terraphim_rolegraph::Error> {
        let rolegraph = terraphim_rolegraph::RoleGraph::new_sync(
            terraphim_types::RoleName::new(&role_name),
            thesaurus.clone(),
        )?;

        Ok(Self {
            role_graph: Arc::new(tokio::sync::RwLock::new(rolegraph)),
            thesaurus,
            search_path: PathBuf::from("."),
        })
    }

    pub fn with_search_path(mut self, path: PathBuf) -> Self {
        self.search_path = path;
        self
    }

    pub async fn search(
        &self,
        query: &str,
        options: &GrepOptions,
    ) -> Result<HybridResults, String> {
        let max_results = options.max_results;
        let search_path = self.search_path.clone();
        let role_graph = self.role_graph.clone();
        let query_owned = query.to_string();

        let thesaurus = self.thesaurus.clone();

        let (kg_concepts, code_results) = match options.haystack {
            Haystack::All | Haystack::Code => {
                let kg_concepts =
                    Self::search_kg(&query_owned, max_results, role_graph.clone(), &thesaurus)
                        .await?;
                let candidate_limit = if kg_concepts.is_empty() {
                    max_results
                } else {
                    max_results.saturating_mul(5).max(max_results).min(1000)
                };
                let code_results =
                    Self::search_code(&query_owned, candidate_limit, search_path.clone()).await?;
                (kg_concepts, code_results)
            }
            Haystack::Docs => {
                let kg_concepts =
                    Self::search_kg(&query_owned, max_results, role_graph.clone(), &thesaurus)
                        .await?;
                (kg_concepts, vec![])
            }
        };

        // KG boost: re-rank code_results so chunks whose source path or content matches
        // a thesaurus concept rank above generic matches. The base relevance from fff is
        // currently uniform (1.0 per match), so without this step the user's knowledge
        // does not influence ordering at all. Boost in place; the boosted score is what
        // the JSON output reports so downstream tools see why a chunk ranked where it did.
        let mut code_results = boost_chunks_with_kg(code_results, &kg_concepts);
        code_results.truncate(max_results);

        Ok(HybridResults {
            code_results,
            doc_results: vec![],
            kg_concepts,
        })
    }

    async fn search_kg(
        query: &str,
        limit: usize,
        graph: Arc<tokio::sync::RwLock<terraphim_rolegraph::RoleGraph>>,
        thesaurus: &terraphim_types::Thesaurus,
    ) -> Result<Vec<KgConcept>, String> {
        let graph_guard = graph.read().await;

        let matches = graph_guard
            .query_graph_with_trigger_fallback(query, None, Some(limit), false)
            .map_err(|e| e.to_string())?;

        if !matches.is_empty() {
            let concepts = matches
                .into_iter()
                .map(|(doc_id, indexed_doc)| KgConcept {
                    id: 0,
                    name: doc_id,
                    display_value: None,
                    score: indexed_doc.rank as f64,
                })
                .collect();
            return Ok(concepts);
        }

        // Fallback: rolegraph returned nothing (graph has no indexed documents yet, or no
        // node matched the query). Fall back to thesaurus-only matching through
        // `terraphim_automata`, preserving the same Aho-Corasick semantics as the rest of
        // Terraphim rather than using ad-hoc substring matching.
        Ok(thesaurus_query_concepts(query, thesaurus, limit))
    }

    async fn search_code(
        query: &str,
        limit: usize,
        search_path: PathBuf,
    ) -> Result<Vec<RetrievedChunk>, String> {
        #[cfg(feature = "code-search")]
        {
            use fff_search::{
                FFFMode, FilePicker, FilePickerOptions, GrepMode, GrepSearchOptions,
                parse_grep_query,
            };

            let mut picker = FilePicker::new(FilePickerOptions {
                base_path: search_path.to_string_lossy().to_string(),
                mode: FFFMode::Ai,
                watch: false,
                cache_budget: None,
                ..FilePickerOptions::default()
            })
            .map_err(|e| format!("FilePicker init failed: {}", e))?;

            picker
                .collect_files()
                .map_err(|e| format!("File scan failed: {}", e))?;

            if picker.get_files().is_empty() {
                return Ok(vec![]);
            }

            let fff_query = parse_grep_query(query);
            let options = GrepSearchOptions {
                max_file_size: 10 * 1024 * 1024,
                max_matches_per_file: 200,
                smart_case: true,
                file_offset: 0,
                page_limit: limit,
                mode: GrepMode::PlainText,
                time_budget_ms: 0,
                before_context: 0,
                after_context: 0,
                classify_definitions: false,
                ..GrepSearchOptions::default()
            };

            let result = picker.grep(&fff_query, &options);

            let chunks = result
                .matches
                .iter()
                .take(limit)
                .filter_map(|m| {
                    let file = result.files.get(m.file_index)?;
                    Some(RetrievedChunk {
                        content: m.line_content.clone(),
                        source: file.relative_path(&picker),
                        line_start: Some(m.line_number as usize),
                        line_end: Some(m.line_number as usize),
                        relevance_score: 1.0,
                        haystack: "code",
                    })
                })
                .collect();

            Ok(chunks)
        }

        #[cfg(not(feature = "code-search"))]
        {
            let _ = (query, limit, search_path);
            // `code-search` is a default feature, so reaching here means the binary was
            // built with `--no-default-features` (or a custom set omitting it). Returning
            // an empty Vec silently makes every query look like "0 results" with no
            // explanation -- warn once so the cause is obvious rather than mysterious.
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                tracing::warn!(
                    "terraphim-grep was built without the `code-search` feature; \
                     file-content search is disabled and every query returns no matches. \
                     Rebuild with `--features code-search` (the default) to enable grep."
                );
            });
            Ok(vec![])
        }
    }

    pub fn fuse_and_rank(&self, mut results: Vec<RetrievedChunk>) -> Vec<RetrievedChunk> {
        results.sort_by(|a, b| {
            b.relevance_score
                .partial_cmp(&a.relevance_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_hybrid_results_empty() {
        let results = HybridResults {
            code_results: vec![],
            doc_results: vec![],
            kg_concepts: vec![],
        };
        assert!(results.is_empty());
        assert_eq!(results.total_results(), 0);
    }

    #[tokio::test]
    async fn test_hybrid_results_to_chunks() {
        let results = HybridResults {
            code_results: vec![RetrievedChunk {
                content: "test".to_string(),
                source: "file1.rs".to_string(),
                line_start: Some(1),
                line_end: Some(1),
                relevance_score: 0.9,
                haystack: "code",
            }],
            doc_results: vec![RetrievedChunk {
                content: "test doc".to_string(),
                source: "file2.md".to_string(),
                line_start: Some(5),
                line_end: Some(5),
                relevance_score: 0.8,
                haystack: "docs",
            }],
            kg_concepts: vec![],
        };

        let chunks = results.to_chunks();
        assert_eq!(chunks.len(), 2);
        assert_eq!(results.total_results(), 2);
    }

    /// Regression for #47: a default build (which now enables `code-search`) must grep
    /// file contents over a populated directory and return matches. Before the fix the
    /// `code-search` feature was off by default, so `search_code` compiled to a no-op stub
    /// and this exact scenario silently produced zero chunks.
    #[cfg(feature = "code-search")]
    #[tokio::test]
    async fn default_build_greps_populated_directory() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        std::fs::write(
            tmp.path().join("alpha.rs"),
            "fn configure_pipeline() { /* pipeline setup */ }\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("beta.rs"),
            "fn run_pipeline() { configure_pipeline(); }\n",
        )
        .unwrap();

        let searcher = HybridSearcher::new(
            "test-role".to_string(),
            terraphim_types::Thesaurus::new("t".to_string()),
        )
        .expect("build hybrid searcher")
        .with_search_path(tmp.path().to_path_buf());

        let results = searcher
            .search(
                "pipeline",
                &GrepOptions {
                    haystack: Haystack::Code,
                    max_results: 50,
                    ..GrepOptions::default()
                },
            )
            .await
            .expect("search should succeed");

        assert!(
            !results.code_results.is_empty(),
            "default build must return file-content matches over {:?}, got none",
            tmp.path()
        );
    }

    fn chunk(source: &str, content: &str, score: f64) -> RetrievedChunk {
        RetrievedChunk {
            content: content.to_string(),
            source: source.to_string(),
            line_start: Some(1),
            line_end: Some(1),
            relevance_score: score,
            haystack: "code",
        }
    }

    fn concept(name: &str, score: f64) -> KgConcept {
        KgConcept {
            id: 0,
            name: name.to_string(),
            display_value: None,
            score,
        }
    }

    fn test_thesaurus(terms: &[&str]) -> terraphim_types::Thesaurus {
        let mut thesaurus = terraphim_types::Thesaurus::new("test".to_string());
        for (idx, term) in terms.iter().enumerate() {
            let key = terraphim_types::NormalizedTermValue::from(*term);
            let normalised = terraphim_types::NormalizedTerm::new(idx as u64, key.clone());
            thesaurus.insert(key, normalised);
        }
        thesaurus
    }

    #[test]
    fn thesaurus_query_concepts_uses_automata_not_substring_expansion() {
        let thesaurus = test_thesaurus(&["auth", "authorisation", "authentication"]);

        let concepts = thesaurus_query_concepts("auth", &thesaurus, 10);

        assert_eq!(concepts.len(), 1);
        assert_eq!(concepts[0].name, "auth");
    }

    #[test]
    fn thesaurus_query_concepts_expands_shared_normalised_term() {
        let mut thesaurus = terraphim_types::Thesaurus::new("test".to_string());
        let normalised = terraphim_types::NormalizedTermValue::from("auth");
        for (idx, term) in ["auth", "authentication", "authorisation"]
            .iter()
            .enumerate()
        {
            let key = terraphim_types::NormalizedTermValue::from(*term);
            thesaurus.insert(
                key,
                terraphim_types::NormalizedTerm::new(idx as u64, normalised.clone()),
            );
        }

        let concepts = thesaurus_query_concepts("auth", &thesaurus, 10);
        let names = concepts
            .into_iter()
            .map(|concept| concept.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["auth", "authentication", "authorisation"]);
    }

    #[test]
    fn kg_boost_promotes_matching_chunks_to_top() {
        // Two chunks with identical base scores. Only one mentions the KG concept in its
        // path or content. After boost, the matching chunk must rank first -- this is the
        // "your knowledge tops the results" guarantee.
        let chunks = vec![
            chunk("src/parse_csv.rs", "fn parse_csv() {}", 1.0),
            chunk("src/retry_policy.rs", "pub struct RetryPolicy {}", 1.0),
        ];
        let concepts = vec![concept("retry_policy", 0.9)];

        let ranked = boost_chunks_with_kg(chunks, &concepts);
        assert_eq!(ranked[0].source, "src/retry_policy.rs");
        assert!(
            ranked[0].relevance_score > ranked[1].relevance_score,
            "KG-matched chunk must outscore the unmatched chunk: {:?} vs {:?}",
            ranked[0].relevance_score,
            ranked[1].relevance_score,
        );
    }

    #[test]
    fn kg_boost_no_concepts_is_a_noop() {
        // No KG concepts -> no boost -> ordering by base score only. Confirms the boost
        // path stays neutral when there's nothing to learn from the KG.
        let chunks = vec![chunk("a.rs", "alpha", 0.5), chunk("b.rs", "beta", 0.9)];
        let ranked = boost_chunks_with_kg(chunks, &[]);
        assert_eq!(ranked[0].source, "b.rs");
        assert!((ranked[0].relevance_score - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn kg_boost_stacks_when_multiple_concepts_match() {
        // A chunk that matches *two* concepts gets a larger boost than one matching only
        // one. Pins down the additive behaviour of score_kg_boost.
        let one_match = chunk("src/retry.rs", "fn retry() {}", 1.0);
        let two_matches = chunk("src/retry.rs", "fn retry() -> backoff::Result<()>", 1.0);
        let concepts = vec![concept("retry", 1.0), concept("backoff", 1.0)];

        let b1 = score_kg_boost(&one_match, &concepts, 1.0);
        let b2 = score_kg_boost(&two_matches, &concepts, 1.0);
        assert!(
            b2 > b1,
            "two-concept match must score higher than one-concept match: {b2} vs {b1}"
        );
    }

    #[test]
    fn kg_boost_does_not_match_concept_embedded_in_larger_word() {
        let author_only = chunk("docs/plan.md", "**Author**: OpenCode", 1.0);
        let concepts = vec![concept("auth", 1.0)];

        let boost = score_kg_boost(&author_only, &concepts, 1.0);

        assert_eq!(boost, 0.0, "auth must not match Author");
    }

    #[test]
    fn kg_boost_matches_concept_at_identifier_boundary() {
        let auth_identifier = chunk("src/auth_middleware.rs", "fn auth_middleware() {}", 1.0);
        let concepts = vec![concept("auth", 1.0)];

        let boost = score_kg_boost(&auth_identifier, &concepts, 1.0);

        assert!(boost > 0.0, "auth should match auth_middleware");
    }

    #[test]
    fn kg_boost_keeps_author_only_chunk_below_real_auth_chunk() {
        let chunks = vec![
            chunk("docs/design.md", "**Author**: OpenCode", 1.0),
            chunk("src/auth_middleware.rs", "fn auth_middleware() {}", 1.0),
        ];
        let concepts = vec![concept("auth", 1.0)];

        let ranked = boost_chunks_with_kg(chunks, &concepts);

        assert_eq!(ranked[0].source, "src/auth_middleware.rs");
        assert_eq!(ranked[1].source, "docs/design.md");
        assert_eq!(ranked[1].relevance_score, 1.0);
    }

    #[test]
    fn test_grep_options_default() {
        let options = GrepOptions::default();
        assert_eq!(options.haystack, Haystack::All);
        assert_eq!(options.context_lines, 0);
        assert_eq!(options.max_results, 50);
        assert!(!options.force_rlm);
        assert!(!options.include_answer);
    }
}
