//! Shared tool-call iteration body (VL-REVIEW2-A1 / VL-CTX-002).

#[allow(clippy::wildcard_imports)]
use super::*;

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
    loop_compact: Option<crate::agent::tool_batch::ToolLoopCompact>,
) -> Result<String> {
    let gate = loop_compact.map(
        |loop_compact| crate::agent::tool_batch::ToolBatchGateExtras {
            loop_compact: Some(loop_compact),
            ..Default::default()
        },
    );
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
        gate.as_ref(),
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
//   • max_iterations is reached (returns the visible text plus a notice), or
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
/// `config` (CLI) or `host_decide` (Web) enable opt-in `host_decide_failover`.
/// Channel surfaces pass neither — notices still apply; Decide failover does not.
#[derive(Clone)]
pub(crate) struct SoftFailLoopCtx<'a> {
    pub session_key: &'a str,
    pub config: Option<&'a Config>,
    /// Pre-built host Decide context (Web `Agent` path — no full [`Config`] retained).
    #[cfg(feature = "ai-protocol")]
    pub host_decide: Option<&'a crate::orchestration::HostDecideHost>,
    pub surface: velaclaw_agent_runtime::SoftFailSurface,
    /// Logical model ids from `[[model_routes]]` (capability catalog, not cost order).
    pub peer_logical_ids: &'a [String],
    /// Route table used to treat `hint:code` and its physical model as the same peer.
    pub model_routes: &'a [crate::config::ModelRouteConfig],
    /// Web/CLI session pick (must not be skipped merely because hop model is `hint:…`).
    pub session_model: Option<&'a str>,
    /// Shared per-node probe governor (DAG hops). None → local to this loop call.
    pub probe: Option<&'a std::sync::Mutex<crate::agent::probe_dedup::HopProbeGovernor>>,
    /// Per-hop tool gist (VL-APE-016 / I18).
    pub hop_tool_accum:
        Option<std::sync::Arc<std::sync::Mutex<crate::agent::graph_scheduler::HopToolAccumulator>>>,
    /// VL-APE-037: cognition hop must not advertise/execute retrieve substitutes.
    pub block_retrieve_tools: bool,
}

#[cfg(feature = "ai-protocol")]
impl SoftFailLoopCtx<'_> {
    fn host_decide_owned(&self) -> Option<crate::orchestration::HostDecideHost> {
        self.config
            .map(crate::orchestration::HostDecideHost::from_config)
    }
}

