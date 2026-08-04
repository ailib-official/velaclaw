//! Tool-call format steering helpers (VL-TTC-014).
//! 工具调用格式纠偏：解析失败时供宿主做一次纠正重试，避免追逐方言。

/// DeepSeek DSML delimiter family (U+FF5C), same wire form as ai-lib-core.
#[cfg(any(test, not(feature = "ai-protocol")))]
const DSML_TAG: &str = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";

/// True when the model appears to attempt a tool call but no calls were parsed.
///
/// Host runtimes should append [`tool_format_correction_message`] and re-chat
/// once instead of leaking raw markup as the final assistant message.
#[cfg(feature = "ai-protocol")]
#[must_use]
pub fn needs_tool_format_correction(raw_text: &str, parsed_call_count: usize) -> bool {
    ai_lib_rust::needs_tool_format_correction(raw_text, parsed_call_count)
}

/// Fallback when ai-protocol (ai-lib) is not linked: detect common markup only.
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
}

/// Short user-role correction appended before a single re-chat when format is wrong.
#[cfg(feature = "ai-protocol")]
#[must_use]
pub fn tool_format_correction_message() -> &'static str {
    ai_lib_rust::tool_format_correction_message()
}

#[cfg(not(feature = "ai-protocol"))]
#[must_use]
pub fn tool_format_correction_message() -> &'static str {
    "Your previous reply tried to call a tool but used an invalid format. \
     Prefer native API tool_calls. If you must use text, emit EXACTLY:\n\
     <tool_call>\n\
     {\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n\
     </tool_call>\n\
     Rules: matching </tool_call> close tag; JSON must have \"name\" and \"arguments\" object; \
     NEVER use DSML delimiters, <shell>, <bash>, or <function>. Call the tool again now."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_correction_when_unparsed_tool_call_markup() {
        let junk = "<tool_call>\nNOT_JSON\n</tool_call>";
        assert!(needs_tool_format_correction(junk, 0));
        assert!(!needs_tool_format_correction(junk, 1));
        assert!(!needs_tool_format_correction("plain answer", 0));
    }

    #[test]
    fn needs_correction_for_dsml_tag() {
        let junk = format!("<{DSML_TAG}>\njunk\n</{DSML_TAG}>");
        assert!(needs_tool_format_correction(&junk, 0));
    }

    #[test]
    fn correction_message_is_canonical_only() {
        let msg = tool_format_correction_message();
        assert!(msg.contains("<tool_call>"));
        assert!(msg.contains("arguments"));
        assert!(msg.contains("DSML"));
        assert!(!msg.contains(DSML_TAG));
    }
}
