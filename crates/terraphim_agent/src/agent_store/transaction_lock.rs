//! Transaction ownership must end before inherited descriptors are closed.
use std::fs::{File, TryLockError};

#[derive(Debug)]
pub(crate) struct TransactionLock {
    file: File,
}
impl TransactionLock {
    pub(super) fn acquire(file: File) -> Result<Self, TryLockError> {
        file.try_lock()?;
        Ok(Self { file })
    }
}

impl Drop for TransactionLock {
    fn drop(&mut self) {
        // Closing this descriptor alone can leave an inherited/duplicated open
        // description holding the lock. End ownership explicitly before close.
        // Drop cannot report an unlock error; closing the file remains the fallback.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::OpenOptions,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            Self(std::env::temp_dir().join(format!(
                "terraphim-transaction-lock-{}-{}-{}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
        fn create(&self) -> File {
            OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .open(&self.0)
                .unwrap()
        }
        fn reopen(&self) -> File {
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.0)
                .unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn completed_transaction_releases_lock_while_duplicate_is_alive() {
        let fixture = Fixture::new();
        let transaction = TransactionLock::acquire(fixture.create()).unwrap();
        let duplicate = transaction.file.try_clone().unwrap();
        assert!(TransactionLock::acquire(fixture.reopen()).is_err());
        drop(transaction);
        let next = TransactionLock::acquire(fixture.reopen());
        drop(duplicate);
        assert!(
            next.is_ok(),
            "completed transaction must release its lock: {next:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn completed_transaction_releases_lock_while_forked_child_is_alive() {
        use std::{
            io::Write,
            os::{fd::AsRawFd, unix::net::UnixStream},
        };
        unsafe extern "C" {
            fn fork() -> i32;
            fn read(fd: i32, buf: *mut std::ffi::c_void, count: usize) -> isize;
            fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
            fn _exit(status: i32) -> !;
        }
        let fixture = Fixture::new();
        let transaction = TransactionLock::acquire(fixture.create()).unwrap();
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
        let next = TransactionLock::acquire(fixture.reopen());
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
}
