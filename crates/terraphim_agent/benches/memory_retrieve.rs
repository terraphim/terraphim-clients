//! Retrieval latency bench for `memory_retrieve::retrieve` (#261, epic #255).
//!
//! Corpus: the committed fixture (`tests/fixtures/memory_bench/corpus.jsonl`,
//! 60 items) tiled to 100, 1,000 and 10,000 items. Tile `t` of item `id`
//! gets the id `<id>-t<t>` and its content unchanged, so the corpus is
//! deterministic and every id stays unique.
//!
//! Queries: the fixture queries that name at least one concept of the
//! committed Terraphim Engineer thesaurus (decided at runtime with the same
//! `find_matches` call `retrieve` uses; PR #279 counted seven). One
//! measurement is one `retrieve` call with `limit = 5` for one query;
//! iterations cycle through the queries in order.
//!
//! Two reports come out of `cargo bench -p terraphim_agent --bench
//! memory_retrieve`:
//!
//! * Criterion's own per-group estimate (mean with a confidence interval;
//!   Criterion 0.8 prints no percentiles).
//! * A custom summary, printed before the Criterion groups, with p50 and p95
//!   (nearest-rank) over a fixed number of timed calls per query, plus the
//!   injected size (`memory_bench::injected_size`) of the top-five hits per
//!   query on the 60-item base corpus.
//!
//! The custom summary runs only when `--bench` is on the command line and
//! `--test` is not, so `cargo test --all-targets` (which runs this binary
//! without `--bench`) and `cargo bench -- --test` stay fast.
//!
//! Ranking is untouched: this file only calls the existing `retrieve`.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use terraphim_agent::memory_bench::{
    Fixture, RETRIEVAL_LIMIT, injected_size, load_fixture, load_thesaurus,
};
use terraphim_agent::memory_retrieve::retrieve;
use terraphim_agent_evolution::MemoryItem;
use terraphim_types::{RoleName, Thesaurus};

const ROLE_NAME: &str = "Terraphim Engineer";
const THESAURUS_FILE: &str = "thesaurus.json";
/// Corpus sizes the design targets are stated for (p95 under 100 ms at 1k,
/// under 1 s at 10k).
const SIZES: [usize; 3] = [100, 1_000, 10_000];
/// Timed calls per query and size in the custom p50/p95 summary.
const SUMMARY_CALLS_PER_QUERY: usize = 5;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("memory_bench")
}

/// Tile the base corpus to exactly `n` items with deterministic id suffixes.
fn tile(base: &[MemoryItem], n: usize) -> Vec<MemoryItem> {
    let mut items = Vec::with_capacity(n);
    for k in 0..n {
        let source = &base[k % base.len()];
        let mut item = source.clone();
        item.id = format!("{}-t{}", source.id, k / base.len());
        items.push(item);
    }
    let mut ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "tiled ids must stay unique");
    items
}

/// Fixture queries that name at least one thesaurus concept, in fixture order.
fn concept_queries(fixture: &Fixture, thesaurus: &Thesaurus) -> Vec<String> {
    fixture
        .queries
        .iter()
        .map(|q| q.query.clone())
        .filter(|q| {
            !terraphim_automata::find_matches(q, thesaurus, false)
                .expect("find_matches over a fixture query")
                .is_empty()
        })
        .collect()
}

fn one_call(role: &RoleName, thesaurus: &Thesaurus, items: &[MemoryItem], query: &str) -> usize {
    retrieve(
        role,
        thesaurus.clone(),
        items,
        query,
        None,
        Some(RETRIEVAL_LIMIT),
    )
    .expect("retrieve must succeed")
    .hits
    .len()
}

