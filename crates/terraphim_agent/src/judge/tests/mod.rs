//! Unit tests for the native-judge scaffolding (Refs #193).
//!
//! Hermetic: no network, no live LLM. The opencode model snapshot is
//! a vendored fixture; the resolver tests build their own in-memory
//! mapping to stay deterministic and independent of the snapshot.

mod family_tests;
mod tier_tests;
mod verdict_meta_tests;