fn with_probe<R>(
    soft_fail: Option<&SoftFailLoopCtx<'_>>,
    local: &mut crate::agent::probe_dedup::HopProbeGovernor,
    f: impl FnOnce(&mut crate::agent::probe_dedup::HopProbeGovernor) -> R,
) -> R {
    if let Some(cell) = soft_fail.and_then(|c| c.probe) {
        let mut g = cell.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    } else {
        f(local)
    }
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
    _fold_cache: Option<&FoldCache>,
    soft_fail: Option<SoftFailLoopCtx<'_>>,
    gate_extras: Option<&crate::agent::tool_batch::ToolBatchGateExtras>,
) -> Result<String> {
    let max_iterations = if max_tool_iterations == 0 {
        DEFAULT_MAX_TOOL_ITERATIONS
    } else {
        max_tool_iterations
    };

    let mut active_model = model.to_string();
    let mut peer_continue_used = false;
    let mut local_probe = Box::new(crate::agent::probe_dedup::HopProbeGovernor::new());
    let mut stage_cursor = crate::agent::artifact_contract::StageCursor::from_configs(
        gate_extras
            .map(|extras| extras.macro_stages.as_slice())
            .unwrap_or(&[]),
    );
    let mut last_visible = String::new();

    let block_retrieve = soft_fail.as_ref().is_some_and(|c| c.block_retrieve_tools);
    let tool_specs: Vec<crate::tools::ToolSpec> = if block_retrieve {
        Vec::new()
    } else {
        tools_registry.iter().map(|tool| tool.spec()).collect()
    };
    let use_native_tools = tool_dispatcher
        .map(|d| d.should_send_tool_specs() && !tool_specs.is_empty())
        .unwrap_or_else(|| provider.supports_native_tools() && !tool_specs.is_empty());

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
            model: active_model.clone(),
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
            active_model.as_str(),
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

        let (
            response_text,
            mut parsed_text,
            mut tool_calls,
            mut assistant_history_content,
            mut native_tool_calls,
        ) = match chat_result {
            Ok(resp) => {
                observer.record_event(&ObserverEvent::LlmResponse {
                    provider: provider_name.to_string(),
                    model: active_model.clone(),
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
                            let (fallback_text, fallback_calls) = parse_tool_calls(&response_text);
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
                    model: active_model.clone(),
                    duration: llm_started_at.elapsed(),
                    success: false,
                    error_message: Some(crate::providers::sanitize_api_error(&e.to_string())),
                });
                #[cfg(feature = "ai-protocol")]
                if let Some(ctx) = &soft_fail {
                    let host_owned = ctx.host_decide_owned();
                    let host = ctx.host_decide.or(host_owned.as_ref());
                    return Err(crate::orchestration::map_provider_limit_error(
                        e,
                        &active_model,
                        ctx.surface,
                        host,
                        ctx.session_key,
                    ));
                }
                return Err(e);
            }
        };

        let mut display_text = if parsed_text.is_empty() {
            response_text.clone()
        } else {
            parsed_text.clone()
        };
        let mut unregistered_ir = 0usize;

        // VL-TTC-015: after envelopes miss, decode line-isolated {name,arguments} IR.
        if tool_calls.is_empty() {
            let allow: std::collections::HashMap<String, String> = tools_registry
                .iter()
                .map(|t| (t.name().to_ascii_lowercase(), t.name().to_string()))
                .collect();
            let decoded = velaclaw_agent_runtime::decode_unwrapped_ir(&response_text, &allow);
            unregistered_ir = decoded.unknown_isolated;
            if !decoded.calls.is_empty() {
                tracing::info!(
                    target: "velaclaw::agent",
                    calls = decoded.calls.len(),
                    "tool_decode: unwrapped IR (no envelope)"
                );
                tool_calls = decoded
                    .calls
                    .into_iter()
                    .map(|c| ParsedToolCall {
                        name: c.name,
                        arguments: c.arguments,
                    })
                    .collect();
                display_text = decoded.remaining;
                parsed_text = display_text.clone();
                let synthetic: Vec<ToolCall> = tool_calls
                    .iter()
                    .enumerate()
                    .map(|(i, c)| ToolCall {
                        id: format!("unwrapped_tool_{i}"),
                        name: c.name.clone(),
                        arguments: c.arguments.to_string(),
                    })
                    .collect();
                assistant_history_content = if text_tool_result_history {
                    build_assistant_history_with_tool_calls(display_text.as_str(), &synthetic)
                } else {
                    build_native_assistant_history(display_text.as_str(), &synthetic)
                };
                native_tool_calls = synthetic;
            } else if unregistered_ir > 0 {
                // Carrier stripped; turn continues without executing unknown tools.
                display_text = decoded.remaining;
                parsed_text = display_text.clone();
            }
        }

        if tool_calls.is_empty()
            && velaclaw_agent_runtime::needs_tool_format_correction(&response_text, 0)
        {
            let names: Vec<String> = tools_registry
                .iter()
                .map(|t| t.name().to_string())
                .collect();
            match try_ir_repair(
                provider,
                &active_model,
                &response_text,
                &names,
                cancellation_token.as_ref(),
                observer,
                provider_name,
            )
            .await?
            {
                Some(repaired) if !repaired.is_empty() => {
                    tracing::info!(
                        target: "velaclaw::agent",
                        repaired = repaired.len(),
                        "tool_format_repair: injected IR into shared tool loop"
                    );
                    tool_calls = repaired
                        .into_iter()
                        .map(|c| ParsedToolCall {
                            name: c.name,
                            arguments: c.arguments,
                        })
                        .collect();
                    let synthetic: Vec<ToolCall> = tool_calls
                        .iter()
                        .enumerate()
                        .map(|(i, c)| ToolCall {
                            id: format!("repair_tool_{i}"),
                            name: c.name.clone(),
                            arguments: c.arguments.to_string(),
                        })
                        .collect();
                    let history_text = if parsed_text.is_empty() {
                        response_text.as_str()
                    } else {
                        parsed_text.as_str()
                    };
                    assistant_history_content = if text_tool_result_history {
                        build_assistant_history_with_tool_calls(history_text, &synthetic)
                    } else {
                        build_native_assistant_history(history_text, &synthetic)
                    };
                    native_tool_calls = synthetic;
                }
                _ => {}
            }
        }

        if tool_calls.is_empty()
            && !peer_continue_used
            && velaclaw_agent_runtime::needs_tool_format_correction(&response_text, 0)
        {
            let peers = soft_fail
                .as_ref()
                .map(|c| c.peer_logical_ids)
                .unwrap_or(&[])
                .to_vec();
            let peers = if peers.is_empty() {
                soft_fail
                    .as_ref()
                    .and_then(|c| c.config)
                    .map(logical_ids_from_config)
                    .unwrap_or_default()
            } else {
                peers
            };
            if let Some(peer) = select_peer_continue_model(
                &active_model,
                &peers,
                soft_fail.as_ref().map(|c| c.model_routes).unwrap_or(&[]),
                soft_fail.as_ref().and_then(|c| c.session_model),
            ) {
                peer_continue_used = true;
                tracing::info!(
                    target: "velaclaw::agent",
                    from = %active_model,
                    to = %peer,
                    "tool_format_peer_continue: retrying shared loop with catalog peer"
                );
                active_model = peer;
                continue;
            }
        }

        if tool_calls.is_empty() {
            // VL-TTC-013: Repair already attempted above. Remaining markup → strip.
            let strip_fail_closed =
                velaclaw_agent_runtime::needs_tool_format_correction(&response_text, 0);
            if strip_fail_closed {
                tracing::warn!(
                    target: "velaclaw::agent",
                    "tool_format_repair_exhausted: stripping markup after IR extract miss"
                );
            }
            let known: std::collections::HashSet<String> = tools_registry
                .iter()
                .map(|t| t.name().to_ascii_lowercase())
                .collect();
            // Sanitize before streaming so CLI/Web never paint the carrier (GOV-007 shared path).
            let mut final_text = crate::util::strip_tool_call_markup(&display_text);
            final_text =
                velaclaw_agent_runtime::strip_isolated_tool_json_artifacts(&final_text, &known);
            if strip_fail_closed {
                let surface = soft_fail
                    .as_ref()
                    .map(|c| c.surface)
                    .unwrap_or(velaclaw_agent_runtime::SoftFailSurface::Cli);
                let session_key = soft_fail.as_ref().map(|c| c.session_key).unwrap_or("");
                #[cfg(feature = "ai-protocol")]
                {
                    let host_owned = soft_fail.as_ref().and_then(|c| c.host_decide_owned());
                    let host = soft_fail
                        .as_ref()
                        .and_then(|c| c.host_decide)
                        .or(host_owned.as_ref());
                    final_text = crate::orchestration::finalize_tool_format_exhausted(
                        &final_text,
                        &active_model,
                        surface,
                        host,
                        session_key,
                    );
                }
                #[cfg(not(feature = "ai-protocol"))]
                {
                    final_text = velaclaw_agent_runtime::append_tool_format_exhausted_notice(
                        &final_text,
                        &active_model,
                        surface,
                    );
                }
            } else if unregistered_ir > 0 {
                final_text = velaclaw_agent_runtime::append_unregistered_ir_notice(&final_text);
            }
            // Progressive draft: sanitized text only (no tool JSON / 入入 wrappers).
            if let Some(ref tx) = on_delta {
                let mut chunk = String::new();
                for word in final_text.split_inclusive(char::is_whitespace) {
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
                        break;
                    }
                }
                if !chunk.is_empty() {
                    let _ = tx.send(chunk).await;
                }
            }
            if stage_cursor.is_active() {
                if let Some(observation) = stage_cursor.note_assistant_claim(&final_text) {
                    history.push(ChatMessage::assistant(response_text.clone()));
                    history.push(ChatMessage::user(observation));
                    continue;
                }
                if let Some(suffix) = stage_cursor.pointer_suffix() {
                    final_text.push_str("\n\n");
                    final_text.push_str(&suffix);
                }
            }
            history.push(ChatMessage::assistant(response_text.clone()));
            return Ok(final_text);
        }

        if tool_calls.iter().any(|call| {
            crate::agent::graph_scheduler::cognition_tool_call_rejected(block_retrieve, &call.name)
        }) {
            anyhow::bail!("{}", crate::agent::graph_scheduler::COGNITION_TOOL_STOP);
        }

        // Print any text the LLM produced alongside tool calls (unless silent)
        let known: std::collections::HashSet<String> = tools_registry
            .iter()
            .map(|t| t.name().to_ascii_lowercase())
            .collect();
        let visible_text = velaclaw_agent_runtime::strip_isolated_tool_json_artifacts(
            &crate::util::strip_tool_call_markup(&display_text),
            &known,
        );
        if !silent && !visible_text.is_empty() {
            let rendered = render_opts.render(&visible_text);
            let prefixed = prefix_agent_lines(&rendered, render_opts.style);
            print!("{prefixed}");
            let _ = std::io::stdout().flush();
        }
        if !visible_text.trim().is_empty() {
            last_visible = visible_text.clone();
        }

        // Execute tool calls and build results. `individual_results` tracks per-call output so
        // native-mode history can emit one role=tool message per tool call with the correct ID.
        //
        // When multiple tool calls are present and interactive CLI approval is not needed, run
        // tool executions concurrently for lower wall-clock latency.
        let mut tool_results = String::new();
        let mut skip_outputs: Vec<Option<String>> = vec![None; tool_calls.len()];
        let mut runnable: Vec<ParsedToolCall> = Vec::new();
        let mut runnable_idx: Vec<usize> = Vec::new();
        for (i, call) in tool_calls.iter().enumerate() {
            if block_retrieve
                && crate::agent::graph_scheduler::is_retrieve_substitute_tool(&call.name)
            {
                skip_outputs[i] =
                    Some(crate::agent::graph_scheduler::RETRIEVE_SUBSTITUTE_BLOCKED.into());
                continue;
            }
            let is_shell = call.name.eq_ignore_ascii_case("shell");
            if is_shell {
                let fp =
                    crate::agent::probe_dedup::tool_probe_fingerprint(&call.name, &call.arguments);
                let decision = with_probe(soft_fail.as_ref(), &mut local_probe, |g| {
                    g.decide_shell(&fp)
                });
                match decision {
                    crate::agent::probe_dedup::ProbeShellDecision::Cap => {
                        skip_outputs[i] =
                            Some(crate::agent::probe_dedup::SHELL_ROUND_CAP_NOTICE.into());
                        continue;
                    }
                    crate::agent::probe_dedup::ProbeShellDecision::SkipRepeat => {
                        skip_outputs[i] =
                            Some(crate::agent::probe_dedup::REPEAT_PROBE_NOTICE.into());
                        continue;
                    }
                    crate::agent::probe_dedup::ProbeShellDecision::Run => {}
                }
            }
            runnable.push(call.clone());
            runnable_idx.push(i);
            if crate::agent::graph_scheduler::is_retrieve_substitute_tool(&call.name) {
                if let Some(ctx) = soft_fail.as_ref() {
                    if let Some(acc) = &ctx.hop_tool_accum {
                        if let Ok(mut guard) = acc.lock() {
                            guard.note_retrieve_executed();
                        }
                    }
                }
            }
        }
        let mut batch_outputs: Vec<String> = vec![String::new(); tool_calls.len()];
        if !runnable.is_empty() {
            let batch_results = tool_batch::execute_tool_batch(
                &runnable,
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
            for (call_i, result) in runnable_idx.into_iter().zip(batch_results) {
                batch_outputs[call_i] = result.output;
            }
        }
        for (i, skip) in skip_outputs.into_iter().enumerate() {
            if let Some(msg) = skip {
                batch_outputs[i] = msg;
            }
        }
        let mut counted_round = false;
        for (call, out) in tool_calls.iter().zip(batch_outputs.iter()) {
            if !call.name.eq_ignore_ascii_case("shell") {
                continue;
            }
            if crate::agent::probe_dedup::shell_output_counts_as_round(out) {
                counted_round = true;
            } else {
                let fp =
                    crate::agent::probe_dedup::tool_probe_fingerprint(&call.name, &call.arguments);
                with_probe(soft_fail.as_ref(), &mut local_probe, |g| {
                    g.retract_unexecuted(&fp);
                });
            }
        }
        if counted_round {
            with_probe(soft_fail.as_ref(), &mut local_probe, |g| {
                g.record_executed_round();
            });
        }
        for out in &batch_outputs {
            with_probe(soft_fail.as_ref(), &mut local_probe, |g| {
                g.note_shell_output(out);
            });
        }
        let hop_close = with_probe(soft_fail.as_ref(), &mut local_probe, |g| g.hop_close());
        let individual_results = batch_outputs;

        if let Some(ctx) = soft_fail.as_ref() {
            if let Some(acc) = &ctx.hop_tool_accum {
                if let Ok(mut guard) = acc.lock() {
                    for out in &individual_results {
                        guard.push_tool_output(out);
                    }
                }
            }
        }

        for (call, result) in tool_calls.iter().zip(individual_results.iter()) {
            let _ = writeln!(
                tool_results,
                "<tool_result name=\"{}\">\n{}\n</tool_result>",
                call.name, result
            );
        }

        if !silent {
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
        let stage_rejected = if stage_cursor.is_active() {
            for output in &individual_results {
                stage_cursor.note_tool_output(output);
            }
            if let Some(observation) = stage_cursor.note_assistant_claim(&visible_text) {
                history.push(ChatMessage::user(observation));
                true
            } else {
                false
            }
        } else {
            false
        };
        if !stage_rejected && hop_close != crate::agent::hop_stop::HopClose::None {
            let mut body =
                crate::agent::probe_dedup::hop_close_visible_body(&visible_text, hop_close);
            if let Some(suffix) = stage_cursor.pointer_suffix() {
                body.push_str("\n\n");
                body.push_str(&suffix);
            }
            return Ok(body);
        }
        compact_between_samples(history, provider, active_model.as_str(), gate_extras).await?;
    }

    Ok(tool_iteration_cap_reply(&last_visible, max_iterations))
}

