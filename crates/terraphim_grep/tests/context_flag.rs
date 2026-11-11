//! Regression test for #471: `-C/--context` must widen each chunk to include the
//! surrounding lines and report the widened `line_start`/`line_end`.

use std::process::Command;

fn run(context: Option<&str>) -> serde_json::Value {
    let tmp = tempfile::tempdir().expect("tempdir");
    // `.rs`, not `.txt`: since #404 the `code` haystack is a source-extension
    // allowlist, so a text file would be excluded from the search entirely.
    std::fs::write(
        tmp.path().join("sample.rs"),
        "// alpha one\n// bravo two\n// charlie needle delta\n// echo four\n// foxtrot five\n",
    )
    .unwrap();
    let mut args = vec![
        "needle",
        "--search-only",
        "--haystack",
        "code",
        "-n",
        "5",
        "--json",
        "--paths",
        tmp.path().to_str().unwrap(),
    ];
    if let Some(c) = context {
        args.extend(["-C", c]);
    }
    let out = Command::new(env!("CARGO_BIN_EXE_terraphim-grep"))
        .args(&args)
        .output()
        .expect("run terraphim-grep");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("json")
}

#[test]
fn context_two_widens_chunk_to_whole_file() {
    let v = run(Some("2"));
    let c = &v["chunks"][0];
    assert_eq!(c["line_start"], 1);
    assert_eq!(c["line_end"], 5);
    assert_eq!(
        c["content"],
        "// alpha one\n// bravo two\n// charlie needle delta\n// echo four\n// foxtrot five"
    );
}

#[test]
fn context_one_is_clamped_to_neighbours() {
    let c = &run(Some("1"))["chunks"][0];
    assert_eq!(c["line_start"], 2);
    assert_eq!(c["line_end"], 4);
}

#[test]
fn context_zero_and_default_return_single_line() {
    for v in [run(Some("0")), run(None)] {
        let c = &v["chunks"][0];
        assert_eq!(c["line_start"], 3);
        assert_eq!(c["line_end"], 3);
        assert_eq!(c["content"], "// charlie needle delta");
    }
}
