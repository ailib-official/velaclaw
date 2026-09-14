//! Host graph scheduler (VL-APE-001 / VL-APE-003 / VL-APE-004).
//! 宿主图调度：成功路径不 observe；LLM native；工具直调；就绪集并行。

use crate::agent::bounded_dag_delivery::{
    ensure_user_visible, hop_body_closes_graph, host_delivery, last_hop_ends_graph,
    looks_like_internodal_envelope, strip_internodal_suffix,
};
use crate::agent::dag_runner::DagManifest;
use crate::agent::dag_runner::DagNode;
use crate::agent::tool_batch::{ParsedToolCall, ToolBatchResult};
use crate::providers::{ChatMessage, ConversationMessage, Provider};
use anyhow::{bail, Result};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasher;

/// After a successful hop: walk remaining, or finish the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterSuccessfulHop {
    NextRemaining,
    FinishDeliver,
    FinishParlor,
}

/// Last hop text already answers the user — do not spend another parlor LLM.
#[must_use]
pub fn hop_text_is_user_visible(body: &str) -> bool {
    let without_markup = crate::util::strip_tool_call_markup(body);
    let stripped = strip_internodal_suffix(&without_markup);
    !stripped.trim().is_empty()
        && !looks_like_internodal_envelope(&stripped)
        && !velaclaw_agent_runtime::looks_like_tool_format_exhausted_notice(&stripped)
}

/// Single-node graphs with visible assistant text skip the parlor LLM (R8).
#[must_use]
pub fn skip_parlor_llm(node_count: usize, last_body: &str) -> bool {
    node_count <= 1 && hop_text_is_user_visible(last_body)
}

const TOOL_EVIDENCE_MAX: usize = 3_500;

fn push_tool_chunk(chunks: &mut Vec<String>, role: &str, content: &str) {
    let body = content.trim();
    if body.is_empty() {
        return;
    }
    if role == "tool" || body.starts_with("[Tool results]") {
        chunks.push(body.to_string());
    }
}

/// Tool stdout from this hop (role=tool / ToolResults), clipped for Memory + parlor.
#[must_use]
pub fn tool_evidence_from_conversation(history: &[ConversationMessage]) -> String {
    let mut chunks = Vec::new();
    for message in history {
        match message {
            ConversationMessage::ToolResults(rows) => {
                for row in rows {
                    push_tool_chunk(&mut chunks, "tool", &row.content);
                }
            }
            ConversationMessage::Chat(chat) => {
                push_tool_chunk(&mut chunks, chat.role.as_str(), &chat.content);
            }
            ConversationMessage::AssistantToolCalls { .. } => {}
        }
    }
    clip_chars(&chunks.join("\n---\n"), TOOL_EVIDENCE_MAX)
}

