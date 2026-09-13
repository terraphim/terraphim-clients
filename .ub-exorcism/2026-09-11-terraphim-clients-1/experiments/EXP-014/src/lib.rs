//! EXP-014: does #[serial] on the env-writing test alone keep unannotated
//! sibling tests from running concurrently with the write? The writer holds a
//! terraphim_test_utils::EnvVarGuard (the registry wrapper's own rule) for
//! 300 ms while a shared AtomicBool says "writer active"; siblings poll it.
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
    use std::time::{Duration, Instant};
    static WRITER_ACTIVE: AtomicBool = AtomicBool::new(false);
    static OVERLAPS: AtomicUsize = AtomicUsize::new(0);
    static SIBLINGS_FINISHED: AtomicUsize = AtomicUsize::new(0);

    #[test]
    #[serial_test::serial]
    fn writer_is_serial() {
        let _g = terraphim_test_utils::EnvVarGuard::set("EXP014_VAR", "1");
        WRITER_ACTIVE.store(true, SeqCst);
        eprintln!("[exp014] writer: guard live at {:?}", Instant::now());
        std::thread::sleep(Duration::from_millis(300));
        WRITER_ACTIVE.store(false, SeqCst);
        eprintln!("[exp014] writer: guard released at {:?}", Instant::now());
    }

    fn sibling(name: &str) {
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(600) {
            if WRITER_ACTIVE.load(SeqCst) {
                OVERLAPS.fetch_add(1, SeqCst);
                eprintln!("[exp014] {name}: observed writer active at {:?} (env EXP014_VAR={:?})", Instant::now(), std::env::var("EXP014_VAR"));
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        SIBLINGS_FINISHED.fetch_add(1, SeqCst);
    }
    #[test] fn sibling_a() { sibling("sibling_a") }
    #[test] fn sibling_b() { sibling("sibling_b") }
    #[test] fn sibling_c() { sibling("sibling_c") }

    #[test]
    fn zz_report() {
        std::thread::sleep(Duration::from_millis(900));
        let o = OVERLAPS.load(SeqCst);
        eprintln!("[exp014] report: overlaps={o} siblings_finished={}", SIBLINGS_FINISHED.load(SeqCst));
        assert!(o > 0, "EXP-014 NO_EVIDENCE: siblings never ran during the serial writer (overlaps=0)");
    }
}
