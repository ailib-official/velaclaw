//! Per-node Context DB writeback + capability Contact (VL-NA-013/014).
//!
//! Live only when `[agent].bounded_dag_live` is on. Does not add a
//! second tool loop: callers still invoke [`super::loop_::run_tool_call_loop`].
//!
//! 节点产物写入既有 Memory；下一步只注入合同允许的块。Contact 按节点 capabilities 选 hint。

use super::bounded_dag::node_system_note;
use super::context_contract::{retrieve_memory_chunks, retrieve_workspace_files};
use super::dag_runner::DagNode;
use super::intent_route::{hint_to_tag, hints_for_tag};
use crate::memory::{Memory, MemoryCategory};
use crate::orchestration::{TurnModelDecision, TurnModelSource};
use crate::providers::ChatMessage;
use anyhow::Result;
use std::path::Path;

/// Clip node output stored as a Daily memory row (layer-3 / tool_result retrieve).
pub const ARTIFACT_MAX_CHARS: usize = 4_096;

/// Prefer long-context / reasoning before cheap speed so mixed tags stay on the right family.
const CONTACT_TAG_PREFERENCE: &[&str] = &[
    "document_understanding",
    "high-reasoning",
    "coding",
    "tool_calling",
    "speed",
];

/// Observable Contact choice for one DAG node (not a second router).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeContact {
    pub model: String,
    pub reason: String,
    pub capabilities: Vec<String>,
}

impl NodeContact {
    #[must_use]
    pub fn observe_line(&self) -> String {
        format!(
            "contact model={} reason={} caps={}",
            self.model,
            self.reason,
            self.capabilities.join(",")
        )
    }

    #[must_use]
    pub fn to_turn_decision(&self) -> TurnModelDecision {
        TurnModelDecision {
            model: self.model.clone(),
            source: TurnModelSource::NodeCapability,
            reason: self.reason.clone(),
        }
    }
}

pub fn artifact_memory_key(session_id: &str, node_id: &str) -> String {
    format!("dag_art:{session_id}:{node_id}")
}

fn clip_artifact(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= ARTIFACT_MAX_CHARS {
        return trimmed.to_string();
    }
    trimmed.chars().take(ARTIFACT_MAX_CHARS).collect()
}

pub async fn store_node_artifact(
    mem: &dyn Memory,
    session_id: &str,
    node_id: &str,
    text: &str,
) -> Result<()> {
    let key = artifact_memory_key(session_id, node_id);
    mem.store(
        &key,
        &clip_artifact(text),
        MemoryCategory::Daily,
        Some(session_id),
    )
    .await
}

pub async fn load_node_artifact(
    mem: &dyn Memory,
    session_id: &str,
    node_id: &str,
) -> Result<Option<String>> {
    let key = artifact_memory_key(session_id, node_id);
    Ok(mem.get(&key).await?.map(|e| e.content))
}

fn chunk_plain_text(chunk: &ai_lib_rust::context::MessageChunk) -> String {
    match &chunk.message.content {
        ai_lib_rust::types::message::MessageContent::Text(s) => s.clone(),
        ai_lib_rust::types::message::MessageContent::Blocks(_) => {
            format!("[{}]", chunk.chunk_id)
        }
    }
}

/// User-role retrieve blobs for this node (workspace / memory / prior artifacts).
pub async fn node_retrieve_texts(
    mem: &dyn Memory,
    workspace_dir: &Path,
    session_id: &str,
    node: &DagNode,
    prior_ids: &[String],
) -> Vec<String> {
    let mut out = Vec::new();
    let mut injected_prior = false;

    for retrieve in &node.context_requirements.retrieve {
        match retrieve.kind.as_str() {
            "workspace" => {
                if let Ok(chunks) = retrieve_workspace_files(workspace_dir) {
                    for chunk in chunks {
                        out.push(chunk_plain_text(&chunk));
                    }
                }
            }
            "memory" => {
                let q = retrieve.query.as_deref().unwrap_or("");
                if let Ok(chunks) = retrieve_memory_chunks(mem, q, 3, Some(session_id)).await {
                    for chunk in chunks {
                        out.push(chunk_plain_text(&chunk));
                    }
                }
            }
            "tool_result" => {
                if let Some(prev) = prior_ids.last() {
                    if let Ok(Some(body)) = load_node_artifact(mem, session_id, prev).await {
                        out.push(format!(
                            "[dag_artifact node={prev} alias={}]\n{body}",
                            retrieve.alias.as_deref().unwrap_or("tool_result")
                        ));
                        injected_prior = true;
                    }
                }
            }
            other => {
                tracing::debug!(kind = other, "bounded DAG retrieve kind skipped");
            }
        }
    }

    let wants_summary = node
        .context_requirements
        .layers
        .iter()
        .any(|&layer| layer >= 3);
    // Live graphs often omit context_requirements. Always pass the previous
    // node's clipped artifact into the next node so generic tasks (ops, docs,
    // planning, review) keep a contract handoff without a second tool loop.
    if !injected_prior {
        if let Some(prev) = prior_ids.last() {
            if let Ok(Some(body)) = load_node_artifact(mem, session_id, prev).await {
                let tag = if wants_summary { "layer=3" } else { "prior" };
                out.push(format!("[dag_artifact node={prev} {tag}]\n{body}"));
            }
        }
    }

    out
}

/// Truncate chat history to the pre-node base, then add node note + retrieve texts.
pub fn reset_chat_scope(
    history: &mut Vec<ChatMessage>,
    base_len: usize,
    node: &DagNode,
    retrieve_texts: &[String],
) {
    history.truncate(base_len);
    history.push(ChatMessage::system(node_system_note(
        &node.id,
        &node.task_type,
    )));
    for text in retrieve_texts {
        history.push(ChatMessage::user(text.clone()));
    }
}

