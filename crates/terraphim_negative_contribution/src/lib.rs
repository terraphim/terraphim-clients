//! Static detection of Explicit Deferral Markers (EDMs) in Rust source files.

// The workspace is a forbid-soundness project: no `unsafe` in the library or
// binary target of this crate (terraphim-clients#447, #310). The integration
// tests under `tests/` are separate crates and keep their own, listed
// exceptions - see the `no_new_unsafe_sites` guard.
#![forbid(unsafe_code)]

mod exclusion;
mod scanner;

pub use exclusion::is_non_production;
pub use scanner::NegativeContributionScanner;
