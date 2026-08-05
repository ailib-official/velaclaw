//! Host-side Decide (ORCH-HOST-001) — CAP reachable ∩ ranking.
//!
//! Matches [`ORCH-DECIDE`] contract fields. Default-off via
//! `[agent].host_decide`. Does **not** call Prism Gateway HTTP for BYOK.

use crate::capability_index::CapabilityCandidate;
use serde::{Deserialize, Serialize};

/// Contract disclaimer — must match Gateway/Eos until production ACK.
pub const NOT_PRODUCTION_SLA: &str = "NOT_PRODUCTION_SLA";

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
    pub model: String,
    pub provider_id: String,
    pub reason: String,
    pub estimated_cost_per_1k_prompt_usd: f64,
    pub fallback_chain: Vec<String>,
    pub disclaimer: String,
}

/// Session / run override (explicit user pick within reachable set).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionModelOverride {
    pub provider_id: String,
    pub model: String,
}

/// Rank reachable CAP candidates into a Decide response.
///
/// Without a pricing table, order is stable by `(provider_id, logical_model_id)`
/// and `reason` is `host_reachable_prefer` (still `NOT_PRODUCTION_SLA`).
#[must_use]
pub fn decide_among_reachable(
    reachable: &[&CapabilityCandidate],
    optimize: OptimizeGoal,
    session_override: Option<&SessionModelOverride>,
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
                    !(c.provider_id == hit.provider_id
                        && model_id_of(c) == model_id_of(hit))
                })
                .map(|c| c.provider_id.clone())
                .collect();
            return Some(HostDecideResponse {
                model: model_id_of(hit)?.to_string(),
                provider_id: hit.provider_id.clone(),
                reason: "session_override".into(),
                estimated_cost_per_1k_prompt_usd: 0.0,
                fallback_chain: rest,
                disclaimer: NOT_PRODUCTION_SLA.into(),
            });
        }
        // Override not in reachable set → ignore (fail open to ranking).
    }

    let mut ranked: Vec<&CapabilityCandidate> = reachable.to_vec();
    ranked.sort_by(|a, b| {
        (&a.provider_id, model_id_of(a)).cmp(&(&b.provider_id, model_id_of(b)))
    });

    // Latency / balanced currently share stable order until pricing health lands;
    // reason string still reflects the requested optimize goal for observability.
    let reason = match optimize {
        OptimizeGoal::Cost => "host_reachable_prefer",
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
    })
}

fn model_id_of(c: &CapabilityCandidate) -> Option<&str> {
    let logical = c.logical_model_id.as_deref()?;
    Some(
        logical
            .rsplit_once('/')
            .map(|(_, m)| m)
            .unwrap_or(logical),
    )
}

/// Prefer logical model id bare segment; if missing, return None (skip).
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
        assert!(decide_among_reachable(&[], OptimizeGoal::Cost, None).is_none());
    }

    #[test]
    fn ranks_stable_and_sets_disclaimer() {
        let a = cand("openai", Some("openai/gpt-4o-mini"));
        let b = cand("groq", Some("groq/llama-3.1-8b-instant"));
        let refs = [&b, &a];
        let decidable = decidable_reachable(&refs);
        let out = decide_among_reachable(&decidable, OptimizeGoal::Cost, None).expect("decide");
        assert_eq!(out.disclaimer, NOT_PRODUCTION_SLA);
        assert_eq!(out.provider_id, "groq");
        assert_eq!(out.model, "llama-3.1-8b-instant");
        assert_eq!(out.reason, "host_reachable_prefer");
        assert!(out.fallback_chain.contains(&"openai".to_string()));
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
        let out =
            decide_among_reachable(&decidable, OptimizeGoal::Cost, Some(&ov)).expect("decide");
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
        let out =
            decide_among_reachable(&decidable, OptimizeGoal::Cost, Some(&ov)).expect("decide");
        assert_eq!(out.provider_id, "groq");
        assert_ne!(out.reason, "session_override");
    }
}
