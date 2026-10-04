//! Native judge subcommand scaffolding (Refs #192).
//!
//! The full `terraphim-agent judge <files...>` subcommand, panel and
//! escalation modes, LLM dispatch, and REPL command are follow-on slices
//! under #192. This module ships the first slice: the `ModelFamily` enum
//! and prefix parser, the generator-aware tier resolver, and the
//! `VerdictMeta` extension that records `generator_model /
//! generator_family / swapped_for_bias` in the verdict JSONL. Refs #193.
//
// The re-exports and inner types are "dead" from the bin-target's
//! perspective until the parent #192 subcommand lands. The test target
//! consumes them; the workspace `cargo clippy --workspace
//! --all-targets -D warnings` (the native-ci gate) sees the test target
//! and is therefore clean. The `#[allow(...)]` suppresses the bin-only
//! noise; the items become live on the parent #192 PR.
#![allow(dead_code, unused_imports)]

mod family;
mod tier;
mod verdict_meta;

use crate::judge::family::{BANNED_MODEL_PREFIXES, BannedModelPrefixes, ModelFamily};
use crate::judge::tier::{
    CliKind, ModelMapping, ResolveError, SwapInfo, TierConfig, TierResolution, TierResolver,
};
use crate::judge::verdict_meta::VerdictMeta;

#[cfg(test)]
mod tests;
