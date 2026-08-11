//! 代理引擎模块，实现自主循环、分类与任务分发。
//!
//! ## Turn unification (ORCH cleanup / VL-CTX-001)
//! - **Shared:** [`crate::orchestration::resolve_turn_model`] (CLI `loop_` + Web
//!   [`agent::Agent::turn`]), [`context_orch::prepare_turn_history`] (compact + layered),
//!   L2 tool_dispatcher merge.
//! - **Still dual (VL-CTX-002):** tool-iteration body (`loop_::run_tool_call_loop` vs
//!   `Agent::turn` inline loop) and approval backends (stdin vs ApprovalHub).
//!   Do not add a third path.
#[allow(clippy::module_inception)]
pub mod agent;
#[cfg(feature = "ai-protocol")]
pub mod candidate_dag;
pub mod classifier;
pub mod context_orch;
#[cfg(feature = "ai-protocol")]
pub mod dag_runner;
pub mod dispatcher;
#[cfg(feature = "ai-protocol")]
pub mod envelope_pilot;
#[cfg(feature = "ai-protocol")]
pub mod intent_route;
pub mod loop_;
pub mod memory_loader;
pub mod prompt;
pub mod prompt_composer;
pub mod tool_batch;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use agent::{Agent, AgentBuilder};
#[allow(unused_imports)]
pub use loop_::{process_message, run};
