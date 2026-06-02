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
