//! Generator-aware tier resolution (Refs #193).
//!
//! `TierResolver::resolve` returns the final tier, model, and family to
//! use for an evaluation, with an optional `swapped_for_bias` describing
//! any same-family swap. The resolver is pure: it takes a parsed
//! `ModelMapping` and a tier name, walks the tier's `fallback` chain
//! at most once per step, and returns a `TierResolution`. No I/O.
//!
//! The parent #192 (native judge subcommand) will compose this with
//! prompt-building, LLM dispatch, and the verdict JSONL emit.

use std::collections::BTreeMap;

use serde::Serialize;

use super::family::ModelFamily;

/// Errors from `TierResolver::resolve`.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ResolveError {
    /// The tier name is not present in the model mapping.
    #[error("unknown tier: {name:?}")]
    UnknownTier { name: String },
}

/// Which CLI the tier dispatches to (Refs `dispatch.ts`). The parent
/// #192 uses this to choose between `opencode`, `claude`, and `curl`
/// subprocesses; #193 only needs to carry the field through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CliKind {
    Opencode,
    Claude,
    Curl,
}

/// One tier's config from `model-mapping.json` (Refs
/// `cto-executive-system/automation/judge/model-mapping.json`). Only
/// the fields the resolver needs are required; the parent #192 adds
/// `timeout_seconds`, `max_budget_usd`, `endpoint`, and `requires_env`.
#[derive(Debug, Clone)]
pub(crate) struct TierConfig {
    pub cli: CliKind,
    pub model: String,
    pub fallback: Option<String>,
}

/// The full `model-mapping.json` `tiers` map. The resolver only needs
/// the tier names and their `TierConfig`s; the parent #192 will hold
/// the rest of the mapping.
#[derive(Debug, Clone, Default)]
pub(crate) struct ModelMapping {
    pub tiers: BTreeMap<String, TierConfig>,
}

/// Description of a same-family swap (Refs #193, "swapped_for_bias").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct SwapInfo {
    pub from_tier: String,
    pub to_tier: String,
}

/// Outcome of resolving a tier for a given generator. The parent #192
/// serialises these fields into the verdict JSONL (alongside the
/// generator's own `generator_model` and `generator_family`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TierResolution {
    /// Final tier name (the resolved tier, after any swap).
    pub tier: String,
    /// Final model identifier (post-swap).
    pub model: String,
    /// Family of the final model. Useful for the parent #192's lane
    /// hierarchy checks.
    pub family: ModelFamily,
    /// `"<from> -> <to>"` when a swap happened to avoid same-family
    /// bias; `None` when no swap was needed or no compatible alternative
    /// existed.
    pub swapped_for_bias: Option<SwapInfo>,
}

impl TierResolution {
    /// String form of `swapped_for_bias` for the verdict JSONL emit
    /// (`None` becomes a JSON `null`).
    pub fn swapped_field(&self) -> Option<String> {
        self.swapped_for_bias
            .as_ref()
            .map(|s| format!("{} -> {}", s.from_tier, s.to_tier))
    }
}

/// Resolves a tier to evaluate, with same-family swap avoidance.
///
/// The resolver is stateless; it can be reused across calls.
#[derive(Debug, Default, Clone)]
pub(crate) struct TierResolver;

impl TierResolver {
    pub fn new() -> Self {
        Self
    }

    /// Resolve the tier to evaluate. If a generator model is provided
    /// and the resolved tier's model has the same family as the
    /// generator, the resolver walks the tier's `fallback` chain to
    /// find a tier whose model is in a **different** family. The
    /// chain is bounded by the number of tiers in the mapping
    /// (visiting a tier twice is treated as no compatible
    /// alternative and yields the original tier with
    /// `swapped_for_bias: None`).
    ///
    /// An unrecognised generator (family `Unknown`) is treated as
    /// "no bias risk known" — no swap is performed, and the
    /// original tier is returned.
    pub fn resolve(
        &self,
        mapping: &ModelMapping,
        tier: &str,
        generator: Option<&str>,
    ) -> Result<TierResolution, ResolveError> {
        let cfg = mapping
            .tiers
            .get(tier)
            .ok_or_else(|| ResolveError::UnknownTier {
                name: tier.to_string(),
            })?;

        // No generator: return the original tier as-is.
        let Some(generator) = generator else {
            return Ok(TierResolution {
                tier: tier.to_string(),
                model: cfg.model.clone(),
                family: ModelFamily::from_model(&cfg.model),
                swapped_for_bias: None,
            });
        };

        let gen_family = ModelFamily::from_model(generator);
        let tier_family = ModelFamily::from_model(&cfg.model);

        // Different family (or generator family unknown): no swap.
        if gen_family == ModelFamily::Unknown || tier_family != gen_family {
            return Ok(TierResolution {
                tier: tier.to_string(),
                model: cfg.model.clone(),
                family: tier_family,
                swapped_for_bias: None,
            });
        }

        // Same family: walk the fallback chain, returning the first
        // tier whose model is in a DIFFERENT family. If every fallback
        // in the chain is same-family (or the chain is exhausted),
        // keep the original tier with `swapped_for_bias: None` so
        // consumers can distinguish "no swap needed" from "swap
        // attempted but no alternative". The `visited` set bounds
        // the walk against cycles.
        let original_tier = tier.to_string();
        let original_model = cfg.model.clone();
        let original_family = tier_family;
        let mut current_tier = original_tier.clone();
        let mut visited = std::collections::BTreeSet::from([current_tier.clone()]);
        let result = loop {
            let current_cfg = match mapping.tiers.get(&current_tier) {
                Some(c) => c,
                None => break None,
            };
            let Some(fallback) = current_cfg.fallback.clone() else {
                break None;
            };
            if !visited.insert(fallback.clone()) {
                break None;
            }
            let fallback_cfg = match mapping.tiers.get(&fallback) {
                Some(c) => c,
                None => break None,
            };
            let fallback_family = ModelFamily::from_model(&fallback_cfg.model);
            if fallback_family != gen_family {
                break Some(TierResolution {
                    tier: fallback.clone(),
                    model: fallback_cfg.model.clone(),
                    family: fallback_family,
                    swapped_for_bias: Some(SwapInfo {
                        from_tier: original_tier.clone(),
                        to_tier: fallback.clone(),
                    }),
                });
            }
            // Fallback is same family too; continue walking the chain.
            current_tier = fallback;
        };
        Ok(result.unwrap_or(TierResolution {
            tier: original_tier,
            model: original_model,
            family: original_family,
            swapped_for_bias: None,
        }))
    }
}
