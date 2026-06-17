//! Request/response and WebSocket frame types for the Local Control API.
//! 本地控制 API 的请求/响应与 WebSocket 帧类型。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct ChatMessageInput {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatApiRequest {
    pub messages: Vec<ChatMessageInput>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChatApiResponse {
    pub id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderApiEntry {
    pub id: String,
    pub available: bool,
    pub required_envs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelApiEntry {
    pub logical_id: String,
    pub provider: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProvidersApiResponse {
    pub providers: Vec<ProviderApiEntry>,
    pub models: Vec<ModelApiEntry>,
}

/// Client → server WebSocket payload.
#[derive(Debug, Clone, Deserialize)]
pub struct WsClientMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<ChatMessageInput>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
}

/// Server → client WebSocket payload (externally tagged by `type`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsServerMessage {
    Delta {
        content: String,
    },
    Done {
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ChatUsage>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
    },
    Error {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_server_delta_serializes() {
        let msg = WsServerMessage::Delta {
            content: "hi".into(),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert_eq!(json, r#"{"type":"delta","content":"hi"}"#);
    }

    #[test]
    fn ws_server_done_serializes() {
        let msg = WsServerMessage::Done {
            usage: Some(ChatUsage {
                input_tokens: 1,
                output_tokens: 2,
            }),
            cost: Some(0.001),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains(r#""type":"done""#));
        assert!(json.contains(r#""cost":0.001"#));
    }

    #[test]
    fn ws_server_error_serializes() {
        let msg = WsServerMessage::Error {
            message: "fail".into(),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert_eq!(json, r#"{"type":"error","message":"fail"}"#);
    }
}
