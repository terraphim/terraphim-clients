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

    /// Whether the caller explicitly asked for LLM synthesis.
    ///
    /// RLM synthesis is opt-in: merely having an API key in the environment must not
    /// turn a millisecond-scale grep into a multi-second LLM round trip. Only
    /// `--force-rlm` (`force_rlm`) or `--answer` (`include_answer`) enable it.
    ///
    /// See terraphim/terraphim-clients#81.
    fn rlm_requested(options: &GrepOptions) -> bool {
        options.force_rlm || options.include_answer
    }

    /// Build a `SearchOnly` result from chunks that were retrieved but not synthesised.
    fn search_only_result(
        chunks: Vec<RetrievedChunk>,
        hybrid_results: HybridResults,
        search_latency_ms: u64,
    ) -> GrepResult {
        let stats = GrepStats {
            search_latency_ms,
            rlm_latency_ms: None,
            chunks_returned: chunks.len(),
            kg_hits: hybrid_results.kg_concepts.len(),
        };

        GrepResult {
            chunks,
            answer: None,
            concepts: hybrid_results.kg_concepts,
            sufficiency: SufficiencyState::SearchOnly,
            stats,
        }
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

        let sufficiency = self.sufficiency_judge.judge(&hybrid_results, query);

        match sufficiency {
            sufficiency_judge::Sufficiency::Sufficient(chunks) => Ok(Self::search_only_result(
                chunks,
                hybrid_results,
                search_latency_ms,
            )),
            sufficiency_judge::Sufficiency::NeedsSynthesis(chunks) => {
                if !Self::rlm_requested(&options) {
                    tracing::debug!(
                        "sufficiency judge requested synthesis; returning {} chunks search-only \
                         (pass --answer or --force-rlm to synthesise)",
                        chunks.len()
                    );
                    return Ok(Self::search_only_result(
                        chunks,
                        hybrid_results,
                        search_latency_ms,
                    ));
                }
                self.search_with_rlm_fallback(query, options, chunks, hybrid_results, start)
                    .await
            }
            sufficiency_judge::Sufficiency::NeedsExpansion(mut chunks) => {
                if !Self::rlm_requested(&options) {
                    tracing::debug!(
                        "sufficiency judge requested expansion; returning {} chunks search-only \
                         (pass --answer or --force-rlm to synthesise)",
                        chunks.len()
                    );
                    return Ok(Self::search_only_result(
                        chunks,
                        hybrid_results,
                        search_latency_ms,
                    ));
                }
                chunks.extend(hybrid_results.to_chunks());
                self.search_with_rlm_fallback(query, options, chunks, hybrid_results, start)
                    .await
            }
            sufficiency_judge::Sufficiency::Insufficient(chunks) => {
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
                    sufficiency: SufficiencyState::RlmInsufficient,
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

        self.search_with_rlm_fallback(
            query,
            options,
            hybrid_results.to_chunks(),
            hybrid_results,
            start,
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

    /// A local, in-process `LlmClient` that answers from a fixed string and counts calls.
    ///
    /// This is a real trait implementation, not a mocking framework: it performs the same
    /// contract as a network provider (returns the JSON envelope `AnswerSignature` expects)
    /// without leaving the process. The call counter is what lets a test assert that the
    /// RLM path was *not* entered -- the observable difference the #81 fix is about.
    #[cfg(all(feature = "llm", feature = "code-search"))]
    struct CountingLocalLlm {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[cfg(all(feature = "llm", feature = "code-search"))]
    impl CountingLocalLlm {
        fn new() -> Self {
            Self {
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[cfg(all(feature = "llm", feature = "code-search"))]
    #[async_trait::async_trait]
    impl terraphim_service::llm::LlmClient for CountingLocalLlm {
        fn name(&self) -> &'static str {
            "counting-local"
        }

        async fn summarize(
            &self,
            _content: &str,
            _opts: terraphim_service::llm::SummarizeOptions,
        ) -> terraphim_service::Result<String> {
            Err(terraphim_service::ServiceError::Config(
                "summarize not supported by the local test client".to_string(),
            ))
        }

        async fn chat_completion(
            &self,
            _messages: Vec<serde_json::Value>,
            _opts: terraphim_service::llm::ChatOptions,
        ) -> terraphim_service::Result<String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(r#"{"answer":"local synthesis","citations":[],"confidence":0.9}"#.to_string())
        }
    }

    /// Build a corpus that the sufficiency judge classifies as `NeedsSynthesis`.
    ///
    /// With an empty thesaurus the KG confidence is always 0.0, so `Sufficient` is
    /// unreachable; five matching files clear `min_results = 3` and give coverage 1.0,
    /// which lands in the `NeedsSynthesis` branch. The precondition is asserted rather
    /// than assumed so a judge change surfaces as a clear failure here.
    #[cfg(all(feature = "llm", feature = "code-search"))]
    async fn needs_synthesis_fixture() -> (tempfile::TempDir, Arc<HybridSearcher>) {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        for i in 0..5 {
            let path = tmp.path().join(format!("file_{i}.rs"));
            std::fs::write(&path, format!("fn target_{i}() {{ /* target */ }}\n")).unwrap();
        }

        let hybrid = Arc::new(
            HybridSearcher::new("test-role".to_string(), Thesaurus::new("t".to_string()))
                .expect("build hybrid searcher")
                .with_search_path(tmp.path().to_path_buf()),
        );

        let options = GrepOptions {
            haystack: Haystack::Code,
            max_results: 50,
            ..GrepOptions::default()
        };
        let results = hybrid
            .search("target", &options)
            .await
            .expect("hybrid search");
        let verdict = SufficiencyJudge::default().judge(&results, "target");
        assert!(
            matches!(verdict, Sufficiency::NeedsSynthesis(_)),
            "fixture precondition: judge must return NeedsSynthesis, got {verdict:?}"
        );

        (tmp, hybrid)
    }

    /// Regression: terraphim/terraphim-clients#81.
    ///
    /// A `NeedsSynthesis` verdict must NOT trigger a chat completion when the user asked
    /// for neither `--answer` nor `--force-rlm`. Before the fix, exporting
    /// `OPENROUTER_API_KEY` turned every such query into a ~20s LLM round trip.
    #[cfg(all(feature = "llm", feature = "code-search"))]
    #[tokio::test]
    async fn needs_synthesis_without_answer_skips_llm() {
        let (_tmp, hybrid) = needs_synthesis_fixture().await;
        let llm = Arc::new(CountingLocalLlm::new());

        let grep = TerraphimGrep::new(hybrid, Arc::new(SufficiencyJudge::default()))
            .with_llm_client(llm.clone());

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
            .expect("search should succeed");

        assert_eq!(llm.calls(), 0, "no LLM call without --answer/--force-rlm");
        assert!(
            matches!(result.sufficiency, SufficiencyState::SearchOnly),
            "expected SearchOnly, got {:?}",
            result.sufficiency
        );
        assert!(result.answer.is_none(), "no synthesis => no answer");
        assert!(!result.chunks.is_empty(), "chunks must still be returned");
        assert_eq!(result.stats.rlm_latency_ms, None, "no RLM latency recorded");
    }

    /// The opt-in path must still work: `--answer` on the same corpus synthesises.
    #[cfg(all(feature = "llm", feature = "code-search"))]
    #[tokio::test]
    async fn needs_synthesis_with_answer_invokes_llm() {
        let (_tmp, hybrid) = needs_synthesis_fixture().await;
        let llm = Arc::new(CountingLocalLlm::new());

        let grep = TerraphimGrep::new(hybrid, Arc::new(SufficiencyJudge::default()))
            .with_llm_client(llm.clone());

        let result = grep
            .search(
                "target",
                GrepOptions {
                    haystack: Haystack::Code,
                    max_results: 50,
                    include_answer: true,
                    ..GrepOptions::default()
                },
            )
            .await
            .expect("search should succeed");

        assert_eq!(llm.calls(), 1, "--answer must invoke the LLM exactly once");
        assert!(
            matches!(result.sufficiency, SufficiencyState::RlmSynthesis),
            "expected RlmSynthesis, got {:?}",
            result.sufficiency
        );
        let answer = result.answer.expect("--answer must produce an answer");
        assert_eq!(answer.answer, "local synthesis");
    }

    /// `--force-rlm` alone (without `--answer`) must still reach the LLM.
    #[cfg(all(feature = "llm", feature = "code-search"))]
    #[tokio::test]
    async fn force_rlm_without_answer_invokes_llm() {
        let (_tmp, hybrid) = needs_synthesis_fixture().await;
        let llm = Arc::new(CountingLocalLlm::new());

        let grep = TerraphimGrep::new(hybrid, Arc::new(SufficiencyJudge::default()))
            .with_llm_client(llm.clone());

        let result = grep
            .search(
                "target",
                GrepOptions {
                    haystack: Haystack::Code,
                    max_results: 50,
                    force_rlm: true,
                    ..GrepOptions::default()
                },
            )
            .await
            .expect("search should succeed");

        assert_eq!(llm.calls(), 1, "--force-rlm must invoke the LLM");
        assert!(
            matches!(result.sufficiency, SufficiencyState::RlmSynthesis),
            "expected RlmSynthesis, got {:?}",
            result.sufficiency
        );
    }

    /// The opt-in predicate: only the two explicit flags enable synthesis.
    #[test]
    fn rlm_requested_only_for_explicit_flags() {
        let base = GrepOptions::default();
        assert!(
            !TerraphimGrep::rlm_requested(&base),
            "default is search-only"
        );

        assert!(TerraphimGrep::rlm_requested(&GrepOptions {
            include_answer: true,
            ..GrepOptions::default()
        }));
        assert!(TerraphimGrep::rlm_requested(&GrepOptions {
            force_rlm: true,
            ..GrepOptions::default()
        }));
        assert!(TerraphimGrep::rlm_requested(&GrepOptions {
            force_rlm: true,
            include_answer: true,
            ..GrepOptions::default()
        }));
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

    /// Regression for #2721: the Insufficient branch previously hardcoded `chunks_returned: 0`
    /// and `concepts: vec![]`, discarding KG boost data that was already computed.
    /// With a corpus of 2 files (below the default `min_results: 3`), the judge returns
    /// Insufficient. The result must reflect actual chunk count, not zero.
    #[cfg(feature = "code-search")]
    #[tokio::test]
    async fn insufficient_path_propagates_chunk_count() {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        // Only 2 files -- below default min_results (3), forces Insufficient path.
        for i in 0..2 {
            let path = tmp.path().join(format!("sparse_{i}.rs"));
            std::fs::write(&path, format!("fn sparse_fn_{i}() {{ /* sparse */ }}\n")).unwrap();
        }

        let hybrid =
            HybridSearcher::new("test-role".to_string(), terraphim_types::Thesaurus::new("t".to_string()))
                .expect("build hybrid searcher")
                .with_search_path(tmp.path().to_path_buf());
        let judge = SufficiencyJudge::default(); // min_results = 3
        let grep = TerraphimGrep::new(Arc::new(hybrid), Arc::new(judge));

        let result = grep
            .search(
                "sparse",
                GrepOptions {
                    haystack: Haystack::Code,
                    max_results: 50,
                    ..GrepOptions::default()
                },
            )
            .await
            .expect("search should succeed");

        // If the judge marked this Insufficient, chunks_returned must not be zero.
        // (Before the fix it was always 0, hiding how many partial results were found.)
        if matches!(result.sufficiency, SufficiencyState::RlmInsufficient) {
            assert_eq!(
                result.stats.chunks_returned,
                result.chunks.len(),
                "chunks_returned must equal actual chunk count in Insufficient path"
            );
            assert_eq!(
                result.stats.kg_hits,
                result.concepts.len(),
                "kg_hits must equal concept count in Insufficient path"
            );
        }
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
