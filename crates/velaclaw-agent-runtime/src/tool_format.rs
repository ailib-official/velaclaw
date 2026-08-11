//! Tool-call format steering + recovery ladder (VL-TTC-014 / VL-TTC-016).
//! 工具调用格式纠偏：类型化泄漏错误 + 固定策略阶梯，避免追逐方言。

/// DeepSeek DSML delimiter family (U+FF5C), same wire form as ai-lib-core.
#[cfg(any(test, not(feature = "ai-protocol")))]
const DSML_TAG: &str = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";

/// Host recovery strategies after format inspection fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolFormatRecoveryStrategy {
    /// Remind canonical `<tool_call>` + `arguments` template.
    CorrectivePrompt,
    /// Demand native API tool_calls only; forbid all text markup.
    NativeOnlyReask,
    /// Strip markup and fail closed (no further re-chat).
    StripFailClosed,
}

impl ToolFormatRecoveryStrategy {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CorrectivePrompt => "CorrectivePrompt",
            Self::NativeOnlyReask => "NativeOnlyReask",
            Self::StripFailClosed => "StripFailClosed",
        }
    }
}

/// Fixed ladder: CorrectivePrompt → NativeOnlyReask → StripFailClosed.
///
/// At most two re-chats per turn (`MAX_RECHAT`), then strip.
#[derive(Debug, Default)]
pub struct ToolFormatLadder {
    recoveries_used: u8,
}

impl ToolFormatLadder {
    pub const MAX_RECHAT: u8 = 2;

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Next strategy when markup is present but no calls were parsed.
    #[must_use]
    pub fn next_strategy(&mut self) -> ToolFormatRecoveryStrategy {
        let strategy = match self.recoveries_used {
            0 => ToolFormatRecoveryStrategy::CorrectivePrompt,
            1 => ToolFormatRecoveryStrategy::NativeOnlyReask,
            _ => ToolFormatRecoveryStrategy::StripFailClosed,
        };
        self.recoveries_used = self.recoveries_used.saturating_add(1);
        strategy
    }

    #[must_use]
    pub fn recoveries_used(&self) -> u8 {
        self.recoveries_used
    }
}

/// True when the model appears to attempt a tool call but no calls were parsed.
#[cfg(feature = "ai-protocol")]
#[must_use]
pub fn needs_tool_format_correction(raw_text: &str, parsed_call_count: usize) -> bool {
    ai_lib_rust::inspect_tool_format(raw_text, parsed_call_count).is_err()
}

#[cfg(not(feature = "ai-protocol"))]
#[must_use]
pub fn needs_tool_format_correction(raw_text: &str, parsed_call_count: usize) -> bool {
    if parsed_call_count > 0 || raw_text.trim().is_empty() {
        return false;
    }
    raw_text.contains(DSML_TAG)
        || raw_text.contains("<tool_call")
        || raw_text.contains("<tool_calls")
        || raw_text.contains("<shell>")
        || raw_text.contains("<bash>")
        || raw_text.contains("<function>")
        || raw_text.contains("_call>")
        || raw_text.contains("$call")
}

/// User-role message for a recovery strategy step.
#[must_use]
pub fn tool_format_recovery_message(strategy: ToolFormatRecoveryStrategy) -> &'static str {
    #[cfg(feature = "ai-protocol")]
    {
        let mapped = match strategy {
            ToolFormatRecoveryStrategy::CorrectivePrompt => {
                ai_lib_rust::ToolFormatRecoveryStrategy::CorrectivePrompt
            }
            ToolFormatRecoveryStrategy::NativeOnlyReask => {
                ai_lib_rust::ToolFormatRecoveryStrategy::NativeOnlyReask
            }
            ToolFormatRecoveryStrategy::StripFailClosed => {
                ai_lib_rust::ToolFormatRecoveryStrategy::StripFailClosed
            }
        };
        ai_lib_rust::tool_format_recovery_message(mapped)
    }
    #[cfg(not(feature = "ai-protocol"))]
    {
        match strategy {
            ToolFormatRecoveryStrategy::CorrectivePrompt => {
                "Your previous reply tried to call a tool but used an invalid format. \
                 Prefer native API tool_calls. If you must use text, emit EXACTLY:\n\
                 <tool_call>\n\
                 {\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n\
                 </tool_call>\n\
                 Rules: matching </tool_call> close tag; JSON must have \"name\" and \"arguments\" object; \
                 NEVER use DSML delimiters, <shell>, <bash>, <function>, _call, or $call. Call the tool again now."
            }
            ToolFormatRecoveryStrategy::NativeOnlyReask => {
                "STOP. Do not emit any text tool markup (no <tool_call>, no DSML, no <shell>, no _call, no $call). \
                 Call tools ONLY via the native API tool_calls / function-calling channel that was provided. \
                 Retry the tool call now using native tool_calls only."
            }
            ToolFormatRecoveryStrategy::StripFailClosed => "",
        }
    }
}

/// Where the soft-fail notice will be shown (ORCH-HOST-004).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoftFailSurface {
    /// Interactive / one-shot CLI agent.
    Cli,
    /// Web Local Control /chat.
    Web,
}

impl SoftFailSurface {
    #[must_use]
    pub fn switch_hint(self) -> &'static str {
        match self {
            Self::Cli => "Switch model with `/model <provider/model>` (list: `/models`).",
            Self::Web => "Switch model with the Web model picker (provider/model id).",
        }
    }
}

