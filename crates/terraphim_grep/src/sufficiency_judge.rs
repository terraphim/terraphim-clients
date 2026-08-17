use super::hybrid_searcher::{HybridResults, RetrievedChunk};

#[derive(Debug, Clone)]
pub struct HeuristicThresholds {
    pub min_coverage: f64,
    pub min_kg_confidence: f64,
    pub min_diversity: usize,
    pub min_results: usize,
}

impl Default for HeuristicThresholds {
    fn default() -> Self {
        Self {
            min_coverage: 0.7,
            min_kg_confidence: 0.5,
            min_diversity: 2,
            min_results: 3,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Sufficiency {
    Sufficient(Vec<RetrievedChunk>),
    NeedsSynthesis(Vec<RetrievedChunk>),
    NeedsExpansion(Vec<RetrievedChunk>),
    Insufficient(Vec<RetrievedChunk>),
}

pub struct SufficiencyJudge {
    thresholds: HeuristicThresholds,
}

/// The measured signals behind one verdict, kept so the verdict can be explained
/// without recomputing the heuristics.
#[derive(Debug, Clone, Copy)]
struct Metrics {
    coverage: f64,
    kg_confidence: f64,
    diversity: usize,
    result_count: usize,
}

impl Metrics {
    /// Signals for the no-results short circuit, where nothing is measured.
    fn empty() -> Self {
        Self {
            coverage: 0.0,
            kg_confidence: 0.0,
            diversity: 0,
            result_count: 0,
        }
    }
}

impl SufficiencyJudge {
    pub fn new(thresholds: HeuristicThresholds) -> Self {
        Self { thresholds }
    }

    pub fn judge(&self, results: &HybridResults, query: &str) -> Sufficiency {
        self.judge_explained(results, query).0
    }

    /// Same verdict as [`SufficiencyJudge::judge`], paired with a human-readable
    /// explanation of why that verdict was reached.
    ///
    /// The explanation names the decisive reason and then reports all four heuristic
    /// dimensions (coverage, KG confidence, haystack diversity, result count) against
    /// their thresholds, so consumers can act on the numbers without reading this file.
    pub fn judge_explained(&self, results: &HybridResults, query: &str) -> (Sufficiency, String) {
        let chunks = results.to_chunks();

        if chunks.is_empty() && results.kg_concepts.is_empty() {
            let decision = Sufficiency::Insufficient(vec![]);
            let explanation = self.explain(query, &decision, Metrics::empty());
            return (decision, explanation);
        }

        let metrics = Metrics {
            coverage: self.calculate_coverage(query, &chunks),
            kg_confidence: self.calculate_kg_confidence(&results.kg_concepts),
            diversity: self.calculate_diversity(&chunks),
            result_count: chunks.len(),
        };

        let decision = if chunks.len() < self.thresholds.min_results {
            Sufficiency::Insufficient(chunks)
        } else if metrics.coverage >= self.thresholds.min_coverage
            && metrics.kg_confidence >= self.thresholds.min_kg_confidence
            && metrics.diversity >= self.thresholds.min_diversity
        {
            Sufficiency::Sufficient(chunks)
        } else if metrics.coverage >= 0.3 && !chunks.is_empty() {
            Sufficiency::NeedsSynthesis(chunks)
        } else if metrics.coverage > 0.0 {
            Sufficiency::NeedsExpansion(chunks)
        } else {
            Sufficiency::Insufficient(chunks)
        };

        let explanation = self.explain(query, &decision, metrics);
        (decision, explanation)
    }

    fn explain(&self, query: &str, decision: &Sufficiency, metrics: Metrics) -> String {
        let thresholds = &self.thresholds;
        let count = metrics.result_count;

        let reason = match decision {
            Sufficiency::Sufficient(_) => format!(
                "Search results answer '{query}' directly; every heuristic threshold was met, \
                 so no LLM synthesis was requested"
            ),
            Sufficiency::NeedsSynthesis(_) => format!(
                "Found {count} chunk(s) for '{query}', but not all direct-answer thresholds \
                 were met; falling back to LLM synthesis"
            ),
            Sufficiency::NeedsExpansion(_) => format!(
                "Found {count} chunk(s) for '{query}' with only partial query coverage; \
                 expanding the result set before LLM synthesis"
            ),
            Sufficiency::Insufficient(_) if count == 0 => {
                format!("No chunks matched '{query}' and no knowledge-graph concepts were found")
            }
            Sufficiency::Insufficient(_) if count < thresholds.min_results => format!(
                "Only {count} chunk(s) matched '{query}'; the minimum for an answer is {}",
                thresholds.min_results
            ),
            Sufficiency::Insufficient(_) => {
                format!("No query term from '{query}' appeared in the {count} retrieved chunk(s)")
            }
        };

        format!(
            "{reason}. Metrics: coverage {:.2} (min {:.2}), KG confidence {:.2} (min {:.2}), \
             haystack diversity {} (min {}), result count {count} (min {}).",
            metrics.coverage,
            thresholds.min_coverage,
            metrics.kg_confidence,
            thresholds.min_kg_confidence,
            metrics.diversity,
            thresholds.min_diversity,
            thresholds.min_results,
        )
    }

    fn calculate_coverage(&self, query: &str, chunks: &[RetrievedChunk]) -> f64 {
        if chunks.is_empty() {
            return 0.0;
        }

        let query_terms: std::collections::HashSet<String> =
            query.split_whitespace().map(|s| s.to_lowercase()).collect();

        if query_terms.is_empty() {
            return 1.0;
        }

        let mut covered_terms = 0usize;
        for term in &query_terms {
            let term_found = chunks.iter().any(|chunk| {
                chunk.content.to_lowercase().contains(term)
                    || chunk.source.to_lowercase().contains(term)
            });
            if term_found {
                covered_terms += 1;
            }
        }

        covered_terms as f64 / query_terms.len() as f64
    }

    fn calculate_kg_confidence(&self, kg_concepts: &[super::hybrid_searcher::KgConcept]) -> f64 {
        if kg_concepts.is_empty() {
            return 0.0;
        }

        let avg_score: f64 =
            kg_concepts.iter().map(|c| c.score).sum::<f64>() / kg_concepts.len() as f64;
        avg_score.min(1.0)
    }

    fn calculate_diversity(&self, chunks: &[RetrievedChunk]) -> usize {
        let haystacks: std::collections::HashSet<&str> =
            chunks.iter().map(|c| c.haystack).collect();
        haystacks.len()
    }
}

impl Default for SufficiencyJudge {
    fn default() -> Self {
        Self::new(HeuristicThresholds::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hybrid_searcher::KgConcept;

    fn make_chunk(content: &str, source: &str, haystack: &'static str) -> RetrievedChunk {
        RetrievedChunk {
            content: content.to_string(),
            source: source.to_string(),
            line_start: Some(1),
            line_end: Some(1),
            relevance_score: 0.8,
            haystack,
        }
    }

    #[test]
    fn test_empty_results_insufficient() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let sufficiency = judge.judge(&results, "test query");
        assert!(matches!(sufficiency, Sufficiency::Insufficient(_)));
    }

    #[test]
    fn test_low_results_insufficient() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![make_chunk("test", "file.rs", "code")],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let sufficiency = judge.judge(&results, "test query");
        assert!(matches!(sufficiency, Sufficiency::Insufficient(_)));
    }

    #[test]
    fn test_high_coverage_sufficient() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![
                make_chunk("retry configuration in test file", "retry.rs", "code"),
                make_chunk("backoff settings", "config.rs", "code"),
            ],
            doc_results: vec![make_chunk("retry docs", "docs.md", "docs")],
            kg_concepts: vec![KgConcept {
                id: 1,
                name: "retry".to_string(),
                display_value: None,
                score: 0.9,
            }],
        };

        let sufficiency = judge.judge(&results, "retry configuration");
        assert!(matches!(sufficiency, Sufficiency::Sufficient(_)));
    }

    #[test]
    fn test_coverage_calculation() {
        let judge = SufficiencyJudge::default();
        let chunks = vec![make_chunk("retry configuration", "file.rs", "code")];

        let coverage = judge.calculate_coverage("retry configuration", &chunks);
        assert!(coverage >= 0.9);

        let coverage2 = judge.calculate_coverage("missing term", &chunks);
        assert!(coverage2 < 0.5);
    }

    #[test]
    fn test_diversity_calculation() {
        let judge = SufficiencyJudge::default();
        let chunks = vec![
            make_chunk("code result", "file.rs", "code"),
            make_chunk("code result 2", "file2.rs", "code"),
        ];
        assert_eq!(judge.calculate_diversity(&chunks), 1);

        let chunks2 = vec![
            make_chunk("code result", "file.rs", "code"),
            make_chunk("doc result", "file.md", "docs"),
        ];
        assert_eq!(judge.calculate_diversity(&chunks2), 2);
    }

    /// Every explanation must name all four heuristic dimensions so a consumer can see
    /// which one drove the decision without reading this file.
    fn assert_covers_all_dimensions(explanation: &str) {
        for dimension in [
            "coverage",
            "KG confidence",
            "haystack diversity",
            "result count",
        ] {
            assert!(
                explanation.contains(dimension),
                "explanation missing '{dimension}': {explanation}"
            );
        }
    }

    #[test]
    fn explanation_for_empty_results_reports_no_matches() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let (sufficiency, explanation) = judge.judge_explained(&results, "migration tree");
        assert!(matches!(sufficiency, Sufficiency::Insufficient(_)));
        assert!(
            explanation.contains("No chunks matched 'migration tree'"),
            "unexpected explanation: {explanation}"
        );
        assert_covers_all_dimensions(&explanation);
    }

