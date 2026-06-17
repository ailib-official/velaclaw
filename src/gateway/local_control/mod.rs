//! Local Control Plane shared types and agent chat runner (VL-UI-002).
//! 本地控制面共享类型与 agent 对话执行（VL-UI-002）。

mod auth;
mod runner;
mod types;

pub use auth::{bearer_token_from_headers, check_pairing_auth};
pub use runner::{
    apply_chat_overrides, chunk_text_for_stream, extract_last_user_message, run_agent_chat,
};
pub use types::{
    ChatApiRequest, ChatApiResponse, ChatMessageInput, ChatUsage, ModelApiEntry, ProviderApiEntry,
    ProvidersApiResponse, WsClientMessage, WsServerMessage,
};
