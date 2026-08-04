//! Agent-loop chat execution for Local Control API (VL-ARCH-001).
//! 本地控制 API 的 agent 循环对话执行（VL-ARCH-001）。

use super::sessions::ChatSessionStore;
use super::types::{ChatApiRequest, ChatApiResponse, ChatMessageInput};
use crate::agent::agent::Agent;
use crate::config::Config;
use crate::protocol_registry::{
    provider_id_from_logical, resolve_local_protocol_root, scan_protocol_root,
};
use crate::providers::ChatMessage;
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

/// Apply per-request model/temperature overrides onto a config clone.
pub fn apply_chat_overrides(mut config: Config, req: &ChatApiRequest) -> Config {
    if let Some(model_id) = &req.model_id {
        let trimmed = model_id.trim();
        // Only honor protocol `provider/model` ids. Bare labels like `deepseek-chat`
        // from older UI session metadata must not clobber the configured default.
        if trimmed.contains('/') {
            let (logical_id, provider) = resolve_chat_model_override(trimmed);
            config.default_model = Some(logical_id);
            config.default_provider = Some(provider);
        }
    }
    if let Some(temp) = req.temperature {
        config.default_temperature = temp;
    }
    config
}

/// Map a chat picker/session model id to `(logical_id, provider)`.
///
/// Composed logical ids under a known provider stay as-is. Bare aggregator
/// wire ids (e.g. `deepseek-ai/deepseek-v4-flash`) remap uniquely via the
/// local protocol registry when possible.
fn resolve_chat_model_override(raw: &str) -> (String, String) {
    let first = provider_id_from_logical(raw).to_string();
    let Some(root) = resolve_local_protocol_root() else {
        return (raw.to_string(), first);
    };
    let Ok(snap) = scan_protocol_root(&root) else {
        return (raw.to_string(), first);
    };
    if snap.provider_by_id(&first).is_some() {
        return (raw.to_string(), first);
    }
    if let Some(entry) = snap.resolve_chat_model_id(raw) {
        return (entry.logical_id.clone(), entry.provider.clone());
    }
    (raw.to_string(), first)
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
pub async fn run_agent_chat(
    config: &Config,
    req: &ChatApiRequest,
    approval_hub: Option<&Arc<crate::approval::ApprovalHub>>,
    human_input_hub: Option<&Arc<crate::approval::HumanInputHub>>,
) -> Result<ChatApiResponse> {
    let user_message = extract_last_user_message(&req.messages)?;
    let effective_config = apply_chat_overrides(config.clone(), req);

    let mut agent = Agent::from_config(&effective_config).context("failed to build agent")?;
    if let Some(hub) = approval_hub {
        agent
            .enable_gateway_approval(Arc::clone(hub), &effective_config)
            .context("wire gateway approval manager")?;
    }
    if let Some(hub) = human_input_hub {
        agent.enable_gateway_hitl(Arc::clone(hub));
    }
    // Seed prior turns so multi-step Web UI chat keeps context (UI sends full history).
    seed_prior_messages(&mut agent, &req.messages)?;
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

/// Inject all messages before the last user turn into a fresh agent (system prompt first).
fn seed_prior_messages(agent: &mut Agent, messages: &[ChatMessageInput]) -> Result<()> {
    let Some(last_user_idx) = messages
        .iter()
        .rposition(|m| m.role == "user" && !m.content.trim().is_empty())
    else {
        return Ok(());
    };
    let prior = &messages[..last_user_idx];
    if prior.is_empty() {
        return Ok(());
    }
    agent.ensure_system_prompt()?;
    for msg in prior {
        let content = msg.content.trim();
        if content.is_empty() {
            continue;
        }
        match msg.role.as_str() {
            "user" => agent.push_chat_message(ChatMessage::user(content)),
            "assistant" => agent.push_chat_message(ChatMessage::assistant(content)),
            // Agent already owns the system prompt; ignore prior system turns.
            _ => {}
        }
    }
    Ok(())
}

/// Append the latest user turn and assistant reply to a persisted session, if `session_id` is set.
pub async fn persist_chat_turn(
    workspace_dir: &Path,
    session_id: Option<&str>,
    req: &ChatApiRequest,
    assistant_content: &str,
) -> Result<()> {
    let Some(id) = session_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };

    let user_message = extract_last_user_message(&req.messages)?;
    let store = ChatSessionStore::new(workspace_dir);
    let to_store = vec![
        ChatMessageInput {
            role: "user".into(),
            content: user_message,
        },
        ChatMessageInput {
            role: "assistant".into(),
            content: assistant_content.to_string(),
        },
    ];
    store
        .append_messages(id, &to_store, req.model_id.as_deref())
        .await
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
    use std::fs;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let old = std::env::var(key).ok();
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.old.as_ref() {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

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
            session_id: None,
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

    #[test]
    fn apply_chat_overrides_ignores_bare_model_label() {
        let mut base = Config::default();
        base.default_provider = Some("nvidia/nemotron-3-super-120b-a12b".into());
        base.default_model = Some("nvidia/nemotron-3-super-120b-a12b".into());
        let req = ChatApiRequest {
            messages: vec![],
            session_id: None,
            model_id: Some("deepseek-chat".into()),
            temperature: None,
            max_tokens: None,
        };
        let updated = apply_chat_overrides(base, &req);
        assert_eq!(
            updated.default_model.as_deref(),
            Some("nvidia/nemotron-3-super-120b-a12b")
        );
        assert_eq!(
            updated.default_provider.as_deref(),
            Some("nvidia/nemotron-3-super-120b-a12b")
        );
    }

    #[test]
    fn apply_chat_overrides_remaps_bare_aggregator_wire_id() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).expect("provider dir");
        fs::write(
            providers.join("nvidia.yaml"),
            r#"
id: nvidia
name: NVIDIA
metadata:
  models:
    deepseek-ai/deepseek-v4-flash:
      context_window: 1000000
"#,
        )
        .expect("manifest");
        let _proto = EnvGuard::set(
            "AI_PROTOCOL_DIR",
            Some(dir.path().to_str().expect("utf8 path")),
        );
        let _path = EnvGuard::set("AI_PROTOCOL_PATH", None);

        let mut base = Config::default();
        base.default_provider = Some("deepseek".into());
        base.default_model = Some("deepseek/deepseek-v4-flash".into());
        let req = ChatApiRequest {
            messages: vec![],
            session_id: None,
            model_id: Some("deepseek-ai/deepseek-v4-flash".into()),
            temperature: None,
            max_tokens: None,
        };
        let updated = apply_chat_overrides(base, &req);
        assert_eq!(
            updated.default_model.as_deref(),
            Some("nvidia/deepseek-ai/deepseek-v4-flash")
        );
        assert_eq!(updated.default_provider.as_deref(), Some("nvidia"));
    }

    #[test]
    fn seed_prior_messages_index_excludes_last_user() {
        let messages = [
            ChatMessageInput {
                role: "user".into(),
                content: "hi".into(),
            },
            ChatMessageInput {
                role: "assistant".into(),
                content: "hello".into(),
            },
            ChatMessageInput {
                role: "user".into(),
                content: "run ls".into(),
            },
        ];
        let last_user_idx = messages
            .iter()
            .rposition(|m| m.role == "user" && !m.content.trim().is_empty())
            .expect("user");
        assert_eq!(last_user_idx, 2);
        assert_eq!(messages[..last_user_idx].len(), 2);
    }
}
