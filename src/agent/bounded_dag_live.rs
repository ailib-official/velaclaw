//! Live bounded DAG: planner node (tool-free chat) → validate → work nodes.
//!
//! When `[agent].bounded_dag_live` is on and `bounded_dag_path` is empty, the
//! session-default model emits a linear L2 JSON via a single `Provider::chat`
//! (no tools). Invalid / non-linear output retries once, then falls back to
//! the handwritten code-fix template. Successful plans are cached per session;
//! fallback graphs are not cached. A non-empty `bounded_dag_path` skips the
//! planner (operator-fixed graph).
//!
//! 有界 DAG live：Planner 是无工具单次 chat；校验失败重试一次再回退手写夹具。

use super::bounded_dag::{format_preview, linear_node_ids, load_bounded_dag};
use super::bounded_dag_context::contact_for_node;
use super::candidate_dag::validate_candidate_dag_json;
use super::dag_runner::{DagManifest, CODE_FIX_TEMPLATE_JSON};
use super::host_phase::HostPhase;
use crate::memory::{Memory, MemoryCategory};
use crate::orchestration::dag_emit::{
    extract_json_object, planner_chat_text, DAG_PLAN_SYSTEM_PROMPT,
};
use crate::providers::{ChatMessage, ChatRequest, Provider};
use anyhow::Result;
use std::fmt::Write;
use std::path::Path;

pub const PLANNED_DAG_KEY_PREFIX: &str = "dag_plan:";
const MAX_SESSION_DAG_FORGET: usize = 32;

/// Short confirmations that mean "run the already previewed graph" (any domain).
const APPROVAL_TOKENS: &[&str] = &[
    "ok", "okay", "yes", "y", "go", "build", "approve", "approved", "proceed", "lgtm", "run",
    "start", "sure", "agree", "同意", "做吧", "执行", "开始", "可以", "好的", "确认", "行",
];

pub fn planned_dag_key(session_id: &str) -> String {
    format!("{PLANNED_DAG_KEY_PREFIX}{session_id}")
}

pub const GRAPH_USER_KEY_PREFIX: &str = "dag_user:";

pub fn graph_user_key(session_id: &str) -> String {
    format!("{GRAPH_USER_KEY_PREFIX}{session_id}")
}

/// Persist the Plan-time user task for work-node USER TASK slots.
pub async fn store_graph_user_task(
    mem: &dyn Memory,
    session_id: &str,
    user_task: &str,
) -> Result<()> {
    let trimmed = user_task.trim();
    if trimmed.is_empty() || is_build_approval(trimmed) {
        return Ok(());
    }
    mem.store(
        &graph_user_key(session_id),
        trimmed,
        crate::memory::MemoryCategory::Daily,
        Some(session_id),
    )
    .await
}

pub async fn load_graph_user_task(mem: &dyn Memory, session_id: &str) -> Result<Option<String>> {
    Ok(mem
        .get(&graph_user_key(session_id))
        .await?
        .map(|e| e.content))
}

/// True when the user is confirming a previewed DAG rather than changing the task.
#[must_use]
pub fn is_build_approval(raw: &str) -> bool {
    let tokens: Vec<String> = raw
        .split(|c: char| !(c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(&c)))
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if tokens.is_empty() {
        return true;
    }
    let chars: usize = tokens.iter().map(|t| t.chars().count()).sum();
    if chars > 32 {
        return false;
    }
    let joined = tokens.join("");
    if joined == "doit" || joined == "goahead" || joined == "approvebuild" {
        return true;
    }
    tokens.iter().all(|t| APPROVAL_TOKENS.contains(&t.as_str()))
}

/// Prefer Plan-time user text; skip approval tokens and retrieve blobs.
pub fn user_task_from_history(history: &[ChatMessage], current: &str) -> String {
    let current = current.trim();
    if !current.is_empty() && !is_build_approval(current) {
        return current.to_string();
    }
    for message in history.iter().rev() {
        if message.role != "user" {
            continue;
        }
        let body = message.content.trim();
        if body.is_empty()
            || body.starts_with("[dag_artifact")
            || body.starts_with("USER TASK")
            || is_build_approval(body)
        {
            continue;
        }
        return body.to_string();
    }
    current.to_string()
}

/// Stored Plan-time task, else last non-approval user in history.
pub async fn work_node_user_task(
    mem: &dyn Memory,
    session_id: &str,
    history: &[ChatMessage],
    current: &str,
) -> String {
    if let Ok(Some(stored)) = load_graph_user_task(mem, session_id).await {
        let trimmed = stored.trim();
        if !trimmed.is_empty() && !is_build_approval(trimmed) {
            return trimmed.to_string();
        }
    }
    user_task_from_history(history, current)
}

