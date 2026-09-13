// EXP-104 (UF-039): fff-search maps files above MMAP_THRESHOLD during grep
// with the watcher disabled (the workspace configuration). A sibling thread
// rewrites the files in place while greps run.
//
//   exp104 rewrite <plain|fuzzy> <iterations>   same-length rewrite that flips
//        0xA9 (2nd byte of 'é') to 0xFF; every returned line_content is checked
//        with std::str::from_utf8. Fuzzy is the from_utf8_unchecked path
//        (grep.rs:1710); plain is the workspace path (from_utf8_lossy sink).
//   exp104 truncate <plain|fuzzy> <iterations>  writer alternates set_len(4096)
//        and full restore; run as a child so the parent records the signal.
//   exp104 child-truncate <plain|fuzzy> <iterations>   (internal)
//   exp104 control <plain|fuzzy> <iterations>   files below the threshold
//        (slow read path) with the same rewriter; must stay clean.
use fff_search::{FFFMode, FilePicker, FilePickerOptions, GrepMode, GrepSearchOptions, parse_grep_query};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::{fs, thread, time::Duration};

const N_FILES: usize = 4;
const LINE: &str = "needle caf\u{e9} line\n"; // 18 bytes, 'é' = C3 A9

fn make_content(lines: usize) -> Vec<u8> {
    LINE.repeat(lines).into_bytes()
}

fn write_files(dir: &Path, good: &[u8]) -> Vec<std::path::PathBuf> {
    (0..N_FILES)
        .map(|i| {
            let p = dir.join(format!("big{i}.txt"));
            fs::write(&p, good).unwrap();
            p
        })
        .collect()
}

fn picker_for(dir: &Path) -> FilePicker {
    // Same options as crates/terraphim_grep/src/hybrid_searcher.rs:354-359.
    let mut picker = FilePicker::new(FilePickerOptions {
        base_path: dir.to_string_lossy().to_string(),
        mode: FFFMode::Ai,
        watch: false,
        cache_budget: None,
        ..FilePickerOptions::default()
    })
    .expect("picker");
    picker.collect_files().expect("collect_files");
    assert_eq!(picker.get_files().len(), N_FILES, "all files collected");
    picker
}

fn options(mode: GrepMode) -> GrepSearchOptions {
    GrepSearchOptions {
        max_file_size: 10 * 1024 * 1024,
        max_matches_per_file: usize::MAX,
        smart_case: true,
        file_offset: 0,
        page_limit: usize::MAX,
        mode,
        time_budget_ms: 0,
        before_context: 0,
        after_context: 0,
        classify_definitions: false,
        ..GrepSearchOptions::default()
    }
}

fn parse_mode(s: &str) -> GrepMode {
    match s {
        "plain" => GrepMode::PlainText,
        "fuzzy" => GrepMode::Fuzzy,
        _ => panic!("mode must be plain|fuzzy"),
    }
}

fn vmmap_has(dir: &Path) -> String {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("vmmap").arg(&pid).output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            let needle = dir.file_name().unwrap().to_string_lossy().to_string();
            let hits: Vec<&str> = s.lines().filter(|l| l.contains(&needle)).collect();
            format!("vmmap: {} region(s) naming the temp dir; first: {:?}", hits.len(), hits.first())
        }
        Err(e) => format!("vmmap unavailable: {e}"),
    }
}

fn spawn_rewriter(paths: Vec<std::path::PathBuf>, good: Arc<Vec<u8>>, bad: Arc<Vec<u8>>, stop: Arc<AtomicBool>, writes: Arc<AtomicU64>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut i = 0u64;
        while !stop.load(Ordering::Relaxed) {
            for p in &paths {
                let mut f = fs::OpenOptions::new().write(true).open(p).unwrap();
                f.seek(SeekFrom::Start(0)).unwrap();
                f.write_all(if i % 2 == 0 { &bad } else { &good }).unwrap();
                writes.fetch_add(1, Ordering::Relaxed);
            }
            i += 1;
        }
    })
}

fn spawn_truncator(paths: Vec<std::path::PathBuf>, good: Arc<Vec<u8>>, stop: Arc<AtomicBool>, writes: Arc<AtomicU64>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            for p in &paths {
                let f = fs::OpenOptions::new().write(true).open(p).unwrap();
                f.set_len(4096).unwrap();
                writes.fetch_add(1, Ordering::Relaxed);
                thread::sleep(Duration::from_micros(200));
                let mut f = fs::OpenOptions::new().write(true).open(p).unwrap();
                f.seek(SeekFrom::Start(0)).unwrap();
                f.write_all(&good).unwrap();
            }
        }
    })
}