fn tool_iteration_cap_reply(last_visible: &str, max_iterations: usize) -> String {
    let notice =
        format!("Stopped after {max_iterations} tool iterations. This reply is incomplete.");
    if last_visible.trim().is_empty() {
        notice
    } else {
        format!("{last_visible}\n\n{notice}")
    }
}

async fn compact_between_samples(
    history: &mut Vec<ChatMessage>,
    provider: &dyn Provider,
    model: &str,
    gate_extras: Option<&crate::agent::tool_batch::ToolBatchGateExtras>,
) -> Result<()> {
    let Some(compact) = gate_extras.and_then(|extras| extras.loop_compact) else {
        return Ok(());
    };
    let summarizer = crate::agent::context_orch::HistorySummarizer { provider, model };
    crate::agent::context_orch::prepare_turn_history(
        history,
        crate::agent::context_orch::PrepareHistoryOpts {
            layered: false,
            compact_context: false,
            async_pool: false,
            max_history: compact.max_history,
            compact_context_ratio: compact.compact_context_ratio,
            summarizer: Some(&summarizer),
            #[cfg(feature = "ai-protocol")]
            extra_chunks: &[],
            #[cfg(feature = "ai-protocol")]
            context_window: compact.context_window,
        },
    )
    .await?;
    Ok(())
}