/// Build + short approval reuses `dag_plan:<session>`. Plan or a new task invalidates.
#[must_use]
pub fn should_reuse_cached_dag(host_phase: HostPhase, user_message: &str) -> bool {
    host_phase == HostPhase::Build && is_build_approval(user_message)
}

/// Drop cached plan + node artifacts for this session (bounded forget count).
pub async fn clear_session_dag_state(mem: &dyn Memory, session_id: &str) -> Result<()> {
    let plan_key = planned_dag_key(session_id);
    let user_key = graph_user_key(session_id);
    let _ = mem.forget(&plan_key).await;
    let _ = mem.forget(&user_key).await;
    let prefix = format!("dag_art:{session_id}:");
    let listed = mem
        .list(Some(&MemoryCategory::Daily), Some(session_id))
        .await
        .unwrap_or_default();
    for (i, entry) in listed.into_iter().enumerate() {
        if i >= MAX_SESSION_DAG_FORGET {
            break;
        }
        if entry.key == plan_key || entry.key == user_key || entry.key.starts_with(&prefix) {
            let _ = mem.forget(&entry.key).await;
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct PlannedLiveDag {
    pub dag: DagManifest,
    pub order: Vec<String>,
    pub used_fallback: bool,
    pub source: &'static str,
}

impl PlannedLiveDag {
    pub fn preview_text(&self) -> String {
        self.preview_with_contact("", &[])
    }

    /// Plan chrome: graph plus per-node Contact (hint → provider family).
    pub fn preview_with_contact(&self, default_model: &str, available_hints: &[String]) -> String {
        let mut out = String::new();
        if self.used_fallback {
            out.push_str(
                "Planner output was not a valid linear L2 DAG; using handwritten fallback.\n\n",
            );
        } else {
            let _ = write!(out, "Planner accepted linear DAG `{}`.\n\n", self.dag.id);
        }
        out.push_str(&format_preview(&self.dag, &self.order));
        if default_model.is_empty() && available_hints.is_empty() {
            return out;
        }
        out.push_str("\nContact (capability → route; planner stays on session default):\n");
        let by_id: std::collections::HashMap<&str, _> =
            self.dag.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
        for id in &self.order {
            let Some(node) = by_id.get(id.as_str()) else {
                continue;
            };
            let contact = contact_for_node(node, default_model, available_hints, None);
            let _ = writeln!(out, "- {}  {}", node.id, contact.observe_line());
        }
        out
    }
}

/// Path override or previously stored plan. `None` means the caller should run the planner node.
pub async fn try_cached_or_fixed_live_dag(
    agent: &crate::config::AgentConfig,
    mem: &dyn Memory,
    session_id: &str,
) -> Result<Option<PlannedLiveDag>> {
    if !agent.bounded_dag_live {
        return Ok(None);
    }
    if let Some((dag, order)) = operator_fixed_live_graph(agent)? {
        return Ok(Some(PlannedLiveDag {
            dag,
            order,
            used_fallback: false,
            source: "operator_path",
        }));
    }
    let fallback = fallback_template_json(agent)?;
    load_stored_planned_dag(mem, session_id, &fallback).await
}

/// Insert planner system prompt immediately before the current user turn.
pub fn wrap_chat_history_for_planner(
    history: &mut Vec<ChatMessage>,
    prompt: &str,
) -> (usize, ChatMessage) {
    let prefix = history.len().saturating_sub(1);
    let user = history.pop().unwrap_or_else(|| ChatMessage::user(""));
    history.push(ChatMessage::system(prompt));
    history.push(user.clone());
    (prefix, user)
}

pub fn restore_chat_history_after_planner(
    history: &mut Vec<ChatMessage>,
    prefix: usize,
    user: ChatMessage,
) {
    history.truncate(prefix);
    history.push(user);
}

/// Cached/operator graph, or run the tool-free planner and persist successful plans only.
///
/// `use_cache`: Build approval reuses `dag_plan:<session>`. Plan and task
/// corrections pass false (caller should have cleared session DAG state).
pub async fn obtain_planned_live_dag_with_provider(
    agent: &crate::config::AgentConfig,
    mem: &dyn Memory,
    session_id: &str,
    provider: &dyn Provider,
    planner_model: &str,
    user_task: &str,
    temperature: f64,
    use_cache: bool,
) -> Result<PlannedLiveDag> {
    if use_cache {
        if let Some(planned) = try_cached_or_fixed_live_dag(agent, mem, session_id).await? {
            return Ok(planned);
        }
    } else if let Some((dag, order)) = operator_fixed_live_graph(agent)? {
        let _ = store_graph_user_task(mem, session_id, user_task).await;
        return Ok(PlannedLiveDag {
            dag,
            order,
            used_fallback: false,
            source: "operator_path",
        });
    }
    let fallback = fallback_template_json(agent)?;
    let planned =
        run_live_planner_chat(provider, planner_model, user_task, temperature, &fallback).await?;
    if planned.used_fallback {
        tracing::info!(
            target: "bounded_dag_live",
            session_id = %session_id,
            "planner used fallback template; not caching for session"
        );
    } else {
        let json = planned_store_json(&planned, &fallback);
        if let Err(err) = store_planned_json(mem, session_id, &json).await {
            tracing::debug!(error = %err, "bounded DAG plan store skipped");
        }
    }
    let _ = store_graph_user_task(mem, session_id, user_task).await;
    Ok(planned)
}

/// Tool-free planner chat with one validation retry before L2 fallback.
pub async fn run_live_planner_chat(
    provider: &dyn Provider,
    planner_model: &str,
    user_task: &str,
    temperature: f64,
    fallback_json: &str,
) -> Result<PlannedLiveDag> {
    let text = planner_chat_text(provider, planner_model, user_task, temperature).await?;
    let planned = resolve_planned_manifest(&text, fallback_json)?;
    if !planned.used_fallback {
        return Ok(planned);
    }

    let extracted = extract_json_object(&text).unwrap_or_else(|| text.trim().to_string());
    let report = validate_candidate_dag_json(&extracted);
    tracing::info!(
        target: "bounded_dag_live",
        category = %report.category,
        message = %report.message,
        "planner output invalid; retrying once"
    );

    let retry_user = format!(
        "Your previous reply failed validation ({}: {}). Reply with ONLY one corrected JSON object — no markdown fences, no prose.\n\nOriginal user task:\n{user_task}",
        report.category, report.message
    );
    let messages = [
        ChatMessage::system(DAG_PLAN_SYSTEM_PROMPT),
        ChatMessage::user(retry_user),
    ];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
    };
    let response = provider.chat(request, planner_model, temperature).await?;
    let retry_text = response.text_or_empty();
    resolve_planned_manifest(retry_text, fallback_json)
}

pub fn operator_fixed_live_graph(
    agent: &crate::config::AgentConfig,
) -> Result<Option<(DagManifest, Vec<String>)>> {
    if !agent.bounded_dag_live {
        return Ok(None);
    }
    let Some(raw) = agent
        .bounded_dag_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let dag = load_bounded_dag(Some(Path::new(raw)))?;
    let order = linear_node_ids(&dag)?;
    Ok(Some((dag, order)))
}

pub fn fallback_template_json(agent: &crate::config::AgentConfig) -> Result<String> {
    if let Some(raw) = agent
        .bounded_dag_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Ok(std::fs::read_to_string(raw)?);
    }
    Ok(CODE_FIX_TEMPLATE_JSON.to_string())
}

/// Parse planner model text into a linear DAG, or the L2 fallback template.
pub fn resolve_planned_manifest(planner_text: &str, fallback_json: &str) -> Result<PlannedLiveDag> {
    let extracted =
        extract_json_object(planner_text).unwrap_or_else(|| planner_text.trim().to_string());
    let report = validate_candidate_dag_json(&extracted);
    if report.valid {
        if let Some(dag) = report.dag {
            if let Ok(order) = linear_node_ids(&dag) {
                return Ok(PlannedLiveDag {
                    dag,
                    order,
                    used_fallback: false,
                    source: "planner",
                });
            }
        }
    }
    let dag = super::dag_runner::parse_dag_json(fallback_json)?;
    let order = linear_node_ids(&dag)?;
    Ok(PlannedLiveDag {
        dag,
        order,
        used_fallback: true,
        source: "fallback_template",
    })
}

pub async fn store_planned_json(mem: &dyn Memory, session_id: &str, json: &str) -> Result<()> {
    mem.store(
        &planned_dag_key(session_id),
        json,
        crate::memory::MemoryCategory::Daily,
        Some(session_id),
    )
    .await
}

pub async fn load_planned_json(mem: &dyn Memory, session_id: &str) -> Result<Option<String>> {
    Ok(mem
        .get(&planned_dag_key(session_id))
        .await?
        .map(|e| e.content))
}

/// Persist whichever graph we will execute (planner JSON or fallback file text).
pub fn planned_store_json(plan: &PlannedLiveDag, fallback_json: &str) -> String {
    if plan.used_fallback {
        fallback_json.to_string()
    } else {
        // Round-trip via parse-able copy from nodes we already validated.
        serde_json::json!({
            "schema_version": plan.dag.schema_version,
            "id": plan.dag.id,
            "description": plan.dag.description,
            "entry": plan.dag.entry,
            "max_steps": plan.dag.max_steps,
            "timeout_secs": plan.dag.timeout_secs,
            "nodes": plan.dag.nodes.iter().map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "task_type": n.task_type,
                    "model_selector": { "capabilities": n.model_selector.capabilities },
                    "context_requirements": {
                        "layers": n.context_requirements.layers,
                        "retrieve": n.context_requirements.retrieve.iter().map(|r| {
                            serde_json::json!({
                                "kind": r.kind,
                                "query": r.query,
                                "alias": r.alias,
                            })
                        }).collect::<Vec<_>>(),
                    },
                    "max_steps": n.max_steps,
                    "next": n.next,
                })
            }).collect::<Vec<_>>(),
        })
        .to_string()
    }
}

