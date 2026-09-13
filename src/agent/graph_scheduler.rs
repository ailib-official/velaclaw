//! Host graph scheduler (VL-APE-001). One hop-end / graph-end policy for CLI + Web.
//! 宿主图调度：成功路径不 observe；parlor 仅全图结束。

use crate::agent::bounded_dag_delivery::{
    ensure_user_visible, hop_body_closes_graph, host_delivery, last_hop_ends_graph,
    looks_like_internodal_envelope, strip_internodal_suffix,
};
use crate::providers::Provider;
use anyhow::Result;

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
}
