//! ORCH-DAG-EMIT: schema-strict candidate handling + opt-in LLM plan emit.
//!
//! Does **not** enable default-on chat. Planning requires `[agent].candidate_dag_emit`
//! (or doctor `--force`).

use crate::agent::candidate_dag::{
    run_candidate_or_fallback, CandidateFailCategory, CandidateRunOptions, CandidateRunReport,
};
use crate::providers::{ChatMessage, ChatRequest, Provider};
use anyhow::Result;

/// Compact schema constraint for the planner model (not the full JSON Schema document).
pub const DAG_PLAN_SYSTEM_PROMPT: &str = r#"You are a DAG planner. Reply with ONLY one JSON object (no markdown fences, no prose).
The object MUST use schema_version "0.1.0" and include:
- id (string), entry (string node id), max_steps (number)
- nodes: array of { id, task_type, model_selector: { capabilities: string[] }, next: string|null }
capabilities MUST be tags such as coding, tool_calling, high-reasoning, speed, document_understanding.
Do not include executable scripts. Linear or simple DAGs only."#;

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
    let json =
        extract_json_object(candidate_text).unwrap_or_else(|| candidate_text.trim().to_string());
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

/// Opt-in: call planner model to generate DAG JSON, then validate → L2 fallback.
///
/// When `emit_enabled` is false, returns `Ok(None)` without calling the provider.
pub async fn plan_emit_or_fallback(
    emit_enabled: bool,
    provider: &dyn Provider,
    planner_model: &str,
    user_task: &str,
    fallback_template_json: &str,
    options: &CandidateRunOptions,
    temperature: f64,
) -> Result<Option<CandidateRunReport>> {
    if !emit_enabled {
        tracing::debug!("candidate_dag_emit disabled; skipping plan emit");
        return Ok(None);
    }

    let messages = [
        ChatMessage::system(DAG_PLAN_SYSTEM_PROMPT),
        ChatMessage::user(format!(
            "User task:\n{user_task}\n\nProduce the DAG JSON object now."
        )),
    ];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
    };
    let response = provider.chat(request, planner_model, temperature).await?;
    let text = response.text_or_empty();
    tracing::info!(
        target: "dag_plan_emit",
        model = %planner_model,
        chars = text.len(),
        "planner model returned candidate text"
    );
    Ok(Some(emit_or_fallback(
        text,
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
    use crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON;
    use crate::providers::ChatResponse;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct ScriptedPlanProvider {
        text: Mutex<Option<String>>,
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl Provider for ScriptedPlanProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(String::new())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ChatResponse> {
            *self.calls.lock().unwrap() += 1;
            let text = self.text.lock().unwrap().clone().unwrap_or_default();
            Ok(ChatResponse {
                text: Some(text),
                tool_calls: vec![],
            })
        }
    }

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

    #[tokio::test]
    async fn plan_emit_skips_when_disabled() {
        let provider = ScriptedPlanProvider {
            text: Mutex::new(Some("{}".into())),
            calls: Mutex::new(0),
        };
        let options = CandidateRunOptions {
            seed_user_message: "t".into(),
            compact_context: false,
            fallback_on_schema_fail: true,
            fallback_on_abort: true,
            stagnation_limit: 0,
        };
        let out = plan_emit_or_fallback(
            false,
            &provider,
            "m",
            "task",
            CODE_FIX_TEMPLATE_JSON,
            &options,
            0.0,
        )
        .await
        .unwrap();
        assert!(out.is_none());
        assert_eq!(*provider.calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn plan_emit_invalid_json_falls_back_to_l2() {
        let provider = ScriptedPlanProvider {
            text: Mutex::new(Some("not-json at all".into())),
            calls: Mutex::new(0),
        };
        let options = CandidateRunOptions {
            seed_user_message: "t".into(),
            compact_context: false,
            fallback_on_schema_fail: true,
            fallback_on_abort: true,
            stagnation_limit: 0,
        };
        let out = plan_emit_or_fallback(
            true,
            &provider,
            "m",
            "fix the bug",
            CODE_FIX_TEMPLATE_JSON,
            &options,
            0.0,
        )
        .await
        .unwrap()
        .expect("report");
        assert_eq!(*provider.calls.lock().unwrap(), 1);
        assert!(out.used_fallback);
    }
}