/// Same evidence extractor for CLI live hops that keep `Vec<ChatMessage>`.
#[must_use]
pub fn tool_evidence_from_chat(history: &[ChatMessage]) -> String {
    let mut chunks = Vec::new();
    for chat in history {
        push_tool_chunk(&mut chunks, chat.role.as_str(), &chat.content);
    }
    clip_chars(&chunks.join("\n---\n"), TOOL_EVIDENCE_MAX)
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

/// Host internodal artifact when the model used tools but left no operator text (I13).
#[must_use]
pub fn hop_contract_body(assistant: &str, tool_evidence: &str) -> String {
    if hop_text_is_user_visible(assistant) {
        return assistant.to_string();
    }
    let evidence = tool_evidence.trim();
    if evidence.is_empty() {
        return assistant.to_string();
    }
    format!(
        "HANDOFF\n\
verdict: tools_ran_without_assistant_text\n\
findings:\n\
{evidence}\n\
pointers:\n\
- host parlor synthesizes the operator report from this-hop-tool\n\
gaps:\n\
- assistant visible text was empty\n"
    )
}

/// Bounded ready-wave size (VL-APE-004). Not a new config key.
pub const MAX_READY_WAVE: usize = 4;

/// Success hops never splice remaining plan (I4). Typed fail uses freeze + A.
#[must_use]
pub fn success_path_splices_remaining() -> bool {
    false
}

/// Completed prefix at typed fail: failed id is excluded; unrun nodes are dropped.
#[must_use]
pub fn freeze_completed_prefix(completed: &[String], failed_id: &str) -> Vec<String> {
    completed
        .iter()
        .filter(|id| id.as_str() != failed_id)
        .cloned()
        .collect()
}

/// One A replan budget, same flag as `dag_fail_auto_replan`.
#[must_use]
pub fn typed_fail_allows_a_replan(auto_enabled: bool, auto_used: bool) -> bool {
    auto_enabled && !auto_used
}

/// Nodes whose predecessors are all completed, preserving manifest order, capped.
#[must_use]
pub fn ready_set<S: BuildHasher>(
    dag: &DagManifest,
    completed: &HashSet<String, S>,
    max: usize,
) -> Vec<String> {
    let preds = predecessor_map(dag);
    let cap = max.max(1);
    dag.nodes
        .iter()
        .filter(|n| !completed.contains(&n.id))
        .filter(|n| {
            preds
                .get(n.id.as_str())
                .map(|p| p.iter().all(|pred| completed.contains(*pred)))
                .unwrap_or(true)
        })
        .map(|n| n.id.clone())
        .take(cap)
        .collect()
}

#[must_use]
pub fn predecessor_map(dag: &DagManifest) -> HashMap<&str, Vec<&str>> {
    let mut preds: HashMap<&str, Vec<&str>> = dag
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), Vec::new()))
        .collect();
    for node in &dag.nodes {
        for suc in node.successors() {
            if let Some(list) = preds.get_mut(suc) {
                if !list.contains(&node.id.as_str()) {
                    list.push(node.id.as_str());
                }
            }
        }
    }
    preds
}

/// Next ids to run: full tool-direct wave, else the first ready node.
#[must_use]
pub fn pick_run_ids<S: BuildHasher>(
    dag: &DagManifest,
    completed: &HashSet<String, S>,
) -> Vec<String> {
    let ready = ready_set(dag, completed, MAX_READY_WAVE);
    if ready.len() > 1 && wave_is_tool_direct(dag, &ready) {
        ready
    } else {
        ready.into_iter().take(1).collect()
    }
}

/// True when every id in `wave` is a tool-only Σ (safe to one `execute_tool_batch`).
#[must_use]
pub fn wave_is_tool_direct(dag: &DagManifest, wave: &[String]) -> bool {
    !wave.is_empty()
        && wave.iter().all(|id| {
            dag.nodes
                .iter()
                .find(|n| n.id == *id)
                .map(node_sigma)
                .is_some_and(|s| s == NodeSigma::ToolDirect)
        })
}

/// GOV-007 hop-end table. Success never observe; typed fail is HopClose, not this fn.
#[must_use]
pub fn after_successful_hop(
    remaining: usize,
    node_count: usize,
    last_body: &str,
) -> AfterSuccessfulHop {
    if remaining > 0 && !hop_body_closes_graph(last_body) && !last_hop_ends_graph(remaining) {
        AfterSuccessfulHop::NextRemaining
    } else if skip_parlor_llm(node_count, last_body) {
        AfterSuccessfulHop::FinishDeliver
    } else {
        AfterSuccessfulHop::FinishParlor
    }
}

/// Node Σ: LLM loop vs host Invoke (VL-APE-003 / GOV-007).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeSigma {
    LlmWork,
    ToolDirect,
}

/// Live LLM hops never store tool results as `ChatMessage::user`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolResultHistoryKind {
    UserShape,
    RoleTool,
}

pub const LIVE_NATIVE_TOOLS_REQUIRED: &str = "live LLM hop requires native tool_calls; this provider has none. Set tool_dispatcher=xml for explicit XML compat (results still must not be user-shaped).";

