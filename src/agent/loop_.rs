use crate::agent::tool_batch::{self, ParsedToolCall};
use crate::approval::{ApprovalManager, ChannelApprovalSession};
use crate::cli_render::{
    format_user_prompt, indent_lines, prefix_agent_lines, RenderOpts, RenderStyle,
};
use crate::config::Config;
#[cfg(not(feature = "ai-protocol"))]
use crate::config::DEFAULT_PROTOCOL_MODEL_ID;
use crate::memory::{self, Memory, MemoryCategory};
use crate::multimodal;
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::{
    self, ChatMessage, ChatRequest, Provider, ProviderCapabilityError, ToolCall,
};
use crate::runtime;
use crate::security::PolicyHandle;
use crate::tools::{self, Tool};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fmt::Write;
use std::io::Write as _;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use velaclaw_agent_runtime::loop_parse::{
    self, build_assistant_history_with_tool_calls, build_native_assistant_history,
    ToolLoopCancelled, DEFAULT_MAX_TOOL_ITERATIONS,
};

pub(crate) use velaclaw_agent_runtime::loop_parse::{
    build_tool_instructions, is_tool_loop_cancelled,
};

/// Session-scoped store for folded CLI payloads (`/expand <id>`).
type FoldCache = Arc<Mutex<HashMap<u64, String>>>;

/// Allocate the next fold id and store `payload` for `/expand`.
fn store_fold_payload(cache: &FoldCache, payload: &str) -> u64 {
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let id = guard.len() as u64 + 1;
    guard.insert(id, payload.to_string());
    id
}

/// Print a tool result block, folding when `render_opts.fold_enabled` and over threshold.
fn print_tool_result_block(
    tool_name: &str,
    result: &str,
    render_opts: RenderOpts,
    fold_cache: Option<&FoldCache>,
) {
    let rendered = render_opts.render(result);
    let total_lines = rendered.split('\n').count();
    let body = indent_lines(&rendered, 2);
    let should_fold =
        render_opts.fold_enabled && fold_cache.is_some() && total_lines > render_opts.fold_lines;
    if !should_fold {
        println!("\n── tool:{tool_name} ──\n{body}\n");
        return;
    }
    let cache = fold_cache.expect("fold_cache checked above");
    let id = store_fold_payload(cache, &rendered);
    let head: String = rendered
        .split('\n')
        .take(render_opts.fold_lines)
        .collect::<Vec<_>>()
        .join("\n");
    let head = indent_lines(&head, 2);
    println!(
        "\n── tool:{tool_name} (前 {} 行 / 共 {total_lines} 行) ──\n{head}\n─────\n用 /expand {id} 展开全部\n",
        render_opts.fold_lines
    );
}

/// Minimum characters per chunk when relaying LLM text to a streaming draft.
const STREAM_CHUNK_MIN_CHARS: usize = 80;

/// Minimum user-message length (in chars) for auto-save to memory.
/// Matches the channel-side constant in `channels/mod.rs`.
const AUTOSAVE_MIN_MESSAGE_CHARS: usize = 20;

fn autosave_memory_key(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4())
}

fn parse_tool_calls(response: &str) -> (String, Vec<ParsedToolCall>) {
    let (text, calls) = loop_parse::parse_tool_calls(response);
    (text, calls.into_iter().map(to_local_call).collect())
}

fn parse_structured_tool_calls(tool_calls: &[ToolCall]) -> Vec<ParsedToolCall> {
    loop_parse::parse_structured_tool_calls(tool_calls)
        .into_iter()
        .map(to_local_call)
        .collect()
}

fn to_local_call(c: velaclaw_agent_runtime::ParsedToolCall) -> ParsedToolCall {
    ParsedToolCall {
        name: c.name,
        arguments: c.arguments,
    }
}

fn parse_arguments_value(raw: Option<&serde_json::Value>) -> serde_json::Value {
    loop_parse::parse_arguments_value(raw)
}

fn parse_tool_call_value(value: &serde_json::Value) -> Option<ParsedToolCall> {
    loop_parse::parse_tool_call_value(value).map(to_local_call)
}

fn parse_tool_calls_from_json_value(value: &serde_json::Value) -> Vec<ParsedToolCall> {
    loop_parse::parse_tool_calls_from_json_value(value)
        .into_iter()
        .map(to_local_call)
        .collect()
}

fn extract_json_values(input: &str) -> Vec<serde_json::Value> {
    loop_parse::extract_json_values(input)
}

fn parse_glm_style_tool_calls(text: &str) -> Vec<(String, serde_json::Value, Option<String>)> {
    loop_parse::parse_glm_style_tool_calls(text)
}

/// Build context preamble by searching memory for relevant entries.
/// Entries with a hybrid score below `min_relevance_score` are dropped to
/// prevent unrelated memories from bleeding into the conversation.
///
/// VL-MEM-001: when `session_id` is set, Conversation/Daily inject only for
/// that session; Core always may inject; legacy `session_id=None` Conversation
/// is excluded.
async fn build_context(
    mem: &dyn Memory,
    user_msg: &str,
    min_relevance_score: f64,
    session_id: Option<&str>,
) -> String {
    let mut context = String::new();

    // Pull relevant memories for this message (no SQL session filter so Core
    // with session_id=None remains visible; apply inject rules below).
    if let Ok(entries) = mem.recall(user_msg, 5, None).await {
        let relevant: Vec<_> = entries
            .iter()
            .filter(|e| match e.score {
                Some(score) => score >= min_relevance_score,
                None => true,
            })
            .filter(|e| memory::should_inject_for_session(e, session_id))
            .collect();

        if !relevant.is_empty() {
            context.push_str("[Memory context]\n");
            for entry in &relevant {
                if memory::is_assistant_autosave_key(&entry.key) {
                    continue;
                }
                let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
            }
            if context == "[Memory context]\n" {
                context.clear();
            } else {
                context.push('\n');
            }
        }
    }

    context
}

/// Build hardware datasheet context from RAG when peripherals are enabled.
/// Includes pin-alias lookup (e.g. "red_led" → 13) when query matches, plus retrieved chunks.
fn build_hardware_context(
    rag: &crate::rag::HardwareRag,
    user_msg: &str,
    boards: &[String],
    chunk_limit: usize,
) -> String {
    if rag.is_empty() || boards.is_empty() {
        return String::new();
    }

    let mut context = String::new();

    // Pin aliases: when user says "red led", inject "red_led: 13" for matching boards
    let pin_ctx = rag.pin_alias_context(user_msg, boards);
    if !pin_ctx.is_empty() {
        context.push_str(&pin_ctx);
    }

    let chunks = rag.retrieve(user_msg, boards, chunk_limit);
    if chunks.is_empty() && pin_ctx.is_empty() {
        return String::new();
    }

    if !chunks.is_empty() {
        context.push_str("[Hardware documentation]\n");
    }
    for chunk in chunks {
        let board_tag = chunk.board.as_deref().unwrap_or("generic");
        let _ = writeln!(
            context,
            "--- {} ({}) ---\n{}\n",
            chunk.source, board_tag, chunk.content
        );
    }
    context.push('\n');
    context
}

/// Execute a single turn of the agent loop: send messages, parse tool calls,
/// execute tools, and loop until the LLM produces a final text response.
/// When `silent` is true, suppresses stdout (for channel use).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn agent_turn(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: f64,
    silent: bool,
    multimodal_config: &crate::config::MultimodalConfig,
    max_tool_iterations: usize,
) -> Result<String> {
    run_tool_call_loop(
        provider,
        history,
        tools_registry,
        observer,
        provider_name,
        model,
        temperature,
        silent,
        None,
        "channel",
        multimodal_config,
        max_tool_iterations,
        None,
        None,
        None,
        None,
        None,
        false,
        RenderOpts {
            style: RenderStyle {
                ansi: false,
                markdown: true,
            },
            fold_lines: 10,
            fold_enabled: false,
        },
        None,
        None,
        None,
    )
    .await
}

// ── Agent Tool-Call Loop ──────────────────────────────────────────────────
// Core agentic iteration: send conversation to the LLM, parse any tool
// calls from the response, execute them, append results to history, and
// repeat until the LLM produces a final text-only answer.
//
// Loop invariant: at the start of each iteration, `history` contains the
// full conversation so far (system prompt + user messages + prior tool
// results). The loop exits when:
//   • the LLM returns no tool calls (final answer), or
//   • max_iterations is reached (runaway safety), or
//   • the cancellation token fires (external abort).

/// Append manifest-backed text tool instructions when the model may emit markup
/// instead of (or alongside) native API tool calls.
#[cfg(feature = "ai-protocol")]
pub(crate) fn append_text_tool_prompt(
    system_prompt: &mut String,
    dispatcher: &dyn crate::agent::dispatcher::ToolDispatcher,
    tools_registry: &[Box<dyn Tool>],
    native_strategy: ai_lib_rust::NativeStrategy,
) {
    let append = !dispatcher.should_send_tool_specs()
        || native_strategy == ai_lib_rust::NativeStrategy::Hybrid;
    if append {
        let instr = dispatcher.prompt_instructions(tools_registry);
        if !instr.is_empty() {
            system_prompt.push_str(&instr);
        }
    }
}

/// Soft-fail UX context for tool loop (ORCH-HOST-004/005).
///
/// `config` is required for opt-in `host_decide_failover` (CLI/Web). Channel
/// surfaces pass `None` — notices still apply; Decide failover does not (no
/// host Decide on the channel path).
#[derive(Clone, Copy)]
pub(crate) struct SoftFailLoopCtx<'a> {
    pub session_key: &'a str,
    pub config: Option<&'a Config>,
    pub surface: velaclaw_agent_runtime::SoftFailSurface,
}