/// Catalog logical ids from `[[model_routes]]` including same-hint `fallbacks`.
pub(crate) fn logical_ids_from_routes(routes: &[crate::config::ModelRouteConfig]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut push = |provider: &str, model: &str| {
        let id = crate::protocol_registry::compose_logical_model_id(provider, model);
        if !id.is_empty() && !ids.iter().any(|x: &String| x.eq_ignore_ascii_case(&id)) {
            ids.push(id);
        }
    };
    for route in routes {
        push(&route.provider, &route.model);
        for peer in &route.fallbacks {
            push(&peer.provider, &peer.model);
        }
    }
    ids
}

/// Catalog logical ids from `[[model_routes]]` (not priced Decide fallbacks).
pub(crate) fn logical_ids_from_config(config: &Config) -> Vec<String> {
    logical_ids_from_routes(&config.model_routes)
}

/// First remaining catalog id that is not the same physical model, preferring the session pick.
pub(crate) fn select_peer_continue_model(
    current: &str,
    peers: &[String],
    routes: &[crate::config::ModelRouteConfig],
    session_model: Option<&str>,
) -> Option<String> {
    let cur_key = crate::protocol_registry::physical_route_key(current, routes);
    if let Some(session) = session_model.map(str::trim).filter(|s| !s.is_empty()) {
        let session_key = crate::protocol_registry::physical_route_key(session, routes);
        if session_key != cur_key {
            return Some(session.to_string());
        }
    }
    let mut ids: Vec<&String> = peers
        .iter()
        .filter(|id| {
            !id.is_empty() && crate::protocol_registry::physical_route_key(id, routes) != cur_key
        })
        .collect();
    ids.sort_unstable();
    ids.into_iter().next().cloned()
}

