//! Unified tool batch execution — approval gate + parallel/sequential dispatch (VL-UR-003).
//! 统一工具批执行：批准门 + 并行/串行调度。

use crate::agent::dispatcher::ParsedToolCall as GateToolCall;
use crate::approval::{
    ApprovalGate, ApprovalHub, ApprovalManager, ChannelApprovalSession, GateDecision, HumanInputHub,
};
use crate::observability::{Observer, ObserverEvent};
use crate::security::PolicyHandle;
use crate::tools::{Tool, ToolExecutionContext};
use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use velaclaw_agent_runtime::normalize_tool_arguments;

pub(crate) use velaclaw_agent_runtime::scrub_credentials;

/// Parsed tool call from LLM output (loop-local shape without provider tool_call_id).
#[derive(Debug, Clone)]
pub(crate) struct ParsedToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Output of one tool invocation in a batch.
#[derive(Debug, Clone)]
pub struct ToolBatchResult {
    pub output: String,
    pub success: bool,
}

/// Optional Web/gateway gate extras (VL-CTX-002): ApprovalHub + secret_slot hub.
#[derive(Clone, Default)]
pub(crate) struct ToolBatchGateExtras {
    pub approval_hub: Option<Arc<ApprovalHub>>,
    pub human_input_hub: Option<Arc<HumanInputHub>>,
}

fn find_tool<'a>(tools: &'a [Box<dyn Tool>], name: &str) -> Option<&'a dyn Tool> {
    tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
}

/// Resolve shell `secret_slot` into stdin secret (same semantics as prior Agent path).
///
/// Gate approval still sees the original args (including `secret_slot`) via
/// `call.arguments.clone()`; this helper strips the slot from the execution
/// copy and consumes the secret from [`HumanInputHub`] so the shell never
/// receives the opaque slot id as a literal argument.
fn build_tool_execution_context(
    call_name: &str,
    args: &mut serde_json::Value,
    shell_human_approved: bool,
    human_input_hub: Option<&HumanInputHub>,
) -> Result<ToolExecutionContext, ToolBatchResult> {
    let mut stdin_secret = None;
    if call_name == "shell" {
        if let Some(slot_id) = args
            .as_object_mut()
            .and_then(|m| m.remove("secret_slot"))
            .and_then(|v| v.as_str().map(|s| s.to_string()))
        {
            let Some(hub) = human_input_hub else {
                return Err(ToolBatchResult {
                    output: "Error: secret_slot requires interactive gateway human input".into(),
                    success: false,
                });
            };
            match hub.secret_slots().take(&slot_id) {
                Some(secret) => stdin_secret = Some(secret),
                None => {
                    return Err(ToolBatchResult {
                        output: format!(
                            "Error: secret_slot '{slot_id}' is missing or already consumed. \
                             Call request_human_input(kind=secret) again."
                        ),
                        success: false,
                    });
                }
            }
        }
    }
    Ok(
        ToolExecutionContext::with_shell_human_approved(shell_human_approved)
            .with_stdin_secret(stdin_secret),
    )
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
    let caption = crate::agent::turn_progress::progress_caption(call_name, &call_arguments);
    let Some(tool) = find_tool(tools_registry, call_name) else {
        return ToolBatchResult {
            output: format!("Unknown tool: {call_name}"),
            success: false,
        };
    };

    observer.record_event(&ObserverEvent::ToolCallStart {
        tool: call_name.to_string(),
        caption: Some(caption.clone()),
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
                summary: Some(caption.clone()),
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
                summary: Some(caption),
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
        if tool_calls
            .iter()
            .any(|call| gate.needs_approval(&call.name))
        {
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
    human_input_hub: Option<&HumanInputHub>,
) -> Result<Vec<ToolBatchResult>> {
    let mut results = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        let mut args = normalize_tool_arguments(&call.name, call.arguments.clone());
        let ctx = match build_tool_execution_context(&call.name, &mut args, false, human_input_hub)
        {
            Ok(ctx) => ctx,
            Err(err) => {
                results.push(err);
                continue;
            }
        };
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
    human_input_hub: Option<&HumanInputHub>,
) -> Result<Vec<ToolBatchResult>> {
    let mut results = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        let mut args = normalize_tool_arguments(&call.name, call.arguments.clone());
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

        let ctx = match build_tool_execution_context(
            &call.name,
            &mut args,
            shell_human_approved,
            human_input_hub,
        ) {
            Ok(ctx) => ctx,
            Err(err) => {
                results.push(err);
                continue;
            }
        };
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
    gate_extras: Option<&ToolBatchGateExtras>,
) -> Result<Vec<ToolBatchResult>> {
    let policy = security.cloned();
    let managed_gate = approval.map(|mgr| {
        let mut gate = ApprovalGate::new(mgr, channel_name, policy.clone());
        if let Some(session) = channel_approval {
            gate = gate.with_channel_session(session);
        }
        if let Some(hub) = gate_extras.and_then(|e| e.approval_hub.clone()) {
            gate = gate.with_hub(hub);
        }
        gate
    });

    let gate_ref: Option<&ApprovalGate<'_>> = managed_gate.as_ref();
    let human_input = gate_extras
        .and_then(|e| e.human_input_hub.as_ref())
        .map(std::convert::AsRef::as_ref);

    // secret_slot resolution requires sequential execution (HITL store is not
    // safe to consume concurrently across a parallel batch).
    let should_parallel =
        should_execute_tools_in_parallel(tool_calls, gate_ref) && human_input.is_none();

    if should_parallel {
        return execute_tools_parallel(tool_calls, tools_registry, observer, cancellation_token)
            .await;
    }

    if let Some(gate) = gate_ref {
        execute_tools_sequential_with_gate(
            tool_calls,
            tools_registry,
            observer,
            gate,
            cancellation_token,
            human_input,
        )
        .await
    } else {
        execute_tools_sequential_no_gate(
            tool_calls,
            tools_registry,
            observer,
            cancellation_token,
            human_input,
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

        assert!(!should_execute_tools_in_parallel(&calls, Some(&gate)));
    }

    #[test]
    fn scrub_credentials_redacts_api_key() {
        let input = "api_key: sk-1234567890abcdef";
        let scrubbed = scrub_credentials(input);
        assert!(scrubbed.contains("[REDACTED]"));
        assert!(!scrubbed.contains("1234567890abcdef"));
    }
}