/// Classify a planner node: tool-only contracts skip `run_tool_call_loop`.
#[must_use]
pub fn node_sigma(node: &DagNode) -> NodeSigma {
    if let Some(s) = node.sigma.as_deref().map(str::trim) {
        if s.eq_ignore_ascii_case("tool_direct") {
            return NodeSigma::ToolDirect;
        }
        if s.eq_ignore_ascii_case("llm_cognition") {
            return NodeSigma::LlmWork;
        }
    }
    if is_tool_only_node(node) {
        NodeSigma::ToolDirect
    } else {
        NodeSigma::LlmWork
    }
}

#[must_use]
pub fn is_tool_only_node(node: &DagNode) -> bool {
    let caps = &node.model_selector.capabilities;
    if caps.is_empty() {
        return invoke_tool_name(&node.task_type).is_some();
    }
    caps.iter().all(|c| invoke_tool_name(c).is_some())
}

fn invoke_tool_name(label: &str) -> Option<&'static str> {
    let t = label.trim().to_ascii_lowercase().replace('_', ".");
    match t.as_str() {
        "shell.exec" | "shell" => Some("shell"),
        "repo.inspect" | "glob.search" | "glob" => Some("glob_search"),
        "file.read" => Some("file_read"),
        _ => None,
    }
}

/// Build the E-side tool call for a tool-only node (no provider chat).
pub(crate) fn direct_tool_call(node: &DagNode) -> Result<ParsedToolCall> {
    if node_sigma(node) != NodeSigma::ToolDirect {
        bail!("node {} is not a tool-only capability", node.id);
    }
    let name = node
        .model_selector
        .capabilities
        .iter()
        .find_map(|c| invoke_tool_name(c))
        .or_else(|| invoke_tool_name(&node.task_type))
        .ok_or_else(|| anyhow::anyhow!("tool-only node {} missing invoke contract", node.id))?;
    let Some(artifact) = node
        .artifact
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        bail!("tool-only node {} missing I contract (artifact)", node.id);
    };
    let arguments = if let Ok(v) = serde_json::from_str::<serde_json::Value>(artifact) {
        if v.is_object() {
            v
        } else {
            default_invoke_args(name, artifact)
        }
    } else {
        default_invoke_args(name, artifact)
    };
    Ok(ParsedToolCall {
        name: name.to_string(),
        arguments,
    })
}

fn default_invoke_args(tool: &str, artifact: &str) -> serde_json::Value {
    match tool {
        "shell" => serde_json::json!({ "command": artifact }),
        "glob_search" => serde_json::json!({ "pattern": artifact }),
        "file_read" => serde_json::json!({ "path": artifact }),
        _ => serde_json::json!({ "input": artifact }),
    }
}