/// Isolated Repair completion: blob → allowlisted IR. Network errors become `Ok(None)`.
async fn try_ir_repair(
    provider: &dyn Provider,
    model: &str,
    failed_blob: &str,
    allowlisted_names: &[String],
    cancellation_token: Option<&CancellationToken>,
    observer: &dyn Observer,
    provider_name: &str,
) -> Result<Option<Vec<velaclaw_agent_runtime::RepairedToolCall>>> {
    if cancellation_token.is_some_and(CancellationToken::is_cancelled) {
        return Err(ToolLoopCancelled.into());
    }
    if allowlisted_names.is_empty() {
        return Ok(None);
    }

    let system = velaclaw_agent_runtime::repair_extract_system_prompt(allowlisted_names);
    let blob = velaclaw_agent_runtime::truncate_repair_blob(failed_blob);
    let messages = [
        ChatMessage::system(system),
        ChatMessage::user(format!(
            "Extract tool calls from this assistant message:\n\n{blob}"
        )),
    ];

    observer.record_event(&ObserverEvent::LlmRequest {
        provider: provider_name.to_string(),
        model: model.to_string(),
        messages_count: messages.len(),
    });
    let started = Instant::now();
    let chat_future = provider.chat(
        ChatRequest {
            messages: &messages,
            tools: None,
        },
        model,
        0.0,
    );
    let chat_result = if let Some(token) = cancellation_token {
        tokio::select! {
            () = token.cancelled() => return Err(ToolLoopCancelled.into()),
            result = chat_future => result,
        }
    } else {
        chat_future.await
    };

    match chat_result {
        Ok(resp) => {
            observer.record_event(&ObserverEvent::LlmResponse {
                provider: provider_name.to_string(),
                model: model.to_string(),
                duration: started.elapsed(),
                success: true,
                error_message: None,
            });
            let allow: std::collections::HashSet<String> =
                allowlisted_names.iter().cloned().collect();
            Ok(Some(velaclaw_agent_runtime::parse_repaired_tool_calls(
                resp.text_or_empty(),
                &allow,
            )))
        }
        Err(e) => {
            observer.record_event(&ObserverEvent::LlmResponse {
                provider: provider_name.to_string(),
                model: model.to_string(),
                duration: started.elapsed(),
                success: false,
                error_message: Some(crate::providers::sanitize_api_error(&e.to_string())),
            });
            tracing::warn!(
                target: "velaclaw::agent",
                error = %crate::providers::sanitize_api_error(&e.to_string()),
                "tool_format_repair: extract call failed; stripping"
            );
            Ok(None)
        }
    }
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
        runtime_kind: config.runtime.kind.clone(),
        sandbox_name: crate::security::describe_effective_sandbox(&config.security)
            .name
            .to_string(),
    };
    security.append_execution_policy_prompt(system_prompt, &extras);
    if http.enabled && http.allow_private_hosts {
        system_prompt.push_str(
            "- HTTP LAN access: enabled for private/local hosts when `autonomy.level = full`.\n\n",
        );
    }
    append_hitl_continuity_guidance(system_prompt);
}