fn grep_loop(picker: &FilePicker, mode: GrepMode, iters: usize) -> (u64, u64, u64, Option<String>) {
    let q = parse_grep_query("needle");
    let opts = options(mode);
    let mut total_matches = 0u64;
    let mut violations = 0u64;
    let mut empty_results = 0u64;
    let mut first_bad: Option<String> = None;
    for _ in 0..iters {
        let r = picker.grep(&q, &opts);
        if r.matches.is_empty() {
            empty_results += 1;
        }
        for m in r.matches.iter() {
            total_matches += 1;
            if std::str::from_utf8(m.line_content.as_bytes()).is_err() {
                violations += 1;
                if first_bad.is_none() {
                    first_bad = Some(format!("{:?}", m.line_content.as_bytes()));
                }
            }
        }
    }
    (total_matches, violations, empty_results, first_bad)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arm = args.get(1).map(String::as_str).unwrap_or("rewrite");
    let mode_s = args.get(2).map(String::as_str).unwrap_or("fuzzy");
    let iters: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);
    let mode = parse_mode(mode_s);

    if arm == "truncate" {
        // Parent: run the child and report how it died.
        let exe = std::env::current_exe().unwrap();
        let st = std::process::Command::new(exe)
            .args(["child-truncate", mode_s, &iters.to_string()])
            .status()
            .expect("spawn child");
        use std::os::unix::process::ExitStatusExt;
        println!(
            "EXP-104 truncate/{mode_s}: child exit code={:?} signal={:?} (SIGBUS=10 SIGSEGV=11 on macOS)",
            st.code(),
            st.signal()
        );
        if st.signal() == Some(10) || st.signal() == Some(11) {
            println!("EXP-104 truncate/{mode_s}: CONFIRMED fault (mapped file shrunk under a live grep)");
        } else {
            println!("EXP-104 truncate/{mode_s}: NO fault observed");
        }
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let big = arm != "control";
    let lines = if big { 4 * 1024 * 1024 / LINE.len() } else { 8 * 1024 / LINE.len() }; // ~4 MiB or ~8 KiB (below the 16 KiB aarch64 threshold)
    let good = Arc::new(make_content(lines));
    let bad: Arc<Vec<u8>> = Arc::new(good.iter().map(|&b| if b == 0xA9 { 0xFF } else { b }).collect());
    assert!(std::str::from_utf8(&bad).is_err());
    let paths = write_files(dir.path(), &good);
    println!(
        "EXP-104 arm={arm} mode={mode_s} iters={iters} files={N_FILES} bytes/file={} (MMAP_THRESHOLD aarch64=16384)",
        good.len()
    );

    let picker = picker_for(dir.path());
    // Warm one grep, then confirm the mapping is live in this process.
    let (m0, v0, _, _) = grep_loop(&picker, mode, 1);
    println!("warm grep: {m0} matches, {v0} violations; {}", vmmap_has(dir.path()));
    let budget = picker.cache_budget();
    println!(
        "cache budget after warm grep: cached_count={} cached_bytes={}",
        budget.cached_count.load(Ordering::Relaxed),
        budget.cached_bytes.load(Ordering::Relaxed)
    );

    let stop = Arc::new(AtomicBool::new(false));
    let writes = Arc::new(AtomicU64::new(0));
    let writer = if arm == "child-truncate" {
        spawn_truncator(paths.clone(), good.clone(), stop.clone(), writes.clone())
    } else {
        spawn_rewriter(paths.clone(), good.clone(), bad.clone(), stop.clone(), writes.clone())
    };
    let started = std::time::Instant::now();
    let (matches, violations, empties, first_bad) = grep_loop(&picker, mode, iters);
    stop.store(true, Ordering::Relaxed);
    writer.join().unwrap();
    println!(
        "EXP-104 arm={arm} mode={mode_s}: iterations={iters} returned_lines={matches} invalid_utf8_lines={violations} empty_results={empties} writer_writes={} elapsed={:.1}s",
        writes.load(Ordering::Relaxed),
        started.elapsed().as_secs_f64()
    );
    if let Some(b) = first_bad {
        let shown: String = b.chars().take(160).collect();
        println!("first invalid line bytes: {shown}...");
    }
    if violations > 0 {
        println!("EXP-104 CONFIRMED: String with invalid UTF-8 returned from grep (arm={arm} mode={mode_s})");
    } else {
        println!("EXP-104 NO_EVIDENCE: no invalid UTF-8 observed (arm={arm} mode={mode_s})");
    }
    if arm == "child-truncate" {
        println!("child-truncate survived {iters} iterations without a fault");
    }
}
