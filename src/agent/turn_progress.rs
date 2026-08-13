//! Shared turn progress mapping for CLI and Web (VL-UX-CANCEL-001 / GOV-007).
//! 将 Observer 事件映射为 CLI/Web 共用的步骤提示。

use crate::observability::traits::ObserverMetric;
use crate::observability::{Observer, ObserverEvent};
use std::any::Any;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;

const SUMMARY_MAX_CHARS: usize = 240;

/// User-facing progress during a tool-loop turn (not the final assistant reply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnProgress {
    Status {
        phase: String,
        detail: String,
    },
    Step {
        kind: String,
        tool: String,
        ok: bool,
        summary: String,
    },
}

/// Truncate tool output for step display (no secrets; caller should scrub first).
pub fn truncate_summary(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    if flat.chars().count() <= SUMMARY_MAX_CHARS {
        return flat.to_string();
    }
    let mut out: String = flat
        .chars()
        .take(SUMMARY_MAX_CHARS.saturating_sub(1))
        .collect();
    out.push('…');
    out
}

/// Map a runtime observer event to a compact progress item.
pub fn event_to_progress(event: &ObserverEvent) -> Option<TurnProgress> {
    match event {
        ObserverEvent::LlmRequest {
            provider, model, ..
        } => Some(TurnProgress::Status {
            phase: "model".into(),
            detail: format!("{provider}/{model}"),
        }),
        ObserverEvent::ToolCallStart { tool } => Some(TurnProgress::Status {
            phase: "tool".into(),
            detail: tool.clone(),
        }),
        ObserverEvent::ToolCall {
            tool,
            success,
            summary,
            ..
        } => Some(TurnProgress::Step {
            kind: "tool_result".into(),
            tool: tool.clone(),
            ok: *success,
            summary: summary.clone().unwrap_or_default(),
        }),
        _ => None,
    }
}

/// Fan-out observer: keep the configured backend, plus optional progress sink.
pub struct ProgressObserver {
    inner: Arc<dyn Observer>,
    tx: Option<Sender<TurnProgress>>,
    print_cli: bool,
}

impl ProgressObserver {
    pub fn forwarding(inner: Arc<dyn Observer>, tx: Sender<TurnProgress>) -> Self {
        Self {
            inner,
            tx: Some(tx),
            print_cli: false,
        }
    }

    pub fn cli(inner: Arc<dyn Observer>) -> Self {
        Self {
            inner,
            tx: None,
            print_cli: true,
        }
    }
}

impl Observer for ProgressObserver {
    fn record_event(&self, event: &ObserverEvent) {
        self.inner.record_event(event);
        if let Some(progress) = event_to_progress(event) {
            if let Some(tx) = &self.tx {
                let _ = tx.try_send(progress.clone());
            }
            if self.print_cli {
                print_cli_progress(&progress);
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        self.inner.record_metric(metric);
    }

    fn flush(&self) {
        self.inner.flush();
    }

    fn name(&self) -> &str {
        "progress"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Dim status / distinct step lines (not Markdown assistant output).
pub fn print_cli_progress(progress: &TurnProgress) {
    match progress {
        TurnProgress::Status { phase, detail } => {
            eprintln!("{}", console::style(format!("· {phase}: {detail}")).dim());
        }
        TurnProgress::Step {
            tool, ok, summary, ..
        } => {
            let tag = if *ok { "ok" } else { "fail" };
            let line = if summary.is_empty() {
                format!("  [{tag}] {tool}")
            } else {
                format!("  [{tag}] {tool}: {summary}")
            };
            eprintln!("{}", console::style(line).cyan());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn truncate_summary_caps_length() {
        let long = "a".repeat(400);
        let out = truncate_summary(&long);
        assert!(out.chars().count() <= SUMMARY_MAX_CHARS);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn llm_request_maps_to_status() {
        let p = event_to_progress(&ObserverEvent::LlmRequest {
            provider: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
            messages_count: 2,
        })
        .expect("mapped");
        assert_eq!(
            p,
            TurnProgress::Status {
                phase: "model".into(),
                detail: "deepseek/deepseek-v4-flash".into(),
            }
        );
    }

    #[test]
    fn tool_call_maps_to_step() {
        let p = event_to_progress(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            duration: Duration::from_millis(1),
            success: true,
            summary: Some("ok output".into()),
        })
        .expect("mapped");
        match p {
            TurnProgress::Step {
                tool, ok, summary, ..
            } => {
                assert_eq!(tool, "shell");
                assert!(ok);
                assert_eq!(summary, "ok output");
            }
            _ => panic!("expected step"),
        }
    }
}
