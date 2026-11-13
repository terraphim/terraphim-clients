//! Unified hooks infrastructure for Terraphim AI.
//!
//! This crate provides shared functionality for Claude Code hooks and Git hooks,
//! including text replacement via knowledge graphs and binary discovery utilities.

// The workspace is a forbid-soundness project: no `unsafe` in the library or
// binary target of this crate (terraphim-clients#447, #310). The integration
// tests under `tests/` are separate crates and keep their own, listed
// exceptions - see the `no_new_unsafe_sites` guard.
#![forbid(unsafe_code)]

mod discovery;
mod replacement;
mod validation;
mod validation_types;

pub use discovery::{BinaryLocation, discover_binary};
pub use replacement::{HookResult, LinkType, ReplacementService};
pub use validation::ValidationService;
pub use validation_types::{ValidationError, ValidationOutcome, ValidationResult};

/// Re-export key types from terraphim_automata for convenience.
pub use terraphim_automata::Matched;
pub use terraphim_types::Thesaurus;
