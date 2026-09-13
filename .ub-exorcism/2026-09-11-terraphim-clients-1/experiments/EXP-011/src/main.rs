//! EXP-011: NaN injection into the three float comparators.
//! Arm 1 (grep): terraphim_grep::hybrid_searcher::boost_chunks_with_kg (pub), sweep of
//!   slice sizes and NaN placements; std sort may only detect a broken total order on
//!   the non-small-sort paths, so small n is "not detected", not "refuted".
//! Arm 2 (markdown): terraphim_agent::learnings::list_all_entries (pub) over
//!   frontmatter carrying `importance_total: NaN` (f64::from_str accepts "NaN").
//! Arm 3 (dedup): SharedLearningStore::store_with_dedup (pub) with two textually empty
//!   learnings already indexed, so avg_doc_len = 0 and BM25 yields NaN for both docs.
//! Arm 4 (masking): NaN KgConcept.score through score_kg_boost's f64::max / f64::min.
use std::panic::{catch_unwind, AssertUnwindSafe};
use terraphim_grep::hybrid_searcher::{boost_chunks_with_kg, score_kg_boost, KgConcept, RetrievedChunk};

fn chunk(score: f64, content: &str) -> RetrievedChunk {
    RetrievedChunk { content: content.to_string(), source: "src/x.rs".to_string(),
        line_start: None, line_end: None, relevance_score: score, haystack: "code" }
}

fn panic_msg(e: Box<dyn std::any::Any + Send>) -> String {
    e.downcast_ref::<String>().cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "<non-string panic payload>".to_string())
}

fn grep_arm() -> bool {
    println!("--- arm 1: grep comparator (boost_chunks_with_kg / fuse_and_rank idiom)");
    let mut any_panic = false;
    let sizes = [5usize, 10, 20, 21, 32, 50, 64, 100, 200, 500, 1000, 5000, 20000];
    let placements: [(&str, Box<dyn Fn(usize, usize) -> bool>); 5] = [
        ("every-7th", Box::new(|i, _| i % 7 == 3)),
        ("first", Box::new(|i, _| i == 0)),
        ("last", Box::new(|i, n| i + 1 == n)),
        ("middle", Box::new(|i, n| i == n / 2)),
        ("half-random", Box::new(|i, _| (i.wrapping_mul(2654435761) >> 7) & 1 == 1)),
    ];
    for &n in &sizes {
        for (label, pred) in &placements {
            let mut v = Vec::with_capacity(n);
            for i in 0..n {
                // pseudo-random finite scores so the input is not presorted
                let s = ((i.wrapping_mul(1103515245).wrapping_add(12345)) % 1000) as f64 / 10.0;
                v.push(chunk(if pred(i, n) { f64::NAN } else { s }, ""));
            }
            let r = catch_unwind(AssertUnwindSafe(|| boost_chunks_with_kg(v, &[])));
            match r {
                Ok(out) => println!("n={n:<6} nan={label:<12} -> returned {} chunks (no panic)", out.len()),
                Err(e) => { any_panic = true; println!("n={n:<6} nan={label:<12} -> PANIC: {}", panic_msg(e)); }
            }
        }
    }
    any_panic
}

fn markdown_arm() -> bool {
    println!("--- arm 2: markdown importance_total NaN through list_all_entries");
    let dir = tempfile::tempdir().expect("tempdir");
    let mut any_panic = false;
    for &n in &[3usize, 30, 300] {
        let sub = dir.path().join(format!("n{n}"));
        std::fs::create_dir_all(&sub).unwrap();
        for i in 0..n {
            let total = if i % 3 == 1 { "NaN".to_string() } else { format!("{:.4}", (i % 17) as f64 / 17.0) };
            let md = format!(
"---\nid: learn-{i}\ncommand: false\nexit_code: 1\nsource: Project\ncaptured_at: 2026-09-12T10:00:{:02}Z\nworking_dir: /tmp\nimportance_total: {total}\nimportance_severity: 0.5000\nimportance_repetition: 1\nimportance_recency: 1.0000\nimportance_has_correction: false\n---\n\n## Command\n\n`false`\n\n## Error Output\n\n```\nboom\n```\n", i % 60);
            std::fs::write(sub.join(format!("learning-{i}.md")), md).unwrap();
        }
        let r = catch_unwind(AssertUnwindSafe(|| terraphim_agent::learnings::list_all_entries(&sub, n)));
        match r {
            Ok(Ok(entries)) => println!("n={n:<4} -> {} entries returned (no panic)", entries.len()),
            Ok(Err(e)) => println!("n={n:<4} -> Err({e}) (no panic)"),
            Err(e) => { any_panic = true; println!("n={n:<4} -> PANIC: {}", panic_msg(e)); }
        }
    }
    any_panic
}

fn dedup_arm() -> bool {
    println!("--- arm 3: store_with_dedup partial_cmp().unwrap() with NaN BM25 scores");
    use terraphim_agent::shared_learning::{SharedLearningStore, StoreConfig};
    use terraphim_types::shared_learning::{LearningSource, SharedLearning};
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = StoreConfig::default();
    cfg.markdown.learnings_dir = dir.path().to_path_buf();
    let store = rt.block_on(SharedLearningStore::open(cfg)).expect("open");
    for _ in 0..2 {
        let empty = SharedLearning::new(String::new(), String::new(), LearningSource::Manual, "exp011".to_string());
        match rt.block_on(store.insert(empty)) { Ok(()) => {}, Err(e) => { println!("insert of empty learning refused: {e}"); return false; } }
    }
    let probe = SharedLearning::new("token".to_string(), String::new(), LearningSource::Manual, "exp011".to_string());
    // block_on from outside any runtime context, so the only panic that can surface is the store's own
    let r = catch_unwind(AssertUnwindSafe(|| rt.block_on(store.store_with_dedup(probe))));
    match r {
        Ok(Ok(res)) => { println!("store_with_dedup -> {res:?} (no panic)"); false }
        Ok(Err(e)) => { println!("store_with_dedup -> Err({e}) (no panic)"); false }
        Err(e) => { println!("store_with_dedup -> PANIC: {}", panic_msg(e)); true }
    }
}

fn masking_arm() {
    println!("--- arm 4: NaN KgConcept.score masking in score_kg_boost");
    let c = chunk(1.0, "retry policy lives here");
    let nan_only = [KgConcept { id: 1, name: "retry".into(), display_value: None, score: f64::NAN }];
    let nan_plus = [KgConcept { id: 1, name: "retry".into(), display_value: None, score: f64::NAN },
                    KgConcept { id: 2, name: "policy".into(), display_value: None, score: 0.5 }];
    for (label, cs) in [("nan-only", &nan_only[..]), ("nan+finite", &nan_plus[..])] {
        let b = score_kg_boost(&c, cs, 0.5);
        println!("{label:<11} boost={b} finite={}", b.is_finite());
    }
}

fn main() {
    println!("EXP-011 rustc={}", option_env!("EXP_RUSTC").unwrap_or("see log header"));
    let a = grep_arm();
    let b = markdown_arm();
    let c = dedup_arm();
    masking_arm();
    println!("EXP-011 RESULT: grep_panic={a} markdown_panic={b} dedup_panic={c}");
    println!("EXP-011 {}", if a || b || c { "PANIC CONFIRMED (safe-Rust panic, not UB)" } else { "NO_EVIDENCE (no panic on this toolchain)" });
}