/// Execute a single turn of the agent loop: send messages, parse tool calls,
/// execute tools, and loop until the LLM produces a final text response.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_tool_call_loop(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools_registry: &[Box<dyn Tool>],
    observer: &dyn Observer,
    provider_name: &str,
    model: &str,
    temperature: f64,
    silent: bool,
    approval: Option<&ApprovalManager>,
    channel_name: &str,
    multimodal_config: &crate::config::MultimodalConfig,
    max_tool_iterations: usize,
    cancellation_token: Option<CancellationToken>,
    on_delta: Option<tokio::sync::mpsc::Sender<String>>,
    tool_dispatcher: Option<&dyn crate::agent::dispatcher::ToolDispatcher>,
    security: Option<&PolicyHandle>,
    channel_approval: Option<ChannelApprovalSession>,
    // When true, tool results use `[Tool results]` user text (Hybrid manifests).
    text_tool_result_history: bool,
    render_opts: RenderOpts,
    fold_cache: Option<&FoldCache>,
    soft_fail: Option<SoftFailLoopCtx<'_>>,
    gate_extras: Option<&crate::agent::tool_batch::ToolBatchGateExtras>,
) -> Result<String> {
    let max_iterations = if max_tool_iterations == 0 {
        DEFAULT_MAX_TOOL_ITERATIONS
    } else {
        max_tool_iterations
    };

    let tool_specs: Vec<crate::tools::ToolSpec> =
        tools_registry.iter().map(|tool| tool.spec()).collect();
    let use_native_tools = tool_dispatcher
        .map(|d| d.should_send_tool_specs() && !tool_specs.is_empty())
        .unwrap_or_else(|| provider.supports_native_tools() && !tool_specs.is_empty());

    // VL-TTC-016: CorrectivePrompt → NativeOnlyReask → StripFailClosed.
    let mut format_ladder = velaclaw_agent_runtime::ToolFormatLadder::new();

    for _iteration in 0..max_iterations {
        if cancellation_token
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return Err(ToolLoopCancelled.into());
        }

        let image_marker_count = multimodal::count_image_markers(history);
        if image_marker_count > 0 && !provider.supports_vision() {
            return Err(ProviderCapabilityError {
                provider: provider_name.to_string(),
                capability: "vision".to_string(),
                message: format!(
                    "received {image_marker_count} image marker(s), but this provider does not support vision input"
                ),
            }
            .into());
        }

        let prepared_messages =
            multimodal::prepare_messages_for_provider(history, multimodal_config).await?;

        observer.record_event(&ObserverEvent::LlmRequest {
            provider: provider_name.to_string(),
            model: model.to_string(),
            messages_count: history.len(),
        });

        let llm_started_at = Instant::now();

        // Unified path via Provider::chat so provider-specific native tool logic
        // (OpenAI/Anthropic/OpenRouter/compatible adapters) is honored.
        let request_tools = if use_native_tools {
            Some(tool_specs.as_slice())
        } else {
            None
        };

        let chat_future = provider.chat(
            ChatRequest {
                messages: &prepared_messages.messages,
                tools: request_tools,
            },
            model,
            temperature,
        );

        let chat_result = if let Some(token) = cancellation_token.as_ref() {
            tokio::select! {
                () = token.cancelled() => return Err(ToolLoopCancelled.into()),
                result = chat_future => result,
            }
        } else {
            chat_future.await
        };

        let (response_text, parsed_text, tool_calls, assistant_history_content, native_tool_calls) =
            match chat_result {
                Ok(resp) => {
                    observer.record_event(&ObserverEvent::LlmResponse {
                        provider: provider_name.to_string(),
                        model: model.to_string(),
                        duration: llm_started_at.elapsed(),
                        success: true,
                        error_message: None,
                    });

                    if let Some(dispatcher) = tool_dispatcher {
                        let response_text = resp.text_or_empty().to_string();
                        let (mut parsed_text, mut disp_calls) = dispatcher.parse_response(&resp);
                        if disp_calls.is_empty() {
                            // VL-TTC-010: manifest parser before residual loop_parse.
                            #[cfg(feature = "ai-protocol")]
                            {
                                let (manifest_text, manifest_calls) =
                                    velaclaw_agent_runtime::parse_manifest_text_tool_fallback(
                                        &response_text,
                                    );
                                if !manifest_calls.is_empty() {
                                    if !manifest_text.is_empty() {
                                        parsed_text = manifest_text;
                                    }
                                    disp_calls = manifest_calls;
                                }
                            }
                            if disp_calls.is_empty() {
                                let (fallback_text, fallback_calls) =
                                    parse_tool_calls(&response_text);
                                if !fallback_calls.is_empty() {
                                    if !fallback_text.is_empty() {
                                        parsed_text = fallback_text;
                                    }
                                    disp_calls = fallback_calls
                                        .into_iter()
                                        .map(|c| crate::agent::dispatcher::ParsedToolCall {
                                            name: c.name,
                                            arguments: c.arguments,
                                            tool_call_id: None,
                                        })
                                        .collect();
                                }
                            }
                        }
                        let calls: Vec<ParsedToolCall> = disp_calls
                            .into_iter()
                            .map(|c| ParsedToolCall {
                                name: c.name,
                                arguments: c.arguments,
                            })
                            .collect();
                        let assistant_history_content = if !resp.tool_calls.is_empty() {
                            build_native_assistant_history(&response_text, &resp.tool_calls)
                        } else if !calls.is_empty() {
                            let synthetic: Vec<ToolCall> = calls
                                .iter()
                                .enumerate()
                                .map(|(i, c)| ToolCall {
                                    id: format!("text_tool_{i}"),
                                    name: c.name.clone(),
                                    arguments: c.arguments.to_string(),
                                })
                                .collect();
                            build_assistant_history_with_tool_calls(
                                if parsed_text.is_empty() {
                                    response_text.as_str()
                                } else {
                                    parsed_text.as_str()
                                },
                                &synthetic,
                            )
                        } else {
                            response_text.clone()
                        };
                        (
                            response_text,
                            parsed_text,
                            calls,
                            assistant_history_content,
                            resp.tool_calls,
                        )
                    } else {
                        let response_text = resp.text_or_empty().to_string();
                        // First try native structured tool calls (OpenAI-format).
                        // Fall back to text-based parsing (XML tags, markdown blocks,
                        // GLM format) only if the provider returned no native calls —
                        // this ensures we support both native and prompt-guided models.
                        let mut calls = parse_structured_tool_calls(&resp.tool_calls);
                        let mut parsed_text = String::new();

                        if calls.is_empty() {
                            let (fallback_text, fallback_calls) = parse_tool_calls(&response_text);
                            if !fallback_text.is_empty() {
                                parsed_text = fallback_text;
                            }
                            calls = fallback_calls;
                        }

                        // Preserve native tool call IDs in assistant history so role=tool
                        // follow-up messages can reference the exact call id.
                        let assistant_history_content = if resp.tool_calls.is_empty() {
                            response_text.clone()
                        } else {
                            build_native_assistant_history(&response_text, &resp.tool_calls)
                        };

                        let native_calls = resp.tool_calls;
                        (
                            response_text,
                            parsed_text,
                            calls,
                            assistant_history_content,
                            native_calls,
                        )
                    }
                }
                Err(e) => {
                    observer.record_event(&ObserverEvent::LlmResponse {
                        provider: provider_name.to_string(),
                        model: model.to_string(),
                        duration: llm_started_at.elapsed(),
                        success: false,
                        error_message: Some(crate::providers::sanitize_api_error(&e.to_string())),
                    });
                    #[cfg(feature = "ai-protocol")]
                    if let Some(ctx) = soft_fail {
                        let host = ctx
                            .config
                            .map(crate::orchestration::HostDecideHost::from_config);
                        return Err(crate::orchestration::map_provider_limit_error(
                            e,
                            model,
                            ctx.surface,
                            host.as_ref(),
                            ctx.session_key,
                        ));
                    }
                    return Err(e);
                }
            };

        let display_text = if parsed_text.is_empty() {
            response_text.clone()
        } else {
            parsed_text
        };

        if tool_calls.is_empty() {
            // VL-TTC-016: typed leakage → recovery ladder.
            let mut strip_fail_closed = false;
            if velaclaw_agent_runtime::needs_tool_format_correction(&response_text, 0) {
                let strategy = format_ladder.next_strategy();
                tracing::warn!(
                    target: "velaclaw::agent",
                    tool_format_strategy = strategy.as_str(),
                    "tool_format_recovery: unparsed tool markup"
                );
                if strategy != velaclaw_agent_runtime::ToolFormatRecoveryStrategy::StripFailClosed {
                    history.push(ChatMessage::assistant(response_text.clone()));
                    history.push(ChatMessage::user(
                        velaclaw_agent_runtime::tool_format_recovery_message(strategy).to_string(),
                    ));
                    continue;
                }
                tracing::warn!(
                    target: "velaclaw::agent",
                    tool_format_strategy = "StripFailClosed",
                    "tool_format_retry_exhausted: stripping markup after recovery ladder"
                );
                strip_fail_closed = true;
            }
            // No tool calls — this is the final response.
            // If a streaming sender is provided, relay the text in small chunks
            // so the channel can progressively update the draft message.
            if let Some(ref tx) = on_delta {
                // Split on whitespace boundaries, accumulating chunks of at least
                // STREAM_CHUNK_MIN_CHARS characters for progressive draft updates.
                let mut chunk = String::new();
                for word in display_text.split_inclusive(char::is_whitespace) {
                    if cancellation_token
                        .as_ref()
                        .is_some_and(CancellationToken::is_cancelled)
                    {
                        return Err(ToolLoopCancelled.into());
                    }
                    chunk.push_str(word);
                    if chunk.len() >= STREAM_CHUNK_MIN_CHARS
                        && tx.send(std::mem::take(&mut chunk)).await.is_err()
                    {
                        break; // receiver dropped
                    }
                }
                if !chunk.is_empty() {
                    let _ = tx.send(chunk).await;
                }
            }
            history.push(ChatMessage::assistant(response_text.clone()));
            let mut final_text = crate::util::strip_tool_call_markup(&display_text);
            if strip_fail_closed {
                let surface = soft_fail
                    .map(|c| c.surface)
                    .unwrap_or(velaclaw_agent_runtime::SoftFailSurface::Cli);
                let session_key = soft_fail.map(|c| c.session_key).unwrap_or("");
                #[cfg(feature = "ai-protocol")]
                {
                    let host = soft_fail
                        .and_then(|c| c.config)
                        .map(crate::orchestration::HostDecideHost::from_config);
                    final_text = crate::orchestration::finalize_tool_format_exhausted(
                        &final_text,
                        model,
                        surface,
                        host.as_ref(),
                        session_key,
                    );
                }
                #[cfg(not(feature = "ai-protocol"))]
                {
                    final_text = velaclaw_agent_runtime::append_tool_format_exhausted_notice(
                        &final_text,
                        model,
                        surface,
                    );
                }
            }
            return Ok(final_text);
        }

        // Print any text the LLM produced alongside tool calls (unless silent)
        let visible_text = crate::util::strip_tool_call_markup(&display_text);
        if !silent && !visible_text.is_empty() {
            let rendered = render_opts.render(&visible_text);
            let prefixed = prefix_agent_lines(&rendered, render_opts.style);
            print!("{prefixed}");
            let _ = std::io::stdout().flush();
        }

        // Execute tool calls and build results. `individual_results` tracks per-call output so
        // native-mode history can emit one role=tool message per tool call with the correct ID.
        //
        // When multiple tool calls are present and interactive CLI approval is not needed, run
        // tool executions concurrently for lower wall-clock latency.
        let mut tool_results = String::new();
        let batch_results = tool_batch::execute_tool_batch(
            &tool_calls,
            tools_registry,
            observer,
            approval,
            security,
            channel_name,
            channel_approval.clone(),
            cancellation_token.as_ref(),
            gate_extras,
        )
        .await?;
        let individual_results: Vec<String> = batch_results.into_iter().map(|r| r.output).collect();

        for (call, result) in tool_calls.iter().zip(individual_results.iter()) {
            let _ = writeln!(
                tool_results,
                "<tool_result name=\"{}\">\n{}\n</tool_result>",
                call.name, result
            );
        }

        if !silent {
            for (call, result) in tool_calls.iter().zip(individual_results.iter()) {
                print_tool_result_block(&call.name, result, render_opts, fold_cache);
            }
            let _ = std::io::stdout().flush();
        }

        // Add assistant message with tool calls + tool results to history.
        // Native mode: use JSON-structured messages so convert_messages() can
        // reconstruct proper OpenAI-format tool_calls and tool result messages.
        // Prompt mode: use XML-based text format as before.
        history.push(ChatMessage::assistant(assistant_history_content));
        if native_tool_calls.is_empty() || text_tool_result_history {
            history.push(ChatMessage::user(format!("[Tool results]\n{tool_results}")));
        } else {
            for (native_call, result) in native_tool_calls.iter().zip(individual_results.iter()) {
                history.push(ChatMessage::tool_with_call_id(&native_call.id, result));
            }
        }
    }

    anyhow::bail!("Agent exceeded maximum tool iterations ({max_iterations})")
}

/// Surface configured autonomy/shell/path policy in the system prompt.
pub(crate) fn append_execution_policy_to_prompt(
    system_prompt: &mut String,
    security: &PolicyHandle,
    config: &Config,
) {
    let http = config
        .http_request
        .effective_for_autonomy(security.autonomy());
    let (self_adjust_allowed_writes, self_adjust_denied_writes) = self_adjust_prompt_fields(config);
    let extras = crate::security::PolicyPromptExtras {
        http_request_enabled: http.enabled,
        proxy_enabled: config.proxy.enabled,
        proxy_http: if config.proxy.enabled {
            config.proxy.http_proxy.clone()
        } else {
            None
        },
        self_adjust_allowed_writes,
        self_adjust_denied_writes,
        policy_patch_enabled: cfg!(feature = "ai-protocol"),
    };
    security.append_execution_policy_prompt(system_prompt, &extras);
    if http.enabled && http.allow_private_hosts {
        system_prompt.push_str(
            "- HTTP LAN access: enabled for private/local hosts when `autonomy.level = full`.\n\n",
        );
    }
}

fn self_adjust_prompt_fields(config: &Config) -> (Vec<String>, Vec<String>) {
    #[cfg(feature = "ai-protocol")]
    {
        match crate::config::discover_and_load(config) {
            Ok(Some(layer)) => {
                if let Some(section) = layer.self_adjust {
                    return (section.allowed_writes, section.denied_writes);
                }
                (
                    vec![
                        "approval.session_allowlist".into(),
                        "approval.session_shell_binaries".into(),
                        "approval.*".into(),
                    ],
                    vec![
                        "security".into(),
                        "security.*".into(),
                        "gateway".into(),
                        "gateway.*".into(),
                        "channels".into(),
                        "channels.*".into(),
                    ],
                )
            }
            Ok(None) | Err(_) => (Vec::new(), Vec::new()),
        }
    }
    #[cfg(not(feature = "ai-protocol"))]
    {
        let _ = config;
        (Vec::new(), Vec::new())
    }
}

// ── CLI Entrypoint ───────────────────────────────────────────────────────
// Wires up all subsystems (observer, runtime, security, memory, tools,
// provider, hardware RAG, peripherals) and enters either single-shot or
// interactive REPL mode. The interactive loop manages history compaction
// and hard trimming to keep the context window bounded.

