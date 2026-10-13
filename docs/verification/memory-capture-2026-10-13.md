# Authored memory capture verification

The public v1.21.19 agent rejected `--content`, ignored lesson text on stdin and
saved only provenance placeholder text. A positive role-graph query consequently
returned zero memory items. A three-test regression run against the pre-fix code
failed as expected. Tests now require actual input and the explicit data-root
store, so the placeholder behavior cannot satisfy the successful capture cases.

The fix adds required `--content` or UTF-8 `--file`, rejects blank/unreadable/
conflicting/oversized input, and preserves the supplied text exactly. It reuses
`MemoryItem` and the existing evolution crate. `TERRAPHIM_DEFAULT_DATA_PATH`
selects the evolution root; without it the historical location is unchanged.

Validation on macOS ARM64 using the CI-pinned Rust 1.96.0:

- 12 CLI tests passed across `memory_capture_cli_tests`, `memory_apply_cli_tests`
  and `memory_rubric_cli_tests`.
- Invalid UTF-8, blank and >1 MB files leave an existing store byte-identical.
- `cargo clippy -p terraphim_agent --all-targets -- -D warnings` passed.
- `cargo fmt --all -- --check` and `git diff --check` passed.
- Independent execution recovered the exact public lesson through a fresh
  process and role graph. Positive query: one item; unrelated query, other role
  and independent empty store: zero. Unicode content was preserved.

Independent code review found no blocking defect; the requested documentation
and invalid-file tests were added. The initial Mac-default Rust 1.99 lint run
failed on unrelated async-trait `double_must_use` diagnostics in sessions; the
repository's pinned 1.96.0 lint gate passed without suppressing those lints.

This is candidate verification, not released-binary acceptance. After CI and
release production, download v1.21.20, verify its published checksum and repeat
the demonstration before publishing the website article/video.

The remaining `memory distill`, `memory provenance` and confidentiality-check
stubs are not qualified by this scoped fix or advertised as working in its
walkthrough. Role-based retrieval is not an access-control boundary.
