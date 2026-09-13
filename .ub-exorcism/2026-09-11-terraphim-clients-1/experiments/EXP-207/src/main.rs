// EXP-207: FFF_FRECENCY_PATH points the MCP server's heed/LMDB unsafe open at a
// hostile data.mdb. Drives the exact workspace path:
//   FrecencyTracker::open(dir) -> open_env (unsafe EnvOpenOptions::open, mmap +
//   meta-page read) -> open_database_safe.
// (crates/terraphim_mcp_server/src/lib.rs:109-118; fff-search dbs/lmdb.rs:154)
//
// Modes:
//   init  <dir>            build a valid populated frecency env (for the generator)
//   drive <corpus_parent>  for each child dir: open (primary, workspace surface)
//                          then an access_count lookup (extended read arm), catch
//                          errors, rely on ASan for an LMDB page-pointer fault.
//
// A clean heed::Error (LMDB rejecting the file) is the refutation; an ASan OOB /
// wild-read inside lmdb/mdb_* is CONFIRMED_UB.

use fff_search::FrecencyTracker;
use std::path::{Path, PathBuf};

fn init(dir: &str) {
    let t = FrecencyTracker::open(dir).expect("open valid env");
    for i in 0..500 {
        let p = PathBuf::from(format!("/some/workspace/file_{i}.rs"));
        t.track_access(&p).expect("track_access");
        if i % 3 == 0 {
            let _ = t.track_access(&p); // extra timestamps -> multi-entry pages
        }
    }
    println!(
        "EXP-207 init: populated env at {dir}; access_count(file_0)={:?}",
        t.access_count(Path::new("/some/workspace/file_0.rs"))
    );
}

fn drive_one(dir: &Path) {
    print!("--- {} : ", dir.display());
    match FrecencyTracker::open(dir) {
        Ok(t) => {
            // extended read arm: force a page traversal (workspace opens-only, but
            // Falsifiability asks to read before scoring NO_EVIDENCE).
            let a = t.access_count(Path::new("/some/workspace/file_0.rs"));
            let b = t.seconds_since_last_access(Path::new("/some/workspace/file_1.rs"));
            let s = t.get_access_score(Path::new("/some/workspace/file_2.rs"), fff_search::FFFMode::Ai);
            println!("OPENED; access_count={a:?} last_access={b:?} score={s}");
        }
        Err(e) => println!("open rejected (refutation): {e}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("init") => {
            init(args.get(2).expect("init <dir>"));
            return;
        }
        Some("drive") => {
            let parent = args.get(2).cloned().unwrap_or_else(|| {
                std::env::var("FRECENCY_CORPUS").expect("drive <corpus_parent> or FRECENCY_CORPUS")
            });
            let p = Path::new(&parent);
            let mut dirs: Vec<PathBuf> = std::fs::read_dir(p)
                .expect("read_dir")
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|d| d.is_dir())
                .collect();
            dirs.sort();
            println!("EXP-207 driving {} crafted env dir(s)", dirs.len());
            for d in &dirs {
                drive_one(d);
            }
            println!("EXP-207 done: inspect ASan for an LMDB page-pointer dereference fault");
        }
        _ => {
            // single-dir fallback matching the block's invocation
            let dir = std::env::var("FRECENCY_DIR").expect("FRECENCY_DIR or a mode arg");
            drive_one(Path::new(&dir));
            println!("EXP-207 done: inspect ASan for an LMDB page-pointer dereference fault");
        }
    }
}
