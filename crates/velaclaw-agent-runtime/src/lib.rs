//! VelaClaw agent runtime — tool dispatcher + BYOK execution (VL-ARCH-007 P2).
//! Agent 运行时：工具分发与 BYOK 执行；loop/channels 仍在主 crate。

pub mod byok;
pub mod dispatcher;
pub mod execution_context;
pub mod provider;
pub mod telemetry;
pub mod tools;

pub use byok::{
    execute_chat_with_retry, init_ai_client_sync, resolve_ai_client, split_logical_model_id,
};
pub use dispatcher::{
    build_tool_dispatcher, build_tool_dispatcher_for_logical_model, text_tool_parser_from_manifest,
    NativeToolDispatcher, ParsedToolCall, ToolDispatcher, ToolExecutionResult, XmlToolDispatcher,
};
pub use execution_context::ToolExecutionContext;
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, ConversationMessage, NativeToolCapable, ToolCall,
    ToolResultMessage,
};
pub use tools::{Tool, ToolResult, ToolSpec};