/// Shared turn-model ladder for CLI (same as Web `Agent::turn`).
#[cfg(feature = "ai-protocol")]
fn resolve_cli_turn_model(
    config: &Config,
    user_message: &str,
    session_key: &str,
    default_model: &str,
    explicit_model: Option<&str>,
    available_hints: &[String],
) -> Result<String> {
    let host_decide = crate::orchestration::HostDecideHost::from_config(config);
    let intent_route = crate::agent::intent_route::IntentRouteHost::from_config(config);
    let req = crate::orchestration::TurnModelRequest {
        user_message,
        session_key,
        default_model,
        explicit_model,
        host_decide: Some(&host_decide),
        intent_route: Some(&intent_route),
        classification: &config.query_classification,
        available_hints,
    };
    Ok(crate::orchestration::resolve_turn_model(&req)?.model)
}

#[cfg(not(feature = "ai-protocol"))]
fn resolve_cli_turn_model(
    config: &Config,
    user_message: &str,
    _session_key: &str,
    default_model: &str,
    _explicit_model: Option<&str>,
    available_hints: &[String],
) -> Result<String> {
    Ok(crate::agent::classifier::resolve_model_for_message(
        &config.query_classification,
        available_hints,
        default_model,
        user_message,
    ))
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    mut config: Config,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
    peripheral_overrides: Vec<String>,
    no_color: bool,
    no_fold: bool,
    extra_prompt_phases: &[crate::agent::prompt_composer::PromptPhase],
) -> Result<String> {
    // CLI `-p/--model` must win over config for both protocol and legacy paths.
    // (Previously the ai-protocol branch discarded these and always used config.)
    let cli_explicit_flags = provider_override
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
        || model_override
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
    if let Some(provider) = provider_override {
        let provider = provider.trim();
        if !provider.is_empty() {
            config.default_provider = Some(provider.to_string());
        }
    }
    if let Some(model) = model_override {
        let model = model.trim();
        if !model.is_empty() {
            config.default_model = Some(model.to_string());
        }
    }

    let interactive = message.is_none();
    let render_opts =
        RenderOpts::from_config(config.cli_render.as_ref(), no_color, no_fold, interactive);
    let fold_cache: FoldCache = Arc::new(Mutex::new(HashMap::new()));

    // ── Wire up agnostic subsystems ──────────────────────────────
    let base_observer = observability::create_observer(&config.observability);
    let observer: Arc<dyn Observer> = Arc::from(base_observer);
    let runtime: Arc<dyn runtime::RuntimeAdapter> =
        Arc::from(runtime::create_runtime(&config.runtime)?);
    let security = PolicyHandle::from_workspace_config(&config)?;

    // ── Memory (the brain) ────────────────────────────────────────
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory_with_storage(
        &config.memory,
        Some(&config.storage.provider.config),
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);
    tracing::info!(backend = mem.name(), "Memory initialized");

    // ── Peripherals (merge peripheral tools into registry) ─
    if !peripheral_overrides.is_empty() {
        tracing::info!(
            peripherals = ?peripheral_overrides,
            "Peripheral overrides from CLI (config boards take precedence)"
        );
    }

    // ── Tools (including memory tools and peripherals) ────────────
    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };
    let (mut tools_registry, _human_input_attach) = tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        mem.clone(),
        composio_key,
        composio_entity_id,
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        &config,
    );

    let peripheral_tools: Vec<Box<dyn Tool>> =
        crate::peripherals::create_peripheral_tools(&config.peripherals).await?;
    if !peripheral_tools.is_empty() {
        tracing::info!(count = peripheral_tools.len(), "Peripheral tools added");
        tools_registry.extend(peripheral_tools);
    }

    // ── Resolve provider ─────────────────────────────────────────
    let provider_runtime_options = providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        velaclaw_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
    };

    #[cfg(feature = "ai-protocol")]
    let (provider, model_name, tool_dispatcher, text_tool_result_history) = {
        let (exec_handle, provider) =
            crate::execution::bootstrap_routed_provider(&config, &provider_runtime_options)?;
        let model_name = exec_handle.logical_model_id().to_string();
        let tool_calling_policy = exec_handle.tool_calling_policy();
        let text_tool_result_history =
            tool_calling_policy.native_strategy == ai_lib_rust::NativeStrategy::Hybrid;
        let workspace_policy = crate::config::discover_and_load(&config)
            .with_context(|| "load workspace agent-policy.yaml")?;
        let workspace_dispatcher = workspace_policy.as_ref().and_then(|p| p.tool_dispatcher());
        let effective = crate::config::EffectivePolicy::resolve(
            config.agent.tool_dispatcher.as_str(),
            workspace_dispatcher,
            None,
            tool_calling_policy,
        );
        let tool_dispatcher = Some(effective.build_dispatcher(provider.as_ref()));
        (
            provider,
            model_name,
            tool_dispatcher,
            text_tool_result_history,
        )
    };

    #[cfg(not(feature = "ai-protocol"))]
    let (provider, model_name, tool_dispatcher, text_tool_result_history) = {
        let provider_name = config
            .default_provider
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID);
        let model_name = config
            .default_model
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID)
            .to_string();
        let provider = providers::create_routed_provider_with_options(
            provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &provider_runtime_options,
            None,
        )?;
        (provider, model_name, None, false)
    };

    let provider_name = model_name
        .split_once('/')
        .map_or(model_name.as_str(), |(provider, _)| provider);

    let available_hints: Vec<String> = config
        .model_routes
        .iter()
        .map(|route| route.hint.clone())
        .collect();

    observer.record_event(&ObserverEvent::AgentStart {
        provider: model_name
            .split_once('/')
            .map_or(model_name.as_str(), |(provider, _)| provider)
            .to_string(),
        model: model_name.to_string(),
    });

    // ── Hardware RAG (datasheet retrieval when peripherals + datasheet_dir) ──
    let hardware_rag: Option<crate::rag::HardwareRag> = config
        .peripherals
        .datasheet_dir
        .as_ref()
        .filter(|d| !d.trim().is_empty())
        .map(|dir| crate::rag::HardwareRag::load(&config.workspace_dir, dir.trim()))
        .and_then(Result::ok)
        .filter(|r: &crate::rag::HardwareRag| !r.is_empty());
    if let Some(ref rag) = hardware_rag {
        tracing::info!(chunks = rag.len(), "Hardware RAG loaded");
    }

    let board_names: Vec<String> = config
        .peripherals
        .boards
        .iter()
        .map(|b| b.board.clone())
        .collect();

    // ── Build system prompt from workspace MD files (OpenClaw framework) ──
    let skills = crate::skills::load_skills_with_config(&config.workspace_dir, &config);
    let mut tool_descs: Vec<(&str, &str)> = vec![
        (
            "shell",
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
        ),
        (
            "file_read",
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
        ),
        (
            "file_write",
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
        ),
        (
            "memory_store",
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
        ),
        (
            "memory_recall",
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
        ),
        (
            "memory_forget",
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
        ),
    ];
    tool_descs.push((
        "cron_add",
        "Create a cron job. Supports schedule kinds: cron, at, every; and job types: shell or agent.",
    ));
    tool_descs.push((
        "cron_list",
        "List all cron jobs with schedule, status, and metadata.",
    ));
    tool_descs.push(("cron_remove", "Remove a cron job by job_id."));
    tool_descs.push((
        "cron_update",
        "Patch a cron job (schedule, enabled, command/prompt, model, delivery, session_target).",
    ));
    tool_descs.push((
        "cron_run",
        "Force-run a cron job immediately and record a run history entry.",
    ));
    tool_descs.push(("cron_runs", "Show recent run history for a cron job."));
    tool_descs.push((
        "screenshot",
        "Capture a screenshot of the current screen. Returns file path and base64-encoded PNG. Use when: visual verification, UI inspection, debugging displays.",
    ));
    tool_descs.push((
        "image_info",
        "Read image file metadata (format, dimensions, size) and optionally base64-encode it. Use when: inspecting images, preparing visual data for analysis.",
    ));
    if config.browser.enabled {
        tool_descs.push((
            "browser_open",
            "Open approved HTTPS URLs in Brave Browser (allowlist-only, no scraping)",
        ));
    }
    if config.composio.enabled {
        tool_descs.push((
            "composio",
            "Execute actions on 1000+ apps via Composio (Gmail, Notion, GitHub, Slack, etc.). Use action='list' to discover, 'execute' to run (optionally with connected_account_id), 'connect' to OAuth.",
        ));
    }
    tool_descs.push((
        "schedule",
        "Manage scheduled tasks (create/list/get/cancel/pause/resume). Supports recurring cron and one-shot delays.",
    ));
    if !config.agents.is_empty() {
        tool_descs.push((
            "delegate",
            "Delegate a sub-task to a specialized agent. Use when: task needs different model/capability, or to parallelize work.",
        ));
    }
    if config.peripherals.enabled && !config.peripherals.boards.is_empty() {
        tool_descs.push((
            "gpio_read",
            "Read GPIO pin value (0 or 1) on connected hardware (STM32, Arduino). Use when: checking sensor/button state, LED status.",
        ));
        tool_descs.push((
            "gpio_write",
            "Set GPIO pin high (1) or low (0) on connected hardware. Use when: turning LED on/off, controlling actuators.",
        ));
        tool_descs.push((
            "arduino_upload",
            "Upload agent-generated Arduino sketch. Use when: user asks for 'make a heart', 'blink pattern', or custom LED behavior on Arduino. You write the full .ino code; VelaClaw compiles and uploads it. Pin 13 = built-in LED on Uno.",
        ));
        tool_descs.push((
            "hardware_memory_map",
            "Return flash and RAM address ranges for connected hardware. Use when: user asks for 'upper and lower memory addresses', 'memory map', or 'readable addresses'.",
        ));
        tool_descs.push((
            "hardware_board_info",
            "Return full board info (chip, architecture, memory map) for connected hardware. Use when: user asks for 'board info', 'what board do I have', 'connected hardware', 'chip info', or 'what hardware'.",
        ));
        tool_descs.push((
            "hardware_memory_read",
            "Read actual memory/register values from Nucleo via USB. Use when: user asks to 'read register values', 'read memory', 'dump lower memory 0-126', 'give address and value'. Params: address (hex, default 0x20000000), length (bytes, default 128).",
        ));
        tool_descs.push((
            "hardware_capabilities",
            "Query connected hardware for reported GPIO pins and LED pin. Use when: user asks what pins are available.",
        ));
    }
    let bootstrap_max_chars = if config.agent.compact_context {
        Some(6000)
    } else {
        None
    };
    let prompt_budget = crate::agent::prompt_composer::system_prompt_char_budget(
        config.agent.compact_context,
        &model_name,
    );
    let native_tools = provider.supports_native_tools();
    let mut system_prompt = crate::channels::build_system_prompt_pyramid(
        &config.workspace_dir,
        &model_name,
        &tool_descs,
        &skills,
        Some(&config.identity),
        bootstrap_max_chars,
        native_tools,
        config.skills.prompt_injection_mode,
        crate::agent::prompt_composer::PromptMode::Full,
        prompt_budget,
    );

    // Append structured tool-use instructions (Hybrid / xml mode; native-only Full skips).
    #[cfg(feature = "ai-protocol")]
    {
        if let Some(ref dispatcher) = tool_dispatcher {
            let strategy = if text_tool_result_history {
                ai_lib_rust::NativeStrategy::Hybrid
            } else if dispatcher.should_send_tool_specs() {
                ai_lib_rust::NativeStrategy::Full
            } else {
                ai_lib_rust::NativeStrategy::TextOnly
            };
            append_text_tool_prompt(
                &mut system_prompt,
                dispatcher.as_ref(),
                &tools_registry,
                strategy,
            );
        } else if !native_tools {
            system_prompt.push_str(&build_tool_instructions(&tools_registry));
        }
    }
    #[cfg(not(feature = "ai-protocol"))]
    {
        if !native_tools {
            system_prompt.push_str(&build_tool_instructions(&tools_registry));
        }
    }
    append_execution_policy_to_prompt(&mut system_prompt, &security, &config);
    crate::agent::prompt_composer::append_phase_sections(
        &mut system_prompt,
        &crate::agent::prompt_composer::default_run_prompt_phases(extra_prompt_phases),
    );

    let tool_dispatcher_ref = tool_dispatcher.as_deref();

    // ── Approval manager (supervised mode) ───────────────────────
    let effective_autonomy = crate::config::resolve_effective_autonomy(&config)?;
    let approval_wiring = crate::config::ApprovalManagerWiring::from_config(&config)?;
    let approval_manager = approval_wiring.spawn_manager(&effective_autonomy);

    // ── Execute ──────────────────────────────────────────────────
    let start = Instant::now();

    let mut final_output = String::new();

    if let Some(msg) = message {
        // One-shot also gets an isolated session so legacy Conversation rows
        // (session_id=None) do not bleed into -m turns.
        let session_id = memory::new_session_id();

        // Auto-save user message to memory (skip short/trivial messages)
        if config.memory.auto_save && msg.chars().count() >= AUTOSAVE_MIN_MESSAGE_CHARS {
            let user_key = autosave_memory_key("user_msg");
            let _ = mem
                .store(
                    &user_key,
                    &msg,
                    MemoryCategory::Conversation,
                    Some(session_id.as_str()),
                )
                .await;
        }

        // Inject memory + hardware RAG context into user message
        let mem_context = build_context(
            mem.as_ref(),
            &msg,
            config.memory.min_relevance_score,
            Some(session_id.as_str()),
        )
        .await;
        let rag_limit = if config.agent.compact_context { 2 } else { 5 };
        let hw_context = hardware_rag
            .as_ref()
            .map(|r| build_hardware_context(r, &msg, &board_names, rag_limit))
            .unwrap_or_default();
        let context = format!("{mem_context}{hw_context}");
        let enriched = if context.is_empty() {
            msg.clone()
        } else {
            format!("{context}{msg}")
        };

        let mut history = vec![
            ChatMessage::system(&system_prompt),
            ChatMessage::user(&enriched),
        ];

        let summarizer = crate::agent::context_orch::HistorySummarizer {
            provider: provider.as_ref(),
            model: &model_name,
        };
        crate::agent::context_orch::prepare_turn_history(
            &mut history,
            crate::agent::context_orch::PrepareHistoryOpts {
                layered: config.agent.envelope_assemble,
                compact_context: config.agent.compact_context,
                async_pool: config.agent.envelope_assemble_async,
                max_history: config.agent.max_history_messages,
                summarizer: Some(&summarizer),
            },
        )
        .await?;

        let turn_model = resolve_cli_turn_model(
            &config,
            &msg,
            session_id.as_str(),
            &model_name,
            if cli_explicit_flags {
                Some(model_name.as_str())
            } else {
                None
            },
            &available_hints,
        )?;

        let response = run_tool_call_loop(
            provider.as_ref(),
            &mut history,
            &tools_registry,
            observer.as_ref(),
            provider_name,
            &turn_model,
            temperature,
            false,
            Some(&approval_manager),
            "cli",
            &config.multimodal,
            config.agent.max_tool_iterations,
            None,
            None,
            tool_dispatcher_ref,
            Some(&security),
            None,
            text_tool_result_history,
            render_opts,
            None,
            Some(SoftFailLoopCtx {
                session_key: session_id.as_str(),
                config: Some(&config),
                surface: velaclaw_agent_runtime::SoftFailSurface::Cli,
            }),
            None,
        )
        .await?;
        final_output = response.clone();
        let rendered = render_opts.render(&response);
        println!("{}", prefix_agent_lines(&rendered, render_opts.style));
        observer.record_event(&ObserverEvent::TurnComplete);
    } else {
        println!("🦀 VelaClaw Interactive Mode");
        println!("Type /help for commands.\n");
        let cli = crate::channels::CliChannel::with_render_opts(render_opts);

        // Persistent conversation history across turns
        let mut history = vec![ChatMessage::system(&system_prompt)];
        let mut session_model = model_name.clone();
        let session_provider = provider_name.to_string();
        // VL-MEM-001: default new session unless user later resumes (no resume UI yet).
        let mut session_id = memory::new_session_id();
        let mut session_explicit = cli_explicit_flags;

        loop {
            print!("{}", format_user_prompt(render_opts.style));
            let _ = std::io::stdout().flush();

            let mut input = String::new();
            match std::io::stdin().read_line(&mut input) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => {
                    eprintln!("\nError reading input: {e}\n");
                    break;
                }
            }

            let user_input = input.trim().to_string();
            if user_input.is_empty() {
                continue;
            }
            match user_input.as_str() {
                "/quit" | "/exit" => break,
                "/help" => {
                    println!("Available commands:");
                    println!("  /help        Show this help message");
                    println!("  /version     Show VelaClaw version");
                    println!("  /models      List providers (or `/models <provider>`)");
                    println!("  /model       Show/set model for this session");
                    println!("  /expand <id> Replay a folded long output by id");
                    println!("  /clear /new  Start a new session (clear this session's memory)");
                    println!("  /quit /exit  Exit interactive mode\n");
                    continue;
                }
                "/version" => {
                    println!(
                        "VelaClaw {} (provider: {}, model: {})\n",
                        env!("CARGO_PKG_VERSION"),
                        session_provider,
                        session_model
                    );
                    continue;
                }
                cmd if cmd.starts_with("/expand") => {
                    let id_str = cmd.strip_prefix("/expand").unwrap_or("").trim();
                    if id_str.is_empty() {
                        println!("Usage: /expand <id>\n");
                        continue;
                    }
                    match id_str.parse::<u64>() {
                        Ok(id) => {
                            let payload = {
                                let guard = fold_cache.lock().unwrap_or_else(|e| e.into_inner());
                                guard.get(&id).cloned()
                            };
                            match payload {
                                Some(text) => {
                                    // Replay raw stored payload without re-rendering.
                                    println!("{text}\n");
                                }
                                None => {
                                    println!("No folded output with id {id}.\n");
                                }
                            }
                        }
                        Err(_) => {
                            println!("Usage: /expand <id>  (id must be a number)\n");
                        }
                    }
                    continue;
                }
                "/clear" | "/new" => {
                    println!(
                        "This will clear the current conversation and delete this session's memory."
                    );
                    println!("Core memories (long-term facts/preferences) will be preserved.");
                    print!("Continue? [y/N] ");
                    let _ = std::io::stdout().flush();

                    let mut confirm = String::new();
                    if std::io::stdin().read_line(&mut confirm).is_err() {
                        continue;
                    }
                    if !matches!(confirm.trim().to_lowercase().as_str(), "y" | "yes") {
                        println!("Cancelled.\n");
                        continue;
                    }

                    history.clear();
                    history.push(ChatMessage::system(&system_prompt));
                    // Clear Conversation/Daily for the *current* session only.
                    let mut cleared = 0;
                    for category in [MemoryCategory::Conversation, MemoryCategory::Daily] {
                        let entries = mem
                            .list(Some(&category), Some(session_id.as_str()))
                            .await
                            .unwrap_or_default();
                        for entry in entries {
                            if mem.forget(&entry.key).await.unwrap_or(false) {
                                cleared += 1;
                            }
                        }
                    }
                    session_id = memory::new_session_id();
                    if cleared > 0 {
                        println!(
                            "Conversation cleared ({cleared} memory entries removed); new session started.\n"
                        );
                    } else {
                        println!("Conversation cleared; new session started.\n");
                    }
                    continue;
                }
                _ => {}
            }

            if let Some((response, new_model)) = crate::channels::handle_cli_runtime_slash_command(
                &user_input,
                &config,
                &session_provider,
                &session_model,
            ) {
                println!("{response}\n");
                if let Some(model) = new_model {
                    session_model = model;
                    session_explicit = true;
                }
                continue;
            }

            // Auto-save conversation turns (skip short/trivial messages)
            if config.memory.auto_save && user_input.chars().count() >= AUTOSAVE_MIN_MESSAGE_CHARS {
                let user_key = autosave_memory_key("user_msg");
                let _ = mem
                    .store(
                        &user_key,
                        &user_input,
                        MemoryCategory::Conversation,
                        Some(session_id.as_str()),
                    )
                    .await;
            }

            // Inject memory + hardware RAG context into user message
            let mem_context = build_context(
                mem.as_ref(),
                &user_input,
                config.memory.min_relevance_score,
                Some(session_id.as_str()),
            )
            .await;
            let rag_limit = if config.agent.compact_context { 2 } else { 5 };
            let hw_context = hardware_rag
                .as_ref()
                .map(|r| build_hardware_context(r, &user_input, &board_names, rag_limit))
                .unwrap_or_default();
            let context = format!("{mem_context}{hw_context}");
            let enriched = if context.is_empty() {
                user_input.clone()
            } else {
                format!("{context}{user_input}")
            };

            history.push(ChatMessage::user(&enriched));

            let summarizer = crate::agent::context_orch::HistorySummarizer {
                provider: provider.as_ref(),
                model: &session_model,
            };
            let prepare_report = crate::agent::context_orch::prepare_turn_history(
                &mut history,
                crate::agent::context_orch::PrepareHistoryOpts {
                    layered: config.agent.envelope_assemble,
                    compact_context: config.agent.compact_context,
                    async_pool: config.agent.envelope_assemble_async,
                    max_history: config.agent.max_history_messages,
                    summarizer: Some(&summarizer),
                },
            )
            .await?;
            if prepare_report.compacted {
                println!("🧹 Auto-compaction complete");
            }

            let turn_model = resolve_cli_turn_model(
                &config,
                &user_input,
                session_id.as_str(),
                &session_model,
                if session_explicit {
                    Some(session_model.as_str())
                } else {
                    None
                },
                &available_hints,
            )?;

            let response = match run_tool_call_loop(
                provider.as_ref(),
                &mut history,
                &tools_registry,
                observer.as_ref(),
                provider_name,
                &turn_model,
                temperature,
                false,
                Some(&approval_manager),
                "cli",
                &config.multimodal,
                config.agent.max_tool_iterations,
                None,
                None,
                tool_dispatcher_ref,
                Some(&security),
                None,
                text_tool_result_history,
                render_opts,
                Some(&fold_cache),
                Some(SoftFailLoopCtx {
                    session_key: session_id.as_str(),
                    config: Some(&config),
                    surface: velaclaw_agent_runtime::SoftFailSurface::Cli,
                }),
                None,
            )
            .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    eprintln!("\nError: {e}\n");
                    continue;
                }
            };
            final_output = response.clone();
            let visible_response = crate::util::strip_tool_call_markup(&response);
            if let Err(e) = crate::channels::Channel::send(
                &cli,
                &crate::channels::traits::SendMessage::new(
                    format!("\n{visible_response}\n"),
                    "user",
                ),
            )
            .await
            {
                eprintln!("\nError sending CLI response: {e}\n");
            }
            observer.record_event(&ObserverEvent::TurnComplete);

            // Post-turn prepare: compact overflow + layered (or trim kill-switch).
            let summarizer = crate::agent::context_orch::HistorySummarizer {
                provider: provider.as_ref(),
                model: &session_model,
            };
            if let Ok(report) = crate::agent::context_orch::prepare_turn_history(
                &mut history,
                crate::agent::context_orch::PrepareHistoryOpts {
                    layered: config.agent.envelope_assemble,
                    compact_context: config.agent.compact_context,
                    async_pool: config.agent.envelope_assemble_async,
                    max_history: config.agent.max_history_messages,
                    summarizer: Some(&summarizer),
                },
            )
            .await
            {
                if report.compacted {
                    println!("🧹 Auto-compaction complete");
                }
            }
        }
    }

    let duration = start.elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
        duration,
        tokens_used: None,
        cost_usd: None,
    });

    Ok(final_output)
}

