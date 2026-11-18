#[path = "../src/agent_store/transaction_lock.rs"]
mod transaction_lock;

#[cfg(unix)]
#[test]
fn completed_transaction_releases_lock_while_forked_child_is_alive() {
    use std::{
        fs::OpenOptions,
        io::Write,
        os::{fd::AsRawFd, unix::net::UnixStream},
    };
    use transaction_lock::TransactionLock;

    unsafe extern "C" {
        fn fork() -> i32;
        fn read(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
        fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
        fn _exit(status: i32) -> !;
    }

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("transaction.lock");
    let transaction = TransactionLock::acquire(
        OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap(),
    )
    .unwrap();
    let (mut parent, child) = UnixStream::pair().unwrap();
    let child_fd = child.as_raw_fd();
    // SAFETY: after fork the child uses only async-signal-safe POSIX calls
    // and _exit, never Rust allocation, locks or destructors. The live
    // stream descriptor and byte buffer remain valid until _exit.
    let pid = unsafe { fork() };
    assert!(pid >= 0);
    if pid == 0 {
        let mut byte = 0_u8;
        unsafe {
            read(child_fd, (&mut byte as *mut u8).cast(), 1);
            _exit(0)
        }
    }
    drop(transaction);
    let next = TransactionLock::acquire(
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap(),
    );
    // Always reap the child before asserting, including a red regression.
    parent.write_all(&[1]).unwrap();
    let mut status = 0;
    // SAFETY: pid is this test's child; status points to a valid integer.
    assert_eq!(unsafe { waitpid(pid, &mut status, 0) }, pid);
    assert_eq!(status, 0);
    assert!(
        next.is_ok(),
        "child lifetime must not extend transaction ownership: {next:?}"
    );
}
