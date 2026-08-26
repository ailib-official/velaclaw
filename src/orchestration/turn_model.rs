//! Shared turn-model ladder for CLI + Web (`Agent::turn`).
//!
//! Precedence (Charter / ORCH): explicit user pick → host_decide →
//! intent_capability_route → query_classification / default_model.
//! Channels remain on `route.model` (documented separately).

use crate::agent::classifier;
use crate::agent::intent_route::IntentRouteHost;
use crate::capability_index::{
    filter_reachable, load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS,
};
use crate::config::QueryClassificationConfig;
use crate::execution::provider_has_usable_key;
use crate::orchestration::host_wire::{try_host_decide_selection, HostDecideHost};
use anyhow::{bail, Result};
use serde::Serialize;

/// Why a turn model was chosen (observe / UX honesty).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnModelSource {
    ExplicitUserPick,
    HostDecide,
    IntentCapabilityRoute,
    QueryClassification,
    DefaultModel,
    /// Linear L2 DAG node `model_selector.capabilities` (VL-NA-014). Opt-in live only.
    NodeCapability,
}

/// Inputs for one turn's model resolution.
#[derive(Debug, Clone)]
pub struct TurnModelRequest<'a> {
    pub user_message: &'a str,
    pub session_key: &'a str,
    pub default_model: &'a str,
    /// CLI `-p/--model` or Web `model_id` (protocol `provider/model`).
    pub explicit_model: Option<&'a str>,
    pub host_decide: Option<&'a HostDecideHost>,
    pub intent_route: Option<&'a IntentRouteHost>,
    pub classification: &'a QueryClassificationConfig,
    pub available_hints: &'a [String],
}

/// Result of [`resolve_turn_model`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnModelDecision {
    pub model: String,
    pub source: TurnModelSource,
    /// Human-readable reason (e.g. `explicit_user_pick`, `host_decide:lowest_cost`).
    pub reason: String,
}

/// Resolve the effective turn model with a single precedence ladder.
pub fn resolve_turn_model(req: &TurnModelRequest<'_>) -> Result<TurnModelDecision> {
    if let Some(raw) = req.explicit_model.map(str::trim).filter(|s| !s.is_empty()) {
        let model = honor_explicit_pick(raw, req.host_decide)?;
        tracing::info!(
            target: "turn_model",
            model = %model,
            source = "explicit_user_pick",
            "turn model selected"
        );
        return Ok(TurnModelDecision {
            model,
            source: TurnModelSource::ExplicitUserPick,
            reason: "explicit_user_pick".into(),
        });
    }

    if let Some(host) = req.host_decide {
        if host.enabled {
            if let Some(selected) =
                try_host_decide_selection(host, req.user_message, req.session_key)?
            {
                let reason = format!(
                    "host_decide:{}:optimize={}:cost_router={}",
                    selected.reason, selected.optimize, selected.used_cost_router
                );
                tracing::info!(
                    target: "turn_model",
                    model = %selected.logical_id,
                    source = "host_decide",
                    reason = %reason,
                    "turn model selected"
                );
                return Ok(TurnModelDecision {
                    model: selected.logical_id,
                    source: TurnModelSource::HostDecide,
                    reason,
                });
            }
        }
    }

    if let Some(host) = req.intent_route {
        if host.enabled {
            let decision = crate::agent::intent_route::resolve_for_host(
                host,
                req.classification,
                req.available_hints,
                req.default_model,
                req.user_message,
                None,
                None,
                false,
            )?;
            let model = decision.selected_model.ok_or_else(|| {
                anyhow::anyhow!("intent route returned no model: {}", decision.reason)
            })?;
            tracing::info!(
                target: "turn_model",
                model = %model,
                source = "intent_capability_route",
                reason = %decision.reason,
                "turn model selected"
            );
            return Ok(TurnModelDecision {
                model,
                source: TurnModelSource::IntentCapabilityRoute,
                reason: format!("intent_capability_route:{}", decision.reason),
            });
        }
    }

    let classified = classifier::resolve_model_for_message(
        req.classification,
        req.available_hints,
        req.default_model,
        req.user_message,
    );
    let (source, reason) = if classified == req.default_model {
        (TurnModelSource::DefaultModel, "default_model".to_string())
    } else {
        (
            TurnModelSource::QueryClassification,
            format!("query_classification:{classified}"),
        )
    };
    tracing::info!(
        target: "turn_model",
        model = %classified,
        source = ?source,
        "turn model selected"
    );
    Ok(TurnModelDecision {
        model: classified,
        source,
        reason,
    })
}