/// Nearest-rank percentile over a sorted sample.
fn percentile(sorted: &[Duration], p: f64) -> Duration {
    assert!(!sorted.is_empty());
    let rank = ((p * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// p50/p95 latency per size and injected size per query, printed to stdout.
fn custom_summary(role: &RoleName, thesaurus: &Thesaurus, base: &[MemoryItem], queries: &[String]) {
    println!("memory_retrieve summary (custom, nearest-rank percentiles)");
    println!(
        "  queries ({}): {}",
        queries.len(),
        queries
            .iter()
            .map(|q| format!("{q:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "  calls per size: {} queries x {} calls",
        queries.len(),
        SUMMARY_CALLS_PER_QUERY
    );

    for &n in &SIZES {
        let items = tile(base, n);
        let mut samples = Vec::with_capacity(queries.len() * SUMMARY_CALLS_PER_QUERY);
        for query in queries {
            for _ in 0..SUMMARY_CALLS_PER_QUERY {
                let start = Instant::now();
                black_box(one_call(role, thesaurus, &items, query));
                samples.push(start.elapsed());
            }
        }
        samples.sort_unstable();
        let p50 = percentile(&samples, 0.50);
        let p95 = percentile(&samples, 0.95);
        let max = samples[samples.len() - 1];
        println!(
            "  items={n:>6}  n={:>3}  p50={:>9.3} ms  p95={:>9.3} ms  max={:>9.3} ms",
            samples.len(),
            p50.as_secs_f64() * 1e3,
            p95.as_secs_f64() * 1e3,
            max.as_secs_f64() * 1e3,
        );
    }

    // Injected size of the top-five hits per query on the committed corpus.
    let mut bytes = Vec::with_capacity(queries.len());
    for query in queries {
        let hits: Vec<MemoryItem> = retrieve(
            role,
            thesaurus.clone(),
            base,
            query,
            None,
            Some(RETRIEVAL_LIMIT),
        )
        .expect("retrieve must succeed")
        .hits
        .into_iter()
        .map(|h| h.item)
        .collect();
        let size = injected_size(query, &hits);
        println!(
            "  injected  query={:?}  hits={}  bytes={}  estimated_tokens={}",
            query,
            hits.len(),
            size.bytes,
            size.estimated_tokens
        );
        bytes.push(size);
    }
    let count = bytes.len() as u64;
    let mean_bytes = bytes.iter().map(|s| s.bytes).sum::<u64>() as f64 / count as f64;
    let mean_tokens = bytes.iter().map(|s| s.estimated_tokens).sum::<u64>() as f64 / count as f64;
    let max_bytes = bytes.iter().map(|s| s.bytes).max().unwrap_or(0);
    let max_tokens = bytes.iter().map(|s| s.estimated_tokens).max().unwrap_or(0);
    println!(
        "  injected  mean_bytes={mean_bytes:.1}  max_bytes={max_bytes}  mean_estimated_tokens={mean_tokens:.1}  max_estimated_tokens={max_tokens}"
    );
}

fn bench_retrieve(
    c: &mut Criterion,
    role: &RoleName,
    thesaurus: &Thesaurus,
    base: &[MemoryItem],
    queries: &[String],
) {
    let mut group = c.benchmark_group("memory_retrieve");
    for &n in &SIZES {
        let items = tile(base, n);
        group.throughput(Throughput::Elements(1));
        // Each retrieve rebuilds a RoleGraph over all items; keep the sample
        // count at Criterion's minimum for the two large corpora.
        if n >= 1_000 {
            group.sample_size(10);
        }
        if n >= 10_000 {
            // Ten samples of a 140 ms to 300 ms call do not fit Criterion's
            // default five-second window; Criterion asked for 19 s.
            group.measurement_time(Duration::from_secs(20));
        }
        let next = Cell::new(0usize);
        group.bench_with_input(BenchmarkId::new("items", n), &items, |b, items| {
            b.iter(|| {
                let query = &queries[next.get() % queries.len()];
                next.set(next.get() + 1);
                black_box(one_call(role, thesaurus, items, query))
            });
        });
    }
    group.finish();
}

fn main() {
    let dir = fixture_dir();
    let fixture = load_fixture(&dir).expect("committed fixture must load");
    let thesaurus =
        load_thesaurus(&dir.join(THESAURUS_FILE)).expect("committed thesaurus must load");
    let role = RoleName::new(ROLE_NAME);
    let queries = concept_queries(&fixture, &thesaurus);
    assert!(
        !queries.is_empty(),
        "at least one fixture query must name a thesaurus concept"
    );

    // `cargo bench` always passes `--bench`; `cargo bench -- --test` passes
    // both, and `cargo test --all-targets` passes neither.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--bench") && !args.iter().any(|a| a == "--test") {
        custom_summary(&role, &thesaurus, &fixture.items, &queries);
    }

    let mut criterion = Criterion::default().configure_from_args();
    bench_retrieve(&mut criterion, &role, &thesaurus, &fixture.items, &queries);
    criterion.final_summary();
}
