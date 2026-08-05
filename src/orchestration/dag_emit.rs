//! ORCH-DAG-EMIT-001: opt-in schema-strict candidate handling (validate → L2).
//!
//! Does **not** enable default-on chat. LLM generation remains caller-supplied
//! JSON (or future gated prompt); this module enforces schema + fallback.

use crate::agent::candidate_dag::{
    run_candidate_or_fallback, CandidateFailCategory, CandidateRunOptions, CandidateRunReport,
};
use anyhow::Result;

/// Extract a JSON object from model text (fenced ```json or raw `{...}`).
#[must_use]
pub fn extract_json_object(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after
            .strip_prefix("json")
            .or_else(|| after.strip_prefix("JSON"))
            .unwrap_or(after);
        let after = after.trim_start_matches('\n');
        if let Some(end) = after.find("```") {
            let body = after[..end].trim();
            if body.starts_with('{') {
                return Some(body.to_string());
            }
        }
    }
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    if end > start {
        Some(trimmed[start..=end].to_string())
    } else {
        None
    }
}

/// Validate candidate JSON (or extracted object) and run with L2 fallback.
pub fn emit_or_fallback(
    candidate_text: &str,
    fallback_template_json: &str,
    options: &CandidateRunOptions,
) -> Result<CandidateRunReport> {
    let json = extract_json_object(candidate_text)
        .unwrap_or_else(|| candidate_text.trim().to_string());
    run_candidate_or_fallback(&json, fallback_template_json, options)
}

/// When `[agent].candidate_dag_emit` is true, run emit_or_fallback; else `Ok(None)`.
pub fn maybe_emit_candidate(
    emit_enabled: bool,
    candidate_text: &str,
    fallback_template_json: &str,
    options: &CandidateRunOptions,
) -> Result<Option<CandidateRunReport>> {
    if !emit_enabled {
        tracing::debug!("candidate_dag_emit disabled; skipping emit path");
        return Ok(None);
    }
    Ok(Some(emit_or_fallback(
        candidate_text,
        fallback_template_json,
        options,
    )?))
}

#[must_use]
pub fn fail_category_name(c: CandidateFailCategory) -> &'static str {
    c.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::candidate_dag::CandidateRunOptions;

    #[test]
    fn extracts_fenced_json() {
        let text = "here\n```json\n{\"id\":\"x\"}\n```\n";
        assert_eq!(extract_json_object(text).as_deref(), Some("{\"id\":\"x\"}"));
    }

    #[test]
    fn extracts_raw_object() {
        assert_eq!(
            extract_json_object("prefix {\"a\":1} suffix").as_deref(),
            Some("{\"a\":1}")
        );
    }

    #[test]
    fn maybe_emit_respects_default_off() {
        let options = CandidateRunOptions {
            seed_user_message: "t".into(),
            compact_context: false,
            fallback_on_schema_fail: true,
            fallback_on_abort: true,
            stagnation_limit: 0,
        };
        let out = maybe_emit_candidate(false, "{}", "{}", &options).unwrap();
        assert!(out.is_none());
    }
}
