// EXP-201 (UF-040): hostile non-UTF-8 filenames inflate the fff-search relative
// path (stored lossily, U+FFFD = 3 bytes per invalid byte) past PATH_BUF_SIZE
// (4096). `ChunkedString::read_to_buf` (simd_path.rs:170-186) truncates at the
// buffer length and returns `from_utf8_unchecked(&buf[..total])`, so the cut can
// land inside a U+FFFD. `FileItem::absolute_path` (types.rs:271-275) is the
// public safe entry that grep uses (`get_content_for_search`, types.rs:676) and
// returns `base_path.join(rel)` built from that str.
//
//   exp201 [root]   root defaults to /tmp/ubx-probe-exp201; the hostile trees are
//                   created under it. Linux only (APFS rejects 0xFF names).
//
// Every string reachable through the public API is checked with
// std::str::from_utf8 on its raw bytes: FileItem::{relative_path, absolute_path,
// file_name, dir_str}, DirItem::{relative_path, dir_name, absolute_path}, and the
// grep result (`line_content`, `files[..].relative_path`). Options mirror
// crates/terraphim_grep/src/hybrid_searcher.rs:354-385.
use fff_search::{
    parse_grep_query, FFFMode, FilePicker, FilePickerOptions, GrepMode, GrepSearchOptions,
};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::{fs, panic};

const NEEDLE_LINE: &str = "needle inside a deep hostile tree\n";

struct Geometry {
    name: &'static str,
    depth: usize,
    comp_len: usize, // number of 0xFF bytes per directory component
}

fn build_tree(root: &Path, g: &Geometry) -> (PathBuf, usize) {
    let base = root.join(g.name);
    if base.exists() {
        fs::remove_dir_all(&base).expect("remove previous tree under the probe root");
    }
    fs::create_dir_all(&base).unwrap();
    let mut p = base.clone();
    for _ in 0..g.depth {
        p.push(OsStr::from_bytes(&vec![0xFFu8; g.comp_len]));
        fs::create_dir(&p).expect("create hostile dir (non-UTF-8 name)");
    }
    p.push("needle.txt");
    fs::write(&p, NEEDLE_LINE.repeat(2000)).unwrap(); // 68 KB, above MMAP_THRESHOLD on every arch
    // Inflated relative length: each component -> comp_len*3 bytes + '/', then "needle.txt".
    let inflated_rel = g.depth * (g.comp_len * 3 + 1) + "needle.txt".len();
    (base, inflated_rel)
}

fn check(label: &str, bytes: &[u8], bad: &mut Vec<String>) {
    if let Err(e) = std::str::from_utf8(bytes) {
        let tail: Vec<String> = bytes[bytes.len().saturating_sub(6)..].iter().map(|b| format!("{b:02x}")).collect();
        let msg = format!("{label}: INVALID UTF-8 ({e}); len={} tail={}", bytes.len(), tail.join(" "));
        println!("  {msg}");
        bad.push(msg);
    }
}

