//! Unified tool batch execution — approval gate + parallel/sequential dispatch (VL-UR-003).
//! 统一工具批执行：批准门 + 并行/串行调度。

use crate::agent::dispatcher::ParsedToolCall as GateToolCall;
use crate::approval::{
    ApprovalGate, ApprovalManager, ChannelApprovalSession, GateDecision,
};
use crate::observability::{Observer, ObserverEvent};
use crate::security::PolicyHandle;
use crate::tools::{Tool, ToolExecutionContext};
use anyhow::Result;
use regex::{Regex, RegexSet};
use std::sync::LazyLock;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

/// Parsed tool call from LLM output (loop-local shape without provider tool_call_id).
#[derive(Debug, Clone)]
pub(crate) struct ParsedToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

static SENSITIVE_KEY_PATTERNS: LazyLock<RegexSet> = LazyLock::new(|| {
    RegexSet::new([
        r"(?i)token",
        r"(?i)api[_-]?key",
        r"(?i)password",
        r"(?i)secret",
        r"(?i)user[_-]?key",
        r"(?i)bearer",
        r"(?i)credential",
    ])
    .unwrap()
});

static SENSITIVE_KV_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(token|api[_-]?key|password|secret|user[_-]?key|bearer|credential)["']?\s*[:=]\s*(?:"([^"]{8,})"|'([^']{8,})'|([a-zA-Z0-9_\-\.]{8,}))"#,
    )
    .unwrap()
});

/// Output of one tool invocation in a batch.
#[derive(Debug, Clone)]
pub struct ToolBatchResult {
    pub output: String,
    pub success: bool,
}

/// Scrub credentials from tool output to prevent accidental exfiltration.
pub(crate) fn scrub_credentials(input: &str) -> String {
    let _ = &SENSITIVE_KEY_PATTERNS;
    SENSITIVE_KV_REGEX
        .replace_all(input, |caps: &regex::Captures| {
            let full_match = &caps[0];
            let key = &caps[1];
            let val = caps
                .get(2)
                .or(caps.get(3))
                .or(caps.get(4))
                .map(|m| m.as_str())
                .unwrap_or("");

            let prefix = if val.len() > 4 { &val[..4] } else { "" };

            if full_match.contains(':') {
                if full_match.contains('"') {
                    format!("\"{}\": \"{}*[REDACTED]\"", key, prefix)
                } else {
                    format!("{}: {}*[REDACTED]", key, prefix)
                }
            } else if full_match.contains('=') {
                if full_match.contains('"') {
                    format!("{}=\"{}*[REDACTED]\"", key, prefix)
                } else {
                    format!("{}={}*[REDACTED]", key, prefix)
                }
            } else {
                format!("{}: {}*[REDACTED]", key, prefix)
            }
        })
        .to_string()
}

fn find_tool<'a>(tools: &'a [Box<dyn Tool>], name: &str) -> Option<&'a dyn Tool> {
    tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
}

/// Map common DSML / model parameter aliases to tool schema keys.
pub(crate) fn normalize_tool_arguments(tool_name: &str, mut args: serde_json::Value) -> serde_json::Value {
    let Some(obj) = args.as_object_mut() else {
        return args;
    };
    match tool_name {
        "file_read" | "file_write" if !obj.contains_key("path") => {
            if let Some(path) = obj.remove("file_path") {
                obj.insert("path".to_string(), path);
            }
        }
        "shell" if !obj.contains_key("command") => {
            if let Some(cmd) = obj.remove("cmd") {
                obj.insert("command".to_string(), cmd);
            }
        }
        _ => {}
    }
    args
}

async fn execute_one_tool(
    call_name: &str,
    call_arguments: serde_json::Value,
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
    ctx: &ToolExecutionContext,
) -> ToolBatchResult {
    let call_arguments = normalize_tool_arguments(call_name, call_arguments);
    let Some(tool) = find_tool(tools_registry, call_name) else {
        return ToolBatchResult {
            output: format!("Unknown tool: {call_name}"),
            success: false,
        };
    };

    observer.record_event(&ObserverEvent::ToolCallStart {
        tool: call_name.to_string(),
    });
    let start = Instant::now();

    let tool_future = tool.execute(call_arguments, ctx);
    let tool_result = if let Some(token) = cancellation_token {
        tokio::select! {
            () = token.cancelled() => {
                return ToolBatchResult {
                    output: "tool loop cancelled".into(),
                    success: false,
                };
            }
            result = tool_future => result,
        }
    } else {
        tool_future.await
    };

    match tool_result {
        Ok(r) => {
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration: start.elapsed(),
                success: r.success,
            });
            if r.success {
                ToolBatchResult {
                    output: scrub_credentials(&r.output),
                    success: true,
                }
            } else {
                ToolBatchResult {
                    output: format!("Error: {}", r.error.unwrap_or_else(|| r.output)),
                    success: false,
                }
            }
        }
        Err(e) => {
            observer.record_event(&ObserverEvent::ToolCall {
                tool: call_name.to_string(),
                duration: start.elapsed(),
                success: false,
            });
            ToolBatchResult {
                output: format!("Error executing {call_name}: {e}"),
                success: false,
            }
        }
    }
}

