//! VelaClaw agent runtime — tool dispatcher + BYOK + loop parse (VL-ARCH-007/008).
//! Agent 运行时：工具分发、BYOK、loop 解析辅助；宿主编排仍在主 crate。

pub mod approval;
pub mod byok;
pub mod dispatcher;
pub mod execution_context;
pub mod loop_parse;
pub mod provider;
pub mod telemetry;
pub mod tools;

pub use approval::{
    is_shell_policy_tool, shell_command_from_args, ApprovalGate, GateDecision,
    HumanApprovalBackend, ShellPolicyHook,
};
pub use byok::{
    execute_chat_with_retry, init_ai_client_sync, resolve_ai_client, split_logical_model_id,
};
pub use dispatcher::{
    build_tool_dispatcher, build_tool_dispatcher_for_logical_model, text_tool_parser_from_manifest,
    NativeToolDispatcher, ParsedToolCall, ToolDispatcher, ToolExecutionResult, XmlToolDispatcher,
};
pub use execution_context::ToolExecutionContext;
pub use loop_parse::{
    is_tool_loop_cancelled, parse_tool_calls, tools_to_openai_format, trim_history,
    ToolLoopCancelled, DEFAULT_MAX_HISTORY_MESSAGES, DEFAULT_MAX_TOOL_ITERATIONS,
};
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, ConversationMessage, NativeToolCapable, ToolCall,
    ToolResultMessage,
};
pub use tools::{Tool, ToolResult, ToolSpec};
