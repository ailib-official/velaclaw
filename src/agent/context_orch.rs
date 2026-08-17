//! Canonical conversation-history preparation (VL-CTX-001 / GOV-007).
//! 统一上下文编排：可选 LLM 摘要 → 分层 `assemble_layered`（默认）或 kill-switch trim。
//!
//! All production surfaces (CLI `loop_`, Web `Agent::turn`, channel dispatch) MUST call
//! [`prepare_turn_history`] — do not re-implement compact / envelope / trim locally.

use crate::providers::{ChatMessage, Provider};
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use velaclaw_agent_runtime::loop_parse::{
    apply_compaction_summary, build_compaction_transcript, trim_history,
    COMPACTION_KEEP_RECENT_MESSAGES, COMPACTION_MAX_SUMMARY_CHARS,
};

/// Optional LLM summarizer for overflow history (same semantics on CLI and Web).
pub struct HistorySummarizer<'a> {
    pub provider: &'a dyn Provider,
    pub model: &'a str,
}

/// Options for the single pre-turn history prepare entry.
#[derive(Clone, Copy)]
pub struct PrepareHistoryOpts<'a> {
    /// When true: run ai-lib `assemble_layered` (normative path). When false: trim only
    /// (emergency kill-switch via `[agent].envelope_assemble = false`).
    pub layered: bool,
    pub compact_context: bool,
    pub async_pool: bool,
    pub max_history: usize,
    pub summarizer: Option<&'a HistorySummarizer<'a>>,
    /// Host-retrieved Layer chunks (workspace / memory-shaped). Empty = history only.
    #[cfg(feature = "ai-protocol")]
    pub extra_chunks: &'a [ai_lib_rust::context::MessageChunk],
}

/// Outcome of [`prepare_turn_history`] for observability / CLI notices.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrepareHistoryReport {
    pub compacted: bool,
    pub layered_applied: bool,
}

/// Single GOV-007 entry: optional compact → layered assemble (or trim kill-switch).
pub async fn prepare_turn_history(
    history: &mut Vec<ChatMessage>,
    opts: PrepareHistoryOpts<'_>,
) -> Result<PrepareHistoryReport> {
    let mut report = PrepareHistoryReport::default();

    if history.is_empty() {
        #[cfg(feature = "ai-protocol")]
        if opts.extra_chunks.is_empty() {
            return Ok(report);
        }
        #[cfg(not(feature = "ai-protocol"))]
        {
            return Ok(report);
        }
    }

    if let Some(summarizer) = opts.summarizer {
        if auto_compact_history(history, summarizer, opts.max_history).await? {
            report.compacted = true;
        }
    }

    #[cfg(feature = "ai-protocol")]
    {
        if opts.layered {
            crate::agent::envelope_pilot::apply_envelope_pilot_async_with_extra(
                history,
                opts.extra_chunks,
                true,
                opts.compact_context,
                opts.async_pool,
            )
            .await?;
            report.layered_applied = true;
            return Ok(report);
        }
    }

    // Kill-switch / non-ai-protocol: hard message-count cap.
    // extra_chunks are Layer 0–5 retrieve and only enter assemble_layered;
    // they are intentionally unused here (emergency trim, not a second inject).
    #[cfg(feature = "ai-protocol")]
    if !opts.extra_chunks.is_empty() {
        tracing::debug!(
            extra = opts.extra_chunks.len(),
            "envelope kill-switch: skipping retrieved extra_chunks"
        );
    }
    trim_history(history, opts.max_history);
    Ok(report)
}

