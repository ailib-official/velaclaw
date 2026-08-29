//! CR-L1/L2 envelope pilot + CR-L3-003 opt-in async schedule façade.
//! Map conversation history → MessageChunk and call `assemble_layered`
//! (sync) or `AssemblePool` / `assemble_layered_async` (opt-in).
//! 试点：将会话历史映射为分层 Envelope；默认同步装配，可选异步调度 façade。

use std::sync::OnceLock;

use crate::providers::ChatMessage;
use ai_lib_rust::context::{
    AssembleError, AssemblePool, AssemblePoolConfig, AssembleStrategy, ContextBudget, ContextLayer,
    LayeredAssembleOptions, MessageAssembler, MessageChunk, ModelCapacity,
};
use ai_lib_rust::types::message::Message;
use anyhow::{bail, Context, Result};

/// Shared bounded pool for CR-L3-003 async Envelope assemble (host opt-in only).
fn assemble_pool() -> &'static AssemblePool {
    static POOL: OnceLock<AssemblePool> = OnceLock::new();
    POOL.get_or_init(|| AssemblePool::new(AssemblePoolConfig::default()))
}

/// Apply layered assembly to a CLI conversation history (sync algorithm truth).
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
    assemble_history_layered_with_extra(history, &[], compact_context)
}

/// Same as [`assemble_history_layered`] plus host-retrieved chunks (VL-MA-001).
pub fn assemble_history_layered_with_extra(
    history: &[ChatMessage],
    extra: &[MessageChunk],
    compact_context: bool,
) -> Result<Vec<ChatMessage>> {
    assemble_history_layered_with_extra_window(history, extra, compact_context, None)
}

