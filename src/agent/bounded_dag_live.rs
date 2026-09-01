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
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::hash::BuildHasher;
use std::path::Path;

pub const PLANNED_DAG_KEY_PREFIX: &str = "dag_plan:";
pub const DAG_FAIL_KEY_PREFIX: &str = "dag_fail:";
const MAX_SESSION_DAG_FORGET: usize = 32;

pub fn planned_dag_key(session_id: &str) -> String {
    format!("{PLANNED_DAG_KEY_PREFIX}{session_id}")
}

pub fn dag_fail_key(session_id: &str) -> String {
    format!("{DAG_FAIL_KEY_PREFIX}{session_id}")
}

pub const GRAPH_USER_KEY_PREFIX: &str = "dag_user:";

pub fn graph_user_key(session_id: &str) -> String {
    format!("{GRAPH_USER_KEY_PREFIX}{session_id}")
}

/// True when this user text should run the live planner + work DAG.
///
/// Greetings and single-shot Q&A stay on the session default model (existing
/// tool loop, no planner). Resume / fail-cursor / env targets still use the DAG.
#[must_use]
pub fn turn_needs_dag(user_task: &str) -> bool {
    let t = normalize_turn(user_task);
    if t.is_empty() {
        return false;
    }
    if is_resume_turn(&t) {
        return true;
    }
    has_execution_intent(&t)
}

/// Live DAG this turn? Fail cursor always yes; otherwise [`turn_needs_dag`].
pub async fn should_run_live_dag(
    agent: &crate::config::AgentConfig,
    mem: &dyn Memory,
    session_id: &str,
    user_task: &str,
    _host_phase: HostPhase,
) -> bool {
    if !agent.bounded_dag_live {
        return false;
    }
    if load_dag_fail(mem, session_id)
        .await
        .ok()
        .flatten()
        .is_some()
    {
        return true;
    }
    turn_needs_dag(user_task)
}

fn normalize_turn(user_task: &str) -> String {
    user_task
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn is_resume_turn(t: &str) -> bool {
    matches!(
        t,
        "继续"
            | "接着"
            | "接着干"
            | "重试"
            | "再试"
            | "continue"
            | "retry"
            | "resume"
            | "proceed"
            | "keep going"
    ) || t.starts_with("继续")
        || t.starts_with("continue ")
        || t.starts_with("retry ")
}

fn has_execution_intent(t: &str) -> bool {
    const MARKERS: &[&str] = &[
        "check",
        "inspect",
        "debug",
        "diagnos",
        "fix ",
        "fix the",
        "deploy",
        "compile",
        "install",
        "restart",
        "health",
        "status",
        " ssh",
        "ssh ",
        "grep",
        "cargo ",
        "git ",
        "curl ",
        "probe",
        "implement",
        "a patch",
        "the patch",
        "look at",
        "look into",
        "list files",
        "read this",
        "write ",
        "检查",
        "查看",
        "排查",
        "诊断",
        "修复",
        "部署",
        "对齐",
        "同步",
        "编译",
        "安装",
        "重启",
        "健康",
        "状态",
        "查一下",
        "查下",
        "跑一下",
        "读一下",
        "改一下",
        "piubt",
        "localhost",
        "192.168",
        "/home/",
        "/usr/",
        "/etc/",
    ];
    if MARKERS.iter().any(|m| t.contains(m)) {
        return true;
    }
    // Bare "fix foo" without trailing space after fix.
    t.starts_with("fix ") || t.contains("/tmp") || t.contains(".rs") || t.contains(".toml")
}

/// Cursor so the next user message can replan the failed node (not a Build approval).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct DagFailCursor {
    pub node_id: String,
    pub index: usize,
    pub err: String,
    pub dag_id: String,
    #[serde(default)]
    pub auto_replan_count: u32,
    #[serde(default)]
    pub fail_class: String,
}

/// Same-turn retry vs stop (VL-NA-024). Dist default off via config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkNodeFailDecision {
    RetrySame { force_default: bool },
    Stop,
}

#[must_use]
pub fn decide_work_node_fail(
    auto_enabled: bool,
    auto_used: bool,
    err: &str,
) -> WorkNodeFailDecision {
    if !auto_enabled || auto_used {
        return WorkNodeFailDecision::Stop;
    }
    match crate::providers::hint_peer::classify_hop_error(err) {
        crate::providers::hint_peer::HopFailClass::Unavailable
        | crate::providers::hint_peer::HopFailClass::Quota => WorkNodeFailDecision::RetrySame {
            force_default: true,
        },
        crate::providers::hint_peer::HopFailClass::Policy
        | crate::providers::hint_peer::HopFailClass::Other => WorkNodeFailDecision::RetrySame {
            force_default: false,
        },
        crate::providers::hint_peer::HopFailClass::Transport => WorkNodeFailDecision::Stop,
    }
}

