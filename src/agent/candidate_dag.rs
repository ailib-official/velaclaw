//! CR-L4-002: candidate DAG validate + L4→L2 fallback + Thought Convergence subset.
//!
//! Library-only in this slice (no default-on host wire — that is CR-L4-003).
//! Reuses the L2 template shell; does not invent a second assemble path or LLM repair.

use crate::agent::dag_runner::{
    parse_dag_json, run_template_dag_with_options, DagManifest, DagRunReport, TemplateRunOptions,
};
use anyhow::{bail, Result};
use serde_json::Value;

/// M3d-aligned fail categories (stable; mirror plans `validate_candidate_dag.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateFailCategory {
    Ok,
    ParseError,
    SchemaValidation,
    UnknownCapability,
    ForbiddenSource,
    GraphIntegrity,
}

impl CandidateFailCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::ParseError => "parse_error",
            Self::SchemaValidation => "schema_validation",
            Self::UnknownCapability => "unknown_capability",
            Self::ForbiddenSource => "forbidden_source",
            Self::GraphIntegrity => "graph_integrity",
        }
    }
}

impl std::fmt::Display for CandidateFailCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Capability tags allowed by L2 `dag-schema.json` v0.1.0.
pub const ALLOWED_CAPABILITY_TAGS: &[&str] = &[
    "high-reasoning",
    "coding",
    "speed",
    "document_understanding",
    "tool_calling",
    "long_context",
];

#[derive(Debug, Clone)]
pub struct CandidateValidateReport {
    pub valid: bool,
    pub category: CandidateFailCategory,
    pub message: String,
    pub dag: Option<DagManifest>,
}

#[derive(Debug, Clone)]
pub struct CandidateRunOptions {
    pub seed_user_message: String,
    pub compact_context: bool,
    /// When candidate schema/graph validation fails, run the L2 fallback template.
    pub fallback_on_schema_fail: bool,
    /// When a validated candidate run aborts, run the L2 fallback template once.
    pub fallback_on_abort: bool,
    /// Passed through to [`TemplateRunOptions::stagnation_limit`] (`0` = off).
    pub stagnation_limit: u32,
}

