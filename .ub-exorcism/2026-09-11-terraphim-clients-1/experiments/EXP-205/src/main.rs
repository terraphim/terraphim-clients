// EXP-205: hostile local git repository reaches bundled libgit2 (lockfile now
// libgit2-sys 0.18.8+1.9.7) through the exact workspace path:
//   FilePicker::new{watch:false, mode:Ai} -> collect_files ->
//   discover_git_workdir (git2::Repository::discover) -> spawn_git_status.
// (crates/terraphim_grep/src/hybrid_searcher.rs:354-385)
//
// Drives $HOSTILE_REPO (single) or, with `drive <parent>`, every child repo.
// Reports peak RSS delta around collect_files so an unbounded delta allocation
// shows up as an RSS spike or (under ASan) an allocation-size abort. A clean
// status with bounded RSS is the refutation (1.9.7 carries the mitigation).

use fff_search::{FFFMode, FilePicker, FilePickerOptions};
use std::path::{Path, PathBuf};

fn peak_rss_kb() -> i64 {
    // ru_maxrss (KiB on Linux) via getrusage(RUSAGE_SELF).
    #[repr(C)]
    #[derive(Default)]
    struct Timeval {
        tv_sec: i64,
        tv_usec: i64,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Rusage {
        ru_utime: Timeval,
        ru_stime: Timeval,
        ru_maxrss: i64,
        rest: [i64; 14],
    }
    extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
    let mut u = Rusage::default();
    unsafe {
        getrusage(0, &mut u);
    }
    u.ru_maxrss
}

fn drive_one(repo: &Path) {
    let before = peak_rss_kb();
    print!("--- {} : ", repo.display());
    let picker = FilePicker::new(FilePickerOptions {
        base_path: repo.to_string_lossy().to_string(),
        mode: FFFMode::Ai,
        watch: false,
        cache_budget: None,
        ..FilePickerOptions::default()
    });
    let mut picker = match picker {
        Ok(p) => p,
        Err(e) => {
            println!("FilePicker::new err: {e}");
            return;
        }
    };
    match picker.collect_files() {
        Ok(_) => {
            let n = picker.get_files().len();
            let after = peak_rss_kb();
            println!(
                "collected {n} files; peak_rss_before={before}KiB peak_rss_after={after}KiB delta={}KiB",
                after - before
            );
        }
        Err(e) => {
            let after = peak_rss_kb();
            println!("collect_files err: {e}; peak_rss_after={after}KiB");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("drive") {
        let parent = args.get(2).cloned().unwrap_or_else(|| {
            std::env::var("HOSTILE_PARENT").expect("drive <parent> or HOSTILE_PARENT")
        });
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(&parent)
            .expect("read_dir")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|d| d.is_dir())
            .collect();
        dirs.sort();
        println!("EXP-205 driving {} repo(s); baseline peak_rss={}KiB", dirs.len(), peak_rss_kb());
        for d in &dirs {
            drive_one(d);
        }
    } else {
        let repo = std::env::var("HOSTILE_REPO").expect("HOSTILE_REPO or `drive <parent>`");
        drive_one(Path::new(&repo));
    }
    println!("EXP-205 done: inspect ASan/RSS for libgit2 delta/config abuse");
}
