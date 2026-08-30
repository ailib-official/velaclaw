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
    /// `plan` or `build` (default). Plan blocks mutating tools (VL-MA-004).
    #[serde(default)]
    pub host_phase: Option<String>,
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
    /// Effective turn model after `resolve_turn_model` (observe / UX honesty).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<String>,
    /// Selection reason (`explicit_user_pick`, `host_decide:…`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_selection_reason: Option<String>,
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
    #[serde(default)]
    pub host_phase: Option<String>,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        selected_model: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model_selection_reason: Option<String>,
    },
    Error {
        message: String,
    },
    ApprovalRequired {
        id: String,
        tool_name: String,
        arguments_summary: String,
        #[serde(default)]
        elevation: bool,
    },
    /// Interactive human input (choice / text / secret / handoff).
    InputRequired {
        id: String,
        kind: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        options: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        risk_note: Option<String>,
    },
    /// Compact turn status (model request / tool start).
    Status {
        phase: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// Intermediate tool result (distinct from assistant delta).
    Step {
        kind: String,
        tool: String,
        ok: bool,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expand: Option<String>,
    },
    /// Live bounded DAG rail (node progress; outline is also in `outline`).
    Dag {
        dag_id: String,
        fallback: bool,
        outline: String,
        nodes: Vec<WsDagNode>,
    },
    /// User cancelled the in-flight turn.
    Cancelled {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WsDagNode {
    pub id: String,
    pub label: String,
    pub task_type: String,
    pub caps: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub contact: String,
    pub status: String,
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
            selected_model: Some("deepseek/deepseek-v4-flash".into()),
            model_selection_reason: Some("explicit_user_pick".into()),
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

    #[test]
    fn ws_server_status_and_step_serialize() {
        let status = WsServerMessage::Status {
            phase: "model".into(),
            detail: Some("deepseek/x".into()),
        };
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(json.contains(r#""type":"status""#));
        assert!(json.contains(r#""phase":"model""#));

        let step = WsServerMessage::Step {
            kind: "tool_result".into(),
            tool: "shell".into(),
            ok: true,
            summary: "ok".into(),
            expand: Some("On branch main".into()),
        };
        let json = serde_json::to_string(&step).expect("serialize");
        assert!(json.contains(r#""type":"step""#));
        assert!(json.contains(r#""tool":"shell""#));
        assert!(json.contains(r#""expand":"On branch main""#));
    }

    #[test]
    fn ws_server_dag_serializes() {
        let msg = WsServerMessage::Dag {
            dag_id: "opcencode-check-upgrade".into(),
            fallback: false,
            outline: "Working in 1 step(s):".into(),
            nodes: vec![WsDagNode {
                id: "check_install".into(),
                label: "check install".into(),
                task_type: "ops-check".into(),
                caps: "coding".into(),
                contact: "hint:code".into(),
                status: "running".into(),
            }],
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains(r#""type":"dag""#));
        assert!(json.contains(r#""status":"running""#));
    }

    #[test]
    fn ws_server_cancelled_serializes() {
        let msg = WsServerMessage::Cancelled {
            message: Some("Stopped.".into()),
        };
        let json = serde_json::to_string(&msg).expect("serialize");
        assert!(json.contains(r#""type":"cancelled""#));
    }
}