impl Default for CandidateRunOptions {
    fn default() -> Self {
        Self {
            seed_user_message: String::new(),
            compact_context: false,
            fallback_on_schema_fail: true,
            fallback_on_abort: true,
            stagnation_limit: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CandidateRunReport {
    pub used_fallback: bool,
    pub schema_category: CandidateFailCategory,
    pub fallback_reason: Option<String>,
    pub run: DagRunReport,
}

/// Validate candidate DAG JSON (structural + capability allowlist + forbidden source).
pub fn validate_candidate_dag_json(json: &str) -> CandidateValidateReport {
    let value: Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(err) => {
            return CandidateValidateReport {
                valid: false,
                category: CandidateFailCategory::ParseError,
                message: err.to_string(),
                dag: None,
            };
        }
    };

    if !value.is_object() {
        return CandidateValidateReport {
            valid: false,
            category: CandidateFailCategory::ParseError,
            message: "root must be a JSON object".into(),
            dag: None,
        };
    }

    if let Some(report) = reject_forbidden_source(&value) {
        return report;
    }
    if let Some(report) = reject_unknown_capabilities(&value) {
        return report;
    }

    match parse_dag_json(json) {
        Ok(dag) => CandidateValidateReport {
            valid: true,
            category: CandidateFailCategory::Ok,
            message: String::new(),
            dag: Some(dag),
        },
        Err(err) => {
            let message = err.to_string();
            let category = if message.contains("next") || message.contains("entry") {
                CandidateFailCategory::GraphIntegrity
            } else {
                CandidateFailCategory::SchemaValidation
            };
            CandidateValidateReport {
                valid: false,
                category,
                message,
                dag: None,
            }
        }
    }
}

fn reject_forbidden_source(value: &Value) -> Option<CandidateValidateReport> {
    let nodes = value.get("nodes")?.as_array()?;
    for node in nodes {
        let Some(source) = node.get("source") else {
            continue;
        };
        let kind = source.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        if kind != "documentation" {
            return Some(CandidateValidateReport {
                valid: false,
                category: CandidateFailCategory::ForbiddenSource,
                message: format!("node source.kind '{kind}' is forbidden (documentation only)"),
                dag: None,
            });
        }
    }
    None
}

fn reject_unknown_capabilities(value: &Value) -> Option<CandidateValidateReport> {
    let nodes = value.get("nodes")?.as_array()?;
    for node in nodes {
        let caps = node
            .pointer("/model_selector/capabilities")
            .and_then(|c| c.as_array())?;
        for cap in caps {
            let Some(tag) = cap.as_str() else {
                return Some(CandidateValidateReport {
                    valid: false,
                    category: CandidateFailCategory::UnknownCapability,
                    message: "capability tag must be a string".into(),
                    dag: None,
                });
            };
            if !ALLOWED_CAPABILITY_TAGS.contains(&tag) {
                return Some(CandidateValidateReport {
                    valid: false,
                    category: CandidateFailCategory::UnknownCapability,
                    message: format!("unknown capability tag '{tag}'"),
                    dag: None,
                });
            }
        }
    }
    None
}

/// Run a candidate DAG, falling back to an L2 handwritten template on policy.
///
/// - Schema/graph fail → L2 fallback when `fallback_on_schema_fail` (else error).
/// - Candidate run abort → L2 fallback when `fallback_on_abort` (else error).
/// - Never silently reports success on schema fail without a successful fallback run.
pub fn run_candidate_or_fallback(
    candidate_json: &str,
    fallback_template_json: &str,
    options: &CandidateRunOptions,
) -> Result<CandidateRunReport> {
    let validated = validate_candidate_dag_json(candidate_json);
    let run_opts = TemplateRunOptions {
        stagnation_limit: options.stagnation_limit,
    };

    if !validated.valid {
        if options.fallback_on_schema_fail {
            let fallback = parse_dag_json(fallback_template_json)?;
            let run = run_template_dag_with_options(
                &fallback,
                &options.seed_user_message,
                options.compact_context,
                &run_opts,
            )?;
            emit_l4_fallback(validated.category, "schema_fail");
            return Ok(CandidateRunReport {
                used_fallback: true,
                schema_category: validated.category,
                fallback_reason: Some(format!(
                    "schema_fail:{}:{}",
                    validated.category, validated.message
                )),
                run,
            });
        }
        bail!(
            "candidate DAG rejected ({}): {}",
            validated.category,
            validated.message
        );
    }

    let dag = validated
        .dag
        .expect("valid candidate must carry a parsed DagManifest");
    match run_template_dag_with_options(
        &dag,
        &options.seed_user_message,
        options.compact_context,
        &run_opts,
    ) {
        Ok(run) => {
            emit_l4_pass(&run);
            Ok(CandidateRunReport {
                used_fallback: false,
                schema_category: CandidateFailCategory::Ok,
                fallback_reason: None,
                run,
            })
        }
        Err(err) => {
            if options.fallback_on_abort {
                let fallback = parse_dag_json(fallback_template_json)?;
                let run = run_template_dag_with_options(
                    &fallback,
                    &options.seed_user_message,
                    options.compact_context,
                    &run_opts,
                )?;
                emit_l4_fallback(CandidateFailCategory::Ok, "run_abort");
                Ok(CandidateRunReport {
                    used_fallback: true,
                    schema_category: CandidateFailCategory::Ok,
                    fallback_reason: Some(format!("run_abort:{err}")),
                    run,
                })
            } else {
                Err(err)
            }
        }
    }
}

fn emit_l4_pass(run: &DagRunReport) {
    tracing::info!(
        dag_id = %run.dag_id,
        m3c_pass = true,
        m3e_fallback = false,
        m2_steps = run.steps,
        "candidate_dag_run"
    );
}

fn emit_l4_fallback(category: CandidateFailCategory, reason: &str) {
    tracing::info!(
        m3c_pass = false,
        m3d_category = category.as_str(),
        m3e_fallback = true,
        fallback_reason = reason,
        "candidate_dag_fallback"
    );
}

/// CR-L4-003: host knobs for the opt-in shadow path (default-off).
#[derive(Debug, Clone)]
pub struct CandidateShadowHost {
    pub enabled: bool,
    pub compact_context: bool,
    pub stagnation_limit: u32,
}

impl CandidateShadowHost {
    pub fn from_agent_config(agent: &crate::config::AgentConfig) -> Self {
        Self {
            enabled: agent.candidate_dag_shadow,
            compact_context: agent.compact_context,
            stagnation_limit: agent.candidate_dag_stagnation_limit,
        }
    }
}

/// Run candidate→fallback when `[agent].candidate_dag_shadow` is true; otherwise `Ok(None)`.
///
/// Does not enable a default-on live agent loop. Callers (future CR-L4 host wire /
/// doctor probes that opt in) must pass candidate + fallback JSON explicitly.
pub fn maybe_run_candidate_shadow(
    host: &CandidateShadowHost,
    candidate_json: &str,
    fallback_template_json: &str,
    seed_user_message: &str,
) -> Result<Option<CandidateRunReport>> {
    if !host.enabled {
        tracing::debug!("candidate_dag_shadow disabled; skipping shadow run");
        return Ok(None);
    }
    let options = CandidateRunOptions {
        seed_user_message: seed_user_message.to_string(),
        compact_context: host.compact_context,
        fallback_on_schema_fail: true,
        fallback_on_abort: true,
        stagnation_limit: host.stagnation_limit,
    };
    let report = run_candidate_or_fallback(candidate_json, fallback_template_json, &options)?;
    tracing::info!(
        shadow = true,
        used_fallback = report.used_fallback,
        dag_id = %report.run.dag_id,
        m3d_category = report.schema_category.as_str(),
        "candidate_dag_shadow_run"
    );
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON;
    use crate::config::AgentConfig;

    const VALID_CANDIDATE: &str = r#"{
      "schema_version":"0.1.0",
      "id":"candidate-linear-review",
      "entry":"scan",
      "max_steps":4,
      "nodes":[
        {"id":"scan","task_type":"review","model_selector":{"capabilities":["coding","high-reasoning"]},
         "context_requirements":{"layers":[0,1,2]},"next":"summarize"},
        {"id":"summarize","task_type":"review","model_selector":{"capabilities":["speed"]},
         "context_requirements":{"layers":[0,1]},"next":null}
      ]
    }"#;

    const BAD_CAP: &str = r#"{
      "schema_version":"0.1.0",
      "id":"candidate-bad-cap",
      "entry":"only",
      "max_steps":1,
      "nodes":[
        {"id":"only","task_type":"chat","model_selector":{"capabilities":["super-intelligence"]},
         "context_requirements":{"layers":[0,1]},"next":null}
      ]
    }"#;

