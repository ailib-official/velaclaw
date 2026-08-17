//! Context Contract + host retrieve (VL-MA-001).
//! 上下文合同与宿主取数：只声明层/来源/预算意图，结果喂给既有 `assemble_layered`。
//!
//! Cognition (A) only. No writeback, no afterTurn lifecycle, no DAG stepping,
//! no PromptTier (P0–P3) fields — those stay on `prompt_composer`.

use std::fs;
use std::path::{Path, PathBuf};

use ai_lib_rust::context::{
    AssembleStrategy, ContextBudget, ContextLayer, LayeredAssembleOptions, MessageAssembler,
    MessageChunk,
};
use ai_lib_rust::types::message::Message;
use anyhow::Result;

use crate::memory::{Memory, MemoryEntry};

/// Allowed declaration keys (negative test: no writeback / lifecycle / prompt).
pub const CONTRACT_FIELDS: &[&str] = &["layers", "retrieve_kind", "budget"];

/// Workspace files already injected as prompt P0–P3 — not Layer 0–5 retrieve.
const PROMPT_SEED_NAMES: &[&str] = &["AGENTS.md", "SOUL.md", "TOOLS.md", "IDENTITY.md", "USER.md"];

/// Optional workspace notes retrieved as Relevant (not prompt seeds).
const WORKSPACE_RETRIEVE_NAMES: &[&str] = &["NOTES.md", "MEMORY.md", "CONTEXT.md", "HEARTBEAT.md"];

const WORKSPACE_FILE_CHAR_CAP: usize = 4_096;

/// Where retrieved chunks come from (may be issued more than once per turn).
/// `Session` is declared for the M0 contract shape; M1 has no session retriever yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrieveKind {
    Workspace,
    Memory,
    /// Placeholder (M0 inventory). Not retrieved in M1; do not treat as wired.
    Session,
}

/// Budget intent mapped onto ai-lib [`ContextBudget`] (host; not a second assembler).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetIntent {
    Small,
    Large,
    ExplicitTokens(u32),
}

/// I — cognition declaration. Keep this struct small on purpose (R3).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ContextContract {
    pub layers: Vec<u8>,
    pub retrieve_kind: RetrieveKind,
    pub budget: BudgetIntent,
}

impl ContextContract {
    pub fn declaration_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }
}

impl BudgetIntent {
    pub fn to_context_budget(self) -> ContextBudget {
        match self {
            Self::Small => ContextBudget::new(400, 0, 1),
            Self::Large => ContextBudget::new(8_192, 0, 1),
            Self::ExplicitTokens(n) => ContextBudget::new(n, 0, 1),
        }
    }
}

/// Marker prefix in chunk_id so tests can assert retrieve_kind after merge.
pub fn chunk_id_for(kind: RetrieveKind, suffix: &str) -> String {
    match kind {
        RetrieveKind::Workspace => format!("ws-{suffix}"),
        RetrieveKind::Memory => format!("mem-{suffix}"),
        RetrieveKind::Session => format!("sess-{suffix}"),
    }
}

pub fn layered_options_for_budget(budget: ContextBudget) -> LayeredAssembleOptions {
    LayeredAssembleOptions {
        budget,
        strategy: AssembleStrategy::Chat,
        ..Default::default()
    }
}

/// Web chat sessions live **inside** the workspace tree (inventory sessions.rs:53-55).
pub fn chat_sessions_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join(".velaclaw").join("chat_sessions")
}

/// Config / credentials home is **not** the workspace (typical `VELACLAW_CONFIG_DIR`).
pub fn path_layout_contract_holds(workspace_dir: &Path, config_dir: &Path) -> bool {
    let sessions = chat_sessions_dir(workspace_dir);
    sessions.starts_with(workspace_dir)
        && !sessions.starts_with(config_dir)
        && workspace_dir != config_dir
}

/// Read allowlisted workspace notes as Relevant chunks (`retrieve_kind=workspace`).
pub fn retrieve_workspace_files(workspace_dir: &Path) -> Result<Vec<MessageChunk>> {
    let mut chunks = Vec::new();
    if !workspace_dir.is_dir() {
        return Ok(chunks);
    }
    for (i, name) in WORKSPACE_RETRIEVE_NAMES.iter().enumerate() {
        if PROMPT_SEED_NAMES.contains(name) {
            continue;
        }
        let path = workspace_dir.join(name);
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let content = if trimmed.chars().count() > WORKSPACE_FILE_CHAR_CAP {
            trimmed.chars().take(WORKSPACE_FILE_CHAR_CAP).collect()
        } else {
            trimmed.to_string()
        };
        let body = format!("[retrieve:workspace {name}]\n{content}");
        chunks.push(MessageChunk::new(
            ContextLayer::Relevant,
            i as u64,
            Message::user(body),
            chunk_id_for(RetrieveKind::Workspace, name),
        ));
    }
    Ok(chunks)
}

