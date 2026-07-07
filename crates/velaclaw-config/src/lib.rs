//! VelaClaw configuration crate — defaults, L2 agent-policy, EffectivePolicy merge (VL-ARCH-004).
//! 配置类型与分层策略；不含 agent/channels 运行时依赖。

pub mod defaults;

#[cfg(feature = "ai-protocol")]
pub mod agent_policy;
#[cfg(feature = "ai-protocol")]
pub mod effective_execution_policy;
#[cfg(feature = "ai-protocol")]
pub mod effective_policy;

pub use defaults::{DEFAULT_PROTOCOL_MODEL_ID, DEFAULT_PROTOCOL_MODEL_LABEL};

#[cfg(feature = "ai-protocol")]
pub use agent_policy::{
    AgentPolicyLayer, ApprovalPolicySection, AutonomyPolicySection, SelfAdjustSection,
};
#[cfg(feature = "ai-protocol")]
pub use effective_execution_policy::{
    merge_autonomy_layers, AutonomyLayerValues, EffectiveExecutionPolicy,
};
#[cfg(feature = "ai-protocol")]
pub use effective_policy::{merge_tool_dispatcher, EffectivePolicy};
