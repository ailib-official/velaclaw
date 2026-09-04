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
pub const DAG_PLAN_SYSTEM_PROMPT: &str = r#"You are a DAG planner. Reply with ONLY one JSON object (no markdown fences, no prose, no tool calls).
The object MUST use schema_version "0.1.0" and include:
- id (string), entry (string node id), max_steps (number, <= 8)
- nodes: 1 to 8 items of { id, task_type, model_selector: { capabilities: string[] }, next: string|null, context_requirements?: { layers: number[], retrieve?: object[] } }
Choose the node count from THIS task's deliverables (1–8), not from whether capabilities match. One node only when the user asked for a single result (one greeting is not a DAG — the host skips you; one file patch; one yes/no). If they asked for several independent results (service up? node health? a named route probe?), give one node per deliverable even when every node is tool_calling. Do not collapse "A then B, especially C" into one node.
Each node is a verifiable artifact state change, not a shell action. Work backward from the operator-visible deliverable. The host writes the user-facing conclusion after the last node. Do not invent inspect/diagnose/report splits, empty "gather context" nodes, or a final summarize hop unless the user asked for a written report as an artifact.
Each node lists ONE primary capability first (optional extras after). Tags: coding (patches/shell), tool_calling (status/checks), high-reasoning (analysis that needs a reasoning family), speed (cheap/short), document_understanding (read/summarize). Different work → different first tags so Contact can route to different [[model_routes]] families. Do not name providers or model IDs.
Do not pad every node with coding+tool_calling. Runtime already injects workspace retrieve and the previous node's artifact.
The graph MUST be a single linear chain: entry walks next until null and covers every node (no branches, no unused nodes).
Each work node must finish with few tool rounds: batch related shell into one command (`&&` / pipes / one remote ssh wrapping several checks). Do not include executable scripts.

Example (one hop — a single ops check):
{"schema_version":"0.1.0","id":"ops-one","entry":"check","max_steps":8,"nodes":[{"id":"check","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"next":null}]}

Example (three hops — service, node health, named-route probe; same capability is OK):
{"schema_version":"0.1.0","id":"proxy-health","entry":"service","max_steps":8,"nodes":[{"id":"service","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"next":"nodes"},{"id":"nodes","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"next":"google-route"},{"id":"google-route","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"next":null}]}

Example (two hops — code then a cheap verify):
{"schema_version":"0.1.0","id":"patch-verify","entry":"patch","max_steps":8,"nodes":[{"id":"patch","task_type":"write","model_selector":{"capabilities":["coding"]},"next":"verify"},{"id":"verify","task_type":"ops-check","model_selector":{"capabilities":["speed"]},"context_requirements":{"layers":[3],"retrieve":[{"kind":"tool_result"}]},"next":null}]}"#;

/// Tool-free planner turn: one system prompt + user task → raw model text.
pub async fn planner_chat_text(
    provider: &dyn Provider,
    planner_model: &str,
    user_task: &str,
    temperature: f64,
) -> Result<String> {
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
        target: "bounded_dag_planner",
        model = %planner_model,
        chars = text.len(),
        "planner model returned candidate text"
    );
    Ok(text.to_string())
}

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

    let text = planner_chat_text(provider, planner_model, user_task, temperature).await?;
    Ok(Some(emit_or_fallback(
        &text,
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
    fn planner_prompt_splits_by_deliverable_not_capability() {
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("proxy-health"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("Do not collapse"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("one node per deliverable"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("host writes the user-facing conclusion"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("verifiable artifact"));
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
