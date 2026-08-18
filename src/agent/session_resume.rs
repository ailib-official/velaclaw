//! CLI/Web session resume via the existing ChatSessionStore (VL-MA-004 / R8).
//! 会话续传：复用 ChatSessionStore，不恢复未完成 tool 环。

use crate::gateway::local_control::sessions::ChatSessionStore;
use crate::gateway::local_control::types::ChatMessageInput;
use crate::providers::ChatMessage;
use anyhow::Result;
use std::path::Path;

pub fn history_from_session_messages(messages: &[ChatMessageInput]) -> Vec<ChatMessage> {
    messages
        .iter()
        .filter_map(|m| {
            let content = m.content.trim();
            if content.is_empty() {
                return None;
            }
            match m.role.as_str() {
                "user" => Some(ChatMessage::user(content)),
                "assistant" => Some(ChatMessage::assistant(content)),
                "system" => Some(ChatMessage::system(content)),
                _ => None,
            }
        })
        .collect()
}

pub async fn load_or_create_session(
    workspace_dir: &Path,
    session_id: Option<&str>,
) -> Result<(String, Vec<ChatMessage>)> {
    let store = ChatSessionStore::new(workspace_dir);
    if let Some(id) = session_id.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(existing) = store.get(id).await? {
            return Ok((
                existing.id,
                history_from_session_messages(&existing.messages),
            ));
        }
        anyhow::bail!("session not found: {id}");
    }
    let created = store.create(None, None).await?;
    Ok((created.id, Vec::new()))
}

pub async fn append_user_assistant_turn(
    workspace_dir: &Path,
    session_id: &str,
    user: &str,
    assistant: &str,
    model_id: Option<&str>,
) -> Result<()> {
    let store = ChatSessionStore::new(workspace_dir);
    let msgs = [
        ChatMessageInput {
            role: "user".into(),
            content: user.to_string(),
        },
        ChatMessageInput {
            role: "assistant".into(),
            content: assistant.to_string(),
        },
    ];
    store.append_messages(session_id, &msgs, model_id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_session_id_roundtrip_loads_prior_user_text() {
        let dir = tempfile::tempdir().unwrap();
        let (id, empty) = load_or_create_session(dir.path(), None).await.unwrap();
        assert!(empty.is_empty());
        append_user_assistant_turn(dir.path(), &id, "hello-ctx", "ack", None)
            .await
            .unwrap();
        let (again, hist) = load_or_create_session(dir.path(), Some(&id)).await.unwrap();
        assert_eq!(again, id);
        assert!(hist.iter().any(|m| m.content.contains("hello-ctx")));
    }
}