async fn auto_compact_history(
    history: &mut Vec<ChatMessage>,
    summarizer: &HistorySummarizer<'_>,
    max_history: usize,
) -> Result<bool> {
    let has_system = history.first().is_some_and(|m| m.role == "system");
    let non_system_count = if has_system {
        history.len().saturating_sub(1)
    } else {
        history.len()
    };

    if non_system_count <= max_history {
        return Ok(false);
    }

    let start = if has_system { 1 } else { 0 };
    // Reserve one slot under `max_history` for the injected `[Compaction summary]`
    // message so kill-switch `trim_history` does not drop it immediately.
    let keep_budget = max_history.saturating_sub(1).max(1);
    let keep_recent = COMPACTION_KEEP_RECENT_MESSAGES
        .min(non_system_count)
        .min(keep_budget);
    let compact_count = non_system_count.saturating_sub(keep_recent);
    if compact_count == 0 {
        return Ok(false);
    }

    let compact_end = start + compact_count;
    let to_compact: Vec<ChatMessage> = history[start..compact_end].to_vec();
    let transcript = build_compaction_transcript(&to_compact);

    let summarizer_system = crate::agent::prompt_composer::build_compact_summarizer_system();
    let summarizer_user = format!(
        "Summarize the following conversation history for context preservation. Keep it short (max 12 bullet points).\n\n{}",
        transcript
    );

    let summary_raw = summarizer
        .provider
        .chat_with_system(
            Some(&summarizer_system),
            &summarizer_user,
            summarizer.model,
            0.2,
        )
        .await
        .unwrap_or_else(|_| truncate_with_ellipsis(&transcript, COMPACTION_MAX_SUMMARY_CHARS));

    let summary = truncate_with_ellipsis(&summary_raw, COMPACTION_MAX_SUMMARY_CHARS);
    apply_compaction_summary(history, start, compact_end, &summary);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ChatRequest, ChatResponse};
    use async_trait::async_trait;

    struct StubProvider {
        reply: String,
    }

    #[async_trait]
    impl Provider for StubProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok(self.reply.clone())
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ChatResponse> {
            Ok(ChatResponse {
                text: Some(self.reply.clone()),
                tool_calls: vec![],
            })
        }
    }

    #[tokio::test]
    async fn prepare_kill_switch_trims_without_layered() {
        let mut history = vec![ChatMessage::system("sys")];
        for i in 0..10 {
            history.push(ChatMessage::user(format!("u{i}")));
            history.push(ChatMessage::assistant(format!("a{i}")));
        }
        let report = prepare_turn_history(
            &mut history,
            PrepareHistoryOpts {
                layered: false,
                compact_context: false,
                async_pool: false,
                max_history: 4,
                extra_chunks: &[],
                summarizer: None,
            },
        )
        .await
        .unwrap();
        assert!(!report.layered_applied);
        assert!(!report.compacted);
        assert_eq!(history.first().map(|m| m.role.as_str()), Some("system"));
        assert!(history.len() <= 5); // system + 4
    }

    #[tokio::test]
    async fn prepare_compacts_when_summarizer_and_over_limit() {
        let provider = StubProvider {
            reply: "- prior topic was greetings".into(),
        };
        let summarizer = HistorySummarizer {
            provider: &provider,
            model: "stub-model",
        };
        let mut history = vec![ChatMessage::system("sys")];
        for i in 0..30 {
            history.push(ChatMessage::user(format!("user message number {i}")));
            history.push(ChatMessage::assistant(format!("assistant reply {i}")));
        }
        let before = history.len();
        let report = prepare_turn_history(
            &mut history,
            PrepareHistoryOpts {
                layered: false,
                compact_context: false,
                async_pool: false,
                max_history: 10,
                extra_chunks: &[],
                summarizer: Some(&summarizer),
            },
        )
        .await
        .unwrap();
        assert!(report.compacted);
        assert!(history.len() < before);
        assert!(
            history
                .iter()
                .any(|m| m.content.contains("[Compaction summary]")),
            "expected compaction summary marker to survive prepare"
        );
    }

    #[tokio::test]
    async fn prepare_compact_then_trim_preserves_summary_under_cap() {
        let provider = StubProvider {
            reply: "- prior topic was greetings".into(),
        };
        let summarizer = HistorySummarizer {
            provider: &provider,
            model: "stub-model",
        };
        let mut history = vec![ChatMessage::system("sys")];
        for i in 0..40 {
            history.push(ChatMessage::user(format!("user message number {i}")));
            history.push(ChatMessage::assistant(format!("assistant reply {i}")));
        }
        let report = prepare_turn_history(
            &mut history,
            PrepareHistoryOpts {
                layered: false,
                compact_context: false,
                async_pool: false,
                max_history: 8,
                extra_chunks: &[],
                summarizer: Some(&summarizer),
            },
        )
        .await
        .unwrap();
        assert!(report.compacted);
        assert!(history.len() <= 9); // system + max_history
        assert!(history
            .iter()
            .any(|m| m.content.contains("[Compaction summary]")));
    }

    #[cfg(feature = "ai-protocol")]
    #[tokio::test]
    async fn prepare_merges_extra_chunks_through_layered_entry() {
        let extra =
            crate::agent::context_contract::memory_fixture_chunks(&[("k", "fixture-memory")]);
        let mut history = vec![ChatMessage::system("sys"), ChatMessage::user("ask")];
        let report = prepare_turn_history(
            &mut history,
            PrepareHistoryOpts {
                layered: true,
                compact_context: false,
                async_pool: false,
                max_history: 32,
                extra_chunks: &extra,
                summarizer: None,
            },
        )
        .await
        .unwrap();
        assert!(report.layered_applied);
        assert!(history
            .iter()
            .any(|m| m.content.contains("[retrieve:memory")));
    }
}