    #[test]
    fn explanation_for_low_result_count_reports_minimum() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![make_chunk("test", "file.rs", "code")],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let (sufficiency, explanation) = judge.judge_explained(&results, "test query");
        assert!(matches!(sufficiency, Sufficiency::Insufficient(_)));
        assert!(
            explanation.contains("Only 1 chunk(s) matched 'test query'"),
            "unexpected explanation: {explanation}"
        );
        assert!(
            explanation.contains("minimum for an answer is 3"),
            "explanation should name the min_results threshold: {explanation}"
        );
        assert_covers_all_dimensions(&explanation);
    }

    #[test]
    fn explanation_for_sufficient_reports_direct_answer() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![
                make_chunk("retry configuration in test file", "retry.rs", "code"),
                make_chunk("backoff settings", "config.rs", "code"),
            ],
            doc_results: vec![make_chunk("retry docs", "docs.md", "docs")],
            kg_concepts: vec![KgConcept {
                id: 1,
                name: "retry".to_string(),
                display_value: None,
                score: 0.9,
            }],
        };

        let (sufficiency, explanation) = judge.judge_explained(&results, "retry configuration");
        assert!(matches!(sufficiency, Sufficiency::Sufficient(_)));
        assert!(
            explanation.contains("no LLM synthesis was requested"),
            "unexpected explanation: {explanation}"
        );
        assert!(
            explanation.contains("coverage 1.00"),
            "explanation should report the measured coverage: {explanation}"
        );
        assert_covers_all_dimensions(&explanation);
    }

    #[test]
    fn explanation_for_synthesis_mentions_llm_fallback() {
        let judge = SufficiencyJudge::default();
        // Three chunks clear `min_results`, coverage is 0.5 ("retry" hit, "backoff" missed),
        // which lands between the synthesis floor (0.3) and the direct-answer bar (0.7).
        let results = HybridResults {
            code_results: vec![
                make_chunk("retry once", "a.rs", "code"),
                make_chunk("retry twice", "b.rs", "code"),
                make_chunk("retry thrice", "c.rs", "code"),
            ],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let (sufficiency, explanation) = judge.judge_explained(&results, "retry backoff");
        assert!(
            matches!(sufficiency, Sufficiency::NeedsSynthesis(_)),
            "expected NeedsSynthesis, got {sufficiency:?}"
        );
        assert!(
            explanation.contains("falling back to LLM synthesis"),
            "unexpected explanation: {explanation}"
        );
        assert!(
            explanation.contains("Found 3 chunk(s) for 'retry backoff'"),
            "explanation should report the query and count: {explanation}"
        );
        assert_covers_all_dimensions(&explanation);
    }

    #[test]
    fn explanation_for_expansion_mentions_partial_coverage() {
        let judge = SufficiencyJudge::default();
        // Coverage 0.25 (only "retry" of four query terms is present) is above zero but
        // below the 0.3 synthesis floor, so the judge asks for expansion.
        let results = HybridResults {
            code_results: vec![
                make_chunk("retry once", "a.rs", "code"),
                make_chunk("retry twice", "b.rs", "code"),
                make_chunk("retry thrice", "c.rs", "code"),
            ],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let (sufficiency, explanation) =
            judge.judge_explained(&results, "retry backoff jitter ceiling");
        assert!(
            matches!(sufficiency, Sufficiency::NeedsExpansion(_)),
            "expected NeedsExpansion, got {sufficiency:?}"
        );
        assert!(
            explanation.contains("expanding the result set"),
            "unexpected explanation: {explanation}"
        );
        assert_covers_all_dimensions(&explanation);
    }

    #[test]
    fn explanation_for_zero_coverage_reports_no_matching_terms() {
        let judge = SufficiencyJudge::default();
        // Enough chunks to clear `min_results`, but no query term appears in any of them.
        let results = HybridResults {
            code_results: vec![
                make_chunk("alpha", "a.rs", "code"),
                make_chunk("beta", "b.rs", "code"),
                make_chunk("gamma", "c.rs", "code"),
            ],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        let (sufficiency, explanation) = judge.judge_explained(&results, "zzzznomatch");
        assert!(matches!(sufficiency, Sufficiency::Insufficient(_)));
        assert!(
            explanation.contains("No query term from 'zzzznomatch'"),
            "unexpected explanation: {explanation}"
        );
        assert_covers_all_dimensions(&explanation);
    }

    /// `judge` must stay a thin wrapper: the explained variant may not change the verdict.
    #[test]
    fn judge_explained_agrees_with_judge() {
        let judge = SufficiencyJudge::default();
        let results = HybridResults {
            code_results: vec![
                make_chunk("retry once", "a.rs", "code"),
                make_chunk("retry twice", "b.rs", "code"),
                make_chunk("retry thrice", "c.rs", "code"),
            ],
            doc_results: vec![],
            kg_concepts: vec![],
        };

        for query in ["", "retry", "retry backoff", "zzzznomatch"] {
            let plain = judge.judge(&results, query);
            let (explained, explanation) = judge.judge_explained(&results, query);
            assert_eq!(
                std::mem::discriminant(&plain),
                std::mem::discriminant(&explained),
                "verdict diverged for query '{query}'"
            );
            assert!(!explanation.is_empty(), "empty explanation for '{query}'");
        }
    }

    #[test]
    fn test_kg_confidence_calculation() {
        let judge = SufficiencyJudge::default();
        let concepts = vec![
            KgConcept {
                id: 1,
                name: "test".to_string(),
                display_value: None,
                score: 0.9,
            },
            KgConcept {
                id: 2,
                name: "test2".to_string(),
                display_value: None,
                score: 0.7,
            },
        ];
        let confidence = judge.calculate_kg_confidence(&concepts);
        assert!((confidence - 0.8).abs() < 0.001);

        let empty_confidence = judge.calculate_kg_confidence(&[]);
        assert_eq!(empty_confidence, 0.0);
    }
}
