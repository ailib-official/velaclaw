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

/// CR-L4-004: stable M3c/d/e structured-log contract (logs only — no Prometheus/Grafana gate).
///
/// Field names and M3d category strings are part of the public operator contract.
/// See `docs/l4-m3-metrics.md`.
pub mod m3_metrics {
    use super::CandidateFailCategory;

    /// Target name when a candidate DAG run succeeds without L2 fallback (M3c pass).
    pub const EVENT_PASS: &str = "candidate_dag_run";
    /// Target name when L4→L2 fallback fires (M3e) and/or schema fail is recorded (M3d).
    pub const EVENT_FALLBACK: &str = "candidate_dag_fallback";
    /// Target name for opt-in shadow host observe (CR-L4-003); carries the same M3 fields.
    pub const EVENT_SHADOW: &str = "candidate_dag_shadow_run";
    /// Target name when schema/graph validation fails and fallback is disabled (M3d only).
    pub const EVENT_SCHEMA_FAIL: &str = "candidate_dag_schema_fail";

    pub const FIELD_M3C_PASS: &str = "m3c_pass";
    pub const FIELD_M3D_CATEGORY: &str = "m3d_category";
    pub const FIELD_M3E_FALLBACK: &str = "m3e_fallback";
    pub const FIELD_FALLBACK_REASON: &str = "fallback_reason";
    pub const FIELD_DAG_ID: &str = "dag_id";
    pub const FIELD_M2_STEPS: &str = "m2_steps";
    pub const FIELD_SHADOW: &str = "shadow";

    /// Stable M3d category vocabulary (aligned with CR-L4-001 / `CandidateFailCategory`).
    pub const M3D_CATEGORIES: &[&str] = &[
        "ok",
        "parse_error",
        "schema_validation",
        "unknown_capability",
        "forbidden_source",
        "graph_integrity",
    ];

    #[must_use]
    pub fn m3d_category_is_stable(category: &str) -> bool {
        M3D_CATEGORIES.contains(&category)
    }

    #[must_use]
    pub fn all_fail_categories() -> [CandidateFailCategory; 6] {
        [
            CandidateFailCategory::Ok,
            CandidateFailCategory::ParseError,
            CandidateFailCategory::SchemaValidation,
            CandidateFailCategory::UnknownCapability,
            CandidateFailCategory::ForbiddenSource,
            CandidateFailCategory::GraphIntegrity,
        ]
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
        emit_l4_schema_fail(validated.category, &validated.message);
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
    // M3c pass + explicit M3d=ok + M3e=false (CR-L4-004 field contract).
    tracing::info!(
        dag_id = %run.dag_id,
        m3c_pass = true,
        m3d_category = CandidateFailCategory::Ok.as_str(),
        m3e_fallback = false,
        m2_steps = run.steps,
        "{}",
        m3_metrics::EVENT_PASS
    );
}

fn emit_l4_fallback(category: CandidateFailCategory, reason: &str) {
    // M3e fallback; M3d category when schema/graph caused the path (else `ok` on run_abort).
    tracing::info!(
        m3c_pass = false,
        m3d_category = category.as_str(),
        m3e_fallback = true,
        fallback_reason = reason,
        "{}",
        m3_metrics::EVENT_FALLBACK
    );
}

fn emit_l4_schema_fail(category: CandidateFailCategory, message: &str) {
    // M3d-only path: validation failed and L2 fallback was not taken.
    tracing::info!(
        m3c_pass = false,
        m3d_category = category.as_str(),
        m3e_fallback = false,
        fallback_reason = message,
        "{}",
        m3_metrics::EVENT_SCHEMA_FAIL
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
        m3c_pass = !report.used_fallback,
        m3d_category = report.schema_category.as_str(),
        m3e_fallback = report.used_fallback,
        m2_steps = report.run.steps,
        "{}",
        m3_metrics::EVENT_SHADOW
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
    fn m3d_category_strings_are_stable_contract() {
        assert_eq!(
            m3_metrics::M3D_CATEGORIES.len(),
            m3_metrics::all_fail_categories().len()
        );
        for cat in m3_metrics::all_fail_categories() {
            assert!(
                m3_metrics::m3d_category_is_stable(cat.as_str()),
                "unstable category string: {}",
                cat.as_str()
            );
        }
        assert_eq!(CandidateFailCategory::Ok.as_str(), "ok");
        assert_eq!(
            CandidateFailCategory::UnknownCapability.as_str(),
            "unknown_capability"
        );
        assert_eq!(m3_metrics::EVENT_PASS, "candidate_dag_run");
        assert_eq!(m3_metrics::EVENT_FALLBACK, "candidate_dag_fallback");
        assert_eq!(m3_metrics::EVENT_SCHEMA_FAIL, "candidate_dag_schema_fail");
        assert_eq!(m3_metrics::EVENT_SHADOW, "candidate_dag_shadow_run");
        assert_eq!(m3_metrics::FIELD_M3C_PASS, "m3c_pass");
        assert_eq!(m3_metrics::FIELD_M3D_CATEGORY, "m3d_category");
        assert_eq!(m3_metrics::FIELD_M3E_FALLBACK, "m3e_fallback");
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
