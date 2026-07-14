//! CR-L2-004: diagnostic entry for handwritten template DAG fixtures.
//!
//! Runs the host strategy shell (`agent::dag_runner`) without an LLM.
//! Independent of `[agent].template_dag` (that flag gates future runtime wiring).

use crate::agent::dag_runner::{load_dag_path, run_template_dag, DagRunReport};
use anyhow::Result;
use std::path::Path;

/// Load a fixture from `path`, run the template shell, print a short report.
///
/// Returns `Ok` on success; returns the runner error (fail-closed) on abort.
pub fn run_template_dag_fixture(
    path: &Path,
    seed_user_message: &str,
    compact_context: bool,
) -> Result<DagRunReport> {
    let dag = load_dag_path(path)?;
    println!("🩺 Template DAG check");
    println!("  fixture: {}", path.display());
    println!("  dag_id:  {}", dag.id);
    println!("  entry:   {}", dag.entry);
    println!("  nodes:   {}", dag.nodes.len());
    println!("  max_steps: {}", dag.max_steps);
    println!();

    match run_template_dag(&dag, seed_user_message, compact_context) {
        Ok(report) => {
            print_report(&report);
            println!("  ✅ template DAG walk succeeded");
            Ok(report)
        }
        Err(err) => {
            eprintln!("  ❌ template DAG aborted: {err}");
            Err(err)
        }
    }
}

fn print_report(report: &DagRunReport) {
    println!("  success: {}", report.success);
    println!("  steps:   {}", report.steps);
    if let Some(reason) = &report.abort_reason {
        println!("  abort:   {reason}");
    }
    for visit in &report.visits {
        println!(
            "  visit:   {} ({}) caps={:?} assembled={}",
            visit.node_id, visit.task_type, visit.capabilities, visit.assembled_messages
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_fixture(json: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp fixture");
        f.write_all(json.as_bytes()).expect("write fixture");
        f
    }

    #[test]
    fn doctor_template_dag_happy_path() {
        let f = write_fixture(CODE_FIX_TEMPLATE_JSON);
        let report = run_template_dag_fixture(f.path(), "fix the null check", false).unwrap();
        assert!(report.success);
        assert_eq!(report.steps, 3);
        assert_eq!(report.dag_id, "code-fix-template");
    }

    #[test]
    fn doctor_template_dag_fail_closed_max_steps() {
        let mut dag: serde_json::Value =
            serde_json::from_str(CODE_FIX_TEMPLATE_JSON).expect("parse fixture");
        dag["max_steps"] = serde_json::json!(1);
        let f = write_fixture(&dag.to_string());
        let err = run_template_dag_fixture(f.path(), "x", false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("max_steps"), "{err}");
    }
}
