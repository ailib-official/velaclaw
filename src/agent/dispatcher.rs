//! Tool dispatcher shim — delegates to `velaclaw-agent-runtime` (VL-ARCH-007).
//! 工具分发 shim：委托至 agent-runtime crate。

pub use velaclaw_agent_runtime::dispatcher::{
    text_tool_parser_from_manifest, NativeToolDispatcher, ParsedToolCall, ToolDispatcher,
    ToolExecutionResult, XmlToolDispatcher,
};

use crate::providers::Provider;

/// Build a manifest-aware tool dispatcher for a velaclaw [`Provider`].
pub fn build_tool_dispatcher(
    dispatcher_choice: &str,
    provider: &dyn Provider,
    policy: ai_lib_rust::ToolCallingPolicy,
) -> Box<dyn ToolDispatcher> {
    velaclaw_agent_runtime::dispatcher::build_tool_dispatcher(dispatcher_choice, provider, policy)
}

/// Resolve manifest `tool_calling` for a logical model and build dispatcher.
#[cfg(feature = "ai-protocol")]
pub fn build_tool_dispatcher_for_logical_model(
    dispatcher_choice: &str,
    logical_model_id: &str,
    provider: &dyn Provider,
) -> anyhow::Result<Box<dyn ToolDispatcher>> {
    velaclaw_agent_runtime::dispatcher::build_tool_dispatcher_for_logical_model(
        dispatcher_choice,
        logical_model_id,
        provider,
    )
}
