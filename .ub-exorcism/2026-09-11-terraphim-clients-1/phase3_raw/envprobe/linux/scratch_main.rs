// Scratch reproduction for F-002: one writer thread mutates the process
// environment the way terraphim_test_utils::EnvVarGuard does (set_var on
// construction, set_var/remove_var on Drop) while sibling threads open
// rusqlite connections, whose first open runs sqlite's unixTempFileInit
// (getenv("SQLITE_TMPDIR"), getenv("TMPDIR")) outside std's ENV_LOCK.
// No synchronisation between the threads on purpose: this mirrors the
// mixed #[serial] / non-serial layout of tui_service_tests.
use std::sync::{Arc, Barrier};
use std::thread;

fn main() {
    let readers: usize = std::env::var("SCRATCH_READERS").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let writes: usize = std::env::var("SCRATCH_WRITES").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
    let dir = std::env::temp_dir().join(format!("ubx-probe-scratch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let barrier = Arc::new(Barrier::new(readers + 1));

    let mut handles = Vec::new();
    for i in 0..readers {
        let b = barrier.clone();
        let path = dir.join(format!("r{i}.db"));
        handles.push(thread::spawn(move || {
            b.wait();
            for _ in 0..3 {
                let c = rusqlite::Connection::open(&path).expect("open");
                c.execute_batch("CREATE TABLE IF NOT EXISTS t(x); INSERT INTO t VALUES(1);").unwrap();
                drop(c);
            }
        }));
    }
    let b = barrier.clone();
    let wdir = dir.clone();
    let writer = thread::spawn(move || {
        let dir = wdir;
        b.wait();
        for i in 0..writes {
            // EnvVarGuard::set equivalent
            unsafe { std::env::set_var("HOME", dir.display().to_string()); }
            unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.join("cfg").display().to_string()); }
            unsafe { std::env::set_var("TERRAPHIM_DATA_PATH", dir.join(format!("d{i}")).display().to_string()); }
            // Drop equivalent (restore / remove)
            unsafe { std::env::remove_var("TERRAPHIM_DATA_PATH"); }
            unsafe { std::env::remove_var("XDG_CONFIG_HOME"); }
        }
    });
    writer.join().unwrap();
    for h in handles { h.join().unwrap(); }
    let _ = std::fs::remove_dir_all(&dir);
    println!("scratch ok");
}
