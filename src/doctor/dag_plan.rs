//! ORCH-DAG-EMIT-002: doctor probe — LLM plan → validate → L2 (opt-in / --force).

use crate::agent::candidate_dag::CandidateRunOptions;
use crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON;
use crate::config::Config;
use crate::orchestration::dag_emit::plan_emit_or_fallback;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Call planner model when emit is enabled (or `--force`), then emit_or_fallback.
pub async fn run_dag_plan(
    config: &Config,
    message: &str,
    fallback: Option<&Path>,
    force: bool,
    compact: bool,
    stagnation_limit: u32,
    temperature: f64,
) -> Result<()> {
    let emit_enabled = force || config.agent.candidate_dag_emit;

    println!("🩺 VelaClaw Doctor — DAG Plan Emit (ORCH-DAG-EMIT-002)");
    println!(
        "  flag candidate_dag_emit: {}",
        config.agent.candidate_dag_emit
    );
    println!("  observe force-on:         {force}");
    println!("  emit_enabled:             {emit_enabled}");
    println!("  (planning ≠ default Agent::turn chat path)");
    println!();

    if !emit_enabled {
        println!("ℹ️  Plan emit disabled (default-off).");
        println!("   Enable with `[agent].candidate_dag_emit = true`, or pass `--force`.");
        return Ok(());
    }

    let fallback_owned = match fallback {
        Some(p) => {
            fs::read_to_string(p).with_context(|| format!("read fallback {}", p.display()))?
        }
        None => CODE_FIX_TEMPLATE_JSON.to_string(),
    };

    let options = CandidateRunOptions {
        seed_user_message: message.to_string(),
        compact_context: compact,
        fallback_on_schema_fail: true,
        fallback_on_abort: true,
        stagnation_limit,
    };

    let runtime_opts = crate::providers::ProviderRuntimeOptions::default();
    let (execution, provider) = crate::execution::bootstrap_routed_provider(config, &runtime_opts)?;
    let planner_model = execution.logical_model_id().to_string();
    println!("  planner_model:            {planner_model}");

    let report = plan_emit_or_fallback(
        true,
        provider.as_ref(),
        &planner_model,
        message,
        &fallback_owned,
        &options,
        temperature,
    )
    .await?
    .expect("emit_enabled");

    println!("  used_fallback:            {}", report.used_fallback);
    println!(
        "  m3d_category:             {}",
        report.schema_category.as_str()
    );
    println!("  dag_id:                   {}", report.run.dag_id);
    println!("  steps:                    {}", report.run.steps);
    Ok(())
}
