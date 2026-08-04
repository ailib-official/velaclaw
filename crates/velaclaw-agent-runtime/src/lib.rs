//! VelaClaw agent runtime — tool dispatcher + BYOK + loop helpers (VL-ARCH-007/008/010).
//! Agent 运行时：工具分发、BYOK、loop 解析/指令/擦除辅助；宿主编排仍在主 crate。

pub mod approval;
pub mod byok;
pub mod dispatcher;
pub mod execution_context;
pub mod loop_parse;
pub mod provider;
pub mod telemetry;
pub mod tool_format;
pub mod tool_util;
pub mod tools;

pub use approval::{
    is_shell_policy_tool, shell_command_from_args, ApprovalGate, GateDecision,
    HumanApprovalBackend, ShellPolicyHook,
};
pub use byok::{
    execute_chat_with_retry, init_ai_client_sync, resolve_ai_client, split_logical_model_id,
};
#[cfg(feature = "ai-protocol")]
pub use dispatcher::parse_manifest_text_tool_fallback;
pub use dispatcher::{
    build_tool_dispatcher, build_tool_dispatcher_for_logical_model, text_tool_parser_from_manifest,
    NativeToolDispatcher, ParsedToolCall, ToolDispatcher, ToolExecutionResult, XmlToolDispatcher,
};
pub use execution_context::ToolExecutionContext;
pub use loop_parse::{
    build_tool_instructions, is_tool_loop_cancelled, parse_tool_calls, tools_to_openai_format,
    trim_history, ToolLoopCancelled, DEFAULT_MAX_HISTORY_MESSAGES, DEFAULT_MAX_TOOL_ITERATIONS,
};
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, ConversationMessage, NativeToolCapable, ToolCall,
    ToolResultMessage,
};
pub use tool_format::{
    needs_tool_format_correction, tool_format_correction_message, tool_format_recovery_message,
    ToolFormatLadder, ToolFormatRecoveryStrategy,
};
pub use tool_util::{normalize_tool_arguments, scrub_credentials};
pub use tools::{Tool, ToolResult, ToolSpec};