/// Memory-shaped chunks from existing [`MemoryEntry`] rows (no embeddings required).
pub fn chunks_from_memory_entries(entries: &[MemoryEntry]) -> Vec<MessageChunk> {
    entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let body = format!("[retrieve:memory {}]\n{}", entry.key, entry.content.trim());
            MessageChunk::new(
                ContextLayer::Summary,
                i as u64,
                Message::user(body),
                chunk_id_for(RetrieveKind::Memory, &entry.key),
            )
            .with_summary(true)
        })
        .collect()
}

/// Fixture helper for tests / M1 memory kind without a live sqlite embedder.
pub fn memory_fixture_chunks(pairs: &[(&str, &str)]) -> Vec<MessageChunk> {
    let entries: Vec<MemoryEntry> = pairs
        .iter()
        .map(|(key, content)| MemoryEntry {
            id: format!("fix-{key}"),
            key: (*key).to_string(),
            content: (*content).to_string(),
            category: crate::memory::MemoryCategory::Core,
            timestamp: "2026-08-17T00:00:00Z".into(),
            session_id: None,
            score: None,
        })
        .collect();
    chunks_from_memory_entries(&entries)
}

/// Sync-shaped recall via the existing Memory trait (production backend for kind=memory).
///
/// Recalls without SQL session filter so Core (`session_id=None`) stays visible,
/// then applies [`crate::memory::should_inject_for_session`].
pub async fn retrieve_memory_chunks(
    mem: &dyn Memory,
    query: &str,
    limit: usize,
    session_id: Option<&str>,
) -> Result<Vec<MessageChunk>> {
    let entries = mem.recall(query, limit, None).await?;
    let filtered: Vec<_> = entries
        .into_iter()
        .filter(|entry| crate::memory::should_inject_for_session(entry, session_id))
        .filter(|entry| !crate::memory::is_assistant_autosave_key(&entry.key))
        .collect();
    Ok(chunks_from_memory_entries(&filtered))
}

/// Workspace notes + memory recall for one `assemble_layered` extra set (GOV-007).
pub async fn retrieve_turn_extra_chunks(
    workspace_dir: &Path,
    mem: &dyn Memory,
    query: &str,
    session_id: Option<&str>,
) -> Vec<MessageChunk> {
    let mut extra = retrieve_workspace_files(workspace_dir).unwrap_or_default();
    match retrieve_memory_chunks(mem, query, 5, session_id).await {
        Ok(memory_chunks) => extra.extend(memory_chunks),
        Err(error) => {
            tracing::debug!(error = %error, "memory retrieve skipped for extra_chunks");
        }
    }
    extra
}