pub async fn load_stored_planned_dag(
    mem: &dyn Memory,
    session_id: &str,
    fallback_json: &str,
) -> Result<Option<PlannedLiveDag>> {
    let Some(json) = load_planned_json(mem, session_id).await? else {
        return Ok(None);
    };
    Ok(Some(resolve_planned_manifest(&json, fallback_json)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAPER_JSON: &str = r#"{
      "schema_version": "0.1.0",
      "id": "paper-slides",
      "entry": "read",
      "max_steps": 8,
      "nodes": [
        {"id":"read","task_type":"summarize","model_selector":{"capabilities":["document_understanding"]},"next":"slides"},
        {"id":"slides","task_type":"write","model_selector":{"capabilities":["speed"]},"next":null}
      ]
    }"#;

    #[test]
    fn valid_linear_planner_json_is_accepted() {
        let plan = resolve_planned_manifest(PAPER_JSON, CODE_FIX_TEMPLATE_JSON).unwrap();
        assert!(!plan.used_fallback);
        assert_eq!(plan.order, vec!["read", "slides"]);
        assert_eq!(plan.dag.id, "paper-slides");
    }

    #[test]
    fn garbage_falls_back_to_code_fix() {
        let plan = resolve_planned_manifest("not json", CODE_FIX_TEMPLATE_JSON).unwrap();
        assert!(plan.used_fallback);
        assert_eq!(plan.order, vec!["locate", "patch", "verify"]);
    }

    #[test]
    fn operator_path_skips_empty_path() {
        let agent = crate::config::AgentConfig {
            bounded_dag_live: true,
            bounded_dag_path: None,
            ..crate::config::AgentConfig::default()
        };
        assert!(operator_fixed_live_graph(&agent).unwrap().is_none());
    }

    #[test]
    fn fenced_json_is_extracted() {
        let text = format!("Sure.\n```json\n{PAPER_JSON}\n```\n");
        let plan = resolve_planned_manifest(&text, CODE_FIX_TEMPLATE_JSON).unwrap();
        assert!(!plan.used_fallback);
        assert_eq!(plan.dag.id, "paper-slides");
    }

    #[test]
    fn preview_lists_contact_for_hints() {
        let plan = resolve_planned_manifest(PAPER_JSON, CODE_FIX_TEMPLATE_JSON).unwrap();
        let text = plan.preview_with_contact(
            "deepseek/deepseek-v4-flash",
            &["document".into(), "fast".into()],
        );
        assert!(text.contains("hint:document"), "{text}");
        assert!(text.contains("hint:fast"), "{text}");
    }

    struct TwoShotPlanner {
        responses: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl Provider for TwoShotPlanner {
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
        ) -> anyhow::Result<crate::providers::ChatResponse> {
            let mut guard = self.responses.lock().unwrap();
            let text = if guard.is_empty() {
                "not json".into()
            } else {
                guard.remove(0)
            };
            Ok(crate::providers::ChatResponse {
                text: Some(text),
                tool_calls: vec![],
            })
        }
    }

    #[tokio::test]
    async fn planner_retry_accepts_second_response() {
        let provider = TwoShotPlanner {
            responses: std::sync::Mutex::new(vec![PAPER_JSON.into()]),
        };
        let plan = run_live_planner_chat(
            &provider,
            "deepseek/deepseek-v4-flash",
            "read paper",
            0.0,
            CODE_FIX_TEMPLATE_JSON,
        )
        .await
        .unwrap();
        assert!(!plan.used_fallback);
        assert_eq!(plan.dag.id, "paper-slides");
    }

    #[test]
    fn short_confirmations_reuse_cached_dag() {
        assert!(is_build_approval("同意，做吧"));
        assert!(is_build_approval("ok"));
        assert!(is_build_approval("Approve Build"));
        assert!(should_reuse_cached_dag(HostPhase::Build, "yes"));
        assert!(!should_reuse_cached_dag(HostPhase::Plan, "yes"));
        assert!(!is_build_approval(
            "this is not velaclaw; put the plan in the home directory"
        ));
    }
}
