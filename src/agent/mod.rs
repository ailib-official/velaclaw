//! 代理引擎模块，实现自主循环、分类与任务分发。
#[allow(clippy::module_inception)]
pub mod agent;
#[cfg(feature = "ai-protocol")]
pub mod candidate_dag;
pub mod classifier;
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
