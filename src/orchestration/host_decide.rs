//! Host-side Decide (ORCH-HOST-001/002/003) — CAP reachable ∩ pricing / stub.
//!
//! Matches Decide contract fields. Default-off via `[agent].host_decide`.
//! Pricing mirrors prism-core CostRouter JSON + reason codes (HOST-002).
//! ORCH-HOST-003: `model` is the wire key after `{provider_id}/` (may contain
//! `/`), aligned with [`crate::protocol_registry::compose_logical_model_id`].

use crate::capability_index::CapabilityCandidate;
use crate::orchestration::pricing::{decide_providers_for_model, PricingTable};
use serde::{Deserialize, Serialize};

/// Contract disclaimer — must match Gateway/Eos until production ACK.
pub const NOT_PRODUCTION_SLA: &str = "NOT_PRODUCTION_SLA";

/// Embedded example pricing (same shape as Eos prism-core fixture).
pub const EMBEDDED_PRICING_JSON: &str = include_str!("fixtures/pricing_example.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizeGoal {
    Cost,
    Latency,
    Balanced,
}

impl OptimizeGoal {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cost" => Some(Self::Cost),
            "latency" => Some(Self::Latency),
            "balanced" => Some(Self::Balanced),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cost => "cost",
            Self::Latency => "latency",
            Self::Balanced => "balanced",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostDecideResponse {
    /// Wire / model key after `{provider_id}/` (may be multi-segment, e.g.
    /// `deepseek-ai/deepseek-v4-flash`). Not a last-path-segment bare name.
    pub model: String,
    pub provider_id: String,
    pub reason: String,
    pub estimated_cost_per_1k_prompt_usd: f64,
    pub fallback_chain: Vec<String>,
    pub disclaimer: String,
    /// Observe-only: whether pricing/CostRouter-shaped path was used.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub used_cost_router: bool,
}

/// Session / run override (explicit user pick within reachable set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionModelOverride {
    pub provider_id: String,
    /// Wire key after `{provider_id}/` (may contain `/`).
    pub model: String,
}

/// Load default embedded pricing table (ORCH-HOST-002).
#[must_use]
pub fn load_embedded_pricing() -> Option<PricingTable> {
    PricingTable::from_json_str(EMBEDDED_PRICING_JSON).ok()
}

/// Rank reachable CAP candidates into a Decide response.
#[must_use]
pub fn decide_among_reachable(
    reachable: &[&CapabilityCandidate],
    optimize: OptimizeGoal,
    session_override: Option<&SessionModelOverride>,
    pricing: Option<&PricingTable>,
) -> Option<HostDecideResponse> {
    if reachable.is_empty() {
        return None;
    }

    if let Some(ov) = session_override {
        if let Some(hit) = reachable.iter().find(|c| {
            c.provider_id == ov.provider_id
                && model_id_of(c).is_some_and(|m| m == ov.model.as_str())
        }) {
            let rest: Vec<String> = reachable
                .iter()
                .filter(|c| {
                    !(c.provider_id == hit.provider_id && model_id_of(c) == model_id_of(hit))
                })
                .map(|c| c.provider_id.clone())
                .collect();
            let model = model_id_of(hit)?.to_string();
            return Some(HostDecideResponse {
                model: model.clone(),
                provider_id: hit.provider_id.clone(),
                reason: "session_override".into(),
                estimated_cost_per_1k_prompt_usd: pricing
                    .and_then(|p| p.cost_per_1k_prompt(&hit.provider_id, &model))
                    .unwrap_or(0.0),
                fallback_chain: rest,
                disclaimer: NOT_PRODUCTION_SLA.into(),
                used_cost_router: pricing.is_some(),
            });
        }
    }

    if let Some(table) = pricing {
        if let Some(out) = decide_with_pricing(reachable, optimize, table) {
            return Some(out);
        }
    }

    stub_rank(reachable, optimize)
}