/// Host fill + **the** assemble entry (sync). Callers may retry after HardBudget with a
/// smaller extra set; still this function.
pub fn assemble_contract_chunks(
    history_chunks: &[MessageChunk],
    extra: &[MessageChunk],
    budget: ContextBudget,
) -> Result<ai_lib_rust::context::AssembleReport, ai_lib_rust::context::AssembleError> {
    let mut chunks = extra.to_vec();
    chunks.extend(history_chunks.iter().cloned());
    MessageAssembler::assemble_layered(&chunks, &layered_options_for_budget(budget))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ChatMessage;
    use ai_lib_rust::context::AssembleError;

    fn history_chunks() -> Vec<MessageChunk> {
        crate::agent::envelope_pilot::chat_history_to_chunks(&[
            ChatMessage::system("sys"),
            ChatMessage::user("ask"),
        ])
    }

    #[test]
    fn contract_declaration_only_allows_i_fields() {
        let c = ContextContract {
            layers: vec![ContextLayer::System.as_u8(), ContextLayer::Relevant.as_u8()],
            retrieve_kind: RetrieveKind::Workspace,
            budget: BudgetIntent::Small,
        };
        let v = c.declaration_json();
        let obj = v.as_object().expect("object");
        for key in obj.keys() {
            assert!(
                CONTRACT_FIELDS.contains(&key.as_str()),
                "unexpected contract field {key}"
            );
        }
        for required in CONTRACT_FIELDS {
            assert!(obj.contains_key(*required), "missing {required}");
        }
        let dumped = v.to_string();
        assert!(!dumped.contains("writeback"));
        assert!(!dumped.contains("afterTurn"));
        assert!(!dumped.contains("after_turn"));
        assert!(!dumped.contains("PromptTier"));
        assert!(!dumped.contains("dag"));
    }

    #[test]
    fn budget_small_vs_large_drops_soft_layers() {
        let mut extra = memory_fixture_chunks(&[("core-fact", "remember the gate")]);
        extra.extend(retrieve_workspace_files(Path::new("/nonexistent")).unwrap());
        extra.push(MessageChunk::new(
            ContextLayer::Background,
            99,
            Message::user("B".repeat(4000)),
            chunk_id_for(RetrieveKind::Workspace, "bg"),
        ));
        let history = history_chunks();
        let small =
            assemble_contract_chunks(&history, &extra, BudgetIntent::Small.to_context_budget())
                .expect("small budget should fit critical");
        let large =
            assemble_contract_chunks(&history, &extra, BudgetIntent::Large.to_context_budget())
                .expect("large budget");
        assert!(
            large.dropped_prefix < small.dropped_prefix
                || large.messages.len() > small.messages.len(),
            "small dropped={} kept={} large dropped={} kept={}",
            small.dropped_prefix,
            small.messages.len(),
            large.dropped_prefix,
            large.messages.len()
        );
    }

    #[test]
    fn hard_budget_violation_is_explicit() {
        let history = vec![MessageChunk::new(
            ContextLayer::System,
            0,
            Message::system("S".repeat(400)),
            "hist-sys",
        )];
        let err = assemble_contract_chunks(&history, &[], ContextBudget::new(5, 0, 1)).unwrap_err();
        assert!(matches!(err, AssembleError::HardBudgetViolation { .. }));
    }

    #[test]
    fn dual_retrieve_kind_same_assemble_entry() {
        let ws = vec![MessageChunk::new(
            ContextLayer::Relevant,
            1,
            Message::user("[retrieve:workspace NOTES.md]\nproject note"),
            chunk_id_for(RetrieveKind::Workspace, "NOTES.md"),
        )];
        let mem = memory_fixture_chunks(&[("pref", "likes rust")]);
        let mut extra = ws;
        extra.extend(mem);
        let history = history_chunks();
        let report =
            assemble_contract_chunks(&history, &extra, BudgetIntent::Large.to_context_budget())
                .expect("assemble");
        let joined: String = report
            .messages
            .iter()
            .map(|m| match &m.content {
                ai_lib_rust::types::message::MessageContent::Text(t) => t.clone(),
                ai_lib_rust::types::message::MessageContent::Blocks(_) => String::new(),
            })
            .collect();
        assert!(joined.contains("[retrieve:workspace"));
        assert!(joined.contains("[retrieve:memory"));
    }

    #[test]
    fn sessions_dir_is_under_workspace_not_config() {
        let workspace = PathBuf::from("/tmp/velaclaw-ws");
        let config = PathBuf::from("/tmp/velaclaw-config");
        assert!(path_layout_contract_holds(&workspace, &config));
        assert_eq!(
            chat_sessions_dir(&workspace),
            workspace.join(".velaclaw").join("chat_sessions")
        );
    }

    #[test]
    fn retrieve_workspace_skips_prompt_seeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("AGENTS.md"), "do not retrieve as layer").unwrap();
        fs::write(dir.path().join("NOTES.md"), "task note").unwrap();
        let chunks = retrieve_workspace_files(dir.path()).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].chunk_id.starts_with("ws-"));
        assert!(!chunks.iter().any(|c| c.chunk_id.contains("AGENTS")));
    }

    #[tokio::test]
    async fn recall_becomes_memory_chunks_on_same_assemble_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = crate::config::MemoryConfig {
            backend: "sqlite".into(),
            ..crate::config::MemoryConfig::default()
        };
        let mem = crate::memory::create_memory(&cfg, dir.path(), None).expect("sqlite");
        mem.store(
            "gate_decision",
            "keep landlock optional",
            crate::memory::MemoryCategory::Core,
            None,
        )
        .await
        .unwrap();
        mem.store(
            "other_chat",
            "unrelated chatter",
            crate::memory::MemoryCategory::Conversation,
            Some("sess-other"),
        )
        .await
        .unwrap();
        let extra =
            retrieve_turn_extra_chunks(dir.path(), mem.as_ref(), "landlock", Some("sess-now"))
                .await;
        assert!(
            extra.iter().any(|c| c.chunk_id.contains("gate_decision")),
            "core recall missing: {:?}",
            extra.iter().map(|c| &c.chunk_id).collect::<Vec<_>>()
        );
        assert!(!extra.iter().any(|c| c.chunk_id.contains("other_chat")));
        let history = history_chunks();
        let report =
            assemble_contract_chunks(&history, &extra, BudgetIntent::Large.to_context_budget())
                .expect("assemble");
        let joined: String = report
            .messages
            .iter()
            .map(|m| match &m.content {
                ai_lib_rust::types::message::MessageContent::Text(t) => t.clone(),
                ai_lib_rust::types::message::MessageContent::Blocks(_) => String::new(),
            })
            .collect();
        assert!(joined.contains("[retrieve:memory"));
        assert!(joined.contains("keep landlock optional"));
    }
}
