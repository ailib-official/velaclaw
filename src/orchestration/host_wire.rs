//! Wire host Decide into agent model selection (ORCH-HOST-001/003/004).

use crate::capability_index::{
    filter_reachable, load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS,
};
use crate::execution::provider_has_usable_key;
use crate::orchestration::host_decide::{
    decidable_reachable, decide_among_reachable, logical_id_from_decision, OptimizeGoal,
    SessionModelOverride,
};
use crate::orchestration::session_override;
use crate::protocol_registry::provider_id_from_logical;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Context for host Decide (config dir + knobs). Built from full [`Config`].
#[derive(Debug, Clone)]
pub struct HostDecideHost {
    pub enabled: bool,
    pub optimize: String,
    /// ORCH-HOST-004: soft-fail / quota may advance session_override along fallbacks.
    pub failover: bool,
    pub config_dir: PathBuf,
}

impl HostDecideHost {
    pub fn from_config(config: &crate::config::Config) -> Self {
        let config_dir = config
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        Self {
            enabled: config.agent.host_decide,
            optimize: config.agent.host_decide_optimize.clone(),
            failover: config.agent.host_decide_failover,
            config_dir,
        }
    }
}

/// When host Decide is enabled, pick a reachable model or return `Ok(None)`.
pub fn try_host_decide_model(
    host: &HostDecideHost,
    user_message: &str,
    session_key: &str,
) -> Result<Option<String>> {
    Ok(try_host_decide_selection(host, user_message, session_key)?.map(|s| s.logical_id))
}

/// Observable host Decide selection (model + reason codes).
#[derive(Debug, Clone, PartialEq)]
pub struct HostDecideSelection {
    pub logical_id: String,
    pub reason: String,
    pub used_cost_router: bool,
    pub optimize: String,
    /// Remaining reachable logical ids after the selected one (ORCH-HOST-004).
    pub fallback_logical_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct LastHostDecide {
    logical_id: String,
    fallback_logical_ids: Vec<String>,
}

static LAST_DECIDE: Mutex<Option<HashMap<String, LastHostDecide>>> = Mutex::new(None);

fn last_map() -> std::sync::MutexGuard<'static, Option<HashMap<String, LastHostDecide>>> {
    LAST_DECIDE.lock().unwrap_or_else(|e| e.into_inner())
}

fn remember_host_decide(session_key: &str, selection: &HostDecideSelection) {
    let mut guard = last_map();
    let store = guard.get_or_insert_with(HashMap::new);
    store.insert(
        session_key.to_string(),
        LastHostDecide {
            logical_id: selection.logical_id.clone(),
            fallback_logical_ids: selection.fallback_logical_ids.clone(),
        },
    );
}

fn parse_session_override(logical_id: &str) -> Option<SessionModelOverride> {
    let logical_id = logical_id.trim();
    if logical_id.is_empty() {
        return None;
    }
    let provider_id = provider_id_from_logical(logical_id).to_string();
    let model = logical_id
        .strip_prefix(&format!("{provider_id}/"))
        .unwrap_or(logical_id)
        .to_string();
    if provider_id.is_empty() || model.is_empty() {
        return None;
    }
    Some(SessionModelOverride { provider_id, model })
}

/// When failover is enabled, advance session override to the next Decide fallback.
///
/// Returns the new logical id when an override was written.
#[must_use]
pub fn maybe_apply_host_decide_failover(
    host: &HostDecideHost,
    session_key: &str,
    failed_logical_id: &str,
) -> Option<String> {
    if !host.enabled || !host.failover {
        return None;
    }
    let last = {
        let guard = last_map();
        guard.as_ref()?.get(session_key).cloned()
    }?;

    let next = last
        .fallback_logical_ids
        .iter()
        .find(|id| id.as_str() != failed_logical_id && id.as_str() != last.logical_id.as_str())
        .cloned()?;

    let ov = parse_session_override(&next)?;
    session_override::set_override(session_key, Some(ov));
    tracing::info!(
        target: "host_decide",
        from = %failed_logical_id,
        to = %next,
        session_key = %session_key,
        "host_decide_failover: session override set for next turn"
    );
    Some(next)
}