/// Persist the original user task for work-node USER TASK slots.
pub async fn store_graph_user_task(
    mem: &dyn Memory,
    session_id: &str,
    user_task: &str,
) -> Result<()> {
    let trimmed = user_task.trim();
    if trimmed.is_empty() {
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

/// Prefer stored original task, else last user in history.
pub fn user_task_from_history(history: &[ChatMessage], current: &str) -> String {
    let current = current.trim();
    if !current.is_empty() {
        return current.to_string();
    }
    for message in history.iter().rev() {
        if message.role != "user" {
            continue;
        }
        let body = message.content.trim();
        if body.is_empty() || body.starts_with("[dag_artifact") || body.starts_with("USER TASK") {
            continue;
        }
        return body.to_string();
    }
    current.to_string()
}

/// Stored original task, else last user in history.
pub async fn work_node_user_task(
    mem: &dyn Memory,
    session_id: &str,
    history: &[ChatMessage],
    current: &str,
) -> String {
    if let Ok(Some(stored)) = load_graph_user_task(mem, session_id).await {
        let trimmed = stored.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    user_task_from_history(history, current)
}

pub async fn store_dag_fail(
    mem: &dyn Memory,
    session_id: &str,
    cursor: &DagFailCursor,
) -> Result<()> {
    let json = serde_json::to_string(cursor)?;
    mem.store(
        &dag_fail_key(session_id),
        &json,
        crate::memory::MemoryCategory::Daily,
        Some(session_id),
    )
    .await
}

pub async fn load_dag_fail(mem: &dyn Memory, session_id: &str) -> Result<Option<DagFailCursor>> {
    let Some(entry) = mem.get(&dag_fail_key(session_id)).await? else {
        return Ok(None);
    };
    Ok(serde_json::from_str(&entry.content).ok())
}

pub async fn clear_dag_fail(mem: &dyn Memory, session_id: &str) -> Result<()> {
    let _ = mem.forget(&dag_fail_key(session_id)).await;
    Ok(())
}

/// Drop cached plan, fail cursor, and node artifacts for this session.
pub async fn clear_session_dag_state(mem: &dyn Memory, session_id: &str) -> Result<()> {
    let plan_key = planned_dag_key(session_id);
    let user_key = graph_user_key(session_id);
    let fail_key = dag_fail_key(session_id);
    let _ = mem.forget(&plan_key).await;
    let _ = mem.forget(&user_key).await;
    let _ = mem.forget(&fail_key).await;
    let prefix = format!("dag_art:{session_id}:");
    let listed = mem
        .list(Some(&MemoryCategory::Daily), Some(session_id))
        .await
        .unwrap_or_default();
    for (i, entry) in listed.into_iter().enumerate() {
        if i >= MAX_SESSION_DAG_FORGET {
            break;
        }
        if entry.key == plan_key
            || entry.key == user_key
            || entry.key == fail_key
            || entry.key.starts_with(&prefix)
        {
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
    /// Skip already-finished nodes when retrying after a fail cursor.
    pub resume_from: usize,
    /// Original task plus user guidance for the failed node.
    pub graph_task_override: Option<String>,
}

impl PlannedLiveDag {
    pub fn preview_text(&self) -> String {
        self.preview_with_contact("", &[])
    }

    /// Short operator-facing step list (chat), not the debug Plan chrome dump.
    pub fn brief_outline(&self, user_message: &str) -> String {
        brief_dag_outline(
            user_message,
            &self.dag,
            &self.order,
            self.used_fallback,
            self.resume_from,
        )
    }
}

/// Prefer CJK copy when the user prompt contains Han characters.
#[must_use]
pub fn user_prefers_cjk(text: &str) -> bool {
    text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

#[must_use]
pub fn prettify_node_id(id: &str) -> String {
    id.replace(['_', '-'], " ")
}

/// Chat-facing outline: numbered steps in the user's script, no Contact dump.
#[must_use]
pub fn brief_dag_outline(
    user_message: &str,
    dag: &crate::agent::dag_runner::DagManifest,
    order: &[String],
    used_fallback: bool,
    resume_from: usize,
) -> String {
    let cjk = user_prefers_cjk(user_message);
    let by_id: std::collections::HashMap<&str, _> =
        dag.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut lines = Vec::new();
    if cjk {
        if used_fallback {
            lines.push("规划未能得到有效步骤，改用手写回退图。".to_string());
        }
        if resume_from > 0 {
            lines.push(format!(
                "按你的新说明，从第 {} 步起重新规划并继续（共 {} 步）：",
                resume_from + 1,
                order.len()
            ));
        } else {
            lines.push(format!("将分 {} 步处理：", order.len()));
        }
    } else {
        if used_fallback {
            lines.push("Planner did not produce a valid step list; using a fallback graph.".into());
        }
        if resume_from > 0 {
            lines.push(format!(
                "Replanning from step {} of {} with your new guidance:",
                resume_from + 1,
                order.len()
            ));
        } else {
            lines.push(format!("Working in {} step(s):", order.len()));
        }
    }
    for (i, id) in order.iter().enumerate() {
        let label = prettify_node_id(id);
        let task = by_id
            .get(id.as_str())
            .map(|n| n.task_type.as_str())
            .unwrap_or("-");
        if cjk {
            lines.push(format!("{}. {}（{}）", i + 1, label, task));
        } else {
            lines.push(format!("{}. {label} ({task})", i + 1));
        }
    }
    if cjk {
        lines.push("开始执行。".into());
    } else {
        lines.push("Starting now.".into());
    }
    lines.join("\n")
}

/// Persistable stop line when a work node fails (does not dump the graph).
#[must_use]
pub fn format_work_node_stop(
    user_message: &str,
    node_id: &str,
    err: &str,
    completed: usize,
    total: usize,
) -> String {
    let pretty = prettify_node_id(node_id);
    let err = err.trim();
    if user_prefers_cjk(user_message) {
        format!(
            "已在第 {completed}/{total} 步停住（`{pretty}`）。\n{err}\n针对这一步发送新说明，即可重新规划该步并继续。"
        )
    } else {
        format!(
            "Stopped at step {completed}/{total} (`{pretty}`).\n{err}\nSend guidance for this step to replan it and continue."
        )
    }
}

/// Rail snapshot: one row per graph order, with pending/running/ok/error.
#[must_use]
pub fn live_dag_node_rows<S: BuildHasher>(
    dag: &crate::agent::dag_runner::DagManifest,
    order: &[String],
    running: Option<&str>,
    completed: &HashSet<String, S>,
    failed: Option<&str>,
) -> Vec<LiveDagNodeRow> {
    let by_id: std::collections::HashMap<&str, _> =
        dag.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    order
        .iter()
        .map(|id| {
            let node = by_id.get(id.as_str());
            let status = if failed == Some(id.as_str()) {
                "error"
            } else if completed.contains(id) {
                "ok"
            } else if running == Some(id.as_str()) {
                "running"
            } else {
                "pending"
            };
            LiveDagNodeRow {
                id: id.clone(),
                label: prettify_node_id(id),
                task_type: node
                    .map(|n| n.task_type.clone())
                    .unwrap_or_else(|| "-".into()),
                caps: node
                    .map(|n| n.model_selector.capabilities.join(","))
                    .unwrap_or_default(),
                contact: String::new(),
                status,
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct LiveDagNodeRow {
    pub id: String,
    pub label: String,
    pub task_type: String,
    pub caps: String,
    pub contact: String,
    pub status: &'static str,
}

/// Resolved hop labels for the live rail (`RouterProvider` pin after peer fallback).
#[must_use]
pub fn dag_contact_labels(
    provider: &dyn Provider,
    dag: &DagManifest,
    order: &[String],
    session_model: &str,
    hints: &[String],
) -> std::collections::HashMap<String, String> {
    let mut out = HashMap::new();
    let by_id: HashMap<&str, _> = dag.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    for id in order {
        let Some(node) = by_id.get(id.as_str()) else {
            continue;
        };
        let contact = contact_for_node(node, session_model, hints, None);
        out.insert(id.clone(), provider.routed_model_label(&contact.model));
    }
    out
}

/// Structured WS/CLI progress for the live rail.
#[must_use]
pub fn live_dag_progress<S, C>(
    dag_id: &str,
    fallback: bool,
    outline: &str,
    dag: &crate::agent::dag_runner::DagManifest,
    order: &[String],
    running: Option<&str>,
    completed: &HashSet<String, S>,
    failed: Option<&str>,
    contacts: Option<&HashMap<String, String, C>>,
) -> crate::agent::turn_progress::TurnProgress
where
    S: BuildHasher,
    C: BuildHasher,
{
    use crate::agent::turn_progress::{DagNodeProgress, TurnProgress};
    TurnProgress::Dag {
        dag_id: dag_id.to_string(),
        fallback,
        outline: outline.to_string(),
        nodes: live_dag_node_rows(dag, order, running, completed, failed)
            .into_iter()
            .map(|r| {
                let contact = contacts
                    .and_then(|m| m.get(&r.id).cloned())
                    .unwrap_or_default();
                DagNodeProgress {
                    id: r.id,
                    label: r.label,
                    task_type: r.task_type,
                    caps: r.caps,
                    contact,
                    status: r.status.to_string(),
                }
            })
            .collect(),
    }
}

impl PlannedLiveDag {
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
            resume_from: 0,
            graph_task_override: None,
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
/// `use_cache`: reuse `dag_plan:<session>` when already loaded for a repair.
/// New tasks pass false after `clear_session_dag_state`.
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
            resume_from: 0,
            graph_task_override: None,
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

/// Combine original graph task with the user's failed-node guidance.
#[must_use]
pub fn repair_graph_task(original: &str, node_id: &str, guidance: &str) -> String {
    let original = original.trim();
    let guidance = guidance.trim();
    format!("{original}\n\nFailed node `{node_id}` — user guidance for this step:\n{guidance}")
}

#[must_use]
pub fn repair_planner_user_prompt(
    original: &str,
    fail: &DagFailCursor,
    completed: &[String],
    guidance: &str,
) -> String {
    let completed = if completed.is_empty() {
        "(none)".to_string()
    } else {
        completed.join(", ")
    };
    format!(
        "Replan remaining work after a failed node. Reply with ONLY one linear DAG JSON object \
(schema_version 0.1.0). Include 1 to 6 remaining nodes. Do not redo completed nodes.\n\n\
Original user task:\n{original}\n\n\
Graph id: {}\nCompleted node ids: {completed}\n\
Failed node: {} (0-based index {})\nFail class: {}\nFailure:\n{}\n\n\
User guidance for this node (and remaining if needed):\n{guidance}",
        fail.dag_id,
        fail.node_id,
        fail.index,
        if fail.fail_class.is_empty() {
            "unspecified"
        } else {
            fail.fail_class.as_str()
        },
        fail.err.trim()
    )
}

/// Prefix completed nodes from `stored`, append remaining from `repair`.
#[must_use]
pub fn splice_remaining_plan(
    stored: &PlannedLiveDag,
    fail: &DagFailCursor,
    remaining: PlannedLiveDag,
) -> PlannedLiveDag {
    let resume_from = fail.index.min(stored.order.len());
    let prefix: Vec<String> = stored.order.iter().take(resume_from).cloned().collect();
    let prefix_set: std::collections::HashSet<&str> = prefix.iter().map(String::as_str).collect();
    let mut nodes: Vec<_> = stored
        .dag
        .nodes
        .iter()
        .filter(|n| prefix_set.contains(n.id.as_str()))
        .cloned()
        .collect();
    let mut rest_order: Vec<String> = Vec::new();
    for id in &remaining.order {
        if prefix_set.contains(id.as_str()) {
            continue;
        }
        let Some(node) = remaining.dag.nodes.iter().find(|n| n.id == *id) else {
            continue;
        };
        rest_order.push(id.clone());
        nodes.push(node.clone());
    }
    if rest_order.is_empty() {
        rest_order = stored.order.iter().skip(resume_from).cloned().collect();
        for id in &rest_order {
            if nodes.iter().any(|n| n.id == *id) {
                continue;
            }
            if let Some(node) = stored.dag.nodes.iter().find(|n| n.id == *id) {
                nodes.push(node.clone());
            }
        }
    }
    if let (Some(last_prefix), Some(first_rest)) = (prefix.last(), rest_order.first()) {
        for node in &mut nodes {
            if node.id == *last_prefix {
                node.next = Some(first_rest.clone());
            }
        }
    }
    for i in 0..rest_order.len() {
        let nxt = rest_order.get(i + 1).cloned();
        if let Some(n) = nodes.iter_mut().find(|n| n.id == rest_order[i]) {
            n.next = nxt;
        }
    }
    let order: Vec<String> = prefix
        .iter()
        .cloned()
        .chain(rest_order.iter().cloned())
        .collect();
    let mut dag = stored.dag.clone();
    dag.nodes = nodes;
    if !prefix.is_empty() {
        dag.entry = stored.dag.entry.clone();
    } else if let Some(first) = order.first() {
        dag.entry = first.clone();
    }
    PlannedLiveDag {
        dag,
        order,
        used_fallback: remaining.used_fallback,
        source: "repair",
        resume_from,
        graph_task_override: remaining.graph_task_override,
    }
}

/// New task → full plan. Failed node + new prompt → replan remaining from that node.
pub async fn prepare_session_live_dag(
    agent: &crate::config::AgentConfig,
    mem: &dyn Memory,
    session_id: &str,
    provider: &dyn Provider,
    planner_model: &str,
    user_task: &str,
    temperature: f64,
    host_phase: HostPhase,
) -> Result<PlannedLiveDag> {
    if host_phase == HostPhase::Plan {
        clear_session_dag_state(mem, session_id).await?;
        return obtain_planned_live_dag_with_provider(
            agent,
            mem,
            session_id,
            provider,
            planner_model,
            user_task,
            temperature,
            false,
        )
        .await;
    }

    if let Some(fail) = load_dag_fail(mem, session_id).await? {
        if let Some(mut stored) = try_cached_or_fixed_live_dag(agent, mem, session_id).await? {
            let original = load_graph_user_task(mem, session_id)
                .await?
                .unwrap_or_else(|| user_task.to_string());
            let completed: Vec<String> = stored.order.iter().take(fail.index).cloned().collect();
            let repair_user = repair_planner_user_prompt(&original, &fail, &completed, user_task);
            let fallback = fallback_template_json(agent)?;
            let remaining = run_live_planner_chat(
                provider,
                planner_model,
                &repair_user,
                temperature,
                &fallback,
            )
            .await?;
            let override_task = repair_graph_task(&original, &fail.node_id, user_task);
            if remaining.used_fallback {
                stored.resume_from = fail.index.min(stored.order.len());
                stored.graph_task_override = Some(override_task);
                stored.source = "repair_keep";
                return Ok(stored);
            }
            let mut spliced = splice_remaining_plan(&stored, &fail, remaining);
            if linear_node_ids(&spliced.dag).is_err() {
                stored.resume_from = fail.index.min(stored.order.len());
                stored.graph_task_override = Some(override_task);
                stored.source = "repair_keep";
                return Ok(stored);
            }
            spliced.graph_task_override = Some(override_task);
            let json = planned_store_json(&spliced, &fallback);
            let _ = store_planned_json(mem, session_id, &json).await;
            return Ok(spliced);
        }
        let _ = clear_dag_fail(mem, session_id).await;
    }

    clear_session_dag_state(mem, session_id).await?;
    obtain_planned_live_dag_with_provider(
        agent,
        mem,
        session_id,
        provider,
        planner_model,
        user_task,
        temperature,
        false,
    )
    .await
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
                    resume_from: 0,
                    graph_task_override: None,
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
        resume_from: 0,
        graph_task_override: None,
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
    fn turn_needs_dag_skips_greetings() {
        assert!(!turn_needs_dag("hello"));
        assert!(!turn_needs_dag("Hello!"));
        assert!(!turn_needs_dag("你好"));
        assert!(!turn_needs_dag("谢谢"));
        assert!(!turn_needs_dag("what is a dag"));
        assert!(!turn_needs_dag("please dispatch the email"));
        assert!(!turn_needs_dag("what is a workspace"));
    }

    #[test]
    fn turn_needs_dag_ops_and_resume() {
        assert!(turn_needs_dag("fix the compiler error"));
        assert!(turn_needs_dag(
            "你检查piubt上xray代理服务的状态，然后检查各代理节点的健康状态"
        ));
        assert!(turn_needs_dag("继续"));
        assert!(turn_needs_dag("read this paper, write intro slides"));
        assert!(turn_needs_dag("apply a patch to the file"));
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
    fn splice_keeps_prefix_and_resumes_at_failed_node() {
        let stored = resolve_planned_manifest(
            r#"{
              "schema_version":"0.1.0","id":"ops","entry":"a","max_steps":8,
              "nodes":[
                {"id":"a","task_type":"ops-check","model_selector":{"capabilities":["coding"]},"next":"b"},
                {"id":"b","task_type":"ops-check","model_selector":{"capabilities":["coding"]},"next":"c"},
                {"id":"c","task_type":"summarize","model_selector":{"capabilities":["speed"]},"next":null}
              ]
            }"#,
            CODE_FIX_TEMPLATE_JSON,
        )
        .unwrap();
        let remaining = resolve_planned_manifest(
            r#"{
              "schema_version":"0.1.0","id":"ops-repair","entry":"b","max_steps":8,
              "nodes":[
                {"id":"b","task_type":"ops-check","model_selector":{"capabilities":["coding"]},"next":"c"},
                {"id":"c","task_type":"summarize","model_selector":{"capabilities":["document_understanding"]},"next":null}
              ]
            }"#,
            CODE_FIX_TEMPLATE_JSON,
        )
        .unwrap();
        let fail = DagFailCursor {
            node_id: "b".into(),
            index: 1,
            err: "timeout".into(),
            dag_id: "ops".into(),
            ..Default::default()
        };
        let spliced = splice_remaining_plan(&stored, &fail, remaining);
        assert_eq!(spliced.resume_from, 1);
        assert_eq!(spliced.order, vec!["a", "b", "c"]);
        assert_eq!(spliced.source, "repair");
        let b = spliced.dag.nodes.iter().find(|n| n.id == "b").unwrap();
        assert_eq!(b.task_type, "ops-check");
        assert_eq!(b.next.as_deref(), Some("c"));
        let c = spliced.dag.nodes.iter().find(|n| n.id == "c").unwrap();
        assert!(c.next.is_none());
        assert!(linear_node_ids(&spliced.dag).is_ok());
    }

    #[test]
    fn repair_prompt_asks_for_remaining_not_approval() {
        let fail = DagFailCursor {
            node_id: "check_install".into(),
            index: 0,
            err: "timeout".into(),
            dag_id: "opcencode-check-upgrade".into(),
            ..Default::default()
        };
        let prompt = repair_planner_user_prompt(
            "请检查 opcencode",
            &fail,
            &[],
            "不要用 find /，改查 which 和版本号",
        );
        assert!(prompt.contains("Failed node: check_install"));
        assert!(prompt.contains("不要用 find /"));
        assert!(!prompt.contains("Approve Build"));
        assert!(!prompt.contains("继续"));
    }

    #[test]
    fn work_node_stop_invites_step_guidance() {
        let zh = format_work_node_stop("请检查", "check_install", "timeout", 1, 3);
        assert!(zh.contains("已在第 1/3 步停住"));
        assert!(zh.contains("针对这一步发送新说明"));
        let en = format_work_node_stop("please check", "check_install", "timeout", 1, 3);
        assert!(en.contains("guidance for this step"));
    }

    #[test]
    fn brief_outline_uses_cjk_when_prompt_has_han() {
        let plan = PlannedLiveDag {
            dag: crate::agent::dag_runner::parse_dag_json(
                r#"{
                  "schema_version":"0.1.0","id":"opcencode-check-upgrade","entry":"check_install",
                  "max_steps":6,"nodes":[
                    {"id":"check_install","task_type":"ops-check","model_selector":{"capabilities":["coding"]},"next":"upgrade"},
                    {"id":"upgrade","task_type":"upgrade","model_selector":{"capabilities":["coding"]},"next":null}
                  ]
                }"#,
            )
            .unwrap(),
            order: vec!["check_install".into(), "upgrade".into()],
            used_fallback: false,
            source: "test",
            resume_from: 0,
            graph_task_override: None,
        };
        let out = plan.brief_outline("请检查 opcencode 是否要升级");
        assert!(out.contains("将分 2 步处理"));
        assert!(out.contains("check install"));
        assert!(!out.contains("Bounded task DAG"));
        assert!(!out.contains("contact model="));
    }

    #[test]
    fn auto_replan_retries_unavailable_not_dns() {
        assert_eq!(
            decide_work_node_fail(true, false, "HTTP 404 (not_found): Function missing"),
            WorkNodeFailDecision::RetrySame {
                force_default: true
            }
        );
        assert_eq!(
            decide_work_node_fail(true, false, "Network transport error: dns error"),
            WorkNodeFailDecision::Stop
        );
        assert_eq!(
            decide_work_node_fail(false, false, "HTTP 410 Gone"),
            WorkNodeFailDecision::Stop
        );
        assert_eq!(
            decide_work_node_fail(true, true, "HTTP 410 Gone"),
            WorkNodeFailDecision::Stop
        );
    }
}
