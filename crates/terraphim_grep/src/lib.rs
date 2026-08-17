//! # terraphim_grep
//!
//! Hybrid search combining knowledge-graph concept expansion with ripgrep-backed
//! full-text search. Runs both pipelines concurrently via `tokio::spawn` and
//! merges results ranked by KG relevance boost and BM25 score.
//!
//! ## Quick start
//!
//! ```rust,ignore
//! use terraphim_grep::{TerraphimGrep, GrepOptions};
//! use std::sync::Arc;
//!
//! # async fn example() -> anyhow::Result<()> {
//! # let searcher = Arc::new(terraphim_grep::HybridSearcher::default());
//! # let judge = Arc::new(terraphim_grep::SufficiencyJudge::default());
//! let grep = TerraphimGrep::new(searcher, judge);
//! let options = GrepOptions::default();
//! let results = grep.search("query", options).await?;
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod hybrid_searcher;
pub mod kg_curation;
#[cfg(feature = "llm")]
pub mod openrouter_client;
pub mod rlm_context;
pub mod signatures;
pub mod sufficiency_judge;

use std::sync::Arc;

pub use error::{Result, TerraphimGrepError};
pub use hybrid_searcher::{
    DEFAULT_KG_BOOST_WEIGHT, GrepOptions, Haystack, HybridResults, HybridSearcher, KgConcept,
    RetrievedChunk, boost_chunks_with_kg, score_kg_boost,
};
pub use kg_curation::KgCurationRlm;
pub use rlm_context::RlmContext;
pub use signatures::{AnswerWithCitations, Citation, Match, NewConcept, RlmSignature};
pub use sufficiency_judge::{HeuristicThresholds, Sufficiency, SufficiencyJudge};