#[must_use]
pub fn tool_direct_body(results: &[ToolBatchResult]) -> String {
    let joined = results
        .iter()
        .map(|r| r.output.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if joined.trim().is_empty() {
        "ok".into()
    } else {
        joined
    }
}

/// Explicit `xml` is the only live XML compat; auto must not silent-degrade.
/// `native_on_wire` is the hop dispatcher sending native tool specs (not a second loop).
#[must_use]
pub fn live_llm_fail_closed(dispatcher_cfg: &str, native_on_wire: bool) -> bool {
    if dispatcher_cfg.trim().eq_ignore_ascii_case("xml") {
        return false;
    }
    !native_on_wire
}

pub fn ensure_live_llm_native(dispatcher_cfg: &str, native_on_wire: bool) -> Result<()> {
    if live_llm_fail_closed(dispatcher_cfg, native_on_wire) {
        bail!("{LIVE_NATIVE_TOOLS_REQUIRED}");
    }
    Ok(())
}

/// Live hops: never Hybrid/user-shape tool history (R9).
#[must_use]
pub fn live_hop_text_tool_result_history() -> bool {
    false
}

#[must_use]
pub fn live_work_text_history(bounded_dag_live: bool, session_text_history: bool) -> bool {
    if bounded_dag_live {
        live_hop_text_tool_result_history()
    } else {
        session_text_history
    }
}

#[must_use]
pub fn tool_result_history_kind(text_tool_result_history: bool) -> ToolResultHistoryKind {
    if text_tool_result_history {
        ToolResultHistoryKind::UserShape
    } else {
        ToolResultHistoryKind::RoleTool
    }
}

/// Append batch results as role=tool (never user-shape).
pub fn append_role_tool_results(history: &mut Vec<ChatMessage>, results: &[ToolBatchResult]) {
    for (i, result) in results.iter().enumerate() {
        history.push(ChatMessage::tool_with_call_id(
            format!("direct-{i}"),
            &result.output,
        ));
    }
}

pub fn append_loop_tool_results(
    history: &mut Vec<ChatMessage>,
    native_tool_calls: &[crate::providers::ToolCall],
    individual_results: &[String],
    xml_block: &str,
    text_tool_result_history: bool,
) {
    match tool_result_history_kind(text_tool_result_history) {
        ToolResultHistoryKind::UserShape => {
            history.push(ChatMessage::user(format!("[Tool results]\n{xml_block}")));
        }
        ToolResultHistoryKind::RoleTool => {
            if native_tool_calls.is_empty() {
                for (i, result) in individual_results.iter().enumerate() {
                    history.push(ChatMessage::tool_with_call_id(
                        format!("compat-{i}"),
                        result,
                    ));
                }
            } else {
                for (native_call, result) in native_tool_calls.iter().zip(individual_results.iter())
                {
                    history.push(ChatMessage::tool_with_call_id(&native_call.id, result));
                }
            }
        }
    }
}

/// VL-APE-005 / §3: `chat_only` success path is one LLM call (no observe, no parlor).
#[must_use]
pub const fn chat_only_success_llm_calls() -> usize {
    1
}

/// VL-APE-005 / §3: successful work hops add zero observe LLM calls.
#[must_use]
pub const fn observe_llm_on_successful_hops(_work_hops: usize) -> usize {
    0
}

/// VL-APE-005 / §3: parlor LLM is at most one, and zero when skip_parlor_llm.
#[must_use]
pub fn parlor_llm_budget(node_count: usize, last_body: &str) -> usize {
    match after_successful_hop(0, node_count, last_body) {
        AfterSuccessfulHop::FinishParlor => 1,
        AfterSuccessfulHop::FinishDeliver | AfterSuccessfulHop::NextRemaining => 0,
    }
}

/// Graph-end delivery used by [`crate::agent::agent::Agent::turn`] and CLI live DAG.
pub async fn finish_live_graph(
    provider: &dyn Provider,
    model: &str,
    temperature: f64,
    user_task: &str,
    last_body: &str,
    prior_visible: &str,
    node_count: usize,
) -> Result<String> {
    match after_successful_hop(0, node_count, last_body) {
        AfterSuccessfulHop::FinishDeliver | AfterSuccessfulHop::NextRemaining => {
            Ok(ensure_user_visible(user_task, last_body))
        }
        AfterSuccessfulHop::FinishParlor => {
            host_delivery(
                provider,
                model,
                temperature,
                user_task,
                last_body,
                prior_visible,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_mid_hop_walks_remaining() {
        assert_eq!(
            after_successful_hop(2, 3, "located"),
            AfterSuccessfulHop::NextRemaining
        );
    }

    #[test]
    fn single_node_visible_skips_parlor() {
        assert_eq!(
            after_successful_hop(0, 1, "Google 路由当前可用。"),
            AfterSuccessfulHop::FinishDeliver
        );
        assert!(skip_parlor_llm(1, "done"));
        assert!(!skip_parlor_llm(3, "verified"));
    }

    #[test]
    fn empty_or_internodal_last_hop_uses_parlor() {
        assert_eq!(
            after_successful_hop(0, 1, ""),
            AfterSuccessfulHop::FinishParlor
        );
        let internodal = hop_contract_body("", "Cargo.toml\nREADME.md");
        assert!(looks_like_internodal_envelope(&internodal));
        assert_eq!(
            after_successful_hop(0, 1, &internodal),
            AfterSuccessfulHop::FinishParlor
        );
        assert!(hop_contract_body("Google 路由当前可用。", "ignored").contains("Google"));
    }

    #[test]
    fn multi_node_end_uses_parlor_policy() {
        assert_eq!(
            after_successful_hop(0, 3, "verified"),
            AfterSuccessfulHop::FinishParlor
        );
    }

    #[test]
    fn coding_node_is_llm_sigma() {
        let dag = crate::agent::dag_runner::parse_dag_json(
            crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON,
        )
        .unwrap();
        let locate = dag.nodes.iter().find(|n| n.id == "locate").unwrap();
        assert_eq!(node_sigma(locate), NodeSigma::LlmWork);
    }

    #[test]
    fn shell_exec_node_is_tool_direct() {
        let dag = crate::agent::dag_runner::parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"ls","entry":"ls","max_steps":2,"nodes":[{"id":"ls","task_type":"shell.exec","model_selector":{"capabilities":["shell.exec"]},"artifact":"pwd","next":null}]}"#,
        )
        .unwrap();
        assert_eq!(node_sigma(&dag.nodes[0]), NodeSigma::ToolDirect);
        let call = direct_tool_call(&dag.nodes[0]).unwrap();
        assert_eq!(call.name, "shell");
        assert_eq!(call.arguments["command"], "pwd");
    }

    #[test]
    fn live_llm_auto_fails_without_native() {
        assert!(live_llm_fail_closed("auto", false));
        assert!(!live_llm_fail_closed("auto", true));
        assert!(!live_llm_fail_closed("xml", false));
        assert!(live_llm_fail_closed("native", false));
    }

    #[test]
    fn live_history_is_role_tool_not_user() {
        assert_eq!(
            tool_result_history_kind(live_hop_text_tool_result_history()),
            ToolResultHistoryKind::RoleTool
        );
        let mut hist = Vec::new();
        append_loop_tool_results(
            &mut hist,
            &[],
            &["ok".into()],
            "<tool_result>ok</tool_result>",
            false,
        );
        assert!(hist.iter().all(|m| m.role == "tool"));
        assert!(hist.iter().all(|m| m.role != "user"));
    }

    #[test]
    fn xml_compat_may_user_shape_only_when_flag_set() {
        let mut hist = Vec::new();
        append_loop_tool_results(&mut hist, &[], &["ok".into()], "xml", true);
        assert_eq!(hist[0].role, "user");
        assert!(hist[0].content.contains("[Tool results]"));
    }

    #[tokio::test]
    async fn tool_only_node_invokes_batch_without_provider() {
        use crate::observability::NoopObserver;
        use crate::tools::{Tool, ToolExecutionContext, ToolResult};
        use async_trait::async_trait;

        struct EchoShell;
        #[async_trait]
        impl Tool for EchoShell {
            fn name(&self) -> &str {
                "shell"
            }
            fn description(&self) -> &str {
                "echo"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({ "type": "object" })
            }
            async fn execute(
                &self,
                args: serde_json::Value,
                _ctx: &ToolExecutionContext,
            ) -> anyhow::Result<ToolResult> {
                Ok(ToolResult {
                    success: true,
                    output: format!("ran {}", args["command"]),
                    error: None,
                })
            }
        }

        let dag = crate::agent::dag_runner::parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"ls","entry":"ls","max_steps":2,"nodes":[{"id":"ls","task_type":"shell.exec","model_selector":{"capabilities":["shell.exec"]},"artifact":"pwd","next":null}]}"#,
        )
        .unwrap();
        let call = direct_tool_call(&dag.nodes[0]).unwrap();
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(EchoShell)];
        let results = crate::agent::tool_batch::execute_tool_batch(
            std::slice::from_ref(&call),
            &tools,
            &NoopObserver,
            None,
            None,
            "cli",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        assert!(results[0].success);
        assert!(results[0].output.contains("pwd"));
        let mut hist = Vec::new();
        append_role_tool_results(&mut hist, &results);
        assert!(hist.iter().all(|m| m.role == "tool"));
    }

    #[test]
    fn independent_fork_nodes_are_co_ready() {
        let dag = crate::agent::dag_runner::parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"fork","entry":"start","max_steps":8,"nodes":[
            {"id":"start","task_type":"coding","model_selector":{"capabilities":["coding"]},"next":"a","fork":["b"]},
            {"id":"a","task_type":"shell.exec","model_selector":{"capabilities":["shell.exec"]},"artifact":"pwd","next":"join"},
            {"id":"b","task_type":"shell.exec","model_selector":{"capabilities":["shell.exec"]},"artifact":"pwd","next":"join"},
            {"id":"join","task_type":"coding","model_selector":{"capabilities":["coding"]},"next":null}
            ]}"#,
        )
        .unwrap();
        let empty = HashSet::new();
        let after_start = HashSet::from(["start".into()]);
        let ready0 = ready_set(&dag, &empty, MAX_READY_WAVE);
        assert_eq!(ready0, vec!["start".to_string()]);
        let ready1 = ready_set(&dag, &after_start, MAX_READY_WAVE);
        assert!(ready1.contains(&"a".to_string()) && ready1.contains(&"b".to_string()));
        assert!(!ready1.contains(&"join".to_string()));
        assert!(wave_is_tool_direct(&dag, &ready1));
        assert_eq!(pick_run_ids(&dag, &after_start), ready1);
        let both = HashSet::from(["start".into(), "a".into(), "b".into()]);
        assert_eq!(
            ready_set(&dag, &both, MAX_READY_WAVE),
            vec!["join".to_string()]
        );
        assert!(!success_path_splices_remaining());
        assert_eq!(
            freeze_completed_prefix(&["start".into(), "a".into()], "b"),
            vec!["start".to_string(), "a".to_string()]
        );
        assert!(typed_fail_allows_a_replan(true, false));
        assert!(!typed_fail_allows_a_replan(true, true));
        assert!(!typed_fail_allows_a_replan(false, false));
    }

    #[test]
    fn ms_ape_r1_llm_budget_table() {
        assert_eq!(chat_only_success_llm_calls(), 1);
        assert_eq!(observe_llm_on_successful_hops(3), 0);
        assert_eq!(observe_llm_on_successful_hops(8), 0);
        assert_eq!(parlor_llm_budget(1, "Google 路由当前可用。"), 0);
        assert_eq!(parlor_llm_budget(3, "verified"), 1);
        assert!(parlor_llm_budget(8, "verified") <= 1);
        assert!(!success_path_splices_remaining());
        assert!(!crate::config::AgentConfig::default().bounded_dag_live);
        assert!(!crate::config::AgentConfig::default().candidate_dag_emit);
    }

    struct CountChat {
        n: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Provider for CountChat {
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
            _request: crate::providers::ChatRequest<'_>,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<crate::providers::ChatResponse> {
            self.n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::providers::ChatResponse {
                text: Some("ok".into()),
                tool_calls: vec![],
            })
        }
    }

    #[tokio::test]
    async fn ms_ape_r1_parlor_counts_at_most_one_provider_chat() {
        const ENVELOPE: &str =
            "HANDOFF\nverdict: partial\nfindings:\n- issue\npointers:\n- next\ngaps:\n- unknown";
        let p = CountChat {
            n: std::sync::atomic::AtomicUsize::new(0),
        };
        let _ = finish_live_graph(&p, "m", 0.0, "task", ENVELOPE, "", 3)
            .await
            .unwrap();
        let envelope_calls = p.n.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            envelope_calls <= 1,
            "parlor LLM must be ≤1, got {envelope_calls}"
        );
        assert_eq!(
            envelope_calls, 1,
            "internodal last hop spends the parlor budget"
        );
        let vis = CountChat {
            n: std::sync::atomic::AtomicUsize::new(0),
        };
        let _ = finish_live_graph(&vis, "m", 0.0, "task", "verified", "", 3)
            .await
            .unwrap();
        assert_eq!(
            vis.n.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "visible last hop must not add a parlor LLM"
        );
        let p0 = CountChat {
            n: std::sync::atomic::AtomicUsize::new(0),
        };
        let _ = finish_live_graph(&p0, "m", 0.0, "task", "Google 路由当前可用。", "", 1)
            .await
            .unwrap();
        assert_eq!(p0.n.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
