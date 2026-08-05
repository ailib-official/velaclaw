//! ORCH-HOST-001: doctor explain for host Decide (CAP reachable ∩ ranking).

use crate::capability_index::{
    filter_reachable, load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS,
};
use crate::config::Config;
use crate::execution::provider_has_usable_key;
use crate::orchestration::host_decide::{
    decidable_reachable, decide_among_reachable, OptimizeGoal, SessionModelOverride,
};
use crate::orchestration::session_override;
use crate::orchestration::HostDecideHost;
use anyhow::Result;

/// Print host Decide decision (observe-only; no LLM).
///
/// When `force` is true, runs even if `[agent].host_decide` is false.
/// Optional `--set-override provider/model` updates process-local session override
/// (must still be in reachable set at decide time).
pub fn run_host_decide(
    config: &Config,
    message: &str,
    tag: Option<&str>,
    force: bool,
    set_override: Option<&str>,
    clear_override: bool,
    session_key: &str,
) -> Result<()> {
    let mut host = HostDecideHost::from_config(config);
    if force {
        host.enabled = true;
    }

    println!("🩺 VelaClaw Doctor — Host Decide (ORCH-HOST-001)");
    println!(
        "  flag host_decide:           {}",
        config.agent.host_decide
    );
    println!("  optimize:                  {}", host.optimize);
    println!("  observe force-on:          {force}");
    println!("  session_key:               {session_key}");
    println!("  config_dir:                {}", host.config_dir.display());
    println!();

    if clear_override {
        session_override::set_override(session_key, None);
        println!("cleared session override for session_key={session_key}");
    }

    if let Some(spec) = set_override {
        let (provider_id, model) = parse_provider_model(spec)?;
        session_override::set_override(
            session_key,
            Some(SessionModelOverride {
                provider_id: provider_id.clone(),
                model: model.clone(),
            }),
        );
        println!("set session override → {provider_id}/{model}");
    }

    if !host.enabled {
        println!("ℹ️  Host Decide disabled (default-off). Explicit model / prior routes apply.");
        println!("   Enable with `[agent].host_decide = true`, or pass `--force`.");
        return Ok(());
    }

    let optimize =
        OptimizeGoal::parse(host.optimize.as_str()).unwrap_or(OptimizeGoal::Cost);
    let (index, _) = load_or_rebuild_for_config(&host.config_dir, false)?;

    let declared = if let Some(t) = tag {
        lookup_tag(&index, t)?
    } else {
        let preferred: &[&str] = if message.to_ascii_lowercase().contains("pdf")
            || message.to_ascii_lowercase().contains("document")
        {
            &["document_understanding", "coding", "tool_calling"]
        } else {
            &["coding", "tool_calling", "high-reasoning", "speed"]
        };
        let mut found: &[crate::capability_index::CapabilityCandidate] = &[];
        for t in preferred.iter().chain(CAPABILITY_TAGS.iter()) {
            if let Ok(c) = lookup_tag(&index, t) {
                if !c.is_empty() {
                    found = c;
                    break;
                }
            }
        }
        found
    };

    println!("  declared candidates:       {}", declared.len());
    let reachable = filter_reachable(declared, provider_has_usable_key);
    println!("  reachable (keys ∩ dist):   {}", reachable.len());
    for c in &reachable {
        let mid = c
            .logical_model_id
            .as_deref()
            .unwrap_or("(no logical_model_id)");
        println!("    - {} / {}", c.provider_id, mid);
    }

    let decidable = decidable_reachable(&reachable);
    let ov = session_override::get_override(session_key);
    if let Some(ref o) = ov {
        println!(
            "  active override:           {}/{}",
            o.provider_id, o.model
        );
    }

    match decide_among_reachable(&decidable, optimize, ov.as_ref()) {
        Some(d) => {
            println!();
            println!("Decision:");
            println!("  model:        {}", d.model);
            println!("  provider_id:  {}", d.provider_id);
            println!("  reason:       {}", d.reason);
            println!(
                "  est_cost/1k:  {}",
                d.estimated_cost_per_1k_prompt_usd
            );
            println!("  fallback:     {:?}", d.fallback_chain);
            println!("  disclaimer:   {}", d.disclaimer);
            Ok(())
        }
        None => {
            println!();
            println!("No decidable reachable model (empty CAP view or missing logical ids).");
            Ok(())
        }
    }
}

fn parse_provider_model(spec: &str) -> Result<(String, String)> {
    let (p, m) = spec
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("override must be provider/model, got '{spec}'"))?;
    if p.is_empty() || m.is_empty() {
        anyhow::bail!("override must be provider/model, got '{spec}'");
    }
    Ok((p.to_string(), m.to_string()))
}