/// Whether multiple tool calls may run concurrently (gate-aware).
pub(crate) fn should_execute_tools_in_parallel(
    tool_calls: &[ParsedToolCall],
    gate: Option<&ApprovalGate<'_>>,
) -> bool {
    if tool_calls.len() <= 1 {
        return false;
    }

    if let Some(gate) = gate {
        if tool_calls.iter().any(|call| gate.needs_approval(&call.name)) {
            return false;
        }
    }

    true
}

async fn execute_tools_parallel(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<ToolBatchResult>> {
    let ctx_default = ToolExecutionContext::default();
    let futures: Vec<_> = tool_calls
        .iter()
        .map(|call| {
            execute_one_tool(
                &call.name,
                call.arguments.clone(),
                tools_registry,
                observer,
                cancellation_token,
                &ctx_default,
            )
        })
        .collect();

    Ok(futures_util::future::join_all(futures).await)
}

async fn execute_tools_sequential_no_gate(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<ToolBatchResult>> {
    let ctx = ToolExecutionContext::default();
    let mut results = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        let args = normalize_tool_arguments(&call.name, call.arguments.clone());
        results.push(
            execute_one_tool(
                &call.name,
                args,
                tools_registry,
                observer,
                cancellation_token,
                &ctx,
            )
            .await,
        );
    }

    Ok(results)
}

async fn execute_tools_sequential_with_gate(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    gate: &ApprovalGate<'_>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<ToolBatchResult>> {
    let mut results = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        let args = normalize_tool_arguments(&call.name, call.arguments.clone());
        let gate_call = GateToolCall {
            name: call.name.clone(),
            arguments: call.arguments.clone(),
            tool_call_id: None,
        };

        let (shell_human_approved, proceed) = match gate.decide_async(&gate_call).await {
            GateDecision::Denied { message } => {
                results.push(ToolBatchResult {
                    output: message,
                    success: false,
                });
                (false, false)
            }
            GateDecision::Proceed {
                shell_human_approved,
            } => (shell_human_approved, true),
        };

        if !proceed {
            continue;
        }

        let ctx = ToolExecutionContext::with_shell_human_approved(shell_human_approved);
        results.push(
            execute_one_tool(
                &call.name,
                args,
                tools_registry,
                observer,
                cancellation_token,
                &ctx,
            )
            .await,
        );
    }

    Ok(results)
}

/// Execute a batch of tool calls with optional approval manager and security policy gate.
pub(crate) async fn execute_tool_batch(
    tool_calls: &[ParsedToolCall],
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    approval: Option<&ApprovalManager>,
    security: Option<&PolicyHandle>,
    channel_name: &str,
    channel_approval: Option<ChannelApprovalSession>,
    cancellation_token: Option<&CancellationToken>,
) -> Result<Vec<ToolBatchResult>> {
    let policy = security.cloned();
    let managed_gate = approval.map(|mgr| {
        let mut gate = ApprovalGate::new(mgr, channel_name, policy.clone());
        if let Some(session) = channel_approval {
            gate = gate.with_channel_session(session);
        }
        gate
    });

    let gate_ref: Option<&ApprovalGate<'_>> = managed_gate.as_ref();

    let should_parallel = should_execute_tools_in_parallel(tool_calls, gate_ref);

    if should_parallel {
        return execute_tools_parallel(
            tool_calls,
            tools_registry,
            observer,
            cancellation_token,
        )
        .await;
    }

    if let Some(gate) = gate_ref {
        execute_tools_sequential_with_gate(
            tool_calls,
            tools_registry,
            observer,
            gate,
            cancellation_token,
        )
        .await
    } else {
        execute_tools_sequential_no_gate(
            tool_calls,
            tools_registry,
            observer,
            cancellation_token,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AutonomyConfig;

    #[test]
    fn should_execute_tools_in_parallel_returns_false_for_single_call() {
        let calls = vec![ParsedToolCall {
            name: "file_read".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        }];

        assert!(!should_execute_tools_in_parallel(&calls, None));
    }

    #[test]
    fn should_execute_tools_in_parallel_returns_false_when_gate_needs_approval() {
        let calls = vec![
            ParsedToolCall {
                name: "shell".to_string(),
                arguments: serde_json::json!({"command": "pwd"}),
            },
            ParsedToolCall {
                name: "http_request".to_string(),
                arguments: serde_json::json!({"url": "https://example.com"}),
            },
        ];
        let approval_cfg = AutonomyConfig::default();
        let approval_mgr = ApprovalManager::from_config(&approval_cfg);
        let gate = ApprovalGate::new(&approval_mgr, "cli", None);

        assert!(!should_execute_tools_in_parallel(
            &calls,
            Some(&gate)
        ));
    }

    #[test]
    fn scrub_credentials_redacts_api_key() {
        let input = "api_key: sk-1234567890abcdef";
        let scrubbed = scrub_credentials(input);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("1234567890abcdef"));
    }
}
