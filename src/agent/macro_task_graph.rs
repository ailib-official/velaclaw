//! VL-MACRO-001 (G1 opt-in): bounded **task** graph scheduler skeleton.
//! Each graph node must wrap the canonical tool loop (`invoke_tool_loop_once`), not a
//! second loop implementation (GOV-007).
//!
//! Slice 2 (this module): single-node `Build` graph — behavior-equivalent to RAO turn
//! when `[agent].macro_task_graph_enabled = true`. Multi-node + VIS follow in later PRs.

use anyhow::Result;

use super::agent::Agent;
use super::host_phase::HostPhase;

/// Run one bounded task-graph turn. Current slice: exactly one Build node → tool loop.
pub async fn run_task_graph_turn(agent: &mut Agent, user_message: &str) -> Result<String> {
    tracing::info!(
        target: "velaclaw::macro_task_graph",
        nodes = 1,
        "macro_task_graph_enabled: single-node Build hop → canonical run_tool_call_loop"
    );
    agent.set_host_phase(HostPhase::Build);
    agent.invoke_tool_loop_once(user_message).await
}
