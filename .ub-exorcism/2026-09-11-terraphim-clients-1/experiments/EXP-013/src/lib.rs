//! EXP-013: the sync `terraphim_types::shared_learning::LearningStore` impl on
//! `SharedLearningStore` (store.rs:608-612 block_on bridge) called from a
//! current_thread runtime and from no runtime at all.
#[cfg(test)]
mod tests {
    use terraphim_agent::shared_learning::{SharedLearningStore, StoreConfig};
    use terraphim_types::shared_learning::LearningStore;

    async fn open_store() -> SharedLearningStore {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = StoreConfig::default();
        cfg.markdown.learnings_dir = dir.path().to_path_buf();
        std::mem::forget(dir);
        SharedLearningStore::open(cfg).await.unwrap()
    }

    // bare #[tokio::test] is the current_thread flavour
    #[tokio::test]
    #[should_panic(expected = "can call blocking only when running on the multi-threaded runtime")]
    async fn sync_trait_panics_on_current_thread() {
        let store = open_store().await;
        let dyn_store: &dyn LearningStore = &store;
        let _ = dyn_store.get("missing");
    }

    #[test]
    #[should_panic(expected = "there is no reactor running")]
    fn sync_trait_panics_without_runtime() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let store = rt.block_on(open_store());
        drop(rt);
        let dyn_store: &dyn LearningStore = &store;
        let _ = dyn_store.get("missing");
    }

    // positive control: block_in_place on a multi_thread runtime with a single
    // worker; ten concurrent sync calls from spawned tasks must all complete
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn sync_trait_completes_on_multi_thread_single_worker() {
        let store = std::sync::Arc::new(open_store().await);
        let mut handles = Vec::new();
        for i in 0..10 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let dyn_store: &dyn LearningStore = &*s;
                let r = dyn_store.get(&format!("missing-{i}"));
                assert!(r.is_err(), "missing id must be Err, not Ok");
            }));
        }
        for h in handles { h.await.expect("task completed without panic"); }
    }
}