fn run_geometry(root: &Path, g: &Geometry) -> (usize, usize) {
    let (base, inflated_rel) = build_tree(root, g);
    let base_str = base.to_string_lossy().to_string();
    let abs_expected = base_str.len() + 1 + inflated_rel;
    println!(
        "== geometry {} depth={} comp_len={} base_len={} inflated_rel={} inflated_abs={} ({} PATH_BUF_SIZE=4096; cut residue mod 3 = {})",
        g.name, g.depth, g.comp_len, base_str.len(), inflated_rel, abs_expected,
        if abs_expected > 4096 { "exceeds" } else { "within" },
        if inflated_rel > 4096 { (4096 % (g.comp_len * 3 + 1)) % 3 } else { 0 }
    );
    // `FileItem::absolute_path` (types.rs:271) reads the relative path into its own
    // [u8; 4096], so the cut is at relative byte 4096; `write_absolute_path`
    // (types.rs:337) cuts at 4096 - base_len - 1 instead. Residue 0 = cut on a
    // U+FFFD boundary (valid), 1 or 2 = cut inside a U+FFFD (invalid).
    let mut bad: Vec<String> = Vec::new();
    let mut panics = 0usize;

    let mut picker = FilePicker::new(FilePickerOptions {
        base_path: base_str.clone(),
        mode: FFFMode::Ai,
        watch: false,
        cache_budget: None,
        ..FilePickerOptions::default()
    })
    .expect("picker");
    picker.collect_files().expect("collect_files");
    let files = picker.get_files();
    let dirs = picker.get_dirs();
    println!("  collected files={} dirs={}", files.len(), dirs.len());
    if files.is_empty() {
        println!("  VOID: the hostile file was not collected (walker filter?)");
    }

    for (i, f) in files.iter().enumerate() {
        let rel = f.relative_path(&picker);
        println!("  file[{i}] relative_path_len={} (stored byte_len={})", rel.len(), f.relative_path_len());
        check(&format!("file[{i}].relative_path"), rel.as_bytes(), &mut bad);
        check(&format!("file[{i}].file_name"), f.file_name(&picker).as_bytes(), &mut bad);
        check(&format!("file[{i}].dir_str"), f.dir_str(&picker).as_bytes(), &mut bad);
        let abs = f.absolute_path(&picker, &base);
        let ab = abs.as_os_str().as_bytes();
        println!(
            "  file[{i}] absolute_path bytes={} exists={} (expected untruncated {})",
            ab.len(), abs.exists(), abs_expected
        );
        check(&format!("file[{i}].absolute_path"), ab, &mut bad);
    }
    for (i, d) in dirs.iter().enumerate() {
        let rel = d.relative_path(&picker);
        check(&format!("dir[{i}].relative_path(len={})", rel.len()), rel.as_bytes(), &mut bad);
        let abs = d.absolute_path(&picker, &base);
        check(&format!("dir[{i}].absolute_path"), abs.as_os_str().as_bytes(), &mut bad);
        match panic::catch_unwind(panic::AssertUnwindSafe(|| d.dir_name(&picker))) {
            Ok(name) => check(&format!("dir[{i}].dir_name(len={})", name.len()), name.as_bytes(), &mut bad),
            Err(_) => {
                panics += 1;
                println!("  dir[{i}].dir_name PANICKED (rel len {}, last_segment_offset {})", rel.len(), d.last_segment_offset());
            }
        }
    }

    // Workspace grep path (hybrid_searcher.rs:371-385).
    let q = parse_grep_query("needle");
    let opts = GrepSearchOptions {
        max_file_size: 10 * 1024 * 1024,
        max_matches_per_file: 200,
        smart_case: true,
        file_offset: 0,
        page_limit: 100,
        mode: GrepMode::PlainText,
        time_budget_ms: 0,
        before_context: 0,
        after_context: 0,
        classify_definitions: false,
        ..GrepSearchOptions::default()
    };
    let r = picker.grep(&q, &opts);
    println!(
        "  grep: matches={} files={} total_files_searched={} total_files={} filtered_file_count={} files_with_matches={}",
        r.matches.len(), r.files.len(), r.total_files_searched, r.total_files, r.filtered_file_count, r.files_with_matches
    );
    for (i, m) in r.matches.iter().enumerate().take(200) {
        check(&format!("grep.matches[{i}].line_content"), m.line_content.as_bytes(), &mut bad);
    }
    for (i, f) in r.files.iter().enumerate() {
        check(&format!("grep.files[{i}].relative_path"), f.relative_path(&picker).as_bytes(), &mut bad);
    }
    println!("  geometry {}: invalid_strings={} panics={}", g.name, bad.len(), panics);
    (bad.len(), panics)
}

fn main() {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp/ubx-probe-exp201"));
    fs::create_dir_all(&root).unwrap();
    println!("EXP-201 root={} os={} arch={}", root.display(), std::env::consts::OS, std::env::consts::ARCH);
    let geoms = [
        Geometry { name: "control-2x250", depth: 2, comp_len: 250 }, // inflated ~1512 < 4096
        Geometry { name: "hostile-6x249", depth: 6, comp_len: 249 },
        Geometry { name: "hostile-6x250", depth: 6, comp_len: 250 },
        Geometry { name: "hostile-6x251", depth: 6, comp_len: 251 },
        Geometry { name: "hostile-3x255", depth: 3, comp_len: 255 }, // shortest tree that still crosses 4096 with a short base
        Geometry { name: "hostile-15x255", depth: 15, comp_len: 255 }, // raw path 35+15*256 < PATH_MAX; residue 2
        Geometry { name: "hostile-7x200", depth: 7, comp_len: 200 },   // residue 1
        Geometry { name: "hostile-14x100", depth: 14, comp_len: 100 }, // residue 0: cut on a U+FFFD boundary, expected clean
    ];
    let mut total_bad = 0;
    let mut total_panics = 0;
    let mut control_bad = 0;
    for g in &geoms {
        let (b, p) = run_geometry(&root, g);
        if g.name.starts_with("control") { control_bad += b; }
        total_bad += b;
        total_panics += p;
    }
    println!("EXP-201 totals: invalid_strings={total_bad} (control={control_bad}) dir_name_panics={total_panics}");
    if control_bad > 0 {
        println!("EXP-201 VOID: control tree produced invalid strings");
    } else if total_bad > 0 {
        println!("EXP-201 CONFIRMED: invalid-UTF-8 path string surfaced through the safe fff-search API");
    } else {
        println!("EXP-201 NO_EVIDENCE: no invalid path string observed");
    }
}