/// Map node `model_selector.capabilities` to a `hint:` id or the session default.
///
/// Live bounded-DAG work nodes should pass `explicit_model = None` so the
/// session picker stays the **planner** default and does not flatten Contact.
/// CLI `--model` / Web picker still set `default_model` for the planner turn.
/// Does not enable `host_decide` / CAP live.
pub fn contact_for_node(
    node: &DagNode,
    default_model: &str,
    available_hints: &[String],
    explicit_model: Option<&str>,
) -> NodeContact {
    let capabilities = node.model_selector.capabilities.clone();
    if let Some(raw) = explicit_model.map(str::trim).filter(|s| !s.is_empty()) {
        return NodeContact {
            model: raw.to_string(),
            reason: "explicit_user_pick".into(),
            capabilities,
        };
    }

    for tag in CONTACT_TAG_PREFERENCE {
        if !capabilities
            .iter()
            .any(|c| hint_to_tag(c).is_some_and(|t| t.eq_ignore_ascii_case(tag)) || c == tag)
        {
            continue;
        }
        let mut candidates = hints_for_tag(tag);
        if !candidates.iter().any(|h| h.eq_ignore_ascii_case(tag)) {
            candidates.push(tag);
        }
        for hint in candidates {
            if available_hints.iter().any(|h| h.eq_ignore_ascii_case(hint)) {
                let canon = available_hints
                    .iter()
                    .find(|h| h.eq_ignore_ascii_case(hint))
                    .cloned()
                    .unwrap_or_else(|| hint.to_string());
                return NodeContact {
                    model: format!("hint:{canon}"),
                    reason: format!("node_capability:{tag}:hint:{canon}"),
                    capabilities,
                };
            }
        }
    }

    NodeContact {
        model: default_model.to_string(),
        reason: "node_capability:unmapped_default".into(),
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dag_runner::{parse_dag_json, CODE_FIX_TEMPLATE_JSON};

    #[test]
    fn verify_prefers_speed_hint() {
        let dag = parse_dag_json(CODE_FIX_TEMPLATE_JSON).unwrap();
        let verify = dag.nodes.iter().find(|n| n.id == "verify").unwrap();
        let c = contact_for_node(
            verify,
            "deepseek/deepseek-v4-flash",
            &["fast".into(), "code".into()],
            None,
        );
        assert_eq!(c.model, "hint:fast");
        assert!(c.reason.contains("speed"), "{}", c.reason);
    }

    #[test]
    fn locate_prefers_coding_hint() {
        let dag = parse_dag_json(CODE_FIX_TEMPLATE_JSON).unwrap();
        let locate = dag.nodes.iter().find(|n| n.id == "locate").unwrap();
        let c = contact_for_node(
            locate,
            "deepseek/deepseek-v4-flash",
            &["fast".into(), "code".into()],
            None,
        );
        assert_eq!(c.model, "hint:code");
        assert!(c.reason.contains("coding"), "{}", c.reason);
    }

    #[test]
    fn explicit_pick_wins() {
        let dag = parse_dag_json(CODE_FIX_TEMPLATE_JSON).unwrap();
        let locate = dag.nodes.iter().find(|n| n.id == "locate").unwrap();
        let c = contact_for_node(
            locate,
            "default/x",
            &["code".into()],
            Some("nvidia/nemotron"),
        );
        assert_eq!(c.model, "nvidia/nemotron");
        assert_eq!(c.reason, "explicit_user_pick");
    }

    #[test]
    fn document_prefers_document_hint() {
        let json = r#"{
          "schema_version": "0.1.0",
          "id": "paper",
          "entry": "read",
          "max_steps": 8,
          "nodes": [
            {"id":"read","task_type":"summarize","model_selector":{"capabilities":["document_understanding"]},"next":null}
          ]
        }"#;
        let dag = parse_dag_json(json).unwrap();
        let read = &dag.nodes[0];
        let c = contact_for_node(
            read,
            "deepseek/deepseek-v4-flash",
            &["document".into(), "fast".into()],
            None,
        );
        assert_eq!(c.model, "hint:document");
        assert!(c.reason.contains("document_understanding"), "{}", c.reason);
    }

    #[test]
    fn artifact_key_is_session_scoped() {
        assert_eq!(artifact_memory_key("s1", "locate"), "dag_art:s1:locate");
    }

    #[tokio::test]
    async fn empty_retrieve_still_injects_prior_artifact() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = crate::config::MemoryConfig {
            backend: "sqlite".into(),
            ..crate::config::MemoryConfig::default()
        };
        let mem = crate::memory::create_memory(&cfg, tmp.path(), None).unwrap();
        let json = r#"{
          "schema_version": "0.1.0",
          "id": "generic",
          "entry": "a",
          "max_steps": 8,
          "nodes": [
            {"id":"a","task_type":"read","model_selector":{"capabilities":["document_understanding"]},"next":"b"},
            {"id":"b","task_type":"write","model_selector":{"capabilities":["high-reasoning"]},"next":null}
          ]
        }"#;
        let dag = parse_dag_json(json).unwrap();
        let b = dag.nodes.iter().find(|n| n.id == "b").unwrap();
        store_node_artifact(mem.as_ref(), "sess", "a", "PRIOR_BODY_UNIQUE")
            .await
            .unwrap();
        let texts = node_retrieve_texts(mem.as_ref(), tmp.path(), "sess", b, &["a".into()]).await;
        assert!(
            texts.iter().any(|t| t.contains("PRIOR_BODY_UNIQUE")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("dag_artifact")),
            "{texts:?}"
        );
    }
}
