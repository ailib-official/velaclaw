//! Wire host Decide into agent model selection (ORCH-HOST-001).

use crate::capability_index::{
    filter_reachable, load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS,
};
use crate::execution::provider_has_usable_key;
use crate::orchestration::host_decide::{
    decidable_reachable, decide_among_reachable, OptimizeGoal,
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
    if !host.enabled {
        return Ok(None);
    }

    let optimize =
        OptimizeGoal::parse(host.optimize.as_str()).unwrap_or(OptimizeGoal::Cost);

    let (index, _) = load_or_rebuild_for_config(&host.config_dir, false)?;

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
    let Some(decision) = decide_among_reachable(&decidable, optimize, ov.as_ref()) else {
        return Ok(None);
    };

    tracing::info!(
        target: "host_decide",
        provider = %decision.provider_id,
        model = %decision.model,
        reason = %decision.reason,
        optimize = optimize.as_str(),
        "host decide selected model"
    );

    Ok(Some(format!(
        "{}/{}",
        decision.provider_id, decision.model
    )))
}
