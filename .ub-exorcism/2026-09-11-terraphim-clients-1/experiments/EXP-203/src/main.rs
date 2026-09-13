// EXP-203 (UF-042): crafted manifest version strings into semver 1.0.28's
// inline-storage `unsafe` (identifier.rs) under Miri, through a verbatim copy
// of terraphim_update's `is_newer_version_static` (lib.rs:52-62), which is what
// check_update_r2 (:264) and update_r2 (:312) call on the not-yet-verified
// manifest. The inline representation holds at most 8 bytes on 64-bit
// (PTR_BYTES = 8, TAIL_BYTES = 0), so the SSO boundary is 7/8/9 bytes, not
// 22/23/24. Beyond parse, every accepted version is compared, cloned,
// displayed and dropped so the heap repr's varint length header and dealloc
// path run too.
use semver::Version;

fn is_newer_version_static(version1: &str, version2: &str) -> Result<bool, anyhow::Error> {
    use semver::Version;

    let v1 = Version::parse(version1.trim_start_matches('v'))
        .map_err(|e| anyhow::anyhow!("Invalid version '{}': {}", version1, e))?;

    let v2 = Version::parse(version2.trim_start_matches('v'))
        .map_err(|e| anyhow::anyhow!("Invalid version '{}': {}", version2, e))?;

    Ok(v1 > v2)
}

const CURRENT: &str = "1.21.14"; // env!("CARGO_PKG_VERSION") of terraphim-agent at HEAD

fn inputs() -> Vec<String> {
    let mut v = Vec::new();
    // SSO boundary sweep: alphanumeric and numeric identifiers of length 1..=32 in pre-release and build.
    for n in 1..=32 {
        v.push(format!("1.21.15-{}", "a".repeat(n)));
        v.push(format!("1.21.15-{}", "1".repeat(n)));
        v.push(format!("1.21.15+{}", "b".repeat(n)));
        v.push(format!("1.21.15-{}+{}", "c".repeat(n), "d".repeat(n)));
        v.push(format!("1.21.15-{}", (0..n).map(|_| "x").collect::<Vec<_>>().join("."))); // many 1-byte segments
    }
    for n in [7usize, 8, 9, 22, 23, 24, 63, 64, 65, 127, 128, 129, 255, 256, 4096, 16384, 65536] {
        v.push(format!("1.21.15-{}", "p".repeat(n)));
        v.push(format!("1.21.15+{}", "q".repeat(n)));
        v.push(format!("1.21.15-{}", "0".repeat(n)));
        v.push(format!("1.21.15-1{}", "0".repeat(n)));
        v.push(format!("{}.0.0", "9".repeat(n)));
        v.push(format!("1.21.15-{}", (0..n).map(|i| if i % 2 == 0 { "a" } else { "1" }).collect::<Vec<_>>().join(".")));
    }
    for s in [
        "1.0.0-0.0.0.0.0.0.0.0.0.0.0", "1.0.0-01", "1.0.0-00", "1.0.0-0", "1.0.0+00", "1.0.0-", "1.0.0+", "1.0.0-+",
        "1.0.0-.", "1.0.0-a..b", "1.0.0-a.", "1.0.0-.a", "1.0.0--", "1.0.0-a-b.c-d+e-f.g-h",
        "1.0.0-\u{80}", "1.0.0-\u{130}", "1.0.0-é", "1.0.0+\u{ffff}", "1.0.0-a\u{0}b", "1.0.0-a b",
        "", "v", "vv1.0.0", "v1.21.15", "1", "1.0", "1.0.0.0", "18446744073709551615.0.0", "18446744073709551616.0.0",
        "1.0.0-18446744073709551616", "1.0.0-18446744073709551615", " 1.0.0", "1.0.0 ", "1.0.0\n", "01.0.0",
        "1.21.14", "1.21.13", "1.21.15", "1.21.14-alpha", "1.21.14+build", "1.21.14-alpha.1.beta.2+sha.abcdef0123456789",
    ] {
        v.push(s.to_string());
    }
    v.push(format!("1.0.0-{}", "\u{e9}".repeat(3000)));
    v.push(format!("1.0.0-{}", "a".repeat(6).chars().chain("\u{e9}".chars()).collect::<String>())); // non-ASCII at byte 7/8
    v.push(format!("1.0.0-{}", "a".repeat(7).chars().chain("\u{e9}".chars()).collect::<String>()));
    v.push(format!("1.0.0-{}", "a".repeat(8).chars().chain("\u{e9}".chars()).collect::<String>()));
    v.push(format!("1.0.0-{}", "a".repeat(2000).chars().chain("\u{e9}".chars()).collect::<String>()));
    v
}

fn main() {
    let ins = inputs();
    let (mut ok, mut err, mut panics, mut newer) = (0usize, 0usize, 0usize, 0usize);
    let mut kept: Vec<Version> = Vec::new();
    for s in &ins {
        let r = std::panic::catch_unwind(|| is_newer_version_static(s, CURRENT));
        match r {
            Ok(Ok(b)) => {
                ok += 1;
                if b { newer += 1; }
                let v = Version::parse(s.trim_start_matches('v')).unwrap();
                let c = v.clone();
                let shown = c.to_string();
                assert_eq!(shown, s.trim_start_matches('v'), "Display round trip");
                assert!(v == c && !(v < c) && !(v > c));
                assert_eq!(v.pre.as_str().len() + v.build.as_str().len() + 2 <= s.len() + 2, true);
                let _ = std::hint::black_box(v.pre.as_str());
                let _ = std::hint::black_box(v.build.as_str());
                kept.push(v); // dropped at the end so heap reprs are freed after clones died
                if kept.len() % 50 == 0 { kept.sort(); }
                drop(c);
            }
            Ok(Err(e)) => {
                err += 1;
                let m = e.to_string();
                if ins.len() <= 400 || s.len() < 80 { println!("err len={}: {}", s.len(), m.chars().take(100).collect::<String>()); }
            }
            Err(_) => { panics += 1; println!("PANIC (safe) len={}", s.len()); }
        }
    }
    // Also the reverse direction (manifest as `current`) and self-compare.
    for s in &ins {
        let _ = std::panic::catch_unwind(|| is_newer_version_static(CURRENT, s));
        let _ = std::panic::catch_unwind(|| is_newer_version_static(s, s));
    }
    kept.sort();
    kept.dedup();
    println!("EXP-203: inputs={} ok={} err={} panics={} newer_than_current={} kept_unique={}", ins.len(), ok, err, panics, newer, kept.len());
    println!("EXP-203 DONE: inspect the Miri log for any semver-frame UB report");
}
