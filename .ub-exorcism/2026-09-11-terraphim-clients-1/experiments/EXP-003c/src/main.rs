// EXP-003c: Miri matrix on terraphim_command_runtime.
//
// The crate has zero #[test] functions, so the Phase 3 `cargo miri test --lib`
// executed nothing ("running 0 tests") and the vacuous pass was recorded as
// NEEDS_REFINEMENT rather than as evidence. This harness supplies the missing
// test surface: it drives every public helper of
// crates/terraphim_command_runtime/src/lib.rs against a ConfigState built
// in-process, so Miri has real code to execute on all four axes.
//
// Covered: get_config, get_selected_role, list_roles_with_info,
// find_role_by_name_or_shortname (exact hit, case-insensitive hit, shortname
// hit, miss, empty query, non-ASCII query whose to_lowercase changes byte
// length).
//
// NOT covered: save_config. It calls Persistable::save, which writes to the
// configured persistence backends (sled / opendal / network profiles). That is
// real I/O against host state, not a property of this crate's own code, and it
// is out of scope for a UB probe; the helper is three lines and holds the same
// lock as get_config, which is covered.
//
// No executor is started: a noop-waker block_on drives the futures. The
// helpers only await tokio::sync::Mutex::lock, which is uncontended here and
// completes on the first poll, so no reactor is needed and Miri does not have
// to model a thread pool.
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use terraphim_config::{Config, ConfigState, Role};
use terraphim_types::RoleName;

struct NoopWaker;
impl Wake for NoopWaker {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWaker));
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::hint::spin_loop(),
        }
    }
}

fn role(name: &str, shortname: Option<&str>) -> Role {
    Role {
        shortname: shortname.map(|s| s.to_string()),
        name: RoleName::new(name),
        ..Default::default()
    }
}

fn state() -> ConfigState {
    let mut config = Config::default();
    config
        .roles
        .insert(RoleName::new("Terraphim Engineer"), role("Terraphim Engineer", Some("eng")));
    config
        .roles
        .insert(RoleName::new("Default"), role("Default", None));
    // A role whose name and shortname change byte length under to_lowercase
    // (U+0130 lowercases to two code points), which is the interesting input
    // for find_role_by_name_or_shortname's case-insensitive comparison.
    config
        .roles
        .insert(RoleName::new("\u{130}stanbul"), role("\u{130}stanbul", Some("\u{130}ST")));
    config.selected_role = RoleName::new("Terraphim Engineer");
    config.default_role = RoleName::new("Default");
    ConfigState {
        config: Arc::new(tokio::sync::Mutex::new(config)),
        roles: Default::default(),
    }
}

fn main() {
    let st = state();
    let mut checks = 0usize;

    // get_config: clones the whole Config out from under the lock.
    let cfg = block_on(terraphim_command_runtime::get_config(&st));
    assert_eq!(cfg.roles.len(), 3);
    assert_eq!(cfg.selected_role.to_string(), "Terraphim Engineer");
    checks += 1;
    println!("get_config: roles={} selected={}", cfg.roles.len(), cfg.selected_role);

    // get_selected_role.
    let sel = block_on(terraphim_command_runtime::get_selected_role(&st));
    assert_eq!(sel.to_string(), "Terraphim Engineer");
    checks += 1;
    println!("get_selected_role: {sel}");

    // list_roles_with_info: borrows every name and shortname out of the map.
    let mut listed = block_on(terraphim_command_runtime::list_roles_with_info(&st));
    listed.sort();
    assert_eq!(listed.len(), 3);
    checks += 1;
    println!("list_roles_with_info: {listed:?}");

    // find_role_by_name_or_shortname across the interesting query shapes.
    let queries: [(&str, Option<&str>); 9] = [
        ("Terraphim Engineer", Some("Terraphim Engineer")),
        ("terraphim engineer", Some("Terraphim Engineer")),
        ("TERRAPHIM ENGINEER", Some("Terraphim Engineer")),
        ("eng", Some("Terraphim Engineer")),
        ("ENG", Some("Terraphim Engineer")),
        ("Default", Some("Default")),
        ("nope", None),
        ("", None),
        ("\u{130}ST", Some("\u{130}stanbul")),
    ];
    for (q, expected) in queries {
        let got = block_on(terraphim_command_runtime::find_role_by_name_or_shortname(&st, q));
        let got_s = got.as_ref().map(|r| r.to_string());
        println!("find({q:?}) -> {got_s:?}");
        match expected {
            Some(e) => assert_eq!(got_s.as_deref(), Some(e), "query {q:?}"),
            // The U+0130 role may or may not match depending on to_lowercase
            // behaviour; only the pure-ASCII expectations are asserted as None.
            None => assert!(got_s.is_none(), "query {q:?} unexpectedly matched {got_s:?}"),
        }
        checks += 1;
    }

    // Shared ownership: two clones of the state take the lock in turn, so the
    // Arc refcount and the Mutex guard drop path both run more than once.
    let a = st.clone();
    let b = st.clone();
    let ra = block_on(terraphim_command_runtime::get_selected_role(&a));
    let rb = block_on(terraphim_command_runtime::get_config(&b));
    assert_eq!(ra, rb.selected_role);
    drop(a);
    let rc = block_on(terraphim_command_runtime::list_roles_with_info(&b));
    assert_eq!(rc.len(), 3);
    checks += 1;
    println!("clone/lock round trip: ok");

    println!("EXP-003c: {checks} checks passed over 4 of the 5 public helpers (save_config excluded: real persistence I/O)");
    println!("EXP-003c DONE: inspect the Miri log for any UB report");
}