fn decide_with_pricing(
    reachable: &[&CapabilityCandidate],
    optimize: OptimizeGoal,
    table: &PricingTable,
) -> Option<HostDecideResponse> {
    let models: Vec<&str> = reachable.iter().filter_map(|c| model_id_of(c)).collect();
    let unique_model = models
        .first()
        .copied()
        .filter(|_| models.iter().all(|m| *m == models[0]));

    if let Some(model_id) = unique_model {
        let providers: Vec<String> = reachable
            .iter()
            .filter(|c| model_id_of(c) == Some(model_id))
            .map(|c| c.provider_id.clone())
            .collect();
        if let Some((provider_id, est, reason, fallback)) =
            decide_providers_for_model(model_id, &providers, optimize, table)
        {
            return Some(HostDecideResponse {
                model: model_id.to_string(),
                provider_id,
                reason,
                estimated_cost_per_1k_prompt_usd: est,
                fallback_chain: fallback,
                disclaimer: NOT_PRODUCTION_SLA.into(),
                used_cost_router: true,
            });
        }
    }

    let mut scored: Vec<(&CapabilityCandidate, f64)> = reachable
        .iter()
        .filter_map(|c| {
            let model = model_id_of(c)?;
            let cost = table
                .cost_per_1k_prompt(&c.provider_id, model)
                .unwrap_or(f64::MAX);
            Some((*c, cost))
        })
        .collect();
    if scored.is_empty() {
        return None;
    }

    match optimize {
        OptimizeGoal::Cost | OptimizeGoal::Balanced => {
            scored.sort_by(|a, b| {
                a.1.partial_cmp(&b.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        (&a.0.provider_id, model_id_of(a.0))
                            .cmp(&(&b.0.provider_id, model_id_of(b.0)))
                    })
            });
        }
        OptimizeGoal::Latency => {
            scored.sort_by(|a, b| {
                (&a.0.provider_id, model_id_of(a.0)).cmp(&(&b.0.provider_id, model_id_of(b.0)))
            });
        }
    }

    let (first, cost) = scored.first()?;
    let model = model_id_of(first)?.to_string();
    let reason = match optimize {
        OptimizeGoal::Cost => "lowest_cost",
        OptimizeGoal::Latency => "host_reachable_latency_stub",
        OptimizeGoal::Balanced => "balanced_score",
    };
    let fallback_chain: Vec<String> = scored
        .iter()
        .skip(1)
        .map(|(c, _)| c.provider_id.clone())
        .collect();

    Some(HostDecideResponse {
        model,
        provider_id: first.provider_id.clone(),
        reason: reason.into(),
        estimated_cost_per_1k_prompt_usd: if *cost == f64::MAX { 0.0 } else { *cost },
        fallback_chain,
        disclaimer: NOT_PRODUCTION_SLA.into(),
        used_cost_router: true,
    })
}

fn stub_rank(
    reachable: &[&CapabilityCandidate],
    optimize: OptimizeGoal,
) -> Option<HostDecideResponse> {
    let mut ranked: Vec<&CapabilityCandidate> = reachable.to_vec();
    ranked.sort_by(|a, b| (&a.provider_id, model_id_of(a)).cmp(&(&b.provider_id, model_id_of(b))));

    let reason = match optimize {
        OptimizeGoal::Cost => "host_reachable_prefer_stub",
        OptimizeGoal::Latency => "host_reachable_latency_stub",
        OptimizeGoal::Balanced => "host_reachable_balanced_stub",
    };

    let first = ranked.first()?;
    let model = model_id_of(first)?.to_string();
    let fallback_chain: Vec<String> = ranked
        .iter()
        .skip(1)
        .map(|c| c.provider_id.clone())
        .collect();

    Some(HostDecideResponse {
        model,
        provider_id: first.provider_id.clone(),
        reason: reason.into(),
        estimated_cost_per_1k_prompt_usd: 0.0,
        fallback_chain,
        disclaimer: NOT_PRODUCTION_SLA.into(),
        used_cost_router: false,
    })
}

