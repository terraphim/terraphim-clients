//! Explicit synthesis requests must not be discarded by the sufficiency heuristic.
#![cfg(all(feature = "llm", feature = "code-search"))]

use std::sync::Arc;
use terraphim_grep::{
    GrepOptions, HeuristicThresholds, HybridSearcher, SufficiencyJudge, TerraphimGrep,
};
use terraphim_types::Thesaurus;

#[tokio::test]
async fn sufficient_results_honor_answer_request_without_inventing_an_answer() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("brief.md"), "Decision: public preview.\n").unwrap();
    let searcher = HybridSearcher::new("public-demo".into(), Thesaurus::new("public-demo".into()))
        .unwrap()
        .with_search_path(root.path().to_path_buf());
    let judge = SufficiencyJudge::new(HeuristicThresholds {
        min_coverage: 0.0,
        min_kg_confidence: 0.0,
        min_diversity: 0,
        min_results: 1,
    });
    // Real retrieval/judge, deliberately no provider: requested synthesis must explain
    // the unavailable provider rather than claim search answered the request.
    let grep = TerraphimGrep::new(Arc::new(searcher), Arc::new(judge));
    let plain = grep
        .search("Decision", GrepOptions::default())
        .await
        .unwrap();
    assert!(
        plain
            .sufficiency_explanation
            .contains("Results sufficient on their own")
    );
    let answer = grep
        .search(
            "Decision",
            GrepOptions {
                include_answer: true,
                ..GrepOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(plain.chunks.len(), answer.chunks.len());
    assert!(answer.answer.is_none());
    assert!(
        answer
            .sufficiency_explanation
            .contains("no LLM client is configured"),
        "{}",
        answer.sufficiency_explanation
    );
}
