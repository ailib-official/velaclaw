//! CR-L4-003: diagnostic entry for candidate DAG shadow observe.
//!
//! Runs validate + L2 fallback via `agent::candidate_dag` without an LLM.
//! Independent of `[agent].candidate_dag_shadow` (that flag gates host
//! `maybe_run_candidate_shadow`); doctor always executes the probe.

use crate::agent::candidate_dag::{
    run_candidate_or_fallback, CandidateRunOptions, CandidateRunReport,
};
use crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON;
use anyhow::{Context, Result};
use std::path::Path;

/// Load candidate (+ optional fallback) JSON, run shadow probe, print a report.
pub fn run_candidate_dag_fixture(
    candidate_path: &Path,
    fallback_path: Option<&Path>,
    seed_user_message: &str,
    compact_context: bool,
    stagnation_limit: u32,
) -> Result<CandidateRunReport> {
    let candidate = std::fs::read_to_string(candidate_path)
        .with_context(|| format!("read candidate DAG {}", candidate_path.display()))?;
    let fallback = match fallback_path {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("read fallback template {}", path.display()))?,
        None => CODE_FIX_TEMPLATE_JSON.to_string(),
    };

    println!("🩺 Candidate DAG shadow check (CR-L4-003)");
    println!("  candidate: {}", candidate_path.display());
    println!(
        "  fallback:  {}",
        fallback_path
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(embedded code-fix-template)".into())
    );
    println!("  stagnation_limit: {stagnation_limit}");
    println!();

    let options = CandidateRunOptions {
        seed_user_message: seed_user_message.to_string(),
        compact_context,
        fallback_on_schema_fail: true,
        fallback_on_abort: true,
        stagnation_limit,
    };

    match run_candidate_or_fallback(&candidate, &fallback, &options) {
        Ok(report) => {
            print_report(&report);
            if report.used_fallback {
                println!("  ⚠️  used L2 fallback (schema fail or run abort)");
            } else {
                println!("  ✅ candidate DAG walk succeeded without fallback");
            }
            Ok(report)
        }
        Err(err) => {
            eprintln!("  ❌ candidate DAG shadow aborted: {err}");
            Err(err)
        }
    }
}

fn print_report(report: &CandidateRunReport) {
    println!("  schema_category: {}", report.schema_category);
    println!("  used_fallback:   {}", report.used_fallback);
    if let Some(reason) = &report.fallback_reason {
        println!("  fallback_reason: {reason}");
    }
    println!("  dag_id:          {}", report.run.dag_id);
    println!("  success:         {}", report.run.success);
    println!("  steps:           {}", report.run.steps);
    if let Some(reason) = &report.run.abort_reason {
        println!("  abort:           {reason}");
    }
    for visit in &report.run.visits {
        println!(
            "  visit:           {} ({}) caps={:?} assembled={}",
            visit.node_id, visit.task_type, visit.capabilities, visit.assembled_messages
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_json(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp");
        f.write_all(json.as_bytes()).expect("write");
        f
    }

    #[test]
    fn doctor_candidate_dag_happy_path() {
        let candidate = write_json(
            r#"{
              "schema_version":"0.1.0",
              "id":"candidate-linear-review",
              "entry":"scan",
              "max_steps":4,
              "nodes":[
                {"id":"scan","task_type":"review","model_selector":{"capabilities":["coding"]},
                 "context_requirements":{"layers":[0,1]},"next":"summarize"},
                {"id":"summarize","task_type":"review","model_selector":{"capabilities":["speed"]},
                 "context_requirements":{"layers":[0,1]},"next":null}
              ]
            }"#,
        );
        let report =
            run_candidate_dag_fixture(candidate.path(), None, "doctor probe", false, 0).unwrap();
        assert!(!report.used_fallback);
        assert!(report.run.success);
        assert_eq!(report.run.dag_id, "candidate-linear-review");
    }

    #[test]
    fn doctor_candidate_dag_falls_back_on_bad_cap() {
        let candidate = write_json(
            r#"{
              "schema_version":"0.1.0",
              "id":"bad",
              "entry":"only",
              "max_steps":1,
              "nodes":[
                {"id":"only","task_type":"chat","model_selector":{"capabilities":["super-intelligence"]},
                 "context_requirements":{"layers":[0,1]},"next":null}
              ]
            }"#,
        );
        let report =
            run_candidate_dag_fixture(candidate.path(), None, "doctor probe", false, 0).unwrap();
        assert!(report.used_fallback);
        assert_eq!(report.run.dag_id, "code-fix-template");
        assert!(report.run.success);
    }
}