/// Wire model id: strip `{provider_id}/` prefix only (ORCH-HOST-003).
///
/// Preserves multi-segment aggregator wires such as
/// `nvidia/deepseek-ai/deepseek-v4-flash` → `deepseek-ai/deepseek-v4-flash`.
/// Do **not** use last-path-segment truncation (`rsplit_once`).
fn model_id_of(c: &CapabilityCandidate) -> Option<&str> {
    let logical = c.logical_model_id.as_deref()?.trim();
    if logical.is_empty() {
        return None;
    }
    let provider = c.provider_id.trim();
    if provider.is_empty() {
        return Some(logical);
    }
    let prefix = format!("{provider}/");
    if let Some(rest) = logical.strip_prefix(prefix.as_str()) {
        if rest.is_empty() {
            return None;
        }
        return Some(rest);
    }
    if logical == provider {
        return None;
    }
    // Mismatched prefix (should be rare in CAP index): keep prior last-segment
    // fallback so decidable filtering still yields something observable.
    Some(logical.rsplit_once('/').map(|(_, m)| m).unwrap_or(logical))
}

/// Prefer candidates with a usable wire model id; if missing, return None (skip).
#[must_use]
pub fn decidable_reachable<'a>(
    reachable: &[&'a CapabilityCandidate],
) -> Vec<&'a CapabilityCandidate> {
    reachable
        .iter()
        .copied()
        .filter(|c| model_id_of(c).is_some())
        .collect()
}

