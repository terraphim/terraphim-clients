# BUILD.md -- canonical CI command set (terraphim-clients)

This file is the authoritative command set for CI. Both the interim ADF
build-runner and the future native `terraphim-native` runner consume the bash
block below. `cargo build/clippy/test` are transformed to `rch exec -- ...`
by the knowledge graph (terraphim-agent replace); `cargo fmt` stays on the host.

## Build and test

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace --no-fail-fast
```

## Coverage (optional)

The first coverage lane runs under `nextest` so per-test process isolation is
preserved. `SSL_CERT_FILE` must point at a real CA bundle path on the runner
host (see gitea-infrastructure HANDOVER.md, 'Host Tooling'). Refs #313.

```bash
# terraphim-native (Gitea Actions)
cargo install cargo-llvm-cov --locked --root /usr/local
cargo install cargo-nextest --locked --root /usr/local
rustup component add llvm-tools-preview
TERRAPHIM_SERVER_BIN=/tmp/terraphim_server_install/bin/terraphim_server \
  cargo llvm-cov nextest --workspace --all-targets --no-fail-fast --lcov --output-path lcov.info

# ubuntu-latest (GitHub Actions)
cargo llvm-cov nextest --workspace --lib --no-fail-fast --lcov --output-path lcov.info
```