    #[test]
    fn validate_candidate_ok() {
        let report = validate_candidate_dag_json(VALID_CANDIDATE);
        assert!(report.valid);
        assert_eq!(report.category, CandidateFailCategory::Ok);
        assert_eq!(report.dag.as_ref().unwrap().id, "candidate-linear-review");
    }

    #[test]
    fn validate_unknown_capability_fail_closed() {
        let report = validate_candidate_dag_json(BAD_CAP);
        assert!(!report.valid);
        assert_eq!(report.category, CandidateFailCategory::UnknownCapability);
    }

    #[test]
    fn schema_fail_falls_back_to_l2_template() {
        let report = run_candidate_or_fallback(
            BAD_CAP,
            CODE_FIX_TEMPLATE_JSON,
            &CandidateRunOptions {
                seed_user_message: "fix null".into(),
                ..CandidateRunOptions::default()
            },
        )
        .unwrap();
        assert!(report.used_fallback);
        assert_eq!(
            report.schema_category,
            CandidateFailCategory::UnknownCapability
        );
        assert!(report.run.success);
        assert_eq!(report.run.dag_id, "code-fix-template");
    }

    #[test]
    fn schema_fail_without_fallback_errors() {
        let err = run_candidate_or_fallback(
            BAD_CAP,
            CODE_FIX_TEMPLATE_JSON,
            &CandidateRunOptions {
                fallback_on_schema_fail: false,
                fallback_on_abort: false,
                ..CandidateRunOptions::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("unknown_capability") || err.contains("rejected"),
            "{err}"
        );
    }

    #[test]
    fn valid_candidate_runs_without_fallback() {
        let report = run_candidate_or_fallback(
            VALID_CANDIDATE,
            CODE_FIX_TEMPLATE_JSON,
            &CandidateRunOptions {
                seed_user_message: "review patch".into(),
                ..CandidateRunOptions::default()
            },
        )
        .unwrap();
        assert!(!report.used_fallback);
        assert!(report.run.success);
        assert_eq!(report.run.dag_id, "candidate-linear-review");
        assert_eq!(report.run.steps, 2);
    }

    #[test]
    fn run_abort_falls_back_to_l2() {
        // Valid schema but max_steps=1 forces abort on multi-node candidate.
        let tight = r#"{
          "schema_version":"0.1.0",
          "id":"candidate-tight",
          "entry":"scan",
          "max_steps":1,
          "nodes":[
            {"id":"scan","task_type":"review","model_selector":{"capabilities":["coding"]},
             "context_requirements":{"layers":[0,1]},"next":"summarize"},
            {"id":"summarize","task_type":"review","model_selector":{"capabilities":["speed"]},
             "context_requirements":{"layers":[0,1]},"next":null}
          ]
        }"#;
        let report = run_candidate_or_fallback(
            tight,
            CODE_FIX_TEMPLATE_JSON,
            &CandidateRunOptions {
                seed_user_message: "x".into(),
                fallback_on_schema_fail: false,
                fallback_on_abort: true,
                ..CandidateRunOptions::default()
            },
        )
        .unwrap();
        assert!(report.used_fallback);
        assert!(report
            .fallback_reason
            .as_deref()
            .unwrap_or("")
            .contains("run_abort"));
        assert_eq!(report.run.dag_id, "code-fix-template");
        assert!(report.run.success);
    }