/// Process a single message through the full agent (with tools, peripherals, memory).
/// Used by channels (Telegram, Discord, etc.) to enable hardware and tool use.
pub async fn process_message(config: Config, message: &str) -> Result<String> {
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let runtime: Arc<dyn runtime::RuntimeAdapter> =
        Arc::from(runtime::create_runtime(&config.runtime)?);
    let security = PolicyHandle::from_workspace_config(&config)?;
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory_with_storage(
        &config.memory,
        Some(&config.storage.provider.config),
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);

    let (composio_key, composio_entity_id) = if config.composio.enabled {
        (
            config.composio.api_key.as_deref(),
            Some(config.composio.entity_id.as_str()),
        )
    } else {
        (None, None)
    };
    let (mut tools_registry, _human_input_attach) = tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        mem.clone(),
        composio_key,
        composio_entity_id,
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &config.agents,
        config.api_key.as_deref(),
        &config,
    );
    let peripheral_tools: Vec<Box<dyn Tool>> =
        crate::peripherals::create_peripheral_tools(&config.peripherals).await?;
    tools_registry.extend(peripheral_tools);

    let provider_runtime_options = providers::ProviderRuntimeOptions {
        auth_profile_override: None,
        velaclaw_dir: config.config_path.parent().map(std::path::PathBuf::from),
        secrets_encrypt: config.secrets.encrypt,
        reasoning_enabled: config.runtime.reasoning_enabled,
    };
    #[cfg(feature = "ai-protocol")]
    let (provider, model_name) = {
        let (exec_handle, provider) =
            crate::execution::bootstrap_routed_provider(&config, &provider_runtime_options)?;
        let model_name = exec_handle.logical_model_id().to_string();
        (provider, model_name)
    };
    #[cfg(not(feature = "ai-protocol"))]
    let (provider, model_name) = {
        let provider_name = config
            .default_provider
            .as_deref()
            .unwrap_or(DEFAULT_PROTOCOL_MODEL_ID);
        let model_name = config
            .default_model
            .clone()
            .unwrap_or_else(|| "anthropic/claude-sonnet-4-20250514".into());
        let provider = providers::create_routed_provider_with_options(
            provider_name,
            config.api_key.as_deref(),
            config.api_url.as_deref(),
            &config.reliability,
            &config.model_routes,
            &model_name,
            &provider_runtime_options,
            None,
        )?;
        (provider, model_name)
    };
    let provider: Box<dyn Provider> = provider;
    let provider_name = model_name
        .split_once('/')
        .map_or(model_name.as_str(), |(provider, _)| provider);
    let available_hints: Vec<String> = config
        .model_routes
        .iter()
        .map(|route| route.hint.clone())
        .collect();

    let hardware_rag: Option<crate::rag::HardwareRag> = config
        .peripherals
        .datasheet_dir
        .as_ref()
        .filter(|d| !d.trim().is_empty())
        .map(|dir| crate::rag::HardwareRag::load(&config.workspace_dir, dir.trim()))
        .and_then(Result::ok)
        .filter(|r: &crate::rag::HardwareRag| !r.is_empty());
    let board_names: Vec<String> = config
        .peripherals
        .boards
        .iter()
        .map(|b| b.board.clone())
        .collect();

    let skills = crate::skills::load_skills_with_config(&config.workspace_dir, &config);
    let mut tool_descs: Vec<(&str, &str)> = vec![
        ("shell", "Execute terminal commands."),
        ("file_read", "Read file contents."),
        ("file_write", "Write file contents."),
        ("memory_store", "Save to memory."),
        ("memory_recall", "Search memory."),
        ("memory_forget", "Delete a memory entry."),
        ("screenshot", "Capture a screenshot."),
        ("image_info", "Read image metadata."),
    ];
    if config.browser.enabled {
        tool_descs.push(("browser_open", "Open approved URLs in browser."));
    }
    if config.composio.enabled {
        tool_descs.push(("composio", "Execute actions on 1000+ apps via Composio."));
    }
    if config.peripherals.enabled && !config.peripherals.boards.is_empty() {
        tool_descs.push(("gpio_read", "Read GPIO pin value on connected hardware."));
        tool_descs.push((
            "gpio_write",
            "Set GPIO pin high or low on connected hardware.",
        ));
        tool_descs.push((
            "arduino_upload",
            "Upload Arduino sketch. Use for 'make a heart', custom patterns. You write full .ino code; VelaClaw uploads it.",
        ));
        tool_descs.push((
            "hardware_memory_map",
            "Return flash and RAM address ranges. Use when user asks for memory addresses or memory map.",
        ));
        tool_descs.push((
            "hardware_board_info",
            "Return full board info (chip, architecture, memory map). Use when user asks for board info, what board, connected hardware, or chip info.",
        ));
        tool_descs.push((
            "hardware_memory_read",
            "Read actual memory/register values from Nucleo. Use when user asks to read registers, read memory, dump lower memory 0-126, or give address and value.",
        ));
        tool_descs.push((
            "hardware_capabilities",
            "Query connected hardware for reported GPIO pins and LED pin. Use when user asks what pins are available.",
        ));
    }
    let bootstrap_max_chars = if config.agent.compact_context {
        Some(6000)
    } else {
        None
    };
    let prompt_budget = crate::agent::prompt_composer::system_prompt_char_budget(
        config.agent.compact_context,
        &model_name,
    );
    let native_tools = provider.supports_native_tools();
    let mut system_prompt = crate::channels::build_system_prompt_pyramid(
        &config.workspace_dir,
        &model_name,
        &tool_descs,
        &skills,
        Some(&config.identity),
        bootstrap_max_chars,
        native_tools,
        config.skills.prompt_injection_mode,
        crate::agent::prompt_composer::PromptMode::Full,
        prompt_budget,
    );
    if !native_tools {
        system_prompt.push_str(&build_tool_instructions(&tools_registry));
    }
    append_execution_policy_to_prompt(&mut system_prompt, &security, &config);
    crate::agent::prompt_composer::append_phase_sections(
        &mut system_prompt,
        &[crate::agent::prompt_composer::PromptPhase::Approval],
    );

    let session_id = memory::new_session_id();
    let mem_context = build_context(
        mem.as_ref(),
        message,
        config.memory.min_relevance_score,
        Some(session_id.as_str()),
    )
    .await;
    let rag_limit = if config.agent.compact_context { 2 } else { 5 };
    let hw_context = hardware_rag
        .as_ref()
        .map(|r| build_hardware_context(r, message, &board_names, rag_limit))
        .unwrap_or_default();
    let context = format!("{mem_context}{hw_context}");
    let enriched = if context.is_empty() {
        message.to_string()
    } else {
        format!("{context}{message}")
    };

    let mut history = vec![
        ChatMessage::system(&system_prompt),
        ChatMessage::user(&enriched),
    ];

    let turn_model = crate::agent::classifier::resolve_model_for_message(
        &config.query_classification,
        &available_hints,
        &model_name,
        message,
    );

    agent_turn(
        provider.as_ref(),
        &mut history,
        &tools_registry,
        observer.as_ref(),
        provider_name,
        &turn_model,
        config.default_temperature,
        true,
        &config.multimodal,
        config.agent.max_tool_iterations,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use velaclaw_agent_runtime::loop_parse::{
        apply_compaction_summary, build_compaction_transcript, trim_history,
    };
    use velaclaw_agent_runtime::loop_parse::{
        tools_to_openai_format, DEFAULT_MAX_HISTORY_MESSAGES,
    };

    #[test]
    fn test_scrub_credentials() {
        let input = "API_KEY=sk-1234567890abcdef; token: 1234567890; password=\"secret123456\"";
        let scrubbed = tool_batch::scrub_credentials(input);
        assert!(scrubbed.contains("API_KEY=sk-1*[REDACTED]"));
        assert!(scrubbed.contains("token: 1234*[REDACTED]"));
        assert!(scrubbed.contains("password=\"secr*[REDACTED]\""));
        assert!(!scrubbed.contains("abcdef"));
        assert!(!scrubbed.contains("secret123456"));
    }

    #[test]
    fn test_scrub_credentials_json() {
        let input = r#"{"api_key": "sk-1234567890", "other": "public"}"#;
        let scrubbed = tool_batch::scrub_credentials(input);
        assert!(scrubbed.contains("\"api_key\": \"sk-1*[REDACTED]\""));
        assert!(scrubbed.contains("public"));
    }
    use crate::memory::{Memory, MemoryCategory, SqliteMemory};
    use crate::observability::NoopObserver;
    use crate::providers::traits::ProviderCapabilities;
    use crate::providers::ChatResponse;
    use tempfile::TempDir;

    struct NonVisionProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for NonVisionProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("ok".to_string())
        }
    }

    struct VisionProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Provider for VisionProvider {
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities {
                native_tool_calling: false,
                vision: true,
            }
        }

        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("ok".to_string())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ChatResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let marker_count = crate::multimodal::count_image_markers(request.messages);
            if marker_count == 0 {
                anyhow::bail!("expected image markers in request messages");
            }

            if request.tools.is_some() {
                anyhow::bail!("no tools should be attached for this test");
            }

            Ok(ChatResponse {
                text: Some("vision-ok".to_string()),
                tool_calls: Vec::new(),
            })
        }
    }

    struct ScriptedProvider {
        responses: Arc<Mutex<VecDeque<ChatResponse>>>,
    }

    impl ScriptedProvider {
        fn from_text_responses(responses: Vec<&str>) -> Self {
            let scripted = responses
                .into_iter()
                .map(|text| ChatResponse {
                    text: Some(text.to_string()),
                    tool_calls: Vec::new(),
                })
                .collect();
            Self {
                responses: Arc::new(Mutex::new(scripted)),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            anyhow::bail!("chat_with_system should not be used in scripted provider tests");
        }

        async fn chat(
            &self,
            _request: ChatRequest<'_>,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ChatResponse> {
            let mut responses = self
                .responses
                .lock()
                .expect("responses lock should be valid");
            responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("scripted provider exhausted responses"))
        }
    }

    struct DelayTool {
        name: String,
        delay_ms: u64,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    impl DelayTool {
        fn new(
            name: &str,
            delay_ms: u64,
            active: Arc<AtomicUsize>,
            max_active: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                name: name.to_string(),
                delay_ms,
                active,
                max_active,
            }
        }
    }

    #[async_trait]
    impl Tool for DelayTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn description(&self) -> &str {
            "Delay tool for testing parallel tool execution"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                },
                "required": ["value"]
            })
        }

        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: &crate::tools::ToolExecutionContext,
        ) -> anyhow::Result<crate::tools::ToolResult> {
            let now_active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(now_active, Ordering::SeqCst);

            tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;

            self.active.fetch_sub(1, Ordering::SeqCst);

            let value = args
                .get("value")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();

            Ok(crate::tools::ToolResult {
                success: true,
                output: format!("ok:{value}"),
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn run_tool_call_loop_returns_structured_error_for_non_vision_provider() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = NonVisionProvider {
            calls: Arc::clone(&calls),
        };

        let mut history = vec![ChatMessage::user(
            "please inspect [IMAGE:data:image/png;base64,iVBORw0KGgo=]".to_string(),
        )];
        let tools_registry: Vec<Box<dyn Tool>> = Vec::new();
        let observer = NoopObserver;

        let err = run_tool_call_loop(
            &provider,
            &mut history,
            &tools_registry,
            &observer,
            "mock-provider",
            "mock-model",
            0.0,
            true,
            None,
            "cli",
            &crate::config::MultimodalConfig::default(),
            3,
            None,
            None,
            None,
            None,
            None,
            false,
            RenderOpts {
                style: RenderStyle {
                    ansi: false,
                    markdown: true,
                },
                fold_lines: 10,
                fold_enabled: false,
            },
            None,
            None,
            None,
        )
        .await
        .expect_err("provider without vision support should fail");

        assert!(err.to_string().contains("provider_capability_error"));
        assert!(err.to_string().contains("capability=vision"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_tool_call_loop_rejects_oversized_image_payload() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = VisionProvider {
            calls: Arc::clone(&calls),
        };

        let oversized_payload = STANDARD.encode(vec![0_u8; (1024 * 1024) + 1]);
        let mut history = vec![ChatMessage::user(format!(
            "[IMAGE:data:image/png;base64,{oversized_payload}]"
        ))];

        let tools_registry: Vec<Box<dyn Tool>> = Vec::new();
        let observer = NoopObserver;
        let multimodal = crate::config::MultimodalConfig {
            max_images: 4,
            max_image_size_mb: 1,
            allow_remote_fetch: false,
        };

        let err = run_tool_call_loop(
            &provider,
            &mut history,
            &tools_registry,
            &observer,
            "mock-provider",
            "mock-model",
            0.0,
            true,
            None,
            "cli",
            &multimodal,
            3,
            None,
            None,
            None,
            None,
            None,
            false,
            RenderOpts {
                style: RenderStyle {
                    ansi: false,
                    markdown: true,
                },
                fold_lines: 10,
                fold_enabled: false,
            },
            None,
            None,
            None,
        )
        .await
        .expect_err("oversized payload must fail");

        assert!(err
            .to_string()
            .contains("multimodal image size limit exceeded"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn run_tool_call_loop_accepts_valid_multimodal_request_flow() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = VisionProvider {
            calls: Arc::clone(&calls),
        };

        let mut history = vec![ChatMessage::user(
            "Analyze this [IMAGE:data:image/png;base64,iVBORw0KGgo=]".to_string(),
        )];
        let tools_registry: Vec<Box<dyn Tool>> = Vec::new();
        let observer = NoopObserver;

        let result = run_tool_call_loop(
            &provider,
            &mut history,
            &tools_registry,
            &observer,
            "mock-provider",
            "mock-model",
            0.0,
            true,
            None,
            "cli",
            &crate::config::MultimodalConfig::default(),
            3,
            None,
            None,
            None,
            None,
            None,
            false,
            RenderOpts {
                style: RenderStyle {
                    ansi: false,
                    markdown: true,
                },
                fold_lines: 10,
                fold_enabled: false,
            },
            None,
            None,
            None,
        )
        .await
        .expect("valid multimodal payload should pass");

        assert_eq!(result, "vision-ok");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn should_execute_tools_in_parallel_returns_false_for_single_call() {
        let calls = vec![ParsedToolCall {
            name: "file_read".to_string(),
            arguments: serde_json::json!({"path": "a.txt"}),
        }];

        assert!(!tool_batch::should_execute_tools_in_parallel(&calls, None));
    }

    #[test]
    fn should_execute_tools_in_parallel_returns_false_when_approval_is_required() {
        use crate::approval::ApprovalGate;

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
        let approval_cfg = crate::config::AutonomyConfig::default();
        let approval_mgr = ApprovalManager::from_config(&approval_cfg);
        let gate = ApprovalGate::new(&approval_mgr, "cli", None);

        assert!(!tool_batch::should_execute_tools_in_parallel(
            &calls,
            Some(&gate)
        ));
    }

    #[test]
    fn should_execute_tools_in_parallel_returns_true_when_cli_has_no_interactive_approvals() {
        use crate::approval::ApprovalGate;

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
        let approval_cfg = crate::config::AutonomyConfig {
            level: crate::security::AutonomyLevel::Full,
            ..crate::config::AutonomyConfig::default()
        };
        let approval_mgr = ApprovalManager::from_config(&approval_cfg);
        let gate = ApprovalGate::new(&approval_mgr, "cli", None);

        assert!(tool_batch::should_execute_tools_in_parallel(
            &calls,
            Some(&gate)
        ));
    }

    #[tokio::test]
    async fn run_tool_call_loop_executes_multiple_tools_in_parallel_with_ordered_results() {
        let provider = ScriptedProvider::from_text_responses(vec![
            r#"<tool_call>
{"name":"delay_a","arguments":{"value":"A"}}
</tool_call>
<tool_call>
{"name":"delay_b","arguments":{"value":"B"}}
</tool_call>"#,
            "done",
        ]);

        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let tools_registry: Vec<Box<dyn Tool>> = vec![
            Box::new(DelayTool::new(
                "delay_a",
                200,
                Arc::clone(&active),
                Arc::clone(&max_active),
            )),
            Box::new(DelayTool::new(
                "delay_b",
                200,
                Arc::clone(&active),
                Arc::clone(&max_active),
            )),
        ];

        let approval_cfg = crate::config::AutonomyConfig {
            level: crate::security::AutonomyLevel::Full,
            ..crate::config::AutonomyConfig::default()
        };
        let approval_mgr = ApprovalManager::from_config(&approval_cfg);

        let mut history = vec![
            ChatMessage::system("test-system"),
            ChatMessage::user("run tool calls"),
        ];
        let observer = NoopObserver;

        let started = std::time::Instant::now();
        let result = run_tool_call_loop(
            &provider,
            &mut history,
            &tools_registry,
            &observer,
            "mock-provider",
            "mock-model",
            0.0,
            true,
            Some(&approval_mgr),
            "telegram",
            &crate::config::MultimodalConfig::default(),
            4,
            None,
            None,
            None,
            None,
            None,
            false,
            RenderOpts {
                style: RenderStyle {
                    ansi: false,
                    markdown: true,
                },
                fold_lines: 10,
                fold_enabled: false,
            },
            None,
            None,
            None,
        )
        .await
        .expect("parallel execution should complete");
        let elapsed = started.elapsed();

        assert_eq!(result, "done");
        assert!(
            elapsed < Duration::from_millis(350),
            "parallel execution should be faster than sequential fallback; elapsed={elapsed:?}"
        );
        assert!(
            max_active.load(Ordering::SeqCst) >= 2,
            "both tools should overlap in execution"
        );

        let tool_results_message = history
            .iter()
            .find(|msg| msg.role == "user" && msg.content.starts_with("[Tool results]"))
            .expect("tool results message should be present");
        let idx_a = tool_results_message
            .content
            .find("name=\"delay_a\"")
            .expect("delay_a result should be present");
        let idx_b = tool_results_message
            .content
            .find("name=\"delay_b\"")
            .expect("delay_b result should be present");
        assert!(
            idx_a < idx_b,
            "tool results should preserve input order for tool call mapping"
        );
    }

    #[tokio::test]
    async fn run_tool_call_loop_retries_once_on_unparsed_tool_markup() {
        let bad = "<tool_call>\nNOT_JSON\n</tool_call>";
        let good = r#"<tool_call>
{"name":"delay_a","arguments":{"value":"fixed"}}
</tool_call>"#;
        let provider = ScriptedProvider::from_text_responses(vec![bad, good, "all good"]);

        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let tools_registry: Vec<Box<dyn Tool>> = vec![Box::new(DelayTool::new(
            "delay_a",
            1,
            Arc::clone(&active),
            Arc::clone(&max_active),
        ))];

        let approval_cfg = crate::config::AutonomyConfig {
            level: crate::security::AutonomyLevel::Full,
            ..crate::config::AutonomyConfig::default()
        };
        let approval_mgr = ApprovalManager::from_config(&approval_cfg);

        let mut history = vec![
            ChatMessage::system("test-system"),
            ChatMessage::user("run tool"),
        ];
        let observer = NoopObserver;

        let result = run_tool_call_loop(
            &provider,
            &mut history,
            &tools_registry,
            &observer,
            "mock-provider",
            "mock-model",
            0.0,
            true,
            Some(&approval_mgr),
            "cli",
            &crate::config::MultimodalConfig::default(),
            6,
            None,
            None,
            None,
            None,
            None,
            false,
            RenderOpts {
                style: RenderStyle {
                    ansi: false,
                    markdown: true,
                },
                fold_lines: 10,
                fold_enabled: false,
            },
            None,
            None,
            None,
        )
        .await
        .expect("corrective retry should recover");

        assert_eq!(result, "all good");
        assert!(history
            .iter()
            .any(|m| m.role == "user" && m.content.contains("invalid format")));
    }

    #[test]
    fn parse_tool_calls_extracts_single_call() {
        let response = r#"Let me check that.
<tool_call>
{"name": "shell", "arguments": {"command": "ls -la"}}
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert_eq!(text, "Let me check that.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "ls -la"
        );
    }

    #[test]
    fn parse_tool_calls_extracts_multiple_calls() {
        let response = r#"<tool_call>
{"name": "file_read", "arguments": {"path": "a.txt"}}
</tool_call>
<tool_call>
{"name": "file_read", "arguments": {"path": "b.txt"}}
</tool_call>"#;

        let (_, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[1].name, "file_read");
    }

    #[test]
    fn parse_tool_calls_returns_text_only_when_no_calls() {
        let response = "Just a normal response with no tools.";
        let (text, calls) = parse_tool_calls(response);
        assert_eq!(text, "Just a normal response with no tools.");
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_malformed_json() {
        let response = r#"<tool_call>
not valid json
</tool_call>
Some text after."#;

        let (text, calls) = parse_tool_calls(response);
        assert!(calls.is_empty());
        assert!(text.contains("Some text after."));
    }

    #[test]
    fn parse_tool_calls_text_before_and_after() {
        let response = r#"Before text.
<tool_call>
{"name": "shell", "arguments": {"command": "echo hi"}}
</tool_call>
After text."#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.contains("Before text."));
        assert!(text.contains("After text."));
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn parse_tool_calls_handles_openai_format() {
        // OpenAI-style response with tool_calls array
        let response = r#"{"content": "Let me check that for you.", "tool_calls": [{"type": "function", "function": {"name": "shell", "arguments": "{\"command\": \"ls -la\"}"}}]}"#;

        let (text, calls) = parse_tool_calls(response);
        assert_eq!(text, "Let me check that for you.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "ls -la"
        );
    }

    #[test]
    fn parse_tool_calls_handles_openai_format_multiple_calls() {
        let response = r#"{"tool_calls": [{"type": "function", "function": {"name": "file_read", "arguments": "{\"path\": \"a.txt\"}"}}, {"type": "function", "function": {"name": "file_read", "arguments": "{\"path\": \"b.txt\"}"}}]}"#;

        let (_, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[1].name, "file_read");
    }

    #[test]
    fn parse_tool_calls_openai_format_without_content() {
        // Some providers don't include content field with tool_calls
        let response = r#"{"tool_calls": [{"type": "function", "function": {"name": "memory_recall", "arguments": "{}"}}]}"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty()); // No content field
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_recall");
    }

    #[test]
    fn parse_tool_calls_handles_markdown_json_inside_tool_call_tag() {
        let response = r#"<tool_call>
```json
{"name": "file_write", "arguments": {"path": "test.py", "content": "print('ok')"}}
```
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_write");
        assert_eq!(
            calls[0].arguments.get("path").unwrap().as_str().unwrap(),
            "test.py"
        );
    }

    #[test]
    fn parse_tool_calls_handles_noisy_tool_call_tag_body() {
        let response = r#"<tool_call>
I will now call the tool with this payload:
{"name": "shell", "arguments": {"command": "pwd"}}
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "pwd"
        );
    }

    #[test]
    fn parse_tool_calls_handles_xml_nested_tool_payload() {
        let response = r#"<tool_call>
<memory_recall>
<query>project roadmap</query>
</memory_recall>
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_recall");
        assert_eq!(
            calls[0].arguments.get("query").unwrap().as_str().unwrap(),
            "project roadmap"
        );
    }

    #[test]
    fn parse_tool_calls_ignores_xml_thinking_wrapper() {
        let response = r#"<tool_call>
<thinking>Need to inspect memory first</thinking>
<memory_recall>
<query>recent deploy notes</query>
</memory_recall>
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "memory_recall");
        assert_eq!(
            calls[0].arguments.get("query").unwrap().as_str().unwrap(),
            "recent deploy notes"
        );
    }

    #[test]
    fn parse_tool_calls_handles_xml_with_json_arguments() {
        let response = r#"<tool_call>
<shell>{"command":"pwd"}</shell>
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "pwd"
        );
    }

    #[test]
    fn parse_tool_calls_handles_markdown_tool_call_fence() {
        let response = r#"I'll check that.
```tool_call
{"name": "shell", "arguments": {"command": "pwd"}}
```
Done."#;

        let (text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "pwd"
        );
        assert!(text.contains("I'll check that."));
        assert!(text.contains("Done."));
        assert!(!text.contains("```tool_call"));
    }

    #[test]
    fn parse_tool_calls_handles_markdown_tool_call_hybrid_close_tag() {
        let response = r#"Preface
```tool-call
{"name": "shell", "arguments": {"command": "date"}}
</tool_call>
Tail"#;

        let (text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "date"
        );
        assert!(text.contains("Preface"));
        assert!(text.contains("Tail"));
        assert!(!text.contains("```tool-call"));
    }

    #[test]
    fn parse_tool_calls_handles_markdown_shell_fence() {
        let response = r#"I'll run that.
```shell
echo hello
```
Done."#;

        let (text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "echo hello"
        );
        assert!(text.contains("I'll run that."));
        assert!(text.contains("Done."));
    }

    #[test]
    fn parse_tool_calls_handles_markdown_invoke_fence() {
        let response = r#"Checking.
```invoke
{"name": "shell", "arguments": {"command": "date"}}
```
Done."#;

        let (text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "date"
        );
        assert!(text.contains("Checking."));
        assert!(text.contains("Done."));
    }

    #[test]
    fn parse_tool_calls_handles_toolcall_tag_alias() {
        let response = r#"<toolcall>
{"name": "shell", "arguments": {"command": "date"}}
</toolcall>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "date"
        );
    }

    #[test]
    fn parse_tool_calls_handles_tool_dash_call_tag_alias() {
        let response = r#"<tool-call>
{"name": "shell", "arguments": {"command": "whoami"}}
</tool-call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "whoami"
        );
    }

    #[test]
    fn parse_tool_calls_handles_invoke_tag_alias() {
        let response = r#"<invoke>
{"name": "shell", "arguments": {"command": "uptime"}}
</invoke>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "uptime"
        );
    }

    #[test]
    fn parse_tool_calls_recovers_unclosed_tool_call_with_json() {
        let response = r#"I will call the tool now.
<tool_call>
{"name": "shell", "arguments": {"command": "uptime -p"}}"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.contains("I will call the tool now."));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "uptime -p"
        );
    }

    #[test]
    fn parse_tool_calls_recovers_mismatched_close_tag() {
        let response = r#"<tool_call>
{"name": "shell", "arguments": {"command": "uptime"}}
</arg_value>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(
            calls[0].arguments.get("command").unwrap().as_str().unwrap(),
            "uptime"
        );
    }

    #[test]
    fn parse_tool_calls_recovers_cross_alias_closing_tags() {
        let response = r#"<toolcall>
{"name": "shell", "arguments": {"command": "date"}}
</tool_call>"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.is_empty());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn parse_tool_calls_rejects_raw_tool_json_without_tags() {
        // SECURITY: Raw JSON without explicit wrappers should NOT be parsed
        // This prevents prompt injection attacks where malicious content
        // could include JSON that mimics a tool call.
        let response = r#"Sure, creating the file now.
{"name": "file_write", "arguments": {"path": "hello.py", "content": "print('hello')"}}"#;

        let (text, calls) = parse_tool_calls(response);
        assert!(text.contains("Sure, creating the file now."));
        assert_eq!(
            calls.len(),
            0,
            "Raw JSON without wrappers should not be parsed"
        );
    }

    #[test]
    fn build_tool_instructions_includes_all_tools() {
        let security = PolicyHandle::from_config(
            &crate::config::AutonomyConfig::default(),
            std::path::Path::new("/tmp"),
        );
        let tools = tools::default_tools(security);
        let instructions = build_tool_instructions(&tools);

        assert!(instructions.contains("## Tool Use Protocol"));
        assert!(instructions.contains("<tool_call>"));
        assert!(instructions.contains("shell"));
        assert!(instructions.contains("file_read"));
        assert!(instructions.contains("file_write"));
    }

    #[test]
    fn tools_to_openai_format_produces_valid_schema() {
        let security = PolicyHandle::from_config(
            &crate::config::AutonomyConfig::default(),
            std::path::Path::new("/tmp"),
        );
        let tools = tools::default_tools(security);
        let formatted = tools_to_openai_format(&tools);

        assert!(!formatted.is_empty());
        for tool_json in &formatted {
            assert_eq!(tool_json["type"], "function");
            assert!(tool_json["function"]["name"].is_string());
            assert!(tool_json["function"]["description"].is_string());
            assert!(!tool_json["function"]["name"].as_str().unwrap().is_empty());
        }
        // Verify known tools are present
        let names: Vec<&str> = formatted
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"file_read"));
    }

    #[test]
    fn trim_history_preserves_system_prompt() {
        let mut history = vec![ChatMessage::system("system prompt")];
        for i in 0..DEFAULT_MAX_HISTORY_MESSAGES + 20 {
            history.push(ChatMessage::user(format!("msg {i}")));
        }
        let original_len = history.len();
        assert!(original_len > DEFAULT_MAX_HISTORY_MESSAGES + 1);

        trim_history(&mut history, DEFAULT_MAX_HISTORY_MESSAGES);

        // System prompt preserved
        assert_eq!(history[0].role, "system");
        assert_eq!(history[0].content, "system prompt");
        // Trimmed to limit
        assert_eq!(history.len(), DEFAULT_MAX_HISTORY_MESSAGES + 1); // +1 for system
                                                                     // Most recent messages preserved
        let last = &history[history.len() - 1];
        assert_eq!(
            last.content,
            format!("msg {}", DEFAULT_MAX_HISTORY_MESSAGES + 19)
        );
    }

    #[test]
    fn trim_history_noop_when_within_limit() {
        let mut history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi"),
        ];
        trim_history(&mut history, DEFAULT_MAX_HISTORY_MESSAGES);
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn build_compaction_transcript_formats_roles() {
        let messages = vec![
            ChatMessage::user("I like dark mode"),
            ChatMessage::assistant("Got it"),
        ];
        let transcript = build_compaction_transcript(&messages);
        assert!(transcript.contains("USER: I like dark mode"));
        assert!(transcript.contains("ASSISTANT: Got it"));
    }

    #[test]
    fn apply_compaction_summary_replaces_old_segment() {
        let mut history = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("old 1"),
            ChatMessage::assistant("old 2"),
            ChatMessage::user("recent 1"),
            ChatMessage::assistant("recent 2"),
        ];

        apply_compaction_summary(&mut history, 1, 3, "- user prefers concise replies");

        assert_eq!(history.len(), 4);
        assert!(history[1].content.contains("Compaction summary"));
        assert!(history[2].content.contains("recent 1"));
        assert!(history[3].content.contains("recent 2"));
    }

    #[test]
    fn autosave_memory_key_has_prefix_and_uniqueness() {
        let key1 = autosave_memory_key("user_msg");
        let key2 = autosave_memory_key("user_msg");

        assert!(key1.starts_with("user_msg_"));
        assert!(key2.starts_with("user_msg_"));
        assert_ne!(key1, key2);
    }

    #[tokio::test]
    async fn autosave_memory_keys_preserve_multiple_turns() {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();

        let key1 = autosave_memory_key("user_msg");
        let key2 = autosave_memory_key("user_msg");

        mem.store(&key1, "I'm Paul", MemoryCategory::Conversation, None)
            .await
            .unwrap();
        mem.store(&key2, "I'm 45", MemoryCategory::Conversation, None)
            .await
            .unwrap();

        assert_eq!(mem.count().await.unwrap(), 2);

        let recalled = mem.recall("45", 5, None).await.unwrap();
        assert!(recalled.iter().any(|entry| entry.content.contains("45")));
    }

    #[tokio::test]
    async fn build_context_ignores_legacy_assistant_autosave_entries() {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();
        mem.store(
            "assistant_resp_poisoned",
            "User suffered a fabricated event",
            MemoryCategory::Daily,
            Some("sess-a"),
        )
        .await
        .unwrap();
        mem.store(
            "user_msg_real",
            "User asked for concise status updates",
            MemoryCategory::Conversation,
            Some("sess-a"),
        )
        .await
        .unwrap();

        let context = build_context(&mem, "status updates", 0.0, Some("sess-a")).await;
        assert!(context.contains("user_msg_real"));
        assert!(!context.contains("assistant_resp_poisoned"));
        assert!(!context.contains("fabricated event"));
    }

    #[tokio::test]
    async fn build_context_excludes_legacy_and_other_session_conversation() {
        let tmp = TempDir::new().unwrap();
        let mem = SqliteMemory::new(tmp.path()).unwrap();
        // Shared keyword so FTS/hybrid recall returns all rows; inject filter decides.
        mem.store(
            "legacy_shell",
            "hello: 用 shell 执行 echo hello，不要解释。",
            MemoryCategory::Conversation,
            None,
        )
        .await
        .unwrap();
        mem.store(
            "other_sess",
            "hello: previous user messages from other session",
            MemoryCategory::Conversation,
            Some("sess-old"),
        )
        .await
        .unwrap();
        mem.store(
            "core_fact",
            "hello: username is velaclaw_user",
            MemoryCategory::Core,
            None,
        )
        .await
        .unwrap();
        mem.store(
            "current_note",
            "hello: current session note about greeting",
            MemoryCategory::Conversation,
            Some("sess-new"),
        )
        .await
        .unwrap();

        let context = build_context(&mem, "hello", 0.0, Some("sess-new")).await;
        assert!(
            context.contains("username is velaclaw_user"),
            "core should inject; context={context:?}"
        );
        assert!(
            context.contains("current session note"),
            "same-session conversation should inject; context={context:?}"
        );
        assert!(!context.contains("echo hello"));
        assert!(!context.contains("other session"));
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Recovery Tests - Tool Call Parsing Edge Cases
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_tool_calls_handles_empty_tool_result() {
        // Recovery: Empty tool_result tag should be handled gracefully
        let response = r#"I'll run that command.
<tool_result name="shell">

</tool_result>
Done."#;
        let (text, calls) = parse_tool_calls(response);
        assert!(text.contains("Done."));
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_arguments_value_handles_null() {
        // Recovery: null arguments are returned as-is (Value::Null)
        let value = serde_json::json!(null);
        let result = parse_arguments_value(Some(&value));
        assert!(result.is_null());
    }

    #[test]
    fn parse_tool_calls_handles_empty_tool_calls_array() {
        // Recovery: Empty tool_calls array returns original response (no tool parsing)
        let response = r#"{"content": "Hello", "tool_calls": []}"#;
        let (text, calls) = parse_tool_calls(response);
        // When tool_calls is empty, the entire JSON is returned as text
        assert!(text.contains("Hello"));
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_whitespace_only_name() {
        // Recovery: Whitespace-only tool name should return None
        let value = serde_json::json!({"function": {"name": "   ", "arguments": {}}});
        let result = parse_tool_call_value(&value);
        assert!(result.is_none());
    }

    #[test]
    fn parse_tool_calls_handles_empty_string_arguments() {
        // Recovery: Empty string arguments should be handled
        let value = serde_json::json!({"name": "test", "arguments": ""});
        let result = parse_tool_call_value(&value);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "test");
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Recovery Tests - History Management
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn trim_history_with_no_system_prompt() {
        // Recovery: History without system prompt should trim correctly
        let mut history = vec![];
        for i in 0..DEFAULT_MAX_HISTORY_MESSAGES + 20 {
            history.push(ChatMessage::user(format!("msg {i}")));
        }
        trim_history(&mut history, DEFAULT_MAX_HISTORY_MESSAGES);
        assert_eq!(history.len(), DEFAULT_MAX_HISTORY_MESSAGES);
    }

    #[test]
    fn trim_history_preserves_role_ordering() {
        // Recovery: After trimming, role ordering should remain consistent
        let mut history = vec![ChatMessage::system("system")];
        for i in 0..DEFAULT_MAX_HISTORY_MESSAGES + 10 {
            history.push(ChatMessage::user(format!("user {i}")));
            history.push(ChatMessage::assistant(format!("assistant {i}")));
        }
        trim_history(&mut history, DEFAULT_MAX_HISTORY_MESSAGES);
        assert_eq!(history[0].role, "system");
        assert_eq!(history[history.len() - 1].role, "assistant");
    }

    #[test]
    fn trim_history_with_only_system_prompt() {
        // Recovery: Only system prompt should not be trimmed
        let mut history = vec![ChatMessage::system("system prompt")];
        trim_history(&mut history, DEFAULT_MAX_HISTORY_MESSAGES);
        assert_eq!(history.len(), 1);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Recovery Tests - Arguments Parsing
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_arguments_value_handles_invalid_json_string() {
        // Recovery: Invalid JSON string should return empty object
        let value = serde_json::Value::String("not valid json".to_string());
        let result = parse_arguments_value(Some(&value));
        assert!(result.is_object());
        assert!(result.as_object().unwrap().is_empty());
    }

    #[test]
    fn parse_arguments_value_handles_none() {
        // Recovery: None arguments should return empty object
        let result = parse_arguments_value(None);
        assert!(result.is_object());
        assert!(result.as_object().unwrap().is_empty());
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Recovery Tests - JSON Extraction
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn extract_json_values_handles_empty_string() {
        // Recovery: Empty input should return empty vec
        let result = extract_json_values("");
        assert!(result.is_empty());
    }

    #[test]
    fn extract_json_values_handles_whitespace_only() {
        // Recovery: Whitespace only should return empty vec
        let result = extract_json_values("   \n\t  ");
        assert!(result.is_empty());
    }

    #[test]
    fn extract_json_values_handles_multiple_objects() {
        // Recovery: Multiple JSON objects should all be extracted
        let input = r#"{"a": 1}{"b": 2}{"c": 3}"#;
        let result = extract_json_values(input);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn extract_json_values_handles_arrays() {
        // Recovery: JSON arrays should be extracted
        let input = r#"[1, 2, 3]{"key": "value"}"#;
        let result = extract_json_values(input);
        assert_eq!(result.len(), 2);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Recovery Tests - Constants Validation
    // ═══════════════════════════════════════════════════════════════════════

    const _: () = {
        assert!(DEFAULT_MAX_TOOL_ITERATIONS > 0);
        assert!(DEFAULT_MAX_TOOL_ITERATIONS <= 100);
        assert!(DEFAULT_MAX_HISTORY_MESSAGES > 0);
        assert!(DEFAULT_MAX_HISTORY_MESSAGES <= 1000);
    };

    #[test]
    fn constants_bounds_are_compile_time_checked() {
        // Bounds are enforced by the const assertions above.
    }

    // ═══════════════════════════════════════════════════════════════════════
    // Recovery Tests - Tool Call Value Parsing
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_tool_call_value_handles_missing_name_field() {
        // Recovery: Missing name field should return None
        let value = serde_json::json!({"function": {"arguments": {}}});
        let result = parse_tool_call_value(&value);
        assert!(result.is_none());
    }

    #[test]
    fn parse_tool_call_value_handles_top_level_name() {
        // Recovery: Tool call with name at top level (non-OpenAI format)
        let value = serde_json::json!({"name": "test_tool", "arguments": {}});
        let result = parse_tool_call_value(&value);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "test_tool");
    }

    #[test]
    fn parse_tool_call_value_accepts_top_level_parameters_alias() {
        let value = serde_json::json!({
            "name": "schedule",
            "parameters": {"action": "create", "message": "test"}
        });
        let result = parse_tool_call_value(&value).expect("tool call should parse");
        assert_eq!(result.name, "schedule");
        assert_eq!(
            result.arguments.get("action").and_then(|v| v.as_str()),
            Some("create")
        );
    }

    #[test]
    fn parse_tool_call_value_accepts_function_parameters_alias() {
        let value = serde_json::json!({
            "function": {
                "name": "shell",
                "parameters": {"command": "date"}
            }
        });
        let result = parse_tool_call_value(&value).expect("tool call should parse");
        assert_eq!(result.name, "shell");
        assert_eq!(
            result.arguments.get("command").and_then(|v| v.as_str()),
            Some("date")
        );
    }

    #[test]
    fn parse_tool_calls_from_json_value_handles_empty_array() {
        // Recovery: Empty tool_calls array should return empty vec
        let value = serde_json::json!({"tool_calls": []});
        let result = parse_tool_calls_from_json_value(&value);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_tool_calls_from_json_value_handles_missing_tool_calls() {
        // Recovery: Missing tool_calls field should fall through
        let value = serde_json::json!({"name": "test", "arguments": {}});
        let result = parse_tool_calls_from_json_value(&value);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn parse_tool_calls_from_json_value_handles_top_level_array() {
        // Recovery: Top-level array of tool calls
        let value = serde_json::json!([
            {"name": "tool_a", "arguments": {}},
            {"name": "tool_b", "arguments": {}}
        ]);
        let result = parse_tool_calls_from_json_value(&value);
        assert_eq!(result.len(), 2);
    }

    // ═══════════════════════════════════════════════════════════════════════
    // GLM-Style Tool Call Parsing
    // ═══════════════════════════════════════════════════════════════════════

    #[test]
    fn parse_glm_style_browser_open_url() {
        let response = "browser_open/url>https://example.com";
        let calls = parse_glm_style_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert!(calls[0].1["command"].as_str().unwrap().contains("curl"));
        assert!(calls[0].1["command"]
            .as_str()
            .unwrap()
            .contains("example.com"));
    }

    #[test]
    fn parse_glm_style_shell_command() {
        let response = "shell/command>ls -la";
        let calls = parse_glm_style_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert_eq!(calls[0].1["command"], "ls -la");
    }

    #[test]
    fn parse_glm_style_http_request() {
        let response = "http_request/url>https://api.example.com/data";
        let calls = parse_glm_style_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "http_request");
        assert_eq!(calls[0].1["url"], "https://api.example.com/data");
        assert_eq!(calls[0].1["method"], "GET");
    }

    #[test]
    fn parse_glm_style_plain_url() {
        let response = "https://example.com/api";
        let calls = parse_glm_style_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert!(calls[0].1["command"].as_str().unwrap().contains("curl"));
    }

    #[test]
    fn parse_glm_style_json_args() {
        let response = r#"shell/{"command": "echo hello"}"#;
        let calls = parse_glm_style_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "shell");
        assert_eq!(calls[0].1["command"], "echo hello");
    }

    #[test]
    fn parse_glm_style_multiple_calls() {
        let response = r#"shell/command>ls
browser_open/url>https://example.com"#;
        let calls = parse_glm_style_tool_calls(response);
        assert_eq!(calls.len(), 2);
    }

    #[test]
    fn parse_glm_style_tool_call_integration() {
        // Integration test: GLM format should be parsed in parse_tool_calls
        let response = "Checking...\nbrowser_open/url>https://example.com\nDone";
        let (text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert!(text.contains("Checking"));
        assert!(text.contains("Done"));
    }

    #[test]
    fn parse_glm_style_rejects_non_http_url_param() {
        let response = "browser_open/url>javascript:alert(1)";
        let calls = parse_glm_style_tool_calls(response);
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_unclosed_tool_call_tag() {
        let response = "<tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"pwd\"}}\nDone";
        let (text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].arguments["command"], "pwd");
        assert_eq!(text, "Done");
    }

    // ─────────────────────────────────────────────────────────────────────
    // TG4 (inline): parse_tool_calls robustness — malformed/edge-case inputs
    // Prevents: Pattern 4 issues #746, #418, #777, #848
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn parse_tool_calls_empty_input_returns_empty() {
        let (text, calls) = parse_tool_calls("");
        assert!(calls.is_empty(), "empty input should produce no tool calls");
        assert!(text.is_empty(), "empty input should produce no text");
    }

    #[test]
    fn parse_tool_calls_whitespace_only_returns_empty_calls() {
        let (text, calls) = parse_tool_calls("   \n\t  ");
        assert!(calls.is_empty());
        assert!(text.is_empty() || text.trim().is_empty());
    }

    #[test]
    fn parse_tool_calls_nested_xml_tags_handled() {
        // Double-wrapped tool call should still parse the inner call
        let response = r#"<tool_call><tool_call>{"name":"echo","arguments":{"msg":"hi"}}</tool_call></tool_call>"#;
        let (_text, calls) = parse_tool_calls(response);
        // Should find at least one tool call
        assert!(
            !calls.is_empty(),
            "nested XML tags should still yield at least one tool call"
        );
    }

    #[test]
    fn parse_tool_calls_truncated_json_no_panic() {
        // Incomplete JSON inside tool_call tags
        let response = r#"<tool_call>{"name":"shell","arguments":{"command":"ls"</tool_call>"#;
        let (_text, _calls) = parse_tool_calls(response);
        // Should not panic — graceful handling of truncated JSON
    }

    #[test]
    fn parse_tool_calls_empty_json_object_in_tag() {
        let response = "<tool_call>{}</tool_call>";
        let (_text, calls) = parse_tool_calls(response);
        // Empty JSON object has no name field — should not produce valid tool call
        assert!(
            calls.is_empty(),
            "empty JSON object should not produce a tool call"
        );
    }

    #[test]
    fn parse_tool_calls_closing_tag_only_returns_text() {
        let response = "Some text </tool_call> more text";
        let (text, calls) = parse_tool_calls(response);
        assert!(
            calls.is_empty(),
            "closing tag only should not produce calls"
        );
        assert!(
            !text.is_empty(),
            "text around orphaned closing tag should be preserved"
        );
    }

    #[test]
    fn parse_tool_calls_very_large_arguments_no_panic() {
        let large_arg = "x".repeat(100_000);
        let response = format!(
            r#"<tool_call>{{"name":"echo","arguments":{{"message":"{}"}}}}</tool_call>"#,
            large_arg
        );
        let (_text, calls) = parse_tool_calls(&response);
        assert_eq!(calls.len(), 1, "large arguments should still parse");
        assert_eq!(calls[0].name, "echo");
    }

    #[test]
    fn parse_tool_calls_special_characters_in_arguments() {
        let response = r#"<tool_call>{"name":"echo","arguments":{"message":"hello \"world\" <>&'\n\t"}}</tool_call>"#;
        let (_text, calls) = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "echo");
    }

    #[test]
    fn parse_tool_calls_text_with_embedded_json_not_extracted() {
        // Raw JSON without any tags should NOT be extracted as a tool call
        let response = r#"Here is some data: {"name":"echo","arguments":{"message":"hi"}} end."#;
        let (_text, calls) = parse_tool_calls(response);
        assert!(
            calls.is_empty(),
            "raw JSON in text without tags should not be extracted"
        );
    }

    #[test]
    fn parse_tool_calls_multiple_formats_mixed() {
        // Mix of text and properly tagged tool call
        let response = r#"I'll help you with that.

<tool_call>
{"name":"shell","arguments":{"command":"echo hello"}}
</tool_call>

Let me check the result."#;
        let (text, calls) = parse_tool_calls(response);
        assert_eq!(
            calls.len(),
            1,
            "should extract one tool call from mixed content"
        );
        assert_eq!(calls[0].name, "shell");
        assert!(
            text.contains("help you"),
            "text before tool call should be preserved"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // TG4 (inline): scrub_credentials edge cases
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn scrub_credentials_empty_input() {
        let result = tool_batch::scrub_credentials("");
        assert_eq!(result, "");
    }

    #[test]
    fn scrub_credentials_no_sensitive_data() {
        let input = "normal text without any secrets";
        let result = tool_batch::scrub_credentials(input);
        assert_eq!(
            result, input,
            "non-sensitive text should pass through unchanged"
        );
    }

    #[test]
    fn scrub_credentials_short_values_not_redacted() {
        // Values shorter than 8 chars should not be redacted
        let input = r#"api_key="short""#;
        let result = tool_batch::scrub_credentials(input);
        assert_eq!(result, input, "short values should not be redacted");
    }

    // ─────────────────────────────────────────────────────────────────────
    // TG4 (inline): trim_history edge cases
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn trim_history_empty_history() {
        let mut history: Vec<crate::providers::ChatMessage> = vec![];
        trim_history(&mut history, 10);
        assert!(history.is_empty());
    }

    #[test]
    fn trim_history_system_only() {
        let mut history = vec![crate::providers::ChatMessage::system("system prompt")];
        trim_history(&mut history, 10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, "system");
    }

    #[test]
    fn trim_history_exactly_at_limit() {
        let mut history = vec![
            crate::providers::ChatMessage::system("system"),
            crate::providers::ChatMessage::user("msg 1"),
            crate::providers::ChatMessage::assistant("reply 1"),
        ];
        trim_history(&mut history, 2); // 2 non-system messages = exactly at limit
        assert_eq!(history.len(), 3, "should not trim when exactly at limit");
    }

    #[test]
    fn trim_history_removes_oldest_non_system() {
        let mut history = vec![
            crate::providers::ChatMessage::system("system"),
            crate::providers::ChatMessage::user("old msg"),
            crate::providers::ChatMessage::assistant("old reply"),
            crate::providers::ChatMessage::user("new msg"),
            crate::providers::ChatMessage::assistant("new reply"),
        ];
        trim_history(&mut history, 2);
        assert_eq!(history.len(), 3); // system + 2 kept
        assert_eq!(history[0].role, "system");
        assert_eq!(history[1].content, "new msg");
    }

    /// When `build_system_prompt_with_mode` is called with `native_tools = true`,
    /// the output must contain ZERO XML protocol artifacts. In the native path
    /// `build_tool_instructions` is never called, so the system prompt alone
    /// must be clean of XML tool-call protocol.
    #[test]
    fn native_tools_system_prompt_contains_zero_xml() {
        use crate::channels::build_system_prompt_with_mode;

        let tool_summaries: Vec<(&str, &str)> = vec![
            ("shell", "Execute shell commands"),
            ("file_read", "Read files"),
        ];

        let system_prompt = build_system_prompt_with_mode(
            std::path::Path::new("/tmp"),
            "test-model",
            &tool_summaries,
            &[],                                            // no skills
            None,                                           // no identity config
            None,                                           // no bootstrap_max_chars
            true,                                           // native_tools
            crate::config::SkillsPromptInjectionMode::Full, // skills_prompt_mode
        );

        // Must contain zero XML protocol artifacts
        assert!(
            !system_prompt.contains("<tool_call>"),
            "Native prompt must not contain <tool_call>"
        );
        assert!(
            !system_prompt.contains("</tool_call>"),
            "Native prompt must not contain </tool_call>"
        );
        assert!(
            !system_prompt.contains("<tool_result>"),
            "Native prompt must not contain <tool_result>"
        );
        assert!(
            !system_prompt.contains("</tool_result>"),
            "Native prompt must not contain </tool_result>"
        );
        assert!(
            !system_prompt.contains("## Tool Use Protocol"),
            "Native prompt must not contain XML protocol header"
        );

        // Positive: native prompt should still list tools and contain task instructions
        assert!(
            system_prompt.contains("shell"),
            "Native prompt must list tool names"
        );
        assert!(
            system_prompt.contains("## Your Task"),
            "Native prompt should contain task instructions"
        );
    }
}
