//! `HybridSearcher::search_code` must refuse a hostile tree before fff-search
//! indexes it, and must still grep an ordinary tree normally.
//!
//! fff-search stores relative paths lossily, so each invalid byte in an entry
//! name becomes a three-byte U+FFFD. `ChunkedString::read_to_buf` then copies
//! `byte_len.min(buf.len())` bytes into a 4096-byte buffer and hands the result
//! to `core::str::from_utf8_unchecked`, so an inflated path is cut inside a
//! replacement character and the returned `&str` is not valid UTF-8. EXP-201
//! observed invalid-UTF-8 `PathBuf`/`str` values in five of seven hostile
//! geometries, plus `DirItem::dir_name` panicking with "start byte index 10724
//! is out of bounds for string of length 4096". See UF-040 / #296.
//!
//! Real directories throughout -- no mocks. The hostile cases only run on
//! Linux: APFS rejects non-UTF-8 entry names outright, so `create_dir` fails on
//! macOS before the guard is reached. They are `ignore`d rather than `cfg`'d out
//! so that their bodies still compile and lint on this workspace's macOS hosts.

use std::path::PathBuf;

use terraphim_grep::{GrepOptions, Haystack, HybridSearcher, reject_hostile_tree};
use terraphim_types::Thesaurus;

/// Build a tree of `depth` nested directories whose names are `len` repeats of
/// `byte`, returning the deepest path actually created.
#[cfg(unix)]
fn nested_dirs(root: &std::path::Path, byte: u8, len: usize, depth: usize) -> PathBuf {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let raw = vec![byte; len];
    let segment = OsStr::from_bytes(&raw);

    let mut dir = root.to_path_buf();
    for _ in 0..depth {
        dir = dir.join(segment);
        std::fs::create_dir(&dir).expect("create nested directory");
    }
    dir
}

/// A tree of six nested directories each named 250 invalid bytes is refused,
/// and the error names the offending entry.
///
/// 6 x 250 bytes is 1500 raw, which inflates to 4500 under lossy storage --
/// comfortably past fff-search's 4096-byte path buffer.
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "APFS refuses non-UTF-8 entry names, so the tree cannot be built here"
)]
#[cfg(unix)]
fn hostile_non_utf8_tree_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let deepest = nested_dirs(tmp.path(), 0xFF, 250, 6);
    std::fs::write(deepest.join("payload.rs"), "fn needle() {}\n").expect("write payload");

    let err = reject_hostile_tree(tmp.path()).expect_err("hostile tree must be refused");
    assert!(
        err.contains("not valid UTF-8"),
        "error must say why the tree was refused, got: {err}"
    );
    assert!(
        err.contains("\\xff") || err.contains("\\xFF"),
        "error must name the offending entry's bytes, got: {err}"
    );
}

/// The same tree must also be refused through the production entry point, not
/// only by the guard in isolation.
#[tokio::test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "APFS refuses non-UTF-8 entry names, so the tree cannot be built here"
)]
#[cfg(unix)]
async fn hostile_non_utf8_tree_is_refused_by_search() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let deepest = nested_dirs(tmp.path(), 0xFF, 250, 6);
    std::fs::write(deepest.join("payload.rs"), "fn needle() {}\n").expect("write payload");

    let searcher = HybridSearcher::new("Guard Test".to_string(), Thesaurus::new("empty".into()))
        .expect("HybridSearcher builds from an empty thesaurus")
        .with_search_path(tmp.path().to_path_buf());

    let err = searcher
        .search(
            "needle",
            &GrepOptions {
                haystack: Haystack::Code,
                max_results: 5,
                ..GrepOptions::default()
            },
        )
        .await
        .expect_err("search over a hostile tree must fail");

    assert!(
        err.contains("not valid UTF-8"),
        "search must surface the guard's reason, got: {err}"
    );
}

