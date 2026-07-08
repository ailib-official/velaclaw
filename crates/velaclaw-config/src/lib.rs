//! VelaClaw configuration crate — defaults, L2 agent-policy, EffectivePolicy merge (VL-ARCH-004).
//! 配置类型与分层策略；不含 agent/channels 运行时依赖。

pub mod defaults;

#[cfg(feature = "ai-protocol")]
pub mod agent_policy;
#[cfg(feature = "ai-protocol")]
pub mod effective_execution_policy;
#[cfg(feature = "ai-protocol")]
pub mod effective_policy;
#[cfg(feature = "ai-protocol")]
pub mod policy_overrides;

pub use defaults::{DEFAULT_PROTOCOL_MODEL_ID, DEFAULT_PROTOCOL_MODEL_LABEL};

#[cfg(feature = "ai-protocol")]
pub use agent_policy::{
    reject_forbidden_secret_keys, AgentPolicyLayer, ApprovalPolicySection, AutonomyPolicySection,
    SelfAdjustSection,
};
#[cfg(feature = "ai-protocol")]
pub use effective_execution_policy::{
    merge_autonomy_layers, AutonomyLayerValues, EffectiveExecutionPolicy,
};
#[cfg(feature = "ai-protocol")]
pub use effective_policy::{merge_tool_dispatcher, EffectivePolicy};
#[cfg(feature = "ai-protocol")]
pub use policy_overrides::{
    discover_and_load_policy_overrides, load_policy_overrides_from_path, merge_policy_overrides,
    policy_overrides_path, ApprovalOverridesSection, PolicyOverridesLayer, SelfAdjustEnforcer,
    POLICY_OVERRIDES_DIR, POLICY_OVERRIDES_FILE,
};
