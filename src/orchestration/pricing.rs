//! Host-local pricing table mirroring `prism-core::cost_router` (ORCH-HOST-002).
//!
//! The crates.io `prism-core-routing` package used by VelaClaw does not yet export
//! `cost_router`. This module keeps the same JSON shape and reason codes so host
//! Decide stays contract-aligned with Eos/Gateway until the package re-exports it.

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

/// Single-model provider pick (CostRouter-compatible reason codes).
#[must_use]
pub fn decide_providers_for_model(
    model_id: &str,
    providers: &[String],
    optimize: super::host_decide::OptimizeGoal,
    table: &PricingTable,
) -> Option<(String, f64, String, Vec<String>)> {
    if providers.is_empty() {
        return None;
    }
    let mut scored: Vec<(String, f64)> = providers
        .iter()
        .map(|p| {
            let cost = table.cost_per_1k_prompt(p, model_id).unwrap_or(f64::MAX);
            (p.clone(), cost)
        })
        .collect();

    match optimize {
        super::host_decide::OptimizeGoal::Cost | super::host_decide::OptimizeGoal::Balanced => {
            scored.sort_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| a.0.cmp(&b.0))
            });
        }
        super::host_decide::OptimizeGoal::Latency => {
            scored.sort_by(|a, b| a.0.cmp(&b.0));
        }
    }

    let (provider_id, cost) = scored.first()?.clone();
    let reason = match optimize {
        super::host_decide::OptimizeGoal::Cost => "lowest_cost",
        super::host_decide::OptimizeGoal::Latency => "lowest_latency",
        super::host_decide::OptimizeGoal::Balanced => "balanced_score",
    };
    let fallback: Vec<String> = scored.into_iter().skip(1).map(|(p, _)| p).collect();
    let est = if cost == f64::MAX { 0.0 } else { cost };
    Some((provider_id, est, reason.into(), fallback))
}