/// Rebuild full logical id from a Decide response (provider + wire model).
#[must_use]
pub fn logical_id_from_decision(decision: &HostDecideResponse) -> String {
    crate::protocol_registry::compose_logical_model_id(&decision.provider_id, &decision.model)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_index::CapabilityCandidate;

    fn cand(provider: &str, logical: Option<&str>) -> CapabilityCandidate {
        CapabilityCandidate {
            provider_id: provider.into(),
            logical_model_id: logical.map(str::to_string),
            reason: "test".into(),
            source_file: "test.yaml".into(),
        }
    }

    #[test]
    fn empty_reachable_returns_none() {
        assert!(decide_among_reachable(&[], OptimizeGoal::Cost, None, None).is_none());
    }

    #[test]
    fn stub_ranks_stable_and_sets_disclaimer() {
        let a = cand("openai", Some("openai/gpt-4o-mini"));
        let b = cand("groq", Some("groq/llama-3.1-8b-instant"));
        let refs = [&b, &a];
        let decidable = decidable_reachable(&refs);
        let out =
            decide_among_reachable(&decidable, OptimizeGoal::Cost, None, None).expect("decide");
        assert_eq!(out.disclaimer, NOT_PRODUCTION_SLA);
        assert_eq!(out.provider_id, "groq");
        assert_eq!(out.model, "llama-3.1-8b-instant");
        assert_eq!(out.reason, "host_reachable_prefer_stub");
        assert!(!out.used_cost_router);
    }

    #[test]
    fn pricing_picks_lowest_cost() {
        let table = load_embedded_pricing().expect("pricing");
        let a = cand("openai", Some("openai/gpt-4o-mini"));
        let b = cand("groq", Some("groq/llama-3.1-8b-instant"));
        let refs = [&a, &b];
        let decidable = decidable_reachable(&refs);
        let out =
            decide_among_reachable(&decidable, OptimizeGoal::Cost, None, Some(&table)).expect("d");
        assert!(out.used_cost_router);
        assert_eq!(out.provider_id, "groq");
        assert_eq!(out.model, "llama-3.1-8b-instant");
        assert_eq!(out.reason, "lowest_cost");
        assert!(out.estimated_cost_per_1k_prompt_usd < 0.001);
    }

    #[test]
    fn session_override_wins_when_reachable() {
        let a = cand("openai", Some("openai/gpt-4o-mini"));
        let b = cand("groq", Some("groq/llama-3.1-8b-instant"));
        let refs = [&a, &b];
        let decidable = decidable_reachable(&refs);
        let ov = SessionModelOverride {
            provider_id: "openai".into(),
            model: "gpt-4o-mini".into(),
        };
        let out = decide_among_reachable(&decidable, OptimizeGoal::Cost, Some(&ov), None)
            .expect("decide");
        assert_eq!(out.reason, "session_override");
        assert_eq!(out.provider_id, "openai");
        assert_eq!(out.model, "gpt-4o-mini");
    }

    #[test]
    fn rejects_unreachable_override_falls_back() {
        let a = cand("groq", Some("groq/llama-3.1-8b-instant"));
        let refs = [&a];
        let decidable = decidable_reachable(&refs);
        let ov = SessionModelOverride {
            provider_id: "openai".into(),
            model: "gpt-4o".into(),
        };
        let out = decide_among_reachable(&decidable, OptimizeGoal::Cost, Some(&ov), None)
            .expect("decide");
        assert_eq!(out.provider_id, "groq");
        assert_ne!(out.reason, "session_override");
    }

    #[test]
    fn multi_segment_wire_id_preserved_through_decide() {
        let a = cand("nvidia", Some("nvidia/deepseek-ai/deepseek-v4-flash"));
        let b = cand("deepseek", Some("deepseek/deepseek-v4-flash"));
        let refs = [&a, &b];
        let decidable = decidable_reachable(&refs);
        assert_eq!(decidable.len(), 2);

        let out =
            decide_among_reachable(&decidable, OptimizeGoal::Cost, None, None).expect("decide");
        // Alphabetical provider_id: deepseek before nvidia for stub rank.
        assert_eq!(out.provider_id, "deepseek");
        assert_eq!(out.model, "deepseek-v4-flash");
        assert_eq!(logical_id_from_decision(&out), "deepseek/deepseek-v4-flash");

        let ov = SessionModelOverride {
            provider_id: "nvidia".into(),
            model: "deepseek-ai/deepseek-v4-flash".into(),
        };
        let picked = decide_among_reachable(&decidable, OptimizeGoal::Cost, Some(&ov), None)
            .expect("override");
        assert_eq!(picked.reason, "session_override");
        assert_eq!(picked.provider_id, "nvidia");
        assert_eq!(picked.model, "deepseek-ai/deepseek-v4-flash");
        assert_eq!(
            logical_id_from_decision(&picked),
            "nvidia/deepseek-ai/deepseek-v4-flash"
        );
    }

    #[test]
    fn distinct_multi_segment_wires_do_not_collapse_for_pricing_key() {
        // Same bare suffix, different wire — must not share unique_model path.
        let nvidia = cand("nvidia", Some("nvidia/deepseek-ai/deepseek-v4-flash"));
        let deepseek = cand("deepseek", Some("deepseek/deepseek-v4-flash"));
        let refs = [&nvidia, &deepseek];
        let decidable = decidable_reachable(&refs);
        let table = load_embedded_pricing().expect("pricing");
        let out = decide_among_reachable(&decidable, OptimizeGoal::Cost, None, Some(&table))
            .expect("decide");
        // Wire keys differ → scored path (not unique_model CostRouter collapse).
        assert!(
            out.model == "deepseek-ai/deepseek-v4-flash" || out.model == "deepseek-v4-flash",
            "unexpected model {}",
            out.model
        );
        let logical = logical_id_from_decision(&out);
        assert!(
            logical == "nvidia/deepseek-ai/deepseek-v4-flash"
                || logical == "deepseek/deepseek-v4-flash",
            "unexpected logical {logical}"
        );
        assert!(!logical.ends_with("nvidia/deepseek-v4-flash"));
    }
}
