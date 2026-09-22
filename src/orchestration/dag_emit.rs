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

Capability tags — one primary first, optional extras after. Use only these tokens or the declared aliases:
high-reasoning, coding, speed, document_understanding, tool_calling, long_context.
aliases of the same tag tool_calling: tools, shell.exec, file.read, glob.search.
Do not invent other tags. Do not name providers or model IDs. Do not pad every node with coding+tool_calling.

Filled I (Σ-shaped):
- tool_direct artifact = one admit-safe invoke the host can run as-is. Legal: a simple argv (pwd, ls) or ssh <alias> <simple argv>. A pipe `|` between simple programs is allowed. Forbidden in I: $(), backticks, ${, <(, >(, unquoted redirects, tee, find -exec, variable assignment, &&, ||, ;, xargs, sh -c, bash -c, and wrapping several checks in one ssh. Counting, parsing, and interpretation are a later llm_cognition node (work-description I), not a script inside the command. A caption is not a command.
- llm_cognition artifact = a work description, not an invoke.

Node count follows this task's deliverables (1–8), not whether capabilities match. One node only when the user asked for a single result (a greeting is not a DAG — the host skips you). Several independent results → one node per deliverable, each with filled I. Do not stuff every result into one mega-I. Do not emit a path-only object with no nodes. Do not invent inspect/diagnose/report splits or empty gather-context nodes. The host Asks when no node has executable I.

Each node is a verifiable artifact state change. Work backward from the operator-visible deliverable. The host writes the user-facing conclusion after the last node. Runtime already injects workspace retrieve and the previous node's artifact.

Inspect/list/status that allowed tools can finish: capabilities ["tool_calling"], sigma tool_direct, artifact one admit-safe invoke. A named host in the user text → locus remote:<alias> and artifact "ssh <alias> <simple>". Do not invent a hostname. Do not wrap those checks in coding LLM hops.

The graph MUST be a single linear chain: entry walks next until null and covers every node (no branches, no unused nodes).
Optional node fields: sigma ("llm_cognition" or "tool_direct") and locus ("workspace" or "remote:<alias>").

Example (one hop — a single ops check with I):
{"schema_version":"0.1.0","id":"ops-one","entry":"check","max_steps":8,"nodes":[{"id":"check","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"sigma":"tool_direct","artifact":"pwd","next":null}]}

Example (two hops — tool_direct I is a command, cognition I is a work description):
{"schema_version":"0.1.0","id":"two-filled","entry":"check","max_steps":8,"nodes":[{"id":"check","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"sigma":"tool_direct","artifact":"pwd","next":"write"},{"id":"write","task_type":"write","model_selector":{"capabilities":["high-reasoning"]},"sigma":"llm_cognition","artifact":"write the analysis report","next":null}]}"#;

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
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("two-filled"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("one node per deliverable"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("xargs"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("sh -c"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("one admit-safe invoke"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("wrapping several checks"));
        assert!(!DAG_PLAN_SYSTEM_PROMPT.contains("batch related shell"));
        assert!(!DAG_PLAN_SYSTEM_PROMPT.contains("one node whose I covers every result"));
        assert!(!DAG_PLAN_SYSTEM_PROMPT.contains("read the requested sources"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("host writes the user-facing conclusion"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("verifiable artifact"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("tool_direct"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("remote:<alias>"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("\"artifact\":\"pwd\""));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("Do not emit a path-only object with no nodes"));
        assert!(DAG_PLAN_SYSTEM_PROMPT.contains("aliases of the same tag"));
        assert!(!DAG_PLAN_SYSTEM_PROMPT.contains("collapses graphs without two such I values"));
        let two = r#"{"schema_version":"0.1.0","id":"two-filled","entry":"check","max_steps":8,"nodes":[{"id":"check","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"sigma":"tool_direct","artifact":"pwd","next":"write"},{"id":"write","task_type":"write","model_selector":{"capabilities":["high-reasoning"]},"sigma":"llm_cognition","artifact":"write the analysis report","next":null}]}"#;
        let report = crate::agent::candidate_dag::validate_candidate_dag_json(two);
        assert!(report.valid, "{}", report.message);
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
