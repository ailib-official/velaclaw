//! Host-local pricing table mirroring `prism-core::cost_router` (ORCH-HOST-002).
//!
//! The crates.io `prism-core-routing` package used by VelaClaw does not yet export
//! `cost_router`. This module keeps the same JSON shape and reason codes so host
//! Decide stays contract-aligned with Eos/Gateway until the package re-exports it.
//!
//! **Honesty (host embed):** without live `ProviderHealth` latency signals, `latency`
//! / `balanced` MUST NOT claim Eos-style `lowest_latency` / `balanced_score`. Use
//! stub reason codes and set `used_cost_router` only for real cost ranking.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
struct PricingFile {
    #[serde(default)]
    entries: Vec<PricingEntry>,
}

#[derive(Clone, Debug, Deserialize)]
struct EntryPricing {
    prompt_per_1m: f64,
    #[allow(dead_code)]
    completion_per_1m: f64,
    #[serde(default)]
    #[allow(dead_code)]
    reasoning_per_1m: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
struct PricingEntry {
    provider_id: String,
    model_id: String,
    #[serde(flatten)]
    pricing: EntryPricing,
}

#[derive(Clone, Debug, Default)]
pub struct PricingTable {
    by_provider_model: HashMap<(String, String), EntryPricing>,
}

impl PricingTable {
    pub fn from_json_str(json: &str) -> Result<Self, serde_json::Error> {
        let file: PricingFile = serde_json::from_str(json)?;
        let mut table = Self::default();
        for entry in file.entries {
            table
                .by_provider_model
                .insert((entry.provider_id, entry.model_id), entry.pricing);
        }
        Ok(table)
    }

    #[must_use]
    pub fn cost_per_1k_prompt(&self, provider_id: &str, model_id: &str) -> Option<f64> {
        self.by_provider_model
            .get(&(provider_id.to_string(), model_id.to_string()))
            .map(|p| p.prompt_per_1m / 1000.0)
    }
}

/// Result of a same-model multi-provider pick.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderPick {
    pub provider_id: String,
    pub estimated_cost_per_1k_prompt_usd: f64,
    pub reason: String,
    pub fallback: Vec<String>,
    /// True only when `optimize=cost` and at least one known price drove ranking.
    pub used_cost_router: bool,
}

/// Single-model provider pick (CostRouter-compatible where honesty allows).
#[must_use]
pub fn decide_providers_for_model(
    model_id: &str,
    providers: &[String],
    optimize: super::host_decide::OptimizeGoal,
    table: &PricingTable,
) -> Option<ProviderPick> {
    if providers.is_empty() {
        return None;
    }

    use super::host_decide::OptimizeGoal;

    match optimize {
        OptimizeGoal::Cost => {
            let mut scored: Vec<(String, f64)> = providers
                .iter()
                .map(|p| {
                    let cost = table.cost_per_1k_prompt(p, model_id).unwrap_or(f64::MAX);
                    (p.clone(), cost)
                })
                .collect();
            let any_priced = scored.iter().any(|(_, c)| *c != f64::MAX);
            if !any_priced {
                // No usable prices — let caller fall through to stub_rank.
                return None;
            }
            scored.sort_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
            let (provider_id, cost) = scored.first()?.clone();
            let fallback: Vec<String> = scored.into_iter().skip(1).map(|(p, _)| p).collect();
            Some(ProviderPick {
                provider_id,
                estimated_cost_per_1k_prompt_usd: if cost == f64::MAX { 0.0 } else { cost },
                reason: "lowest_cost".into(),
                fallback,
                used_cost_router: true,
            })
        }
        OptimizeGoal::Latency => {
            // No ProviderHealth on host embed — stable alphabetical stub only.
            let mut ranked = providers.to_vec();
            ranked.sort();
            let provider_id = ranked.first()?.clone();
            let est = table
                .cost_per_1k_prompt(&provider_id, model_id)
                .unwrap_or(0.0);
            let fallback: Vec<String> = ranked.into_iter().skip(1).collect();
            Some(ProviderPick {
                provider_id,
                estimated_cost_per_1k_prompt_usd: est,
                reason: "host_reachable_latency_stub".into(),
                fallback,
                used_cost_router: false,
            })
        }
        OptimizeGoal::Balanced => {
            // Contract keeps `balanced`, but host has no latency signal — do not
            // claim `balanced_score`. Cost-sort as a cheap stand-in when priced;
            // otherwise alphabetical stub.
            let mut scored: Vec<(String, f64)> = providers
                .iter()
                .map(|p| {
                    let cost = table.cost_per_1k_prompt(p, model_id).unwrap_or(f64::MAX);
                    (p.clone(), cost)
                })
                .collect();
            let any_priced = scored.iter().any(|(_, c)| *c != f64::MAX);
            if any_priced {
                scored.sort_by(|a, b| {
                    a.1.partial_cmp(&b.1)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.0.cmp(&b.0))
                });
            } else {
                scored.sort_by(|a, b| a.0.cmp(&b.0));
            }
            let (provider_id, cost) = scored.first()?.clone();
            let fallback: Vec<String> = scored.into_iter().skip(1).map(|(p, _)| p).collect();
            Some(ProviderPick {
                provider_id,
                estimated_cost_per_1k_prompt_usd: if cost == f64::MAX { 0.0 } else { cost },
                reason: "host_reachable_balanced_stub".into(),
                fallback,
                used_cost_router: false,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::host_decide::{load_embedded_pricing, OptimizeGoal};

    #[test]
    fn cost_pick_sets_used_cost_router_and_lowest_cost() {
        let table = load_embedded_pricing().expect("pricing");
        let pick = decide_providers_for_model(
            "llama-3.1-8b-instant",
            &["openai".into(), "groq".into()],
            OptimizeGoal::Cost,
            &table,
        )
        .expect("pick");
        assert_eq!(pick.provider_id, "groq");
        assert_eq!(pick.reason, "lowest_cost");
        assert!(pick.used_cost_router);
    }

    #[test]
    fn latency_pick_is_stub_not_lowest_latency() {
        let table = load_embedded_pricing().expect("pricing");
        let pick = decide_providers_for_model(
            "llama-3.1-8b-instant",
            &["openai".into(), "groq".into()],
            OptimizeGoal::Latency,
            &table,
        )
        .expect("pick");
        assert_eq!(pick.reason, "host_reachable_latency_stub");
        assert!(!pick.used_cost_router);
        assert_ne!(pick.reason, "lowest_latency");
        // Alphabetical: groq before openai
        assert_eq!(pick.provider_id, "groq");
    }

    #[test]
    fn balanced_pick_is_stub_not_balanced_score() {
        let table = load_embedded_pricing().expect("pricing");
        let pick = decide_providers_for_model(
            "llama-3.1-8b-instant",
            &["openai".into(), "groq".into()],
            OptimizeGoal::Balanced,
            &table,
        )
        .expect("pick");
        assert_eq!(pick.reason, "host_reachable_balanced_stub");
        assert!(!pick.used_cost_router);
        assert_ne!(pick.reason, "balanced_score");
    }

    #[test]
    fn cost_with_no_prices_returns_none() {
        let table = PricingTable::default();
        assert!(decide_providers_for_model(
            "any-model",
            &["a".into(), "b".into()],
            OptimizeGoal::Cost,
            &table,
        )
        .is_none());
    }
}