/// Same as [`assemble_history_layered_with_extra`] with protocol `context_window`.
pub fn assemble_history_layered_with_extra_window(
    history: &[ChatMessage],
    extra: &[MessageChunk],
    compact_context: bool,
    context_window: Option<u32>,
) -> Result<Vec<ChatMessage>> {
    let chunks = merge_history_and_extra(history, extra);
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let options = layered_options(compact_context, context_window);
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

/// CR-L3-003: same assemble semantics as [`assemble_history_layered`], scheduled via
/// [`AssemblePool`] (bounded concurrency + per-job timeout; fail-closed).
pub async fn assemble_history_layered_async(
    history: &[ChatMessage],
    compact_context: bool,
) -> Result<Vec<ChatMessage>> {
    assemble_history_layered_async_with_extra(history, &[], compact_context).await
}

pub async fn assemble_history_layered_async_with_extra(
    history: &[ChatMessage],
    extra: &[MessageChunk],
    compact_context: bool,
) -> Result<Vec<ChatMessage>> {
    assemble_history_layered_async_with_extra_window(history, extra, compact_context, None).await
}

pub async fn assemble_history_layered_async_with_extra_window(
    history: &[ChatMessage],
    extra: &[MessageChunk],
    compact_context: bool,
    context_window: Option<u32>,
) -> Result<Vec<ChatMessage>> {
    let chunks = merge_history_and_extra(history, extra);
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let options = layered_options(compact_context, context_window);
    let report = MessageAssembler::assemble_layered_async(chunks, options, assemble_pool())
        .await
        .map_err(map_assemble_error)?;

    tracing::debug!(
        dropped_chunks = report.dropped_prefix,
        folded_tool_segments = report.folded_tool_segments,
        kept = report.messages.len(),
        "envelope pilot assemble_layered_async"
    );

    Ok(report.messages.into_iter().map(message_to_chat).collect())
}

fn layered_options(compact_context: bool, context_window: Option<u32>) -> LayeredAssembleOptions {
    let budget = if compact_context {
        ContextBudget::new(8_192, 0, 1)
    } else if let Some(window) = context_window.filter(|w| *w > 0) {
        ContextBudget::from_capacity(ModelCapacity::new(window, 0), 2)
    } else {
        ContextBudget::from_capacity(ModelCapacity::UNKNOWN, 2)
    };
    LayeredAssembleOptions {
        budget,
        strategy: AssembleStrategy::Chat,
        ..Default::default()
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
        AssembleError::QueueFull { max_in_flight } => anyhow::anyhow!(
            "envelope assemble queue full (max_in_flight={max_in_flight}; fail-closed)"
        ),
        AssembleError::Timeout { timeout_ms } => {
            anyhow::anyhow!("envelope assemble timed out after {timeout_ms}ms (fail-closed)")
        }
        AssembleError::WorkerFailed => {
            anyhow::anyhow!("envelope assemble worker failed (fail-closed)")
        }
    }
}

fn merge_history_and_extra(history: &[ChatMessage], extra: &[MessageChunk]) -> Vec<MessageChunk> {
    let mut chunks = extra.to_vec();
    chunks.extend(chat_history_to_chunks(history));
    chunks
}

pub(crate) fn chat_history_to_chunks(history: &[ChatMessage]) -> Vec<MessageChunk> {
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

/// Fail-fast helper used by sync tests / callers when the pilot flag is on.
pub fn apply_envelope_pilot(
    history: &mut Vec<ChatMessage>,
    enabled: bool,
    compact_context: bool,
) -> Result<()> {
    apply_envelope_pilot_with_extra(history, &[], enabled, compact_context)
}

pub fn apply_envelope_pilot_with_extra(
    history: &mut Vec<ChatMessage>,
    extra: &[MessageChunk],
    enabled: bool,
    compact_context: bool,
) -> Result<()> {
    if !enabled || (history.is_empty() && extra.is_empty()) {
        return Ok(());
    }
    let assembled = assemble_history_layered_with_extra(history, extra, compact_context)
        .with_context(|| "CR-L1 envelope pilot assemble_layered")?;
    if assembled.is_empty() {
        bail!("envelope pilot produced empty history");
    }
    *history = assembled;
    Ok(())
}

/// Host path for CLI / channel dispatch (CR-L3-003).
///
/// - `enabled=false` → no-op (default).
/// - `enabled=true`, `use_async_pool=false` → sync `assemble_layered` (CR-L1/L2).
/// - `enabled=true`, `use_async_pool=true` → `AssemblePool` schedule façade (same algorithm).
pub async fn apply_envelope_pilot_async(
    history: &mut Vec<ChatMessage>,
    enabled: bool,
    compact_context: bool,
    use_async_pool: bool,
) -> Result<()> {
    apply_envelope_pilot_async_with_extra(
        history,
        &[],
        enabled,
        compact_context,
        use_async_pool,
        None,
    )
    .await
}

pub async fn apply_envelope_pilot_async_with_extra(
    history: &mut Vec<ChatMessage>,
    extra: &[MessageChunk],
    enabled: bool,
    compact_context: bool,
    use_async_pool: bool,
    context_window: Option<u32>,
) -> Result<()> {
    if !enabled || (history.is_empty() && extra.is_empty()) {
        return Ok(());
    }
    let assembled = if use_async_pool {
        assemble_history_layered_async_with_extra_window(
            history,
            extra,
            compact_context,
            context_window,
        )
        .await
        .with_context(|| "CR-L3-003 envelope pilot assemble_layered_async")?
    } else {
        assemble_history_layered_with_extra_window(history, extra, compact_context, context_window)
            .with_context(|| "CR-L1 envelope pilot assemble_layered")?
    };
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
    fn channel_envelope_pilot_hard_budget_fail_closed() {
        // CR-L2-005: channel dispatch uses apply_envelope_pilot; HardBudget must stay explicit.
        let history = vec![
            ChatMessage::system("S".repeat(400)),
            ChatMessage::user("A".repeat(400)),
        ];
        let chunks = chat_history_to_chunks(&history);
        let options = LayeredAssembleOptions {
            budget: ContextBudget::new(5, 0, 1),
            strategy: AssembleStrategy::Chat,
            ..Default::default()
        };
        let err = MessageAssembler::assemble_layered(&chunks, &options).unwrap_err();
        assert!(matches!(err, AssembleError::HardBudgetViolation { .. }));
        let mut hist = history;
        // Disabled flag must be a no-op even with oversized critical content.
        apply_envelope_pilot(&mut hist, false, true).unwrap();
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn async_assemble_opt_in_flag_off_stays_sync_path() {
        // CR-L3-003: use_async_pool=false keeps sync assemble (flag default).
        let history = vec![ChatMessage::system("sys"), ChatMessage::user("ask")];
        let sync_out = assemble_history_layered(&history, false).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut hist = history.clone();
        rt.block_on(apply_envelope_pilot_async(&mut hist, true, false, false))
            .unwrap();
        assert_eq!(hist.len(), sync_out.len());
        assert_eq!(hist[0].content, sync_out[0].content);
        assert_eq!(hist[1].content, sync_out[1].content);
    }

    #[test]
    fn async_assemble_opt_in_flag_on_matches_sync_under_budget() {
        // CR-L3-003: async façade must match sync algorithm under budget.
        let history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("old"),
            ChatMessage::assistant("reply"),
            ChatMessage::user("ask"),
        ];
        let sync_out = assemble_history_layered(&history, false).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let async_out = rt
            .block_on(assemble_history_layered_async(&history, false))
            .unwrap();
        assert_eq!(sync_out.len(), async_out.len());
        for (s, a) in sync_out.iter().zip(async_out.iter()) {
            assert_eq!(s.role, a.role);
            assert_eq!(s.content, a.content);
        }
    }

    #[test]
    fn async_assemble_opt_in_disabled_is_noop() {
        let mut hist = vec![ChatMessage::system("sys"), ChatMessage::user("ask")];
        let before = hist.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(apply_envelope_pilot_async(&mut hist, false, false, true))
            .unwrap();
        assert_eq!(hist.len(), before.len());
        assert_eq!(hist[0].content, before[0].content);
    }

    #[test]
    fn protocol_context_window_allows_large_system() {
        let history = vec![
            ChatMessage::system("S".repeat(100_000)),
            ChatMessage::user("ask"),
        ];
        let err = assemble_history_layered_with_extra_window(&history, &[], false, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("HardBudgetViolation"), "{err}");
        let kept = assemble_history_layered_with_extra_window(&history, &[], false, Some(128_000))
            .unwrap();
        assert!(kept.iter().any(|m| m.role == "system"));
        assert!(kept.iter().any(|m| m.role == "user" && m.content == "ask"));
    }
}