    #[test]
    fn shadow_host_default_off_is_noop() {
        let host = CandidateShadowHost::from_agent_config(&AgentConfig::default());
        assert!(!host.enabled);
        let out = maybe_run_candidate_shadow(&host, VALID_CANDIDATE, CODE_FIX_TEMPLATE_JSON, "x")
            .unwrap();
        assert!(out.is_none());
    }

    #[test]
    fn shadow_host_opt_in_runs_candidate() {
        let mut agent = AgentConfig::default();
        agent.candidate_dag_shadow = true;
        let host = CandidateShadowHost::from_agent_config(&agent);
        let out = maybe_run_candidate_shadow(
            &host,
            VALID_CANDIDATE,
            CODE_FIX_TEMPLATE_JSON,
            "review patch",
        )
        .unwrap()
        .expect("shadow enabled");
        assert!(!out.used_fallback);
        assert_eq!(out.run.dag_id, "candidate-linear-review");
    }

    #[test]
    fn shadow_host_opt_in_falls_back_on_bad_candidate() {
        let mut agent = AgentConfig::default();
        agent.candidate_dag_shadow = true;
        let host = CandidateShadowHost::from_agent_config(&agent);
        let out = maybe_run_candidate_shadow(&host, BAD_CAP, CODE_FIX_TEMPLATE_JSON, "x")
            .unwrap()
            .expect("shadow enabled");
        assert!(out.used_fallback);
        assert_eq!(out.run.dag_id, "code-fix-template");
    }
}