/// Web/CLI HITL: keep the agent in the loop; modals are for short credentials/choices only.
pub(crate) fn append_hitl_continuity_guidance(system_prompt: &mut String) {
    system_prompt.push_str(
        "## Human-in-the-loop (keep task continuity)\n\n\
             - You are the agent: run work with tools (`shell`, etc.). When policy needs approval, \
           the UI shows Deny / Allow once / Always / Never — wait for that, then continue the same turn.\n\
         - Use `request_human_input` only for short operator input: `choice` (buttons), \
           `secret` (password/token → secret_slot), or `text` (short codes ≤128 chars).\n\
         - Do **not** ask the human to run terminal commands and paste results back into chat \
           or a modal. That is not an agent workflow.\n\
         - Prefer `shell` + approval over `handoff`. Never collect command logs via \
           `request_human_input`.\n\n",
    );
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
pub(crate) fn resolve_cli_turn_model(
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
        lane: crate::orchestration::TurnLane::WorkCognition,
    };
    Ok(crate::orchestration::resolve_turn_model(&req)?.model)
}

#[cfg(not(feature = "ai-protocol"))]
pub(crate) fn resolve_cli_turn_model(
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

#[cfg(test)]
mod peer_continue_tests {
    use super::select_peer_continue_model;

    #[test]
    fn select_peer_skips_current_and_sorts_lexically() {
        let peers = vec!["zeta/m".into(), "alpha/m".into(), "cur/m".into()];
        assert_eq!(
            select_peer_continue_model("cur/m", &peers, &[], None).as_deref(),
            Some("alpha/m")
        );
        assert_eq!(
            select_peer_continue_model("only", &[String::from("only")], &[], None),
            None
        );
    }

    #[test]
    fn select_peer_skips_hint_equivalent_and_prefers_session_pick() {
        let routes = [crate::config::ModelRouteConfig {
            hint: "code".into(),
            provider: "deepseek/deepseek-v4-flash".into(),
            model: "deepseek-v4-flash".into(),
            api_key: None,
            fallbacks: Vec::new(),
        }];
        let peers = vec![
            "deepseek/deepseek-v4-flash/deepseek-v4-flash".into(),
            "openai/gpt-oss-20b".into(),
        ];
        assert_eq!(
            select_peer_continue_model("hint:code", &peers, &routes, None).as_deref(),
            Some("openai/gpt-oss-20b")
        );
        assert_eq!(
            select_peer_continue_model(
                "hint:code",
                &peers,
                &routes,
                Some("nvidia/nemotron-3-ultra-550b-a55b")
            )
            .as_deref(),
            Some("nvidia/nemotron-3-ultra-550b-a55b")
        );
    }

    #[test]
    fn logical_ids_include_route_fallbacks() {
        let routes = [crate::config::ModelRouteConfig {
            hint: "code".into(),
            provider: "deepseek".into(),
            model: "deepseek-v4-flash".into(),
            api_key: None,
            fallbacks: vec![crate::config::ModelRoutePeerConfig {
                provider: "nvidia".into(),
                model: "nemotron-3-ultra-550b-a55b".into(),
            }],
        }];
        let ids = super::logical_ids_from_routes(&routes);
        assert!(ids.iter().any(|id| id.contains("deepseek-v4-flash")));
        assert!(ids
            .iter()
            .any(|id| id.contains("nemotron-3-ultra-550b-a55b")));
        assert_eq!(
            select_peer_continue_model("hint:code", &ids, &routes, None).as_deref(),
            Some("nvidia/nemotron-3-ultra-550b-a55b")
        );
    }
}

#[cfg(test)]
mod hitl_prompt_tests {
    use super::append_hitl_continuity_guidance;

    #[test]
    fn hitl_guidance_rejects_paste_results_workflow() {
        let mut prompt = String::new();
        append_hitl_continuity_guidance(&mut prompt);
        assert!(prompt.contains("Human-in-the-loop"));
        assert!(prompt.contains("request_human_input"));
        assert!(prompt.contains("Do **not** ask the human to run terminal commands"));
        assert!(prompt.contains("shell") && prompt.contains("approval"));
    }
}

#[cfg(test)]
mod loop_e2e_tests {
    use super::run_tool_call_loop;
    use crate::agent::tool_batch::{ToolBatchGateExtras, ToolLoopCompact};
    use crate::observability::NoopObserver;
    use crate::providers::{ChatMessage, ChatRequest, ChatResponse, Provider, ToolCall};
    use crate::tools::{Tool, ToolExecutionContext, ToolResult};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct Script {
        replies: Mutex<Vec<ChatResponse>>,
        seen: Mutex<Vec<Vec<ChatMessage>>>,
        summaries: Mutex<usize>,
    }

    impl Script {
        fn new(replies: Vec<ChatResponse>) -> Self {
            Self {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
                summaries: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl Provider for Script {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            *self.summaries.lock().expect("summary count") += 1;
            Ok("kept the citation".into())
        }

        async fn chat(
            &self,
            request: ChatRequest<'_>,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ChatResponse> {
            self.seen
                .lock()
                .expect("seen")
                .push(request.messages.to_vec());
            let mut replies = self.replies.lock().expect("replies");
            if replies.is_empty() {
                return Ok(ChatResponse {
                    text: Some("done".into()),
                    tool_calls: vec![],
                });
            }
            Ok(replies.remove(0))
        }
    }

    struct OnceFail {
        left: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl Tool for OnceFail {
        fn name(&self) -> &str {
            "probe"
        }
        fn description(&self) -> &str {
            "Look up a source"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            if self.left.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                return Ok(ToolResult {
                    success: false,
                    output: "source missing".into(),
                    error: Some("source missing".into()),
                });
            }
            Ok(ToolResult {
                success: true,
                output: "source alpha".into(),
                error: None,
            })
        }
    }

    struct NotePad {
        path: std::path::PathBuf,
    }

    #[async_trait]
    impl Tool for NotePad {
        fn name(&self) -> &str {
            "write_note"
        }
        fn description(&self) -> &str {
            "Write the note"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"]
            })
        }
        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: &ToolExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            let text = args
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            std::fs::write(&self.path, &text)?;
            Ok(ToolResult {
                success: true,
                output: self.path.display().to_string(),
                error: None,
            })
        }
    }

    struct NoteRead {
        path: std::path::PathBuf,
    }

    #[async_trait]
    impl Tool for NoteRead {
        fn name(&self) -> &str {
            "read_note"
        }
        fn description(&self) -> &str {
            "Read the note"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
            _ctx: &ToolExecutionContext,
        ) -> anyhow::Result<ToolResult> {
            let text = std::fs::read_to_string(&self.path).unwrap_or_default();
            Ok(ToolResult {
                success: true,
                output: format!("{}: {text}", self.path.display()),
                error: None,
            })
        }
    }

    fn call(name: &str, arguments: &str) -> ChatResponse {
        ChatResponse {
            text: Some(String::new()),
            tool_calls: vec![ToolCall {
                id: format!("call-{name}"),
                name: name.into(),
                arguments: arguments.into(),
            }],
        }
    }

    fn call_with_text(name: &str, arguments: &str, text: &str) -> ChatResponse {
        let mut response = call(name, arguments);
        response.text = Some(text.into());
        response
    }

    fn render() -> crate::cli_render::RenderOpts {
        crate::cli_render::RenderOpts {
            style: crate::cli_render::RenderStyle {
                ansi: false,
                markdown: false,
            },
            fold_lines: 4,
            fold_enabled: false,
        }
    }

    async fn drive(
        provider: &Script,
        history: &mut Vec<ChatMessage>,
        tools: &[Box<dyn Tool>],
        max_iterations: usize,
        compact: Option<ToolLoopCompact>,
    ) -> anyhow::Result<String> {
        let gate = ToolBatchGateExtras {
            loop_compact: compact,
            ..ToolBatchGateExtras::default()
        };
        let observer = NoopObserver;
        run_tool_call_loop(
            provider,
            history,
            tools,
            &observer,
            "mock",
            "mock-model",
            0.0,
            true,
            None,
            "cli",
            &crate::config::MultimodalConfig::default(),
            max_iterations,
            None,
            None,
            None,
            None,
            None,
            false,
            render(),
            None,
            None,
            Some(&gate),
        )
        .await
    }

    #[tokio::test]
    async fn direct_answer_does_not_mention_the_iteration_cap() {
        let provider = Script::new(vec![ChatResponse {
            text: Some("The citation is alpha.".into()),
            tool_calls: vec![],
        }]);
        let mut history = vec![ChatMessage::user("What is the citation?")];
        let reply = drive(&provider, &mut history, &[], 4, None)
            .await
            .expect("answer");
        assert_eq!(reply, "The citation is alpha.");
        assert_eq!(*provider.summaries.lock().expect("summaries"), 0);
    }

    #[tokio::test]
    async fn iteration_cap_keeps_the_visible_reply() {
        let provider = Script::new(vec![call_with_text(
            "probe",
            "{}",
            "Gathered the citation.",
        )]);
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(OnceFail {
            left: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })];
        let mut history = vec![ChatMessage::user("Check the source.")];
        let reply = drive(&provider, &mut history, &tools, 1, None)
            .await
            .expect("cap returns text");
        assert!(reply.contains("Gathered the citation."));
        assert!(reply.contains("Stopped after 1 tool iterations"));
    }

    #[tokio::test]
    async fn failed_probe_then_second_probe_reaches_the_answer() {
        static LEFT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
        LEFT.store(1, std::sync::atomic::Ordering::SeqCst);
        let provider = Script::new(vec![
            call("probe", "{}"),
            call("probe", "{}"),
            ChatResponse {
                text: Some("The source is alpha.".into()),
                tool_calls: vec![],
            },
        ]);
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(OnceFail {
            left: Arc::new(std::sync::atomic::AtomicUsize::new(1)),
        })];
        let mut history = vec![ChatMessage::user("Find the source.")];
        let reply = drive(&provider, &mut history, &tools, 4, None)
            .await
            .expect("continues after failure");
        assert!(reply.contains("The source is alpha."));
        assert!(!reply.contains("Stopped after"));
    }

    #[tokio::test]
    async fn write_note_then_read_it_back() {
        let dir = tempfile::tempdir().expect("temp");
        let path = dir.path().join("brief.txt");
        let provider = Script::new(vec![
            call("write_note", &format!("{{\"text\":\"citation line\"}}")),
            call("read_note", "{}"),
            ChatResponse {
                text: Some(format!("Saved {}", path.display())),
                tool_calls: vec![],
            },
        ]);
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(NotePad { path: path.clone() }),
            Box::new(NoteRead { path: path.clone() }),
        ];
        let mut history = vec![ChatMessage::user("Write the note.")];
        let reply = drive(&provider, &mut history, &tools, 4, None)
            .await
            .expect("note");
        assert_eq!(
            std::fs::read_to_string(&path).expect("file"),
            "citation line"
        );
        assert!(reply.contains(&path.display().to_string()));
    }

    #[tokio::test]
    async fn short_history_with_ratio_zero_does_not_summarize() {
        let provider = Script::new(vec![
            call("probe", "{}"),
            ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
            },
        ]);
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(OnceFail {
            left: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })];
        let mut history = vec![ChatMessage::system("system"), ChatMessage::user("Check.")];
        let compact = ToolLoopCompact {
            max_history: 50,
            compact_context_ratio: 0.0,
            context_window: Some(8000),
        };
        let reply = drive(&provider, &mut history, &tools, 3, Some(compact))
            .await
            .expect("short");
        assert_eq!(reply, "done");
        assert_eq!(*provider.summaries.lock().expect("summaries"), 0);
        let seen = provider.seen.lock().expect("seen");
        assert!(seen.len() >= 2);
        assert!(!seen[1]
            .iter()
            .any(|m| m.content.contains("[Compaction summary]")));
    }

    #[tokio::test]
    async fn long_tool_history_compacts_before_the_next_sample() {
        let provider = Script::new(vec![
            call("probe", "{}"),
            ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
            },
        ]);
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(OnceFail {
            left: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        })];
        let mut history = vec![ChatMessage::system("system"), ChatMessage::user("Check.")];
        let compact = ToolLoopCompact {
            max_history: 2,
            compact_context_ratio: 0.0,
            context_window: None,
        };
        let reply = drive(&provider, &mut history, &tools, 3, Some(compact))
            .await
            .expect("compact");
        assert_eq!(reply, "done");
        assert_eq!(*provider.summaries.lock().expect("summaries"), 1);
        let seen = provider.seen.lock().expect("seen");
        assert!(seen[1]
            .iter()
            .any(|m| m.content.contains("[Compaction summary]")));
    }
}
