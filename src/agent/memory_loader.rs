use crate::memory::{self, Memory, MemoryEntry};
use async_trait::async_trait;
use std::fmt::Write;

#[async_trait]
pub trait MemoryLoader: Send + Sync {
    async fn load_context(
        &self,
        memory: &dyn Memory,
        user_message: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<String>;
}

pub struct DefaultMemoryLoader {
    limit: usize,
    min_relevance_score: f64,
}

impl Default for DefaultMemoryLoader {
    fn default() -> Self {
        Self {
            limit: 5,
            min_relevance_score: 0.4,
        }
    }
}

impl DefaultMemoryLoader {
    pub fn new(limit: usize, min_relevance_score: f64) -> Self {
        Self {
            limit: limit.max(1),
            min_relevance_score,
        }
    }
}

fn format_context_entries(
    entries: &[MemoryEntry],
    min_relevance_score: f64,
    session_id: Option<&str>,
) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let mut context = String::from("[Memory context]\n");
    for entry in entries {
        if memory::is_assistant_autosave_key(&entry.key) {
            continue;
        }
        if !memory::should_inject_for_session(entry, session_id) {
            continue;
        }
        if let Some(score) = entry.score {
            if score < min_relevance_score {
                continue;
            }
        }
        let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
    }

    if context == "[Memory context]\n" {
        return String::new();
    }

    context.push('\n');
    context
}

#[async_trait]
impl MemoryLoader for DefaultMemoryLoader {
    async fn load_context(
        &self,
        memory: &dyn Memory,
        user_message: &str,
        session_id: Option<&str>,
    ) -> anyhow::Result<String> {
        // Recall without SQL session filter so Core (often session_id=None) remains
        // visible; apply VL-MEM-001 inject rules in-process.
        let entries = memory.recall(user_message, self.limit, None).await?;
        Ok(format_context_entries(
            &entries,
            self.min_relevance_score,
            session_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{Memory, MemoryCategory, MemoryEntry};
    use std::sync::Arc;

    struct MockMemory;
    struct MockMemoryWithEntries {
        entries: Arc<Vec<MemoryEntry>>,
    }

    #[async_trait]
    impl Memory for MockMemory {
        async fn store(
            &self,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            limit: usize,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            if limit == 0 {
                return Ok(vec![]);
            }
            Ok(vec![MemoryEntry {
                id: "1".into(),
                key: "k".into(),
                content: "v".into(),
                category: MemoryCategory::Conversation,
                timestamp: "now".into(),
                session_id: None,
                score: None,
            }])
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(vec![])
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(0)
        }

        async fn health_check(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            "mock"
        }
    }

    #[async_trait]
    impl Memory for MockMemoryWithEntries {
        async fn store(
            &self,
            _key: &str,
            _content: &str,
            _category: MemoryCategory,
            _session_id: Option<&str>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn recall(
            &self,
            _query: &str,
            limit: usize,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(self.entries.iter().take(limit).cloned().collect())
        }

        async fn get(&self, _key: &str) -> anyhow::Result<Option<MemoryEntry>> {
            Ok(None)
        }

        async fn list(
            &self,
            _category: Option<&MemoryCategory>,
            _session_id: Option<&str>,
        ) -> anyhow::Result<Vec<MemoryEntry>> {
            Ok(self.entries.as_ref().clone())
        }

        async fn forget(&self, _key: &str) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn count(&self) -> anyhow::Result<usize> {
            Ok(self.entries.len())
        }

        async fn health_check(&self) -> bool {
            true
        }

        fn name(&self) -> &str {
            "mock-entries"
        }
    }

    #[tokio::test]
    async fn load_context_returns_empty_when_no_entries() {
        let loader = DefaultMemoryLoader::new(5, 0.0);
        let ctx = loader
            .load_context(&MockMemory, "hi", Some("sess"))
            .await
            .unwrap();
        // Mock returns Conversation with session_id=None → excluded for active session.
        assert!(ctx.is_empty());
    }

    #[tokio::test]
    async fn load_context_includes_same_session_and_core() {
        let entries = Arc::new(vec![
            MemoryEntry {
                id: "1".into(),
                key: "core_pref".into(),
                content: "likes tea".into(),
                category: MemoryCategory::Core,
                timestamp: "now".into(),
                session_id: None,
                score: Some(1.0),
            },
            MemoryEntry {
                id: "2".into(),
                key: "user_msg_old".into(),
                content: "shell echo hello".into(),
                category: MemoryCategory::Conversation,
                timestamp: "now".into(),
                session_id: Some("other".into()),
                score: Some(1.0),
            },
            MemoryEntry {
                id: "3".into(),
                key: "user_msg_now".into(),
                content: "current turn note".into(),
                category: MemoryCategory::Conversation,
                timestamp: "now".into(),
                session_id: Some("sess-a".into()),
                score: Some(1.0),
            },
        ]);
        let mem = MockMemoryWithEntries { entries };
        let loader = DefaultMemoryLoader::new(5, 0.0);
        let ctx = loader
            .load_context(&mem, "hello", Some("sess-a"))
            .await
            .unwrap();
        assert!(ctx.contains("likes tea"));
        assert!(ctx.contains("current turn note"));
        assert!(!ctx.contains("shell echo hello"));
    }
}
