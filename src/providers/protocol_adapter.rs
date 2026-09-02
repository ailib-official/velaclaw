//! Protocol-backed provider adapter — **trait bridge only** (VL-EVO-004).
//!
//! Maps VelaClaw's [`Provider`] trait to an existing `AiClient` built by
//! [`ExecutionHandle`](crate::execution::ExecutionHandle). Execution semantics
//! (init, transport retry, telemetry) live in `crate::execution::byok`.
//! 协议适配器仅负责 trait 桥接；执行逻辑归执行层。

use crate::execution::execute_chat_with_retry;
use crate::providers::traits::{
    ChatMessage, ChatRequest, ChatResponse, Provider, ProviderCapabilities, StreamChunk,
    StreamOptions, StreamResult, ToolCall, ToolsPayload,
};
use crate::telemetry::ByokTelemetryReporter;
use crate::tools::ToolSpec;
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
use std::sync::Arc;

pub struct ProtocolBackedProvider {
    client: Arc<ai_lib_rust::AiClient>,
    provider_id: String,
    model_id: String,
    telemetry: Option<Arc<ByokTelemetryReporter>>,
}

impl ProtocolBackedProvider {
    /// Wrap an existing `AiClient` (model bound at construction; no per-request override).
    pub fn from_client(
        client: Arc<ai_lib_rust::AiClient>,
        logical_model_id: &str,
        telemetry: Option<Arc<ByokTelemetryReporter>>,
    ) -> anyhow::Result<Self> {
        let (provider_id, model_id) = crate::execution::split_logical_model_id(logical_model_id)?;
        Ok(Self {
            client,
            provider_id,
            model_id,
            telemetry,
        })
    }

