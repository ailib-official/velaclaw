//! CR-L2 template DAG runtime shell: load handwritten DAG → walk static `next`
//! → per-node Envelope assemble → fail closed.
//!
//! Opt-in via `[agent].template_dag` (default false). Does not run LLM calls;
//! model capability tags are recorded for host routing integration later.

use crate::providers::ChatMessage;
use ai_lib_rust::context::{
    AssembleError, AssembleStrategy, ContextBudget, ContextLayer, LayeredAssembleOptions,
    MessageAssembler, MessageChunk, ModelCapacity,
};
use ai_lib_rust::types::message::Message;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

/// Embedded golden fixture (public-safe copy of handwritten code-fix template).
pub const CODE_FIX_TEMPLATE_JSON: &str = include_str!("fixtures/code-fix-template.json");

#[derive(Debug, Clone, Deserialize)]
pub struct DagManifest {
    pub schema_version: String,
    pub id: String,
    #[serde(default)]
    pub description: Option<String>,
    pub entry: String,
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    pub nodes: Vec<DagNode>,
}

fn default_max_steps() -> u32 {
    8
}

#[derive(Debug, Clone, Deserialize)]
pub struct DagNode {
    pub id: String,
    pub task_type: String,
    pub model_selector: ModelSelector,
    #[serde(default)]
    pub context_requirements: ContextRequirements,
    #[serde(default)]
    pub max_steps: Option<u32>,
    /// Static edge; JSON `null` → terminal.
    #[serde(default)]
    pub next: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSelector {
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContextRequirements {
    #[serde(default)]
    pub layers: Vec<u8>,
    #[serde(default)]
    pub retrieve: Vec<RetrieveIntent>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetrieveIntent {
    pub kind: String,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DagAbortReason {
    MaxSteps,
    Timeout,
    MissingNode { id: String },
    InvalidNext { from: String, to: String },
    HardBudget,
    Assemble(String),
    EmptyCapabilities { node: String },
}

impl std::fmt::Display for DagAbortReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaxSteps => write!(f, "max_steps"),
            Self::Timeout => write!(f, "timeout"),
            Self::MissingNode { id } => write!(f, "missing_node:{id}"),
            Self::InvalidNext { from, to } => write!(f, "invalid_next:{from}->{to}"),
            Self::HardBudget => write!(f, "hard_budget"),
            Self::Assemble(msg) => write!(f, "assemble:{msg}"),
            Self::EmptyCapabilities { node } => write!(f, "empty_capabilities:{node}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DagNodeVisit {
    pub node_id: String,
    pub task_type: String,
    pub capabilities: Vec<String>,
    pub assembled_messages: usize,
}

#[derive(Debug, Clone)]
pub struct DagRunReport {
    pub dag_id: String,
    pub success: bool,
    pub steps: u32,
    pub abort_reason: Option<DagAbortReason>,
    pub visits: Vec<DagNodeVisit>,
}

/// Parse a handwritten template DAG from JSON text.
pub fn parse_dag_json(json: &str) -> Result<DagManifest> {
    let dag: DagManifest = serde_json::from_str(json).context("parse template DAG JSON")?;
    validate_graph(&dag)?;
    Ok(dag)
}

/// Load a handwritten template DAG from a filesystem path.
pub fn load_dag_path(path: &Path) -> Result<DagManifest> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read template DAG {}", path.display()))?;
    parse_dag_json(&text)
}

fn validate_graph(dag: &DagManifest) -> Result<()> {
    if dag.schema_version != "0.1.0" {
        bail!(
            "unsupported template DAG schema_version '{}' (expected 0.1.0)",
            dag.schema_version
        );
    }
    if dag.nodes.is_empty() {
        bail!("template DAG has no nodes");
    }
    let mut ids = HashMap::new();
    for node in &dag.nodes {
        if ids.insert(node.id.clone(), node).is_some() {
            bail!("duplicate node id '{}'", node.id);
        }
        if node.model_selector.capabilities.is_empty() {
            bail!("node '{}' has empty model_selector.capabilities", node.id);
        }
    }
    if !ids.contains_key(&dag.entry) {
        bail!("entry '{}' not found in nodes", dag.entry);
    }
    for node in &dag.nodes {
        if let Some(next) = &node.next {
            if !ids.contains_key(next) {
                bail!("node '{}' next '{}' not found in nodes", node.id, next);
            }
        }
    }
    Ok(())
}

/// Run the structural template shell: walk `next`, assemble Envelope per node.
///
/// Fail-closed on max_steps, timeout, missing edges, HardBudgetViolation, or
/// empty capabilities. Does not invoke an LLM.
pub fn run_template_dag(
    dag: &DagManifest,
    seed_user_message: &str,
    compact_context: bool,
) -> Result<DagRunReport> {
    let started = Instant::now();
    let nodes: HashMap<&str, &DagNode> = dag.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut visits = Vec::new();
    let mut steps = 0u32;
    let mut current = dag.entry.as_str();
    let mut abort: Option<DagAbortReason> = None;

    loop {
        if let Some(limit) = dag.timeout_secs {
            if started.elapsed().as_secs() >= limit {
                abort = Some(DagAbortReason::Timeout);
                break;
            }
        }
        if steps >= dag.max_steps {
            abort = Some(DagAbortReason::MaxSteps);
            break;
        }

        let Some(node) = nodes.get(current) else {
            abort = Some(DagAbortReason::MissingNode {
                id: current.to_string(),
            });
            break;
        };

        if node.model_selector.capabilities.is_empty() {
            abort = Some(DagAbortReason::EmptyCapabilities {
                node: node.id.clone(),
            });
            break;
        }

        steps += 1;

        let history = seed_history_for_node(node, seed_user_message);
        match assemble_for_node(&history, node, compact_context) {
            Ok(assembled) => {
                visits.push(DagNodeVisit {
                    node_id: node.id.clone(),
                    task_type: node.task_type.clone(),
                    capabilities: node.model_selector.capabilities.clone(),
                    assembled_messages: assembled.len(),
                });
            }
            Err(err) => {
                abort = Some(classify_assemble_abort(&err));
                break;
            }
        }

        match &node.next {
            None => break,
            Some(next_id) => {
                if !nodes.contains_key(next_id.as_str()) {
                    abort = Some(DagAbortReason::InvalidNext {
                        from: node.id.clone(),
                        to: next_id.clone(),
                    });
                    break;
                }
                current = next_id.as_str();
            }
        }
    }

    let success = abort.is_none();
    let report = DagRunReport {
        dag_id: dag.id.clone(),
        success,
        steps,
        abort_reason: abort.clone(),
        visits,
    };
    emit_m2(&report);
    if let Some(reason) = abort {
        bail!(
            "template DAG '{}' aborted after {steps} step(s): {reason}",
            dag.id
        );
    }
    Ok(report)
}

fn classify_assemble_abort(err: &anyhow::Error) -> DagAbortReason {
    let msg = err.to_string();
    if msg.contains("HardBudgetViolation") {
        DagAbortReason::HardBudget
    } else {
        DagAbortReason::Assemble(msg)
    }
}

fn emit_m2(report: &DagRunReport) {
    // M2a success, M2b abort/fallback reason, M2c steps — structured logs only.
    tracing::info!(
        dag_id = %report.dag_id,
        m2_success = report.success,
        m2_steps = report.steps,
        m2_abort = report
            .abort_reason
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        "template_dag_run"
    );
}

fn seed_history_for_node(node: &DagNode, seed_user_message: &str) -> Vec<ChatMessage> {
    let mut history = vec![ChatMessage::system(format!(
        "template DAG node '{}' task_type={}",
        node.id, node.task_type
    ))];

    for retrieve in &node.context_requirements.retrieve {
        let detail = match (retrieve.query.as_deref(), retrieve.alias.as_deref()) {
            (Some(q), _) => q.to_string(),
            (_, Some(a)) => format!("alias:{a}"),
            _ => retrieve.kind.clone(),
        };
        history.push(ChatMessage::tool_with_call_id(
            format!("retrieve-{}", retrieve.kind),
            format!("[{}] {detail}", retrieve.kind),
        ));
    }

    // Ensure critical Active layer content exists for assemble_layered.
    history.push(ChatMessage::user(format!(
        "{seed_user_message}\n(node={} capabilities={:?})",
        node.id, node.model_selector.capabilities
    )));

    // Optional Background filler when layers request layer 4+.
    if node
        .context_requirements
        .layers
        .iter()
        .any(|&layer| layer >= 4)
    {
        history.insert(
            1,
            ChatMessage::assistant("prior background context (template shell)"),
        );
    }

    history
}

fn assemble_for_node(
    history: &[ChatMessage],
    node: &DagNode,
    compact_context: bool,
) -> Result<Vec<ChatMessage>> {
    let chunks = history_to_chunks(history);
    let strategy = if node.task_type == "code-fix" {
        AssembleStrategy::CodeFix
    } else {
        AssembleStrategy::Chat
    };
    let budget = if compact_context {
        ContextBudget::new(8_192, 0, 1)
    } else {
        ContextBudget::from_capacity(ModelCapacity::UNKNOWN, 2)
    };
    let options = LayeredAssembleOptions {
        budget,
        strategy,
        ..Default::default()
    };
    let report =
        MessageAssembler::assemble_layered(&chunks, &options).map_err(map_assemble_error)?;
    Ok(report.messages.into_iter().map(message_to_chat).collect())
}

fn message_to_chat(msg: Message) -> ChatMessage {
    use ai_lib_rust::types::message::{ContentBlock, MessageContent, MessageRole};
    let content = match &msg.content {
        MessageContent::Text(t) => t.clone(),
        MessageContent::Blocks(blocks) => blocks
            .iter()
            .filter_map(|b| {
                if let ContentBlock::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    match msg.role {
        MessageRole::System => ChatMessage::system(content),
        MessageRole::Assistant => ChatMessage::assistant(content),
        MessageRole::Tool => {
            if let Some(id) = msg.tool_call_id {
                ChatMessage::tool_with_call_id(id, content)
            } else {
                ChatMessage::tool(content)
            }
        }
        MessageRole::User => ChatMessage::user(content),
    }
}

fn map_assemble_error(err: AssembleError) -> anyhow::Error {
    match err {
        AssembleError::HardBudgetViolation {
            critical_tokens,
            budget,
        } => anyhow::anyhow!(
            "envelope HardBudgetViolation: critical layers need {critical_tokens} tokens but budget is {budget} (refusing to strip System/Active)"
        ),
        AssembleError::EmptyInput => anyhow::anyhow!("envelope assemble: empty input"),
    }
}

fn history_to_chunks(history: &[ChatMessage]) -> Vec<MessageChunk> {
    let last_user_idx = history
        .iter()
        .rposition(|m| m.role == "user")
        .unwrap_or(usize::MAX);
    history
        .iter()
        .enumerate()
        .map(|(idx, msg)| {
            let layer = match msg.role.as_str() {
                "system" => ContextLayer::System,
                "user" if idx == last_user_idx => ContextLayer::Active,
                "tool" => ContextLayer::Relevant,
                _ => ContextLayer::Background,
            };
            let message = match msg.role.as_str() {
                "system" => Message::system(&msg.content),
                "assistant" => Message::assistant(&msg.content),
                "tool" => Message::tool(
                    msg.tool_call_id.clone().unwrap_or_else(|| "tool".into()),
                    &msg.content,
                ),
                _ => Message::user(&msg.content),
            };
            MessageChunk::new(layer, idx as u64, message, format!("dag-{idx}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_fix_fixture_happy_path() {
        let dag = parse_dag_json(CODE_FIX_TEMPLATE_JSON).unwrap();
        assert_eq!(dag.id, "code-fix-template");
        let report = run_template_dag(&dag, "fix the null check", false).unwrap();
        assert!(report.success);
        assert_eq!(report.steps, 3);
        assert_eq!(report.visits.len(), 3);
        assert_eq!(report.visits[0].node_id, "locate");
        assert_eq!(report.visits[2].node_id, "verify");
        assert!(report.visits[0].assembled_messages >= 1);
    }

    #[test]
    fn max_steps_fail_closed() {
        let mut dag = parse_dag_json(CODE_FIX_TEMPLATE_JSON).unwrap();
        dag.max_steps = 1;
        let err = run_template_dag(&dag, "x", false).unwrap_err().to_string();
        assert!(err.contains("max_steps"), "{err}");
    }

    #[test]
    fn invalid_next_rejected_at_parse() {
        let json = r#"{
          "schema_version":"0.1.0",
          "id":"broken",
          "entry":"a",
          "max_steps":2,
          "nodes":[{"id":"a","task_type":"chat","model_selector":{"capabilities":["speed"]},"next":"missing"}]
        }"#;
        let err = parse_dag_json(json).unwrap_err().to_string();
        assert!(err.contains("next"), "{err}");
    }

    #[test]
    fn hard_budget_fail_closed() {
        // Force HardBudget by assembling critical-only history under tiny budget.
        let history = vec![
            ChatMessage::system("S".repeat(200)),
            ChatMessage::user("U".repeat(200)),
        ];
        let chunks = history_to_chunks(&history);
        let options = LayeredAssembleOptions {
            budget: ContextBudget::new(1, 0, 0),
            strategy: AssembleStrategy::Chat,
            ..Default::default()
        };
        let err = MessageAssembler::assemble_layered(&chunks, &options).unwrap_err();
        assert!(matches!(err, AssembleError::HardBudgetViolation { .. }));
        let mapped = map_assemble_error(err);
        assert!(matches!(
            classify_assemble_abort(&mapped),
            DagAbortReason::HardBudget
        ));
    }
}
