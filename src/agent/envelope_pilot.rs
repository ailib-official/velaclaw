//! CR-L1-002 pilot: map CLI history → MessageChunk and call `assemble_layered`.
//! CLI 试点：将会话历史映射为分层 Envelope 并调用 assemble_layered。

use crate::providers::ChatMessage;
use ai_lib_rust::context::{
    AssembleError, AssembleStrategy, ContextBudget, ContextLayer, LayeredAssembleOptions,
    MessageAssembler, MessageChunk, ModelCapacity,
};
use ai_lib_rust::types::message::Message;
use anyhow::{bail, Context, Result};

/// Apply layered assembly to a CLI conversation history.
///
/// Minimal layer mapping (pilot):
/// - `system` → System (critical)
/// - newest `user` → Active (critical)
/// - `tool` → Relevant
/// - other messages → Background
///
/// On [`AssembleError::HardBudgetViolation`], returns an explicit error (no silent strip).
pub fn assemble_history_layered(
    history: &[ChatMessage],
    compact_context: bool,
) -> Result<Vec<ChatMessage>> {
    if history.is_empty() {
        return Ok(Vec::new());
    }

    let chunks = chat_history_to_chunks(history);
    let budget = if compact_context {
        ContextBudget::new(8_192, 0, 1)
    } else {
        ContextBudget::from_capacity(ModelCapacity::UNKNOWN, 2)
    };
    let options = LayeredAssembleOptions {
        budget,
        strategy: AssembleStrategy::Chat,
        ..Default::default()
    };

    let report =
        MessageAssembler::assemble_layered(&chunks, &options).map_err(map_assemble_error)?;

    tracing::debug!(
        dropped_chunks = report.dropped_prefix,
        folded_tool_segments = report.folded_tool_segments,
        kept = report.messages.len(),
        "envelope pilot assemble_layered"
    );

    Ok(report.messages.into_iter().map(message_to_chat).collect())
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

fn chat_history_to_chunks(history: &[ChatMessage]) -> Vec<MessageChunk> {
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
            let message = chat_to_message(msg);
            MessageChunk::new(layer, idx as u64, message, format!("hist-{idx}"))
        })
        .collect()
}

fn chat_to_message(msg: &ChatMessage) -> Message {
    match msg.role.as_str() {
        "system" => Message::system(&msg.content),
        "assistant" => Message::assistant(&msg.content),
        "tool" => {
            let id = msg
                .tool_call_id
                .clone()
                .unwrap_or_else(|| "tool".to_string());
            Message::tool(id, &msg.content)
        }
        _ => Message::user(&msg.content),
    }
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

/// Fail-fast helper used by the CLI agent path when the pilot flag is on.
pub fn apply_envelope_pilot(
    history: &mut Vec<ChatMessage>,
    enabled: bool,
    compact_context: bool,
) -> Result<()> {
    if !enabled || history.is_empty() {
        return Ok(());
    }
    let assembled = assemble_history_layered(history, compact_context)
        .with_context(|| "CR-L1 envelope pilot assemble_layered")?;
    if assembled.is_empty() {
        bail!("envelope pilot produced empty history");
    }
    *history = assembled;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_under_budget_keeps_system_and_active() {
        let history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("old"),
            ChatMessage::assistant("reply"),
            ChatMessage::user("ask"),
        ];
        let out = assemble_history_layered(&history, false).unwrap();
        assert!(out.iter().any(|m| m.role == "system" && m.content == "sys"));
        assert!(out.iter().any(|m| m.role == "user" && m.content == "ask"));
    }

    #[test]
    fn hard_budget_violation_is_explicit() {
        let history = vec![
            ChatMessage::system("S".repeat(400)),
            ChatMessage::user("A".repeat(400)),
        ];
        // Force tiny budget via compact path still may be large; call assembler with tiny opts inline
        let chunks = chat_history_to_chunks(&history);
        let options = LayeredAssembleOptions {
            budget: ContextBudget::new(5, 0, 1),
            strategy: AssembleStrategy::Chat,
            ..Default::default()
        };
        let err = MessageAssembler::assemble_layered(&chunks, &options).unwrap_err();
        match err {
            AssembleError::HardBudgetViolation { .. } => {}
            AssembleError::EmptyInput => panic!("expected HardBudgetViolation, got EmptyInput"),
        }
        let mapped = map_assemble_error(AssembleError::HardBudgetViolation {
            critical_tokens: 100,
            budget: 5,
        });
        assert!(mapped.to_string().contains("HardBudgetViolation"));
    }
}
