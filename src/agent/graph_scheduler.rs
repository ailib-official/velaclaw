//! Host graph scheduler (VL-APE-001 / VL-APE-003). Hop-end plus node Σ dispatch.
//! 宿主图调度：成功路径不 observe；LLM 节点 native；工具节点直调。

use crate::agent::bounded_dag_delivery::{
    ensure_user_visible, hop_body_closes_graph, host_delivery, last_hop_ends_graph,
    looks_like_internodal_envelope, strip_internodal_suffix,
};
use crate::agent::dag_runner::DagNode;
use crate::agent::tool_batch::{ParsedToolCall, ToolBatchResult};
use crate::providers::{ChatMessage, Provider};
use anyhow::{bail, Result};

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
#[must_use]
pub fn live_llm_fail_closed(dispatcher_cfg: &str, supports_native: bool) -> bool {
    if dispatcher_cfg.trim().eq_ignore_ascii_case("xml") {
        return false;
    }
    !supports_native
}

pub fn ensure_live_llm_native(dispatcher_cfg: &str, supports_native: bool) -> Result<()> {
    if live_llm_fail_closed(dispatcher_cfg, supports_native) {
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
}