/// Explicit pick wins; when a CAP index is available, require reachability (fail closed).
fn honor_explicit_pick(raw: &str, host_decide: Option<&HostDecideHost>) -> Result<String> {
    let Some(host) = host_decide else {
        return Ok(raw.to_string());
    };
    let Ok((index, _)) = load_or_rebuild_for_config(&host.config_dir, false) else {
        return Ok(raw.to_string());
    };

    let mut any_declared = false;
    let mut reachable_ids: Vec<String> = Vec::new();
    for tag in CAPABILITY_TAGS {
        let Ok(declared) = lookup_tag(&index, tag) else {
            continue;
        };
        if declared.is_empty() {
            continue;
        }
        any_declared = true;
        for c in filter_reachable(declared, provider_has_usable_key) {
            let id = match &c.logical_model_id {
                Some(logical) if logical.contains('/') => logical.clone(),
                Some(bare) => format!("{}/{}", c.provider_id, bare),
                None => continue,
            };
            if !reachable_ids.iter().any(|x| x == &id) {
                reachable_ids.push(id);
            }
        }
    }

    if !any_declared || reachable_ids.is_empty() {
        // Index empty / no keys — honor explicit (operator knows their BYOK).
        return Ok(raw.to_string());
    }

    if reachable_ids.iter().any(|id| id == raw) {
        return Ok(raw.to_string());
    }

    // Also accept provider/bare-model matches when logical ids use composed forms.
    let provider = raw.split_once('/').map(|(p, _)| p);
    let bare = raw.rsplit_once('/').map(|(_, m)| m);
    if let (Some(p), Some(m)) = (provider, bare) {
        if reachable_ids.iter().any(|id| {
            id == raw
                || (id.starts_with(&format!("{p}/"))
                    && id.rsplit_once('/').is_some_and(|(_, b)| b == m))
        }) {
            return Ok(raw.to_string());
        }
    }

    bail!(
        "explicit model `{raw}` is not in the CAP reachable set ({} candidates); pick a reachable model or clear the override",
        reachable_ids.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::QueryClassificationConfig;
    use std::path::PathBuf;

    fn empty_classification() -> QueryClassificationConfig {
        QueryClassificationConfig {
            enabled: false,
            rules: vec![],
        }
    }

    #[test]
    fn explicit_pick_wins_without_cap_host() {
        let classification = empty_classification();
        let hints: Vec<String> = vec![];
        let req = TurnModelRequest {
            user_message: "hello",
            session_key: "sess",
            default_model: "deepseek/deepseek-v4-flash",
            explicit_model: Some("nvidia/nemotron-mini"),
            host_decide: None,
            intent_route: None,
            classification: &classification,
            available_hints: &hints,
        };
        let d = resolve_turn_model(&req).unwrap();
        assert_eq!(d.model, "nvidia/nemotron-mini");
        assert_eq!(d.source, TurnModelSource::ExplicitUserPick);
        assert_eq!(d.reason, "explicit_user_pick");
    }

    #[test]
    fn falls_back_to_default_when_flags_off() {
        let classification = empty_classification();
        let hints: Vec<String> = vec![];
        let host = HostDecideHost {
            enabled: false,
            optimize: "cost".into(),
            failover: false,
            config_dir: PathBuf::from("/tmp/velaclaw-turn-model-test-missing"),
        };
        let req = TurnModelRequest {
            user_message: "hello",
            session_key: "sess",
            default_model: "deepseek/deepseek-v4-flash",
            explicit_model: None,
            host_decide: Some(&host),
            intent_route: None,
            classification: &classification,
            available_hints: &hints,
        };
        let d = resolve_turn_model(&req).unwrap();
        assert_eq!(d.model, "deepseek/deepseek-v4-flash");
        assert_eq!(d.source, TurnModelSource::DefaultModel);
    }

    #[test]
    fn disabled_host_decide_does_not_select() {
        let classification = empty_classification();
        let hints: Vec<String> = vec![];
        let host = HostDecideHost {
            enabled: false,
            optimize: "cost".into(),
            failover: false,
            config_dir: PathBuf::from("."),
        };
        let req = TurnModelRequest {
            user_message: "fix rust null check",
            session_key: "sess",
            default_model: "deepseek/deepseek-v4-flash",
            explicit_model: None,
            host_decide: Some(&host),
            intent_route: None,
            classification: &classification,
            available_hints: &hints,
        };
        let d = resolve_turn_model(&req).unwrap();
        assert_eq!(d.source, TurnModelSource::DefaultModel);
    }
}
