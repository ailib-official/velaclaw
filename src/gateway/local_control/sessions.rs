//! File-backed chat session store for Web Chat Phase 2 (VL-UI-003).
//! Web Chat 第二阶段基于文件的会话存储（VL-UI-003）。

use super::types::ChatMessageInput;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSessionSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatSession {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<ChatMessageInput>,
}

#[derive(Debug, Clone)]
pub struct ChatSessionStore {
    root: PathBuf,
}

impl ChatSessionStore {
    pub fn new(workspace_dir: &Path) -> Self {
        Self {
            root: workspace_dir.join(".velaclaw").join("chat_sessions"),
        }
    }

    fn session_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{id}.json"))
    }

    async fn ensure_root(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.root)
            .await
            .with_context(|| format!("create session dir {}", self.root.display()))
    }

    pub async fn list(&self) -> Result<Vec<ChatSessionSummary>> {
        self.ensure_root().await?;
        let mut entries = tokio::fs::read_dir(&self.root).await?;
        let mut summaries = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(session) = Self::read_session_file(&path).await {
                summaries.push(session.summary());
            }
        }

        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    pub async fn get(&self, id: &str) -> Result<Option<ChatSession>> {
        if !is_valid_session_id(id) {
            return Ok(None);
        }
        let path = self.session_path(id);
        if !path.is_file() {
            return Ok(None);
        }
        Ok(Some(Self::read_session_file(&path).await?))
    }

    pub async fn create(
        &self,
        title: Option<String>,
        model_id: Option<String>,
    ) -> Result<ChatSession> {
        self.ensure_root().await?;
        let now = Utc::now().to_rfc3339();
        let session = ChatSession {
            id: Uuid::new_v4().to_string(),
            title: title
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "New chat".to_string()),
            created_at: now.clone(),
            updated_at: now,
            model_id: model_id.filter(|m| !m.trim().is_empty()),
            messages: Vec::new(),
        };
        self.write_session(&session).await?;
        Ok(session)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        if !is_valid_session_id(id) {
            return Ok(false);
        }
        let path = self.session_path(id);
        if !path.is_file() {
            return Ok(false);
        }
        tokio::fs::remove_file(path).await?;
        Ok(true)
    }

    pub async fn append_messages(
        &self,
        id: &str,
        new_messages: &[ChatMessageInput],
        model_id: Option<&str>,
    ) -> Result<()> {
        let Some(mut session) = self.get(id).await? else {
            anyhow::bail!("session not found: {id}");
        };

        session.messages.extend(new_messages.iter().cloned());
        if let Some(model) = model_id.filter(|m| !m.trim().is_empty()) {
            session.model_id = Some(model.to_string());
        }
        if session.title == "New chat" {
            if let Some(first_user) = session
                .messages
                .iter()
                .find(|m| m.role == "user" && !m.content.trim().is_empty())
            {
                session.title = truncate_title(&first_user.content);
            }
        }
        session.updated_at = Utc::now().to_rfc3339();
        self.write_session(&session).await
    }

    async fn write_session(&self, session: &ChatSession) -> Result<()> {
        self.ensure_root().await?;
        let path = self.session_path(&session.id);
        let data = serde_json::to_vec_pretty(session).context("serialize session")?;
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    async fn read_session_file(path: &Path) -> Result<ChatSession> {
        let data = tokio::fs::read(path).await?;
        let session: ChatSession = serde_json::from_slice(&data).context("parse session json")?;
        Ok(session)
    }
}

impl ChatSession {
    fn summary(&self) -> ChatSessionSummary {
        ChatSessionSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            created_at: self.created_at.clone(),
            updated_at: self.updated_at.clone(),
            model_id: self.model_id.clone(),
            message_count: self.messages.len(),
        }
    }
}

fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn truncate_title(text: &str) -> String {
    let trimmed = text.trim().replace('\n', " ");
    const MAX: usize = 48;
    if trimmed.chars().count() <= MAX {
        trimmed
    } else {
        let end = trimmed
            .char_indices()
            .nth(MAX)
            .map(|(i, _)| i)
            .unwrap_or(trimmed.len());
        format!("{}…", &trimmed[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_session_ids() {
        assert!(is_valid_session_id("abc-123_def"));
        assert!(!is_valid_session_id("../etc/passwd"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn truncate_title_limits_length() {
        let long = "a".repeat(80);
        let title = truncate_title(&long);
        assert!(title.chars().count() <= 49);
        assert!(title.ends_with('…'));
    }

    #[tokio::test]
    async fn create_list_get_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChatSessionStore::new(dir.path());

        let created = store
            .create(Some("Test".into()), Some("deepseek/model".into()))
            .await
            .unwrap();
        assert_eq!(created.title, "Test");

        let listed = store.list().await.unwrap();
        assert_eq!(listed.len(), 1);

        let fetched = store.get(&created.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, created.id);

        let user = ChatMessageInput {
            role: "user".into(),
            content: "hello".into(),
        };
        store
            .append_messages(&created.id, &[user], Some("deepseek/model"))
            .await
            .unwrap();

        let updated = store.get(&created.id).await.unwrap().unwrap();
        assert_eq!(updated.messages.len(), 1);
        assert_eq!(updated.title, "hello");

        assert!(store.delete(&created.id).await.unwrap());
        assert!(store.get(&created.id).await.unwrap().is_none());
    }
}
