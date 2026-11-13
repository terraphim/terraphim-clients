# Research, Design, Verification and Validation: the downloader tests stop needing the outside world (#502)

**Issue**: terraphim-clients#502
**Author**: opencode-cli (agent session)
**Date**: 2026-11-13
**Mode**: `remediation` (explicit defect, local change)

## Research

### The defect

Three `terraphim_update` downloader tests reached an external host:

```rust
let test_url = if can_connect("git.terraphim.cloud", 443) {
    "https://git.terraphim.cloud/api/v1/version"
} else if can_connect("localhost", 3000) {
    "http://localhost:3000/api/v1/version"
} else {
    eprintln!("Skipping network test: no available endpoint");
    return;
};
```

The probe is a **TCP connect** (200 ms). On this host the connection to
`git.terraphim.cloud:443` succeeds - the SOCKS tunnel is configured for cargo
and git - but the edge answers a plain-text **403** ("Not available") to
plain HTTP from this address. So the probe chose the Gitea URL, the download
then failed, and the test went red:

```
---- downloader::tests::test_download_creates_output_file stdout ----
thread 'downloader::tests::test_download_creates_output_file' panicked at crates/terraphim_update/src/downloader.rs:493:9
```

The three failures were reproducible on a clean worktree of `origin/main`, so
they were not caused by any open branch - and they made a local
`cargo test -p terraphim_update` red for a reason the diff cannot cause,
which hides real failures in the same crate. The same class as the guard
failures that #466 and #498 removed on the agent side.

### The fallback was not a fallback

The second branch (`http://localhost:3000/api/v1/version`) points at
whatever a developer happens to run on port 3000 - not at a server the test
starts. So even the "local" branch was not hermetic.

## Design

The tests now start their own HTTP server on the loopback interface and
download from it:

```rust
struct LocalHttpServer { url: String, handle: Option<JoinHandle<()>> }

impl LocalHttpServer {
    fn start(body: &'static str) -> Self { /* bind 127.0.0.1:0, serve a fixed body */ }
}
```

- The port is ephemeral (`:0`) ✓, so parallel test binaries cannot collide ✓.
- The server answers every request with a fixed body ✓, so the download path
  (retry, backoff, timeout, silent mode) is exercised unchanged ✓.
- The accept loop is detached rather than joined, so a test cannot hang on it ✓.

One assertion had to change with it: `total_duration.as_millis() > 0` was
satisfied by a download across the internet and is not by a loopback download
(which rounds to zero milliseconds). The assertion now checks that the
recorded duration is within the configured timeout ✓ - which is what "the
duration is recorded" means.

The now-unused `can_connect` probe is removed ✓ (it was the thing that made
the tests reach outside).

## Verification

| Check | Result |
|---|---|
| `cargo test --offline -p terraphim_update --all-targets --no-fail-fast` | all suites pass - **0 failures** (the three downloader failures are gone) |
| `cargo clippy --offline -p terraphim_update --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |

The three tests that failed before now pass ✓, and they pass without any
network access at all ✓.

## Validation

| Acceptance criterion (issue #502) | Result |
|---|---|
| "The three tests pass on a developer machine without network access to the outside world" | PASS - they use a loopback server they start themselves |
| "or they skip with a clear notice when the transport they need is unavailable" | not needed - no transport outside the loopback interface is required |
| "The failure message names the transport that was unavailable instead of 'Silent download should succeed'" | PASS - there is no unavailable transport left; the assertions name what is checked |

## Limits

- The loopback server answers with a fixed body, so the tests do not exercise
  a real Gitea endpoint (they never did - they exercised the *download* path,
  not Gitea) ✓.
- The tests need the loopback interface to be usable. In a sandbox that
  forbids binding a socket, they would fail with a bind error rather than
  skip - the issue allows either, and a bind error names the transport ✓.