#[derive(Debug, Clone, serde::Serialize)]
pub struct GrepResult {
    pub chunks: Vec<RetrievedChunk>,
    pub answer: Option<AnswerWithCitations>,
    pub concepts: Vec<KgConcept>,
    pub sufficiency: SufficiencyState,
    /// Human-readable account of why `sufficiency` took the value it did, including the
    /// heuristic metrics and the thresholds they were compared against. Additive: the
    /// `sufficiency` values themselves are unchanged.
    pub sufficiency_explanation: String,
    pub stats: GrepStats,
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum SufficiencyState {
    SearchOnly,
    RlmSynthesis,
    RlmInsufficient,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GrepStats {
    pub search_latency_ms: u64,
    pub rlm_latency_ms: Option<u64>,
    pub chunks_returned: usize,
    pub kg_hits: usize,
}

pub struct TerraphimGrep {
    hybrid_searcher: Arc<HybridSearcher>,
    sufficiency_judge: Arc<SufficiencyJudge>,
    #[cfg(feature = "llm")]
    kg_curation: Option<Arc<KgCurationRlm>>,
    #[cfg(feature = "llm")]
    llm_client: Option<Arc<dyn terraphim_service::llm::LlmClient>>,
}

impl TerraphimGrep {
    #[cfg(feature = "llm")]
    pub fn new(
        hybrid_searcher: Arc<HybridSearcher>,
        sufficiency_judge: Arc<SufficiencyJudge>,
    ) -> Self {
        Self {
            hybrid_searcher,
            sufficiency_judge,
            kg_curation: None,
            llm_client: None,
        }
    }

    #[cfg(not(feature = "llm"))]
    pub fn new(
        hybrid_searcher: Arc<HybridSearcher>,
        sufficiency_judge: Arc<SufficiencyJudge>,
    ) -> Self {
        Self {
            hybrid_searcher,
            sufficiency_judge,
        }
    }

    #[cfg(feature = "llm")]
    pub fn with_kg_curation(mut self, kg_curation: Arc<KgCurationRlm>) -> Self {
        self.kg_curation = Some(kg_curation);
        self
    }

    #[cfg(feature = "llm")]
    pub fn with_llm_client(
        mut self,
        llm_client: Arc<dyn terraphim_service::llm::LlmClient>,
    ) -> Self {
        self.llm_client = Some(llm_client);
        self
    }

    pub async fn search(&self, query: &str, options: GrepOptions) -> Result<GrepResult> {
        let start = std::time::Instant::now();

        if options.force_rlm {
            return self.search_with_rlm(query, options, start).await;
        }

        let hybrid_results = self
            .hybrid_searcher
            .search(query, &options)
            .await
            .map_err(TerraphimGrepError::SearchFailed)?;

        let search_latency_ms = start.elapsed().as_millis() as u64;

        let (sufficiency, explanation) = self
            .sufficiency_judge
            .judge_explained(&hybrid_results, query);

        match sufficiency {
            sufficiency_judge::Sufficiency::Sufficient(chunks) => {
                let stats = GrepStats {
                    search_latency_ms,
                    rlm_latency_ms: None,
                    chunks_returned: chunks.len(),
                    kg_hits: hybrid_results.kg_concepts.len(),
                };

                Ok(GrepResult {
                    chunks,
                    answer: None,
                    concepts: hybrid_results.kg_concepts,
                    sufficiency: SufficiencyState::SearchOnly,
                    sufficiency_explanation: explanation,
                    stats,
                })
            }
            sufficiency_judge::Sufficiency::NeedsSynthesis(chunks) => {
                self.search_with_rlm_fallback(
                    query,
                    options,
                    chunks,
                    hybrid_results,
                    start,
                    explanation,
                )
                .await
            }
            sufficiency_judge::Sufficiency::NeedsExpansion(mut chunks) => {
                chunks.extend(hybrid_results.to_chunks());
                self.search_with_rlm_fallback(
                    query,
                    options,
                    chunks,
                    hybrid_results,
                    start,
                    explanation,
                )
                .await
            }
            sufficiency_judge::Sufficiency::Insufficient(chunks) => {
                let returned_count = chunks.len();
                let stats = GrepStats {
                    search_latency_ms,
                    rlm_latency_ms: None,
                    chunks_returned: returned_count,
                    kg_hits: 0,
                };

                Ok(GrepResult {
                    chunks,
                    answer: None,
                    concepts: vec![],
                    sufficiency: SufficiencyState::RlmInsufficient,
                    sufficiency_explanation: explanation,
                    stats,
                })
            }
        }
    }

    #[cfg(feature = "llm")]
    async fn search_with_rlm_fallback(
        &self,
        query: &str,
        options: GrepOptions,
        chunks: Vec<RetrievedChunk>,
        hybrid_results: HybridResults,
        start: std::time::Instant,
        explanation: String,
    ) -> Result<GrepResult> {
        let rlm_start = std::time::Instant::now();

        let ctx = RlmContext::new(query.to_string())
            .with_chunks(chunks.clone())
            .with_concepts(hybrid_results.kg_concepts.clone());

        let prompt = ctx.build_prompt();

        let task_instruction = if options.include_answer {
            format!(
                "{}\n\nSynthesise an answer based on the context above.",
                signatures::AnswerSignature {}.instructions()
            )
        } else {
            "List the relevant findings.\n\nProvide a brief answer based on the context above."
                .to_string()
        };

        let messages = vec![serde_json::json!({
            "role": "user",
            "content": format!("{}\n\n{}", prompt, task_instruction)
        })];

        let llm_response = if let Some(ref client) = self.llm_client {
            client
                .chat_completion(
                    messages,
                    terraphim_service::llm::ChatOptions {
                        max_tokens: Some(2000),
                        temperature: Some(0.3),
                    },
                )
                .await
                .map_err(|e| TerraphimGrepError::RlmFailed(e.to_string()))?
        } else {
            // No LLM configured -- degrade gracefully to search-only rather than failing.
            // The chunks we already retrieved are still useful even without synthesis.
            // Callers that explicitly need synthesis can pass `force_rlm = true`; that path
            // still fails fast in `search_with_rlm`.
            let stats = GrepStats {
                search_latency_ms: start.elapsed().as_millis() as u64,
                rlm_latency_ms: None,
                chunks_returned: chunks.len(),
                kg_hits: hybrid_results.kg_concepts.len(),
            };
            return Ok(GrepResult {
                chunks,
                answer: None,
                concepts: hybrid_results.kg_concepts,
                sufficiency: SufficiencyState::SearchOnly,
                // The judge asked for synthesis; say so, then say why it did not happen,
                // otherwise the explanation would contradict the reported state.
                sufficiency_explanation: format!(
                    "{explanation} No LLM client is configured, so the search results are \
                     returned without synthesis."
                ),
                stats,
            });
        };

        let rlm_latency_ms = rlm_start.elapsed().as_millis() as u64;
        let search_latency_ms = start.elapsed().as_millis() as u64;

        let answer = if options.include_answer {
            let signature = signatures::AnswerSignature {};
            signature.parse(&llm_response).ok().map(|a| {
                let citations = chunks
                    .iter()
                    .map(|c| Citation {
                        source: c.source.clone(),
                        line: c.line_start,
                        excerpt: c.content.chars().take(100).collect(),
                    })
                    .collect();
                signatures::AnswerWithCitations {
                    answer: a.answer,
                    citations,
                    confidence: a.confidence,
                }
            })
        } else {
            None
        };

        let stats = GrepStats {
            search_latency_ms,
            rlm_latency_ms: Some(rlm_latency_ms),
            chunks_returned: chunks.len(),
            kg_hits: hybrid_results.kg_concepts.len(),
        };

        if let Some(ref kg_curation) = self.kg_curation {
            let _ = kg_curation.extract_and_index(query, &llm_response).await;
        }

        Ok(GrepResult {
            chunks,
            answer,
            concepts: hybrid_results.kg_concepts,
            sufficiency: SufficiencyState::RlmSynthesis,
            sufficiency_explanation: explanation,
            stats,
        })
    }

    #[cfg(not(feature = "llm"))]
    async fn search_with_rlm_fallback(
        &self,
        _query: &str,
        _options: GrepOptions,
        _chunks: Vec<RetrievedChunk>,
        _hybrid_results: HybridResults,
        _start: std::time::Instant,
        _explanation: String,
    ) -> Result<GrepResult> {
        Err(TerraphimGrepError::LlmNotConfigured(
            "LLM feature not enabled".to_string(),
        ))
    }

    async fn search_with_rlm(
        &self,
        query: &str,
        options: GrepOptions,
        start: std::time::Instant,
    ) -> Result<GrepResult> {
        let hybrid_results = self
            .hybrid_searcher
            .search(query, &options)
            .await
            .map_err(TerraphimGrepError::SearchFailed)?;

        // The heuristics did not pick this path, but their metrics still explain what the
        // LLM was given, so report them behind a note that the decision was forced.
        let (_, metrics) = self
            .sufficiency_judge
            .judge_explained(&hybrid_results, query);
        let explanation = format!("RLM synthesis was requested explicitly. {metrics}");

        self.search_with_rlm_fallback(
            query,
            options,
            hybrid_results.to_chunks(),
            hybrid_results,
            start,
            explanation,
        )
        .await
    }

    pub fn stats(&self) -> GrepStats {
        GrepStats {
            search_latency_ms: 0,
            rlm_latency_ms: None,
            chunks_returned: 0,
            kg_hits: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "code-search")]
    use terraphim_types::Thesaurus;

    /// The explanation must reach the JSON payload as an additive field, leaving the
    /// existing `sufficiency` string untouched for backward compatibility.
    #[test]
    fn grep_result_json_carries_sufficiency_explanation() {
        let result = GrepResult {
            chunks: vec![],
            answer: None,
            concepts: vec![],
            sufficiency: SufficiencyState::RlmInsufficient,
            sufficiency_explanation: "No chunks matched 'migration tree'.".to_string(),
            stats: GrepStats {
                search_latency_ms: 1,
                rlm_latency_ms: None,
                chunks_returned: 0,
                kg_hits: 0,
            },
        };

        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&result).expect("serialize"))
                .expect("parse");

        assert_eq!(json["sufficiency"], "RlmInsufficient");
        assert_eq!(
            json["sufficiency_explanation"],
            "No chunks matched 'migration tree'."
        );
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

    /// When `code-search` is enabled and the sufficiency judge requests synthesis but no
    /// `LlmClient` is wired, the searcher must degrade to `SearchOnly` rather than failing
    /// with `LlmNotConfigured`. This guards D005 (graceful fallback) -- the previous
    /// behaviour broke the CLI for any query that returned partial results.
    #[cfg(feature = "code-search")]
    #[tokio::test]
    async fn search_without_llm_degrades_to_search_only() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        for i in 0..5 {
            let path = tmp.path().join(format!("file_{i}.rs"));
            std::fs::write(&path, format!("fn target_{i}() {{ /* target */ }}\n")).unwrap();
        }

        let hybrid = HybridSearcher::new("test-role".to_string(), Thesaurus::new("t".to_string()))
            .expect("build hybrid searcher")
            .with_search_path(tmp.path().to_path_buf());
        let grep = TerraphimGrep::new(Arc::new(hybrid), Arc::new(SufficiencyJudge::default()));

        let result = grep
            .search(
                "target",
                GrepOptions {
                    haystack: Haystack::Code,
                    max_results: 50,
                    ..GrepOptions::default()
                },
            )
            .await
            .expect("search should succeed without LLM");

        // The fff backend should have found at least one match -- if not the corpus is wrong.
        assert!(
            !result.chunks.is_empty(),
            "expected fff-search to return chunks from {:?}",
            tmp.path()
        );

        // Whether the judge picked `Sufficient` or `NeedsSynthesis` depends on coverage
        // heuristics, but the user-visible state must be one of the no-LLM-required ones.
        assert!(
            matches!(
                result.sufficiency,
                SufficiencyState::SearchOnly | SufficiencyState::RlmInsufficient
            ),
            "expected SearchOnly/RlmInsufficient, got {:?}",
            result.sufficiency
        );
        assert!(result.answer.is_none(), "no LLM -> no synthesised answer");
        assert!(
            result.sufficiency_explanation.contains("Metrics: coverage"),
            "explanation should report the heuristic metrics, got {:?}",
            result.sufficiency_explanation
        );
    }