/// Soft-fail handling shared by CLI + Web (ORCH-HOST-004).
#[must_use]
pub fn finalize_tool_format_exhausted(
    reply: &str,
    model: &str,
    surface: velaclaw_agent_runtime::SoftFailSurface,
    host: Option<&HostDecideHost>,
    session_key: &str,
) -> String {
    let mut out =
        velaclaw_agent_runtime::append_tool_format_exhausted_notice(reply, model, surface);
    if let Some(host) = host {
        if let Some(to) = maybe_apply_host_decide_failover(host, session_key, model) {
            out.push_str(&velaclaw_agent_runtime::host_decide_failover_announce(
                model, &to,
            ));
        }
    }
    out
}

/// Map a provider hard-fail into an actionable user error when it looks like limit/quota.
pub fn map_provider_limit_error(
    err: anyhow::Error,
    model: &str,
    surface: velaclaw_agent_runtime::SoftFailSurface,
    host: Option<&HostDecideHost>,
    session_key: &str,
) -> anyhow::Error {
    let raw = err.to_string();
    let sanitized = crate::providers::sanitize_api_error(&raw);
    if velaclaw_agent_runtime::looks_like_model_retired(&raw) {
        return anyhow::anyhow!(velaclaw_agent_runtime::provider_retired_user_message(
            &sanitized, model, surface,
        ));
    }
    if !velaclaw_agent_runtime::looks_like_provider_limit(&raw) {
        return err;
    }
    let sanitized = crate::providers::sanitize_api_error(&raw);
    let mut msg = velaclaw_agent_runtime::provider_limit_user_message(&sanitized, model, surface);
    if let Some(host) = host {
        if let Some(to) = maybe_apply_host_decide_failover(host, session_key, model) {
            msg.push_str(&velaclaw_agent_runtime::host_decide_failover_announce(
                model, &to,
            ));
        }
    }
    anyhow::anyhow!(msg)
}

