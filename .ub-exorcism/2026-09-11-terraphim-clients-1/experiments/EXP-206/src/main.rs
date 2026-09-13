// EXP-206: crafted SQLite database into bundled SQLite 3.46.0 (rusqlite 0.32,
// libsqlite3-sys 0.30.1, FTS5 enabled) through the exact Cursor-connector path:
//   Connection::open(path) then a fixed SELECT over cursorDiskKV / ItemTable.
// (crates/terraphim_sessions/src/connector/cursor.rs:163;
//  crates/terraphim-session-analyzer/src/connectors/cursor.rs:156)
//
// Reads every *.db / *.vscdb / *.sqlite file under $CRAFTED_DB_DIR, opens it and
// runs the two connector queries. A clean SQLITE_CORRUPT / SQLITE_NOTADB error is
// the refutation; an ASan heap-overflow/OOB inside sqlite3 is CONFIRMED_UB.

use rusqlite::Connection;
use std::path::Path;

fn drive(path: &Path) {
    print!("--- {} : ", path.display());
    // integrity: open then run the two exact connector SELECTs.
    let conn = match Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    ) {
        Ok(c) => c,
        Err(e) => {
            println!("open err: {e}");
            return;
        }
    };

    // Cursor connector query 1 (cursorDiskKV composerData rows).
    let q1 = conn
        .prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%'")
        .and_then(|mut stmt| {
            let it =
                stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
            let mut n = 0usize;
            let mut bytes = 0usize;
            for row in it {
                let (k, v) = row?;
                n += 1;
                bytes += k.len() + v.len();
            }
            Ok((n, bytes))
        });

    // Cursor connector query 2 (ItemTable, second connector).
    let q2 = conn
        .prepare("SELECT key, value FROM ItemTable")
        .and_then(|mut stmt| {
            let it =
                stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
            let mut n = 0usize;
            for row in it {
                let _ = row?;
                n += 1;
            }
            Ok(n)
        });

    // Also run PRAGMA integrity_check, which walks the btree pages in C.
    let ic: Result<String, _> =
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0));

    match (q1, q2, ic) {
        (r1, r2, r3) => {
            println!(
                "q1={} q2={} integrity_check={}",
                match r1 {
                    Ok((n, b)) => format!("{n} rows/{b} bytes"),
                    Err(e) => format!("err({e})"),
                },
                match r2 {
                    Ok(n) => format!("{n} rows"),
                    Err(e) => format!("err({e})"),
                },
                match r3 {
                    Ok(s) => format!("{:?}", &s[..s.len().min(40)]),
                    Err(e) => format!("err({e})"),
                },
            );
        }
    }
}

fn main() {
    println!(
        "EXP-206 sqlite version: {}",
        rusqlite::version()
    );
    let dir = std::env::var("CRAFTED_DB_DIR").unwrap_or_else(|_| {
        // single-file fallback matching the block's invocation
        std::env::var("CRAFTED_DB").expect("set CRAFTED_DB_DIR or CRAFTED_DB")
    });
    let p = Path::new(&dir);
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if p.is_dir() {
        for e in std::fs::read_dir(p).expect("read_dir") {
            let e = e.unwrap();
            let path = e.path();
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if matches!(ext, "db" | "vscdb" | "sqlite" | "corrupt") {
                    files.push(path);
                }
            }
        }
        files.sort();
    } else {
        files.push(p.to_path_buf());
    }
    println!("EXP-206 driving {} database file(s)", files.len());
    for f in &files {
        drive(f);
    }
    println!("EXP-206 done: inspect ASan for a libsqlite3 page/btree/FTS5 fault");
}
