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
}
