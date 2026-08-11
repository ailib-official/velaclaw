//! Wire host Decide into agent model selection (ORCH-HOST-001/003).

use crate::capability_index::{
    filter_reachable, load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS,
};
use crate::execution::provider_has_usable_key;
use crate::orchestration::host_decide::{
    decidable_reachable, decide_among_reachable, logical_id_from_decision, OptimizeGoal,
};
use crate::orchestration::session_override;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Context for host Decide (config dir + knobs). Built from full [`Config`].
#[derive(Debug, Clone)]
pub struct HostDecideHost {
    pub enabled: bool,
    pub optimize: String,
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

    Ok(Some(HostDecideSelection {
        logical_id,
        reason: decision.reason,
        used_cost_router: decision.used_cost_router,
        optimize: optimize.as_str().to_string(),
    }))
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
}