/// When host Decide is enabled, return selection details or `Ok(None)`.
pub fn try_host_decide_selection(
    host: &HostDecideHost,
    user_message: &str,
    session_key: &str,
) -> Result<Option<HostDecideSelection>> {
    if !host.enabled {
        return Ok(None);
    }

    let optimize = OptimizeGoal::parse(host.optimize.as_str()).unwrap_or_else(|| {
        tracing::warn!(
            optimize = %host.optimize,
            "host_decide: invalid host_decide_optimize; falling back to cost"
        );
        OptimizeGoal::Cost
    });

    let (index, _) = match load_or_rebuild_for_config(&host.config_dir, false) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                error = %e,
                config_dir = %host.config_dir.display(),
                "host_decide: CAP index unavailable; skip (turn ladder continues)"
            );
            return Ok(None);
        }
    };

    let preferred_tags: &[&str] = if user_message.to_ascii_lowercase().contains("pdf")
        || user_message.to_ascii_lowercase().contains("document")
    {
        &["document_understanding", "coding", "tool_calling"]
    } else {
        &["coding", "tool_calling", "high-reasoning", "speed"]
    };

    let mut declared: &[crate::capability_index::CapabilityCandidate] = &[];
    for t in preferred_tags.iter().chain(CAPABILITY_TAGS.iter()) {
        if let Ok(c) = lookup_tag(&index, t) {
            if !c.is_empty() {
                declared = c;
                break;
            }
        }
    }
    if declared.is_empty() {
        tracing::debug!("host_decide: no CAP candidates; skip");
        return Ok(None);
    }

    let reachable = filter_reachable(declared, provider_has_usable_key);
    let decidable = decidable_reachable(&reachable);
    if decidable.is_empty() {
        tracing::debug!("host_decide: no decidable reachable models; skip");
        return Ok(None);
    }

    let ov = session_override::get_override(session_key);
    let pricing = crate::orchestration::host_decide::load_embedded_pricing();
    let Some(decision) =
        decide_among_reachable(&decidable, optimize, ov.as_ref(), pricing.as_ref())
    else {
        return Ok(None);
    };

    let logical_id = logical_id_from_decision(&decision);
    let fallback_logical_ids: Vec<String> = decidable
        .iter()
        .filter_map(|c| c.logical_model_id.clone())
        .filter(|id| id != &logical_id)
        .collect();

    tracing::info!(
        target: "host_decide",
        provider = %decision.provider_id,
        model = %decision.model,
        logical_id = %logical_id,
        reason = %decision.reason,
        optimize = optimize.as_str(),
        used_cost_router = decision.used_cost_router,
        "host decide selected model"
    );

    let selection = HostDecideSelection {
        logical_id,
        reason: decision.reason,
        used_cost_router: decision.used_cost_router,
        optimize: optimize.as_str().to_string(),
        fallback_logical_ids,
    };
    remember_host_decide(session_key, &selection);
    Ok(Some(selection))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::host_decide::{HostDecideResponse, NOT_PRODUCTION_SLA};

    #[test]
    fn logical_id_from_decision_preserves_multi_segment_wire() {
        let d = HostDecideResponse {
            model: "deepseek-ai/deepseek-v4-flash".into(),
            provider_id: "nvidia".into(),
            reason: "test".into(),
            estimated_cost_per_1k_prompt_usd: 0.0,
            fallback_chain: vec![],
            disclaimer: NOT_PRODUCTION_SLA.into(),
            used_cost_router: false,
        };
        assert_eq!(
            logical_id_from_decision(&d),
            "nvidia/deepseek-ai/deepseek-v4-flash"
        );
    }

    #[test]
    fn cap_load_failure_soft_skips_when_enabled() {
        let _guard = crate::capability_index::PROTOCOL_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let host = HostDecideHost {
            enabled: true,
            optimize: "cost".into(),
            failover: false,
            config_dir: PathBuf::from("/tmp/velaclaw-orch-host-003-missing-cap"),
        };
        let prev_dir = std::env::var_os("AI_PROTOCOL_DIR");
        let prev_path = std::env::var_os("AI_PROTOCOL_PATH");
        std::env::remove_var("AI_PROTOCOL_DIR");
        std::env::remove_var("AI_PROTOCOL_PATH");
        let out = try_host_decide_selection(&host, "hello", "test-session");
        match prev_dir {
            Some(v) => std::env::set_var("AI_PROTOCOL_DIR", v),
            None => std::env::remove_var("AI_PROTOCOL_DIR"),
        }
        match prev_path {
            Some(v) => std::env::set_var("AI_PROTOCOL_PATH", v),
            None => std::env::remove_var("AI_PROTOCOL_PATH"),
        }
        let selection = out.expect("must not hard-fail turn");
        assert!(selection.is_none());
    }

    #[test]
    fn failover_writes_session_override_from_fallback_chain() {
        let host = HostDecideHost {
            enabled: true,
            optimize: "cost".into(),
            failover: true,
            config_dir: PathBuf::from("/tmp"),
        };
        let key = "orch-host-004-failover";
        session_override::set_override(key, None);
        remember_host_decide(
            key,
            &HostDecideSelection {
                logical_id: "groq/llama-3.1-8b-instant".into(),
                reason: "test".into(),
                used_cost_router: false,
                optimize: "cost".into(),
                fallback_logical_ids: vec![
                    "deepseek/deepseek-v4-flash".into(),
                    "openai/gpt-4o-mini".into(),
                ],
            },
        );
        let next = maybe_apply_host_decide_failover(&host, key, "groq/llama-3.1-8b-instant")
            .expect("failover");
        assert_eq!(next, "deepseek/deepseek-v4-flash");
        let ov = session_override::get_override(key).expect("override");
        assert_eq!(ov.provider_id, "deepseek");
        assert_eq!(ov.model, "deepseek-v4-flash");
        session_override::set_override(key, None);

        let host_off = HostDecideHost {
            failover: false,
            ..host.clone()
        };
        remember_host_decide(
            key,
            &HostDecideSelection {
                logical_id: "groq/llama-3.1-8b-instant".into(),
                reason: "test".into(),
                used_cost_router: false,
                optimize: "cost".into(),
                fallback_logical_ids: vec!["deepseek/deepseek-v4-flash".into()],
            },
        );
        assert!(
            maybe_apply_host_decide_failover(&host_off, key, "groq/llama-3.1-8b-instant").is_none()
        );
    }
}