/// Append a user-visible notice after tool-format recovery is exhausted.
#[must_use]
pub fn append_tool_format_exhausted_notice(
    reply: &str,
    model: &str,
    surface: SoftFailSurface,
) -> String {
    let notice = format!(
        "\n\n---\nVelaClaw notice: tool-format recovery exhausted for model `{model}`. \
         The reply above may be incomplete (tool markup was stripped). {}",
        surface.switch_hint()
    );
    if reply.trim().is_empty() {
        notice.trim_start().to_string()
    } else {
        format!("{reply}{notice}")
    }
}

/// True when an error string looks like provider rate-limit / quota exhaustion.
#[must_use]
pub fn looks_like_provider_limit(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    if lower.contains("429")
        && (lower.contains("too many")
            || lower.contains("rate")
            || lower.contains("limit")
            || lower.contains("quota"))
    {
        return true;
    }
    const HINTS: &[&str] = &[
        "rate limit",
        "rate_limited",
        "quota exhausted",
        "insufficient quota",
        "insufficient_quota",
        "insufficient balance",
        "out of credits",
        "all providers/models failed",
    ];
    HINTS.iter().any(|h| lower.contains(h))
}

/// Build an actionable user-facing message for provider limit / quota hard-fail.
#[must_use]
pub fn provider_limit_user_message(
    sanitized_error: &str,
    model: &str,
    surface: SoftFailSurface,
) -> String {
    format!(
        "VelaClaw notice: provider limit or quota failure for model `{model}`.\n\
         Detail: {sanitized_error}\n\
         {}",
        surface.switch_hint()
    )
}

/// Announce a session model failover (host_decide_failover).
#[must_use]
pub fn host_decide_failover_announce(from_model: &str, to_model: &str) -> String {
    format!(
        "\n\n---\nVelaClaw notice: session model switched from `{from_model}` to `{to_model}` \
         (host_decide_failover). The next turn will use the new model."
    )
}

/// Backward-compatible alias for CorrectivePrompt message (VL-TTC-014).
#[must_use]
pub fn tool_format_correction_message() -> &'static str {
    tool_format_recovery_message(ToolFormatRecoveryStrategy::CorrectivePrompt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_order_is_corrective_then_native_then_strip() {
        let mut ladder = ToolFormatLadder::new();
        assert_eq!(
            ladder.next_strategy(),
            ToolFormatRecoveryStrategy::CorrectivePrompt
        );
        assert_eq!(
            ladder.next_strategy(),
            ToolFormatRecoveryStrategy::NativeOnlyReask
        );
        assert_eq!(
            ladder.next_strategy(),
            ToolFormatRecoveryStrategy::StripFailClosed
        );
        assert_eq!(
            ladder.next_strategy(),
            ToolFormatRecoveryStrategy::StripFailClosed
        );
    }

    #[test]
    fn needs_correction_when_unparsed_tool_call_markup() {
        let junk = "<tool_call>\nNOT_JSON\n</tool_call>";
        assert!(needs_tool_format_correction(junk, 0));
        assert!(!needs_tool_format_correction(junk, 1));
        assert!(!needs_tool_format_correction("plain answer", 0));
    }

    #[test]
    fn needs_correction_for_dsml_and_dollar_call() {
        let junk = format!("<{DSML_TAG}>\njunk\n</{DSML_TAG}>");
        assert!(needs_tool_format_correction(&junk, 0));
        assert!(needs_tool_format_correction(
            "<$call>\n{\"name\":\"shell\"}\n</$call>",
            0
        ));
    }

    #[test]
    fn recovery_messages_are_strategy_keyed() {
        let msg = tool_format_recovery_message(ToolFormatRecoveryStrategy::CorrectivePrompt);
        assert!(msg.contains("<tool_call>"));
        assert!(msg.contains("arguments"));
        let native = tool_format_recovery_message(ToolFormatRecoveryStrategy::NativeOnlyReask);
        assert!(native.contains("native"));
        assert!(
            tool_format_recovery_message(ToolFormatRecoveryStrategy::StripFailClosed).is_empty()
        );
    }

    #[test]
    fn tool_format_exhausted_notice_includes_model_and_switch_hint() {
        let out = append_tool_format_exhausted_notice(
            "partial",
            "groq/llama-3.1-8b-instant",
            SoftFailSurface::Cli,
        );
        assert!(out.contains("partial"));
        assert!(out.contains("groq/llama-3.1-8b-instant"));
        assert!(out.contains("/model"));
        let web = append_tool_format_exhausted_notice("", "openai/gpt-4o", SoftFailSurface::Web);
        assert!(web.contains("model picker"));
        assert!(web.contains("openai/gpt-4o"));
    }

    #[test]
    fn provider_limit_detection_and_message() {
        assert!(looks_like_provider_limit(
            "429 Too Many Requests rate limit"
        ));
        assert!(looks_like_provider_limit("insufficient_quota"));
        assert!(looks_like_provider_limit(
            "All providers/models failed. Attempts:"
        ));
        assert!(!looks_like_provider_limit("connection reset"));
        let msg = provider_limit_user_message(
            "429 rate limited",
            "deepseek/deepseek-v4-flash",
            SoftFailSurface::Web,
        );
        assert!(msg.contains("deepseek/deepseek-v4-flash"));
        assert!(msg.contains("model picker"));
    }

    #[test]
    fn failover_announce_mentions_both_models() {
        let a = host_decide_failover_announce("a/x", "b/y");
        assert!(a.contains("`a/x`"));
        assert!(a.contains("`b/y`"));
        assert!(a.contains("host_decide_failover"));
    }
}