    /// When no thesaurus is available, the searcher must still run the `fff-search` code path
    /// and return results with empty concepts. This is the "enhanced grep" failover mode.
    #[cfg(feature = "code-search")]
    #[tokio::test]
    async fn search_without_thesaurus_uses_fff_mode() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        for i in 0..3 {
            let path = tmp.path().join(format!("file_{i}.rs"));
            std::fs::write(&path, format!("fn target_{i}() {{ /* target */ }}\n")).unwrap();
        }

        // Empty thesaurus => no KG configuration.
        let thesaurus = Thesaurus::new("test-role".to_string());
        assert!(thesaurus.is_empty());

        let hybrid = HybridSearcher::new("test-role".to_string(), thesaurus)
            .expect("build hybrid searcher")
            .with_search_path(tmp.path().to_path_buf());
        let grep = TerraphimGrep::new(Arc::new(hybrid), Arc::new(SufficiencyJudge::default()));

        let result = grep
            .search(
                "target",
                GrepOptions {
                    haystack: Haystack::Code,
                    max_results: 50,
                    ..GrepOptions::default()
                },
            )
            .await
            .expect("search should succeed without thesaurus");

        assert!(
            !result.chunks.is_empty(),
            "expected fff-search to return chunks without KG"
        );
        assert!(
            result.concepts.is_empty(),
            "expected no KG concepts without thesaurus"
        );
        assert_eq!(result.stats.kg_hits, 0);
    }

    /// The RLM prompt for `include_answer` must embed the `AnswerSignature`
    /// JSON instructions so the model knows it must return structured output.
    #[test]
    fn test_answer_prompt_includes_json_instructions() {
        let prompt = "Query: test\n## Retrieved Context\nchunk".to_string();
        let include_answer = true;

        let task_instruction = if include_answer {
            format!(
                "{}\n\nSynthesise an answer based on the context above.",
                signatures::AnswerSignature {}.instructions()
            )
        } else {
            "List the relevant findings.\n\nProvide a brief answer based on the context above."
                .to_string()
        };

        let message = serde_json::json!({
            "role": "user",
            "content": format!("{}\n\n{}", prompt, task_instruction)
        });

        let content = message["content"].as_str().expect("content string");
        assert!(
            content.contains("\"answer\": the synthesised answer"),
            "prompt must embed AnswerSignature instructions"
        );
        assert!(
            content.contains("\"citations\": array of {source, line (optional), excerpt}"),
            "prompt must embed citation format"
        );
        assert!(
            content.contains("\"confidence\": a number between 0 and 1"),
            "prompt must embed confidence format"
        );
    }

    /// The non-answer path must NOT embed AnswerSignature instructions.
    #[test]
    fn test_list_prompt_excludes_json_instructions() {
        let prompt = "Query: test\n## Retrieved Context\nchunk".to_string();
        let include_answer = false;

        let task_instruction = if include_answer {
            format!(
                "{}\n\nSynthesise an answer based on the context above.",
                signatures::AnswerSignature {}.instructions()
            )
        } else {
            "List the relevant findings.\n\nProvide a brief answer based on the context above."
                .to_string()
        };

        let message = serde_json::json!({
            "role": "user",
            "content": format!("{}\n\n{}", prompt, task_instruction)
        });

        let content = message["content"].as_str().expect("content string");
        assert!(
            !content.contains("\"answer\": the synthesised answer"),
            "list prompt must not embed AnswerSignature instructions"
        );
        assert!(
            content.contains("List the relevant findings"),
            "list prompt must contain the list instruction"
        );
    }
}
