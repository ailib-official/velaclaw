//! ORCH-DAG-EMIT-001: doctor probe for schema-strict candidate emit + L2 fallback.

use crate::agent::candidate_dag::CandidateRunOptions;
use crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON;
use crate::orchestration::dag_emit::emit_or_fallback;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Run emit_or_fallback on candidate text/JSON (observe; no LLM generation).
pub fn run_dag_emit(
    candidate: &Path,
    fallback: Option<&Path>,
    message: &str,
    compact: bool,
    stagnation_limit: u32,
) -> Result<()> {
    let candidate_text = fs::read_to_string(candidate)
        .with_context(|| format!("read candidate {}", candidate.display()))?;
    let fallback_owned = match fallback {
        Some(p) => fs::read_to_string(p)
            .with_context(|| format!("read fallback {}", p.display()))?,
        None => CODE_FIX_TEMPLATE_JSON.to_string(),
    };

    let options = CandidateRunOptions {
        seed_user_message: message.to_string(),
        compact_context: compact,
        fallback_on_schema_fail: true,
        fallback_on_abort: true,
        stagnation_limit,
    };

    println!("🩺 VelaClaw Doctor — DAG Emit (ORCH-DAG-EMIT-001)");
    println!("  candidate: {}", candidate.display());
    println!(
        "  fallback:  {}",
        fallback
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(embedded code-fix template)".into())
    );

    let report = emit_or_fallback(&candidate_text, &fallback_owned, &options)?;
    println!("  used_fallback:   {}", report.used_fallback);
    println!("  m3d_category:    {}", report.schema_category.as_str());
    println!("  dag_id:          {}", report.run.dag_id);
    println!("  steps:           {}", report.run.steps);
    Ok(())
}