/// Files at or above fff-search's mmap threshold are exactly the ones the
/// zero-file budget diverts, so the needle must live in one.
///
/// `MMAP_THRESHOLD` is 16 KiB on aarch64 and 4 KiB elsewhere, and
/// `get_cached_content` returns `None` for anything smaller *before* it consults
/// the budget. A small file would therefore take the slow path with or without
/// the fix, and a control built on one proves nothing. 64 KiB clears both
/// thresholds and stays far below the 10 MiB `max_file_size`.
const OVER_MMAP_THRESHOLD: usize = 64 * 1024;

/// Control: an ordinary tree of valid ASCII names still greps normally, and
/// files large enough to have been mmapped still match.
///
/// This is the arm that catches the trap in the UF-039 remedy. Zeroing
/// `max_file_size` -- or using `ContentCacheBudget::zero()` -- would also close
/// `get_content_for_search`'s slow path, so grep would return no matches at all
/// and the mmap fix would look like it worked.
///
/// The large file is what makes the control meaningful. Its path through
/// fff-search is: `size >= MMAP_THRESHOLD`, so the threshold guard does not
/// bail; then `cached_count >= max_files` with `0 >= 0`, so `get_cached_content`
/// returns `None` without mapping; then the slow path reads it into an owned
/// buffer because `size <= max_file_size`; and the needle still matches. That is
/// precisely the behaviour #293 claims, and it is the assertion that would go
/// red for the right reason under a budget that also closed the slow path.
#[tokio::test]
async fn plain_tree_still_greps() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let nested = tmp.path().join("a".repeat(250)).join("b".repeat(250));
    std::fs::create_dir_all(&nested).expect("create control tree");

    // Small file: exercises the threshold guard's early return.
    std::fs::write(
        nested.join("payload.rs"),
        "fn guard_control_needle() { /* found */ }\n",
    )
    .expect("write small payload");

    // Large file: the geometry that used to be mmapped.
    let filler = "// padding so this file clears the mmap threshold\n";
    let mut big = String::with_capacity(OVER_MMAP_THRESHOLD + 128);
    while big.len() < OVER_MMAP_THRESHOLD {
        big.push_str(filler);
    }
    big.push_str("fn guard_control_needle_large() { /* found */ }\n");
    let big_path = nested.join("big_payload.rs");
    std::fs::write(&big_path, &big).expect("write large payload");
    assert!(
        std::fs::metadata(&big_path)
            .expect("stat large payload")
            .len()
            >= OVER_MMAP_THRESHOLD as u64,
        "the large payload must clear fff-search's mmap threshold for this control to mean anything"
    );

    assert!(
        reject_hostile_tree(tmp.path()).is_ok(),
        "a tree of valid ASCII names well inside the buffer must be accepted"
    );

    let searcher = HybridSearcher::new("Guard Test".to_string(), Thesaurus::new("empty".into()))
        .expect("HybridSearcher builds from an empty thesaurus")
        .with_search_path(tmp.path().to_path_buf());

    let results = searcher
        .search(
            "guard_control_needle",
            &GrepOptions {
                haystack: Haystack::Code,
                max_results: 50,
                ..GrepOptions::default()
            },
        )
        .await
        .expect("search over an ordinary tree must succeed");

    assert!(
        results
            .code_results
            .iter()
            .any(|c| c.content.contains("guard_control_needle()")),
        "grep must still return the match in the small file; a zero-file cache \
         budget must not close the slow read path. Got {} results: {:?}",
        results.code_results.len(),
        results.code_results,
    );

    assert!(
        results
            .code_results
            .iter()
            .any(|c| c.content.contains("guard_control_needle_large()")),
        "grep must still return the match in a file at or above fff-search's mmap \
         threshold -- that is the geometry the zero-file budget diverts to the slow \
         path, so this is the assertion #293 actually rests on. Got {} results: {:?}",
        results.code_results.len(),
        results.code_results,
    );
}
