//! `VerdictMeta` — the three new verdict JSONL fields (Refs #193).
//!
//! The parent #192 (native judge subcommand) will own the full
//! `Verdict` struct (compatible with
//! `cto-executive-system/automation/judge/verdict-schema.json`). This
//! slice ships the three *additional* fields that #193 introduces
//! (`generator_model`, `generator_family`, `swapped_for_bias`) as a
//! separate, embeddable struct so the parent can `#[serde(flatten)]
//! VerdictMeta` into its full Verdict when it lands.
//!
//! The serialised shape (snake_case):
//! ```json
//! {
//!   "generator_model": "kimi-for-coding/kimi-for-coding",
//!   "generator_family": "moonshot",
//!   "swapped_for_bias": "deep -> deep_alt"  // or null
//! }
//! ```
//! These are *additional* to the existing verdict JSONL schema; they
//! do not replace any required field.

use serde::Serialize;

use super::family::ModelFamily;
use super::tier::SwapInfo;

/// The three new verdict fields (Refs #193).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct VerdictMeta {
    /// The model that produced the artefact being judged, when the
    /// judge is invoked as an evaluator of generated output (e.g. the
    /// `--generator` flag in `terraphim-agent judge --generator <m>`).
    /// `None` for standalone judge use (judge-only invocations).
    pub generator_model: Option<String>,
    /// Family of `generator_model` (convenience field so consumers do
    /// not have to re-parse the model string).
    pub generator_family: Option<ModelFamily>,
    /// `"<from_tier> -> <to_tier>"` when the tier resolver swapped to
    /// avoid same-family bias; `None` when no swap was needed or no
    /// compatible alternative existed. String form via
    /// `SwapInfo::serialise` to keep the JSONL field a single string
    /// per the issue's contract.
    pub swapped_for_bias: Option<SwapInfo>,
}

impl VerdictMeta {
    /// Build a verdict-meta from the resolver's outcome plus the
    /// optional generator model string. The family and the swap
    /// string are derived from the resolver output so callers do not
    /// have to thread them separately.
    pub fn from_resolver(
        generator: Option<&str>,
        resolution: &super::tier::TierResolution,
    ) -> Self {
        Self {
            generator_model: generator.map(str::to_string),
            generator_family: generator.map(ModelFamily::from_model),
            swapped_for_bias: resolution.swapped_for_bias.clone(),
        }
    }
}
