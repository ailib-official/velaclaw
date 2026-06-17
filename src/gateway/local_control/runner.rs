//! Agent-loop chat execution for Local Control API (VL-ARCH-001).
//! 本地控制 API 的 agent 循环对话执行（VL-ARCH-001）。

use super::types::{ChatApiRequest, ChatApiResponse, ChatMessageInput};
use crate::agent::agent::Agent;
use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use uuid::Uuid;

/// Apply per-request model/temperature overrides onto a config clone.
pub fn apply_chat_overrides(mut config: Config, req: &ChatApiRequest) -> Config {
    if let Some(model_id) = &req.model_id {
        if !model_id.trim().is_empty() {
            config.default_model = Some(model_id.trim().to_string());
            if let Some((provider, _)) = model_id.split_once('/') {
                config.default_provider = Some(provider.to_string());
            }
        }
    }
    if let Some(temp) = req.temperature {
        config.default_temperature = temp;
    }
    config
}

/// Returns the last non-empty user message from the chat history payload.
pub fn extract_last_user_message(messages: &[ChatMessageInput]) -> Result<String> {
    for msg in messages.iter().rev() {
        if msg.role == "user" {
            let trimmed = msg.content.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
    }
    Err(anyhow!(
        "messages must include at least one non-empty user message"
    ))
}

/// Run a single agent turn via `Agent::from_config` + `turn` (full tool loop).
pub async fn run_agent_chat(config: &Config, req: &ChatApiRequest) -> Result<ChatApiResponse> {
    let user_message = extract_last_user_message(&req.messages)?;
    let effective_config = apply_chat_overrides(config.clone(), req);

    let mut agent = Agent::from_config(&effective_config).context("failed to build agent")?;
    let content = agent
        .turn(&user_message)
        .await
        .context("agent turn failed")?;

    Ok(ChatApiResponse {
        id: format!("chat_{}", Uuid::new_v4()),
        content,
        usage: None,
        cost: None,
    })
}

/// Split assistant text into stream-sized chunks for WebSocket `delta` frames.
/// Phase 1 emits post-turn chunks; token-level streaming arrives with EVO-001.
pub fn chunk_text_for_stream(text: &str, chunk_size: usize) -> Vec<String> {
    let size = chunk_size.max(1);
    if text.is_empty() {
        return Vec::new();
    }
    text.chars()
        .collect::<Vec<_>>()
        .chunks(size)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_last_user_message_picks_latest_user() {
        let messages = vec![
            ChatMessageInput {
                role: "user".into(),
                content: "first".into(),
            },
            ChatMessageInput {
                role: "assistant".into(),
                content: "ok".into(),
            },
            ChatMessageInput {
                role: "user".into(),
                content: "second".into(),
            },
        ];
        assert_eq!(
            extract_last_user_message(&messages).expect("user"),
            "second"
        );
    }

    #[test]
    fn extract_last_user_message_rejects_empty() {
        let messages = vec![ChatMessageInput {
            role: "assistant".into(),
            content: "only assistant".into(),
        }];
        assert!(extract_last_user_message(&messages).is_err());
    }

    #[test]
    fn chunk_text_splits_unicode() {
        let chunks = chunk_text_for_stream("hello world", 5);
        assert_eq!(chunks, vec!["hello", " worl", "d"]);
    }

    #[test]
    fn apply_chat_overrides_sets_model_and_provider() {
        let mut base = Config::default();
        base.default_model = Some("old/model".into());
        let req = ChatApiRequest {
            messages: vec![],
            model_id: Some("deepseek/deepseek-v4-pro".into()),
            temperature: Some(0.2),
            max_tokens: None,
        };
        let updated = apply_chat_overrides(base, &req);
        assert_eq!(
            updated.default_model.as_deref(),
            Some("deepseek/deepseek-v4-pro")
        );
        assert_eq!(updated.default_provider.as_deref(), Some("deepseek"));
        assert!((updated.default_temperature - 0.2).abs() < f64::EPSILON);
    }
}
