//! Rebuild `ConversationMessage` history from tool-loop Chat frames (VL-REVIEW2-A2).
//! 从 tool-loop Chat 帧还原结构化会话历史；不搬 `run_tool_call_loop`。

use crate::provider::{ChatMessage, ConversationMessage, ToolCall, ToolResultMessage};

/// Rebuild Agent history from `run_tool_call_loop` Chat frames (VL-CTX-002).
///
/// Restores `AssistantToolCalls` / `ToolResults` from the native wire encoding
/// (`build_native_assistant_history` + `tool_with_call_id`) so observers keep
/// the structured public history shape. Text-tool paths (`[Tool results]` user
/// messages) stay as `Chat`.
pub fn conversation_from_tool_loop_history(messages: &[ChatMessage]) -> Vec<ConversationMessage> {
    let mut out = Vec::with_capacity(messages.len());
    let mut i = 0;
    while i < messages.len() {
        let msg = &messages[i];
        if msg.role == "assistant" {
            if let Some((text, tool_calls)) = try_parse_native_assistant_tool_calls(&msg.content) {
                out.push(ConversationMessage::AssistantToolCalls { text, tool_calls });
                i += 1;
                let mut results = Vec::new();
                while i < messages.len() && messages[i].role == "tool" {
                    results.push(tool_result_from_provider_chat(&messages[i]));
                    i += 1;
                }
                if !results.is_empty() {
                    out.push(ConversationMessage::ToolResults(results));
                }
                continue;
            }
            out.push(ConversationMessage::Chat(msg.clone()));
            i += 1;
            continue;
        }

        if msg.role == "tool" {
            let mut results = Vec::new();
            while i < messages.len() && messages[i].role == "tool" {
                results.push(tool_result_from_provider_chat(&messages[i]));
                i += 1;
            }
            out.push(ConversationMessage::ToolResults(results));
            continue;
        }

        out.push(ConversationMessage::Chat(msg.clone()));
        i += 1;
    }
    out
}

fn try_parse_native_assistant_tool_calls(content: &str) -> Option<(Option<String>, Vec<ToolCall>)> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let raw_calls = value.get("tool_calls")?.as_array()?;
    if raw_calls.is_empty() {
        return None;
    }
    let text = match value.get("content") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(serde_json::Value::Null) | None => None,
        Some(other) => Some(other.to_string()),
    };
    let mut tool_calls = Vec::with_capacity(raw_calls.len());
    for tc in raw_calls {
        let id = tc.get("id")?.as_str()?.to_string();
        let name = tc.get("name")?.as_str()?.to_string();
        let arguments = match tc.get("arguments") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => "{}".into(),
        };
        tool_calls.push(ToolCall {
            id,
            name,
            arguments,
        });
    }
    Some((text, tool_calls))
}

fn tool_result_from_provider_chat(msg: &ChatMessage) -> ToolResultMessage {
    if let Some(id) = msg.tool_call_id.as_ref() {
        return ToolResultMessage {
            tool_call_id: id.clone(),
            content: msg.content.clone(),
        };
    }
    // NativeToolDispatcher.to_provider_messages encodes tool results as JSON body.
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&msg.content) {
        if let (Some(id), Some(content)) = (
            value.get("tool_call_id").and_then(|v| v.as_str()),
            value.get("content").and_then(|v| v.as_str()),
        ) {
            return ToolResultMessage {
                tool_call_id: id.to_string(),
                content: content.to_string(),
            };
        }
    }
    ToolResultMessage {
        tool_call_id: "unknown".into(),
        content: msg.content.clone(),
    }
}