    /// Factory helper for `create_provider` — builds client via execution layer.
    pub fn from_logical_model(provider_id: &str, model_id: &str) -> anyhow::Result<Self> {
        let logical = format!("{provider_id}/{model_id}");
        let init = crate::execution::nvidia_byok_ai_client_logical_id(&logical);
        let client = crate::execution::init_ai_client_sync(&init)?;
        Self::from_client(client, &logical, None)
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    fn telemetry_ref(&self) -> Option<&ByokTelemetryReporter> {
        self.telemetry.as_deref()
    }

    async fn run_chat(
        &self,
        messages: Vec<ai_lib_rust::Message>,
        temperature: f64,
        tools: Option<Vec<serde_json::Value>>,
    ) -> anyhow::Result<ai_lib_rust::client::UnifiedResponse> {
        execute_chat_with_retry(
            &self.client,
            &self.provider_id,
            &self.model_id,
            messages,
            temperature,
            tools,
            self.telemetry_ref(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Protocol provider error: {e}"))
    }

    fn convert_messages(messages: &[ChatMessage]) -> Vec<ai_lib_rust::Message> {
        messages
            .iter()
            .map(|m| match m.role.as_str() {
                "system" => ai_lib_rust::Message::system(&m.content),
                "assistant" => ai_lib_rust::Message::assistant(&m.content),
                "tool" => {
                    // Adapter Message has no assistant tool_calls field. Emitting
                    // role=tool after a text-only assistant causes DeepSeek HTTP 400.
                    let id = m.tool_call_id.as_deref().unwrap_or("unknown");
                    ai_lib_rust::Message::user(format!("[Tool result {id}] {}", m.content))
                }
                _ => ai_lib_rust::Message::user(&m.content),
            })
            .collect()
    }

    fn stream_event_to_chunk(event: ai_lib_rust::StreamingEvent) -> Option<StreamChunk> {
        match event {
            ai_lib_rust::StreamingEvent::PartialContentDelta { content, .. } => {
                (!content.is_empty()).then(|| StreamChunk::delta(content).with_token_estimate())
            }
            ai_lib_rust::StreamingEvent::ThinkingDelta { thinking, .. } => (!thinking.is_empty())
                .then(|| {
                    StreamChunk::delta(format!("[thinking] {thinking}")).with_token_estimate()
                }),
            ai_lib_rust::StreamingEvent::ToolCallStarted {
                tool_call_id,
                tool_name,
                index,
            } => Some(StreamChunk::tool_call_started(
                tool_call_id,
                tool_name,
                index,
            )),
            ai_lib_rust::StreamingEvent::PartialToolCall {
                tool_call_id,
                arguments,
                index,
                is_complete,
            } => Some(StreamChunk::tool_call_arguments(
                tool_call_id,
                arguments,
                index,
                is_complete,
            )),
            ai_lib_rust::StreamingEvent::ToolCallEnded {
                tool_call_id,
                index,
            } => Some(StreamChunk::tool_call_ended(tool_call_id, index)),
            ai_lib_rust::StreamingEvent::StreamEnd { .. } => Some(StreamChunk::final_chunk()),
            ai_lib_rust::StreamingEvent::StreamError { error, .. } => Some(StreamChunk::error(
                format!("Protocol stream error: {error}"),
            )),
            ai_lib_rust::StreamingEvent::Metadata { .. }
            | ai_lib_rust::StreamingEvent::FinalCandidate { .. } => None,
        }
    }
}

#[async_trait]
impl Provider for ProtocolBackedProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: true,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(ai_lib_rust::Message::system(sys));
        }
        messages.push(ai_lib_rust::Message::user(message));

        let response = self.run_chat(messages, temperature, None).await?;

        Ok(response.content)
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let converted = Self::convert_messages(messages);

        let response = self.run_chat(converted, temperature, None).await?;

        Ok(response.content)
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let converted = Self::convert_messages(request.messages);

        let tools = request.tools.map(|tools| {
            tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect::<Vec<_>>()
        });

        let response = self.run_chat(converted, temperature, tools).await?;

        Ok(ChatResponse {
            text: Some(response.content),
            tool_calls: response
                .tool_calls
                .into_iter()
                .map(|tc| ToolCall {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments.to_string(),
                })
                .collect(),
        })
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[serde_json::Value],
        _model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let converted = Self::convert_messages(messages);

        let tools_opt = if tools.is_empty() {
            None
        } else {
            Some(tools.to_vec())
        };

        let response = self.run_chat(converted, temperature, tools_opt).await?;

        Ok(ChatResponse {
            text: Some(response.content),
            tool_calls: response
                .tool_calls
                .into_iter()
                .map(|tc| ToolCall {
                    id: tc.id,
                    name: tc.name,
                    arguments: tc.arguments.to_string(),
                })
                .collect(),
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn stream_chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        temperature: f64,
        _options: StreamOptions,
    ) -> stream::BoxStream<'static, StreamResult<StreamChunk>> {
        let mut messages = Vec::new();
        if let Some(sys) = system_prompt {
            messages.push(ai_lib_rust::Message::system(sys));
        }
        messages.push(ai_lib_rust::Message::user(message));

        let client = Arc::clone(&self.client);

        async_stream::try_stream! {
            let mut stream = client.chat()
                .messages(messages)
                .temperature(temperature)
                .stream()
                .execute_stream()
                .await
                .map_err(|e| crate::providers::traits::StreamError::Provider(e.to_string()))?;

            while let Some(event) = stream.next().await {
                match event {
                    Ok(event) => {
                        if let Some(chunk) = Self::stream_event_to_chunk(event) {
                            let done = chunk.is_final;
                            yield chunk;
                            if done {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        yield StreamChunk::error(e.to_string());
                        break;
                    }
                }
            }
        }
        .boxed()
    }

    fn convert_tools(&self, tools: &[ToolSpec]) -> ToolsPayload {
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();

        ToolsPayload::OpenAI { tools: tools_json }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::StreamToolCallDelta;

    #[test]
    fn test_convert_messages() {
        let messages = vec![
            ChatMessage::system("You are helpful."),
            ChatMessage::user("Hello"),
        ];
        let converted = ProtocolBackedProvider::convert_messages(&messages);
        assert_eq!(converted.len(), 2);
    }

    #[test]
    fn test_convert_messages_tool_role_with_call_id() {
        let messages = vec![ChatMessage::tool_with_call_id("call_1", "result json")];
        let converted = ProtocolBackedProvider::convert_messages(&messages);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].role, ai_lib_rust::MessageRole::User);
        assert!(format!("{:?}", converted[0].content).contains("call_1"));
    }

    #[test]
    fn convert_messages_preserves_multi_turn_tool_conversation() {
        let messages = vec![
            ChatMessage::user("Find the current status"),
            ChatMessage::assistant("I will call a tool."),
            ChatMessage::tool_with_call_id("call_1", r#"{"status":"ok"}"#),
            ChatMessage::assistant("The status is ok."),
        ];
        let converted = ProtocolBackedProvider::convert_messages(&messages);

        assert_eq!(converted.len(), 4);
        assert_eq!(converted[2].role, ai_lib_rust::MessageRole::User);
        assert!(format!("{:?}", converted[2].content).contains("call_1"));

        let serialized = serde_json::to_value(&converted).expect("serialize messages");
        assert_eq!(serialized[0]["role"], "user");
        assert_eq!(serialized[1]["role"], "assistant");
        assert_eq!(serialized[2]["role"], "user");
        assert_eq!(serialized[3]["role"], "assistant");
    }

    #[test]
    fn stream_event_to_chunk_maps_content_and_thinking() {
        let content = ProtocolBackedProvider::stream_event_to_chunk(
            ai_lib_rust::StreamingEvent::PartialContentDelta {
                content: "hello".to_string(),
                sequence_id: Some(1),
            },
        )
        .expect("content chunk");
        assert_eq!(content.delta, "hello");
        assert!(content.tool_call_delta.is_none());
        assert!(!content.is_final);
        assert!(content.token_count > 0);

        let thinking = ProtocolBackedProvider::stream_event_to_chunk(
            ai_lib_rust::StreamingEvent::ThinkingDelta {
                thinking: "checking".to_string(),
                tool_consideration: None,
            },
        )
        .expect("thinking chunk");
        assert_eq!(thinking.delta, "[thinking] checking");
        assert!(thinking.tool_call_delta.is_none());
    }

    #[test]
    fn stream_event_to_chunk_emits_tool_call_deltas() {
        let started = ProtocolBackedProvider::stream_event_to_chunk(
            ai_lib_rust::StreamingEvent::ToolCallStarted {
                tool_call_id: "call_1".to_string(),
                tool_name: "lookup".to_string(),
                index: Some(0),
            },
        )
        .expect("tool start chunk");
        assert_eq!(
            started.tool_call_delta,
            Some(StreamToolCallDelta::Started {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                index: Some(0),
            })
        );

        let partial = ProtocolBackedProvider::stream_event_to_chunk(
            ai_lib_rust::StreamingEvent::PartialToolCall {
                tool_call_id: "call_1".to_string(),
                arguments: r#"{"query":"velaclaw"}"#.to_string(),
                index: Some(0),
                is_complete: Some(false),
            },
        )
        .expect("tool arguments chunk");
        assert_eq!(
            partial.tool_call_delta,
            Some(StreamToolCallDelta::Arguments {
                id: "call_1".to_string(),
                arguments: r#"{"query":"velaclaw"}"#.to_string(),
                index: Some(0),
                is_complete: Some(false),
            })
        );

        let ended = ProtocolBackedProvider::stream_event_to_chunk(
            ai_lib_rust::StreamingEvent::ToolCallEnded {
                tool_call_id: "call_1".to_string(),
                index: Some(0),
            },
        )
        .expect("tool end chunk");
        assert_eq!(
            ended.tool_call_delta,
            Some(StreamToolCallDelta::Ended {
                id: "call_1".to_string(),
                index: Some(0),
            })
        );
    }

    #[test]
    fn stream_event_to_chunk_handles_end_and_ignores_metadata() {
        let final_chunk =
            ProtocolBackedProvider::stream_event_to_chunk(ai_lib_rust::StreamingEvent::StreamEnd {
                finish_reason: Some("stop".to_string()),
            })
            .expect("final chunk");
        assert!(final_chunk.is_final);

        let metadata =
            ProtocolBackedProvider::stream_event_to_chunk(ai_lib_rust::StreamingEvent::Metadata {
                usage: Some(serde_json::json!({"input_tokens": 1})),
                finish_reason: None,
                stop_reason: None,
            });
        assert!(metadata.is_none());
    }
}