/// Reintegrate prepared Chat frames into `ConversationMessage` history.
///
/// When prepare did not change Chat count, replace Chat slots in place so
/// native `AssistantToolCalls` / `ToolResults` keep their temporal order.
/// When compact/layered rewrote the Chat vector length, fall back to a
/// Chat-only history (structured frames from the compacted span are dropped).
pub fn reintegrate_prepared_chat(
    history: &[ConversationMessage],
    prepared: Vec<ChatMessage>,
    original_chat_count: usize,
) -> Vec<ConversationMessage> {
    if prepared.len() == original_chat_count {
        let mut prepared_iter = prepared.into_iter();
        return history
            .iter()
            .map(|msg| match msg {
                ConversationMessage::Chat(_) => ConversationMessage::Chat(
                    prepared_iter
                        .next()
                        .expect("prepared chat count matches original"),
                ),
                other => other.clone(),
            })
            .collect();
    }

    prepared
        .into_iter()
        .map(ConversationMessage::Chat)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loop_parse::build_native_assistant_history;

    #[test]
    fn conversation_from_tool_loop_history_restores_native_frames() {
        let assistant = ChatMessage::assistant(build_native_assistant_history(
            "checking",
            &[ToolCall {
                id: "c1".into(),
                name: "echo".into(),
                arguments: "{}".into(),
            }],
        ));
        let tool = ChatMessage::tool_with_call_id("c1", "tool-out");
        let msgs = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hi"),
            assistant,
            tool,
            ChatMessage::assistant("done"),
        ];
        let conv = conversation_from_tool_loop_history(&msgs);
        assert!(matches!(
            &conv[0],
            ConversationMessage::Chat(m) if m.role == "system"
        ));
        assert!(matches!(
            &conv[1],
            ConversationMessage::Chat(m) if m.role == "user"
        ));
        match &conv[2] {
            ConversationMessage::AssistantToolCalls { text, tool_calls } => {
                assert_eq!(text.as_deref(), Some("checking"));
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].id, "c1");
                assert_eq!(tool_calls[0].name, "echo");
            }
            other => panic!("expected AssistantToolCalls, got {other:?}"),
        }
        match &conv[3] {
            ConversationMessage::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].tool_call_id, "c1");
                assert_eq!(results[0].content, "tool-out");
            }
            other => panic!("expected ToolResults, got {other:?}"),
        }
        assert!(matches!(
            &conv[4],
            ConversationMessage::Chat(m) if m.role == "assistant" && m.content == "done"
        ));
    }

    #[test]
    fn conversation_from_tool_loop_history_keeps_text_tool_results_as_chat() {
        let msgs = vec![
            ChatMessage::user("hi"),
            ChatMessage::assistant("calling"),
            ChatMessage::user("[Tool results]\n<tool_result id=\"x\">ok</tool_result>"),
        ];
        let conv = conversation_from_tool_loop_history(&msgs);
        assert_eq!(conv.len(), 3);
        assert!(conv
            .iter()
            .all(|m| matches!(m, ConversationMessage::Chat(_))));
    }

    #[test]
    fn conversation_from_tool_loop_history_parses_json_body_tool_role() {
        let msgs = vec![ChatMessage::tool(
            serde_json::json!({
                "tool_call_id": "from-json",
                "content": "payload",
            })
            .to_string(),
        )];
        let conv = conversation_from_tool_loop_history(&msgs);
        match &conv[0] {
            ConversationMessage::ToolResults(results) => {
                assert_eq!(results[0].tool_call_id, "from-json");
                assert_eq!(results[0].content, "payload");
            }
            other => panic!("expected ToolResults, got {other:?}"),
        }
    }

    #[test]
    fn reintegrate_prepared_chat_preserves_structured_slots() {
        let history = vec![
            ConversationMessage::Chat(ChatMessage::user("hi")),
            ConversationMessage::AssistantToolCalls {
                text: Some("x".into()),
                tool_calls: vec![ToolCall {
                    id: "c1".into(),
                    name: "echo".into(),
                    arguments: "{}".into(),
                }],
            },
            ConversationMessage::Chat(ChatMessage::assistant("done")),
        ];
        let prepared = vec![
            ChatMessage::user("hi-prep"),
            ChatMessage::assistant("done-prep"),
        ];
        let out = reintegrate_prepared_chat(&history, prepared, 2);
        assert!(matches!(
            &out[0],
            ConversationMessage::Chat(m) if m.content == "hi-prep"
        ));
        assert!(matches!(
            &out[1],
            ConversationMessage::AssistantToolCalls { .. }
        ));
        assert!(matches!(
            &out[2],
            ConversationMessage::Chat(m) if m.content == "done-prep"
        ));
    }
}
