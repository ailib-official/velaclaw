use crate::provider::{
    ChatMessage, ChatResponse, ConversationMessage, NativeToolCapable, ToolResultMessage,
};
use crate::tools::{Tool, ToolSpec};
use serde_json::Value;
use std::fmt::Write;

#[derive(Debug, Clone)]
pub struct ParsedToolCall {
    pub name: String,
    pub arguments: Value,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub name: String,
    pub output: String,
    pub success: bool,
    pub tool_call_id: Option<String>,
}

pub trait ToolDispatcher: Send + Sync {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>);
    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage;
    fn prompt_instructions(&self, tools: &[Box<dyn Tool>]) -> String;
    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage>;
    fn should_send_tool_specs(&self) -> bool;
}

#[cfg(not(feature = "ai-protocol"))]
#[derive(Default)]
pub struct XmlToolDispatcher;

#[cfg(feature = "ai-protocol")]
pub struct XmlToolDispatcher {
    parser: ai_lib_rust::types::StandardTextToolParser,
}

#[cfg(feature = "ai-protocol")]
mod text_parser {
    use super::{ParsedToolCall, Tool, ToolExecutionResult};
    use ai_lib_rust::types::{
        text_tool::{parse_hybrid_tool_calls, PromptLevel, TextToolParser},
        tool::{ToolCall, ToolResult as AiToolResult},
        StandardTextToolParser, TextToolConfig, ToolDefinition,
    };

    pub fn create_parser() -> StandardTextToolParser {
        StandardTextToolParser::new(TextToolConfig {
            lenient_parsing: true,
            prompt_level: PromptLevel::L2,
            ..Default::default()
        })
    }

    pub fn parser_from_manifest(
        tool_calling: Option<&serde_json::Value>,
    ) -> StandardTextToolParser {
        tool_calling
            .map(StandardTextToolParser::from_manifest_tool_calling)
            .unwrap_or_else(create_parser)
    }

    pub fn convert_tool_definitions(tools: &[Box<dyn Tool>]) -> Vec<ToolDefinition> {
        tools
            .iter()
            .map(|tool| ToolDefinition {
                tool_type: "function".to_string(),
                function: ai_lib_rust::types::tool::FunctionDefinition {
                    name: tool.name().to_string(),
                    description: Some(tool.description().to_string()),
                    parameters: Some(tool.parameters_schema()),
                },
            })
            .collect()
    }

    pub fn parse_with_parser(
        parser: &StandardTextToolParser,
        text: &str,
    ) -> (String, Vec<ParsedToolCall>) {
        let (remaining, tool_calls) = parser.parse(text);
        let calls = tool_calls.into_iter().map(tool_call_to_parsed).collect();
        (remaining, calls)
    }

    pub fn parse_hybrid_with_parser(
        parser: &StandardTextToolParser,
        text: &str,
        native: &[ParsedToolCall],
    ) -> (String, Vec<ParsedToolCall>) {
        let native_core: Vec<ToolCall> = native
            .iter()
            .map(|tc| ToolCall {
                id: tc
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "native".to_string()),
                name: tc.name.clone(),
                arguments: tc.arguments.clone(),
            })
            .collect();
        let (remaining, tool_calls) = parse_hybrid_tool_calls(parser, text, &native_core);
        let calls = tool_calls.into_iter().map(tool_call_to_parsed).collect();
        (remaining, calls)
    }

    fn tool_call_to_parsed(tc: ToolCall) -> ParsedToolCall {
        ParsedToolCall {
            name: tc.name,
            arguments: tc.arguments,
            tool_call_id: Some(tc.id),
        }
    }

    pub fn format_results_with_parser(
        parser: &StandardTextToolParser,
        results: &[ToolExecutionResult],
    ) -> String {
        let ai_results: Vec<AiToolResult> = results
            .iter()
            .map(|r| AiToolResult {
                tool_use_id: r
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                content: serde_json::Value::String(r.output.clone()),
                is_error: !r.success,
            })
            .collect();
        parser.format_results(&ai_results)
    }

    pub fn prompt_instructions_with_parser(
        parser: &StandardTextToolParser,
        tools: &[Box<dyn Tool>],
    ) -> String {
        let definitions = convert_tool_definitions(tools);
        parser.prompt_instructions(&definitions)
    }
}

impl XmlToolDispatcher {
    pub fn tool_specs(tools: &[Box<dyn Tool>]) -> Vec<ToolSpec> {
        tools.iter().map(|tool| tool.spec()).collect()
    }
}

/// Build `StandardTextToolParser` from provider manifest `tool_calling` (VL-TTC-002).
#[cfg(feature = "ai-protocol")]
pub fn text_tool_parser_from_manifest(
    tool_calling: Option<&serde_json::Value>,
) -> ai_lib_rust::types::StandardTextToolParser {
    text_parser::parser_from_manifest(tool_calling)
}

/// Build a manifest-aware tool dispatcher (VL-TTC-003/004).
pub fn build_tool_dispatcher(
    dispatcher_choice: &str,
    provider: &dyn NativeToolCapable,
    policy: ai_lib_rust::ToolCallingPolicy,
) -> Box<dyn ToolDispatcher> {
    #[cfg(feature = "ai-protocol")]
    {
        let text_parser = policy.parser.clone();
        match dispatcher_choice {
            "native" => Box::new(NativeToolDispatcher::new(text_parser.clone())),
            "xml" => Box::new(XmlToolDispatcher::new(text_parser)),
            _ if provider.supports_native_tools() && policy.prefer_native_dispatcher() => {
                Box::new(NativeToolDispatcher::new(text_parser.clone()))
            }
            _ => Box::new(XmlToolDispatcher::new(text_parser)),
        }
    }
    #[cfg(not(feature = "ai-protocol"))]
    {
        let _ = policy;
        match dispatcher_choice {
            "native" => Box::new(NativeToolDispatcher::default()),
            "xml" => Box::new(XmlToolDispatcher::default()),
            _ if provider.supports_native_tools() => Box::new(NativeToolDispatcher::default()),
            _ => Box::new(XmlToolDispatcher::default()),
        }
    }
}

/// Resolve manifest `tool_calling` for a logical model and build dispatcher (channels/delegate).
#[cfg(feature = "ai-protocol")]
pub fn build_tool_dispatcher_for_logical_model(
    dispatcher_choice: &str,
    logical_model_id: &str,
    provider: &dyn NativeToolCapable,
) -> anyhow::Result<Box<dyn ToolDispatcher>> {
    let client = crate::byok::init_ai_client_sync(logical_model_id)?;
    let policy = ai_lib_rust::ToolCallingPolicy::from_tool_calling(client.manifest.tool_calling());
    Ok(build_tool_dispatcher(dispatcher_choice, provider, policy))
}

#[cfg(feature = "ai-protocol")]
impl XmlToolDispatcher {
    pub fn new(parser: ai_lib_rust::types::StandardTextToolParser) -> Self {
        Self { parser }
    }
}

#[cfg(feature = "ai-protocol")]
impl Default for XmlToolDispatcher {
    fn default() -> Self {
        Self::new(text_parser::create_parser())
    }
}

#[cfg(feature = "ai-protocol")]
impl ToolDispatcher for XmlToolDispatcher {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>) {
        let text = response.text_or_empty();
        text_parser::parse_with_parser(&self.parser, text)
    }

    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage {
        let content = text_parser::format_results_with_parser(&self.parser, results);
        ConversationMessage::Chat(ChatMessage::user(format!("[Tool results]\n{content}")))
    }

    fn prompt_instructions(&self, tools: &[Box<dyn Tool>]) -> String {
        text_parser::prompt_instructions_with_parser(&self.parser, tools)
    }

    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage> {
        history
            .iter()
            .flat_map(|msg| match msg {
                ConversationMessage::Chat(chat) => vec![chat.clone()],
                ConversationMessage::AssistantToolCalls { text, .. } => {
                    vec![ChatMessage::assistant(text.clone().unwrap_or_default())]
                }
                ConversationMessage::ToolResults(results) => {
                    let mut content = String::new();
                    for result in results {
                        let _ = writeln!(
                            content,
                            "<tool_result id=\"{}\">\n{}\n</tool_result>",
                            result.tool_call_id, result.content
                        );
                    }
                    vec![ChatMessage::user(format!("[Tool results]\n{content}"))]
                }
            })
            .collect()
    }

    fn should_send_tool_specs(&self) -> bool {
        false
    }
}

#[cfg(not(feature = "ai-protocol"))]
impl XmlToolDispatcher {
    fn parse_xml_tool_calls(response: &str) -> (String, Vec<ParsedToolCall>) {
        let mut text_parts = Vec::new();
        let mut calls = Vec::new();
        let mut remaining = response;

        while let Some(start) = remaining.find("<tool_call") {
            let before = &remaining[..start];
            if !before.trim().is_empty() {
                text_parts.push(before.trim().to_string());
            }

            let tag_open_end = remaining[start..]
                .find('>')
                .map(|p| start + p + 1)
                .unwrap_or(start + 11);

            let tag_attrs = &remaining[start + 10..tag_open_end - 1];
            let attr_name = Self::extract_attr(tag_attrs, "name");

            if let Some(end) = remaining[start..].find("</tool_call>") {
                let inner = &remaining[tag_open_end..start + end];
                match serde_json::from_str::<Value>(inner.trim()) {
                    Ok(parsed) => {
                        let json_name = parsed
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let name = if json_name.is_empty() {
                            attr_name.unwrap_or("").to_string()
                        } else {
                            json_name
                        };
                        if name.is_empty() {
                            remaining = &remaining[start + end + 12..];
                            continue;
                        }
                        let arguments = parsed
                            .get("arguments")
                            .or_else(|| parsed.get("parameters"))
                            .cloned()
                            .or_else(|| {
                                if let Value::Object(ref map) = parsed {
                                    let mut args = map.clone();
                                    args.remove("name");
                                    args.remove("approved");
                                    if !args.is_empty() {
                                        Some(Value::Object(args))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
                        let arguments = if let Value::Object(ref map) = arguments {
                            let mut cleaned = map.clone();
                            cleaned.remove("approved");
                            Value::Object(cleaned)
                        } else {
                            arguments
                        };
                        calls.push(ParsedToolCall {
                            name,
                            arguments,
                            tool_call_id: None,
                        });
                    }
                    Err(e) => {
                        tracing::warn!("Malformed <tool_call> JSON: {e}");
                    }
                }
                remaining = &remaining[start + end + 13..];
            } else {
                break;
            }
        }

        if !remaining.trim().is_empty() {
            text_parts.push(remaining.trim().to_string());
        }

        (text_parts.join("\n"), calls)
    }

    fn extract_attr<'a>(tag_str: &'a str, key: &str) -> Option<&'a str> {
        let prefix = format!("{key}=\"");
        let start = tag_str.find(&prefix)?;
        let val_start = start + prefix.len();
        let end = tag_str[val_start..].find('\"')?;
        Some(&tag_str[val_start..val_start + end])
    }
}

#[cfg(not(feature = "ai-protocol"))]
impl ToolDispatcher for XmlToolDispatcher {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>) {
        let text = response.text_or_empty();
        Self::parse_xml_tool_calls(text)
    }

    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage {
        let mut content = String::new();
        for result in results {
            let status = if result.success { "ok" } else { "error" };
            let _ = writeln!(
                content,
                "<tool_result name=\"{}\" status=\"{}\">\n{}\n</tool_result>",
                result.name, status, result.output
            );
        }
        ConversationMessage::Chat(ChatMessage::user(format!("[Tool results]\n{content}")))
    }

    fn prompt_instructions(&self, tools: &[Box<dyn Tool>]) -> String {
        let mut instructions = String::new();
        instructions.push_str("## Tool Use Protocol (MANDATORY)\n\n");
        instructions.push_str(
            "You MUST use the exact format below to invoke tools. The system will ONLY parse <tool_call> blocks — any other format will be IGNORED and treated as plain text.\n\n",
        );
        instructions
            .push_str("**Format** (one JSON object per tool, wrapped in <tool_call> tags):\n\n");
        instructions.push_str(
            "```\n<tool_call>\n{\"name\": \"tool_name\", \"arguments\": {\"param\": \"value\"}}\n</tool_call>\n```\n\n",
        );
        instructions.push_str("**Rules**:\n");
        instructions.push_str("- Always use `<tool_call>` + JSON. Never use `<shell>`, `<bash>`, or any other XML format.\n");
        instructions.push_str("- Each `<tool_call>` block must contain exactly one valid JSON object with `name` and `arguments`.\n");
        instructions.push_str("- For shell commands, use: `<tool_call>\\n{\"name\": \"shell\", \"arguments\": {\"command\": \"...\"}}\\n</tool_call>`\n");
        instructions.push_str("- If no tool is needed, just reply with plain text (no tags).\n\n");
        instructions.push_str("### Available Tools\n\n");

        for tool in tools {
            let _ = writeln!(
                instructions,
                "- **{}**: {}\n  Parameters: `{}`",
                tool.name(),
                tool.description(),
                tool.parameters_schema()
            );
        }

        instructions
    }

    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage> {
        history
            .iter()
            .flat_map(|msg| match msg {
                ConversationMessage::Chat(chat) => vec![chat.clone()],
                ConversationMessage::AssistantToolCalls { text, .. } => {
                    vec![ChatMessage::assistant(text.clone().unwrap_or_default())]
                }
                ConversationMessage::ToolResults(results) => {
                    let mut content = String::new();
                    for result in results {
                        let _ = writeln!(
                            content,
                            "<tool_result id=\"{}\">\n{}\n</tool_result>",
                            result.tool_call_id, result.content
                        );
                    }
                    vec![ChatMessage::user(format!("[Tool results]\n{content}"))]
                }
            })
            .collect()
    }

    fn should_send_tool_specs(&self) -> bool {
        false
    }
}

#[cfg(feature = "ai-protocol")]
pub struct NativeToolDispatcher {
    parser: ai_lib_rust::types::StandardTextToolParser,
}

#[cfg(not(feature = "ai-protocol"))]
pub struct NativeToolDispatcher;

#[cfg(feature = "ai-protocol")]
impl NativeToolDispatcher {
    pub fn new(parser: ai_lib_rust::types::StandardTextToolParser) -> Self {
        Self { parser }
    }
}

#[cfg(feature = "ai-protocol")]
impl Default for NativeToolDispatcher {
    fn default() -> Self {
        Self::new(text_parser::create_parser())
    }
}

#[cfg(not(feature = "ai-protocol"))]
impl Default for NativeToolDispatcher {
    fn default() -> Self {
        Self
    }
}

impl ToolDispatcher for NativeToolDispatcher {
    fn parse_response(&self, response: &ChatResponse) -> (String, Vec<ParsedToolCall>) {
        let text = response.text.clone().unwrap_or_default();
        let mut calls: Vec<ParsedToolCall> = Vec::with_capacity(response.tool_calls.len());

        for tc in &response.tool_calls {
            match serde_json::from_str::<Value>(&tc.arguments) {
                Ok(arguments) => calls.push(ParsedToolCall {
                    name: tc.name.clone(),
                    arguments,
                    tool_call_id: Some(tc.id.clone()),
                }),
                Err(e) => {
                    tracing::warn!(
                        tool = %tc.name,
                        error = %e,
                        "Failed to parse native tool call arguments as JSON; skipping call"
                    );
                }
            }
        }

        // Native empty (or all invalid) + text markup: delegate to ai-lib hybrid parser (ARCH-001).
        if calls.is_empty() {
            #[cfg(feature = "ai-protocol")]
            {
                return text_parser::parse_hybrid_with_parser(&self.parser, &text, &calls);
            }
        }

        (text, calls)
    }

    fn format_results(&self, results: &[ToolExecutionResult]) -> ConversationMessage {
        let messages = results
            .iter()
            .map(|result| ToolResultMessage {
                tool_call_id: result
                    .tool_call_id
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                content: result.output.clone(),
            })
            .collect();
        ConversationMessage::ToolResults(messages)
    }

    fn prompt_instructions(&self, _tools: &[Box<dyn Tool>]) -> String {
        String::new()
    }

    fn to_provider_messages(&self, history: &[ConversationMessage]) -> Vec<ChatMessage> {
        history
            .iter()
            .flat_map(|msg| match msg {
                ConversationMessage::Chat(chat) => vec![chat.clone()],
                ConversationMessage::AssistantToolCalls { text, tool_calls } => {
                    let payload = serde_json::json!({
                        "content": text,
                        "tool_calls": tool_calls,
                    });
                    vec![ChatMessage::assistant(payload.to_string())]
                }
                ConversationMessage::ToolResults(results) => results
                    .iter()
                    .map(|result| {
                        ChatMessage::tool(
                            serde_json::json!({
                                "tool_call_id": result.tool_call_id,
                                "content": result.content,
                            })
                            .to_string(),
                        )
                    })
                    .collect(),
            })
            .collect()
    }

    fn should_send_tool_specs(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ToolCall;

    #[test]
    fn xml_dispatcher_parses_tool_calls() {
        let response = ChatResponse {
            text: Some(
                "Checking\n<tool_call>{\"name\":\"shell\",\"arguments\":{\"command\":\"ls\"}}</tool_call>"
                    .into(),
            ),
            tool_calls: vec![],
        };
        let dispatcher = XmlToolDispatcher::default();
        let (_, calls) = dispatcher.parse_response(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
    }

    #[test]
    fn native_dispatcher_roundtrip() {
        let response = ChatResponse {
            text: Some("ok".into()),
            tool_calls: vec![ToolCall {
                id: "tc1".into(),
                name: "file_read".into(),
                arguments: "{\"path\":\"a.txt\"}".into(),
            }],
        };
        let dispatcher = NativeToolDispatcher::default();
        let (_, calls) = dispatcher.parse_response(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_call_id.as_deref(), Some("tc1"));

        let msg = dispatcher.format_results(&[ToolExecutionResult {
            name: "file_read".into(),
            output: "hello".into(),
            success: true,
            tool_call_id: Some("tc1".into()),
        }]);
        match msg {
            ConversationMessage::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].tool_call_id, "tc1");
            }
            _ => panic!("expected tool results"),
        }
    }

    #[test]
    fn xml_format_results_contains_tool_result_tags() {
        let dispatcher = XmlToolDispatcher::default();
        let msg = dispatcher.format_results(&[ToolExecutionResult {
            name: "shell".into(),
            output: "ok".into(),
            success: true,
            tool_call_id: None,
        }]);
        let rendered = match msg {
            ConversationMessage::Chat(chat) => chat.content,
            _ => String::new(),
        };
        assert!(rendered.contains("<tool_result"));
        assert!(rendered.contains("ok"));
    }

    #[test]
    fn native_format_results_keeps_tool_call_id() {
        let dispatcher = NativeToolDispatcher::default();
        let msg = dispatcher.format_results(&[ToolExecutionResult {
            name: "shell".into(),
            output: "ok".into(),
            success: true,
            tool_call_id: Some("tc-1".into()),
        }]);

        match msg {
            ConversationMessage::ToolResults(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].tool_call_id, "tc-1");
            }
            _ => panic!("expected ToolResults variant"),
        }
    }

    #[test]
    #[cfg(feature = "ai-protocol")]
    fn native_dispatcher_falls_back_when_native_json_is_malformed() {
        let tag = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";
        let text = format!(
            "Checking server.\n\
             <{tag}tool_calls>\n\
             <{tag}invoke name=\"shell\">\n\
             <{tag}parameter name=\"command\" string=\"true\">echo hi</{tag}parameter>\n\
             </{tag}invoke>\n\
             </{tag}tool_calls>"
        );
        let response = ChatResponse {
            text: Some(text),
            tool_calls: vec![ToolCall {
                id: "broken".into(),
                name: "shell".into(),
                arguments: "not-json".into(),
            }],
        };
        let dispatcher = NativeToolDispatcher::default();
        let (remaining, calls) = dispatcher.parse_response(&response);
        assert_eq!(remaining, "Checking server.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].arguments["command"], "echo hi");
    }

    #[test]
    #[cfg(feature = "ai-protocol")]
    fn native_dispatcher_falls_back_to_deepseek_dsml() {
        let tag = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";
        let text = format!(
            "Checking server.\n\
             <{tag}tool_calls>\n\
             <{tag}invoke name=\"shell\">\n\
             <{tag}parameter name=\"command\" string=\"true\">echo hi</{tag}parameter>\n\
             </{tag}invoke>\n\
             </{tag}tool_calls>"
        );
        let response = ChatResponse {
            text: Some(text),
            tool_calls: vec![],
        };
        let dispatcher = NativeToolDispatcher::default();
        let (remaining, calls) = dispatcher.parse_response(&response);
        assert_eq!(remaining, "Checking server.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].arguments["command"], "echo hi");
    }

    #[test]
    #[cfg(feature = "ai-protocol")]
    fn xml_dispatcher_parses_deepseek_dsml() {
        let tag = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";
        let text = format!(
            "<{tag}tool_calls>\n\
             <{tag}invoke name=\"shell\">\n\
             <{tag}parameter name=\"command\" string=\"true\">ls</{tag}parameter>\n\
             </{tag}invoke>\n\
             </{tag}tool_calls>"
        );
        let response = ChatResponse {
            text: Some(text),
            tool_calls: vec![],
        };
        let dispatcher = XmlToolDispatcher::default();
        let (_, calls) = dispatcher.parse_response(&response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].arguments["command"], "ls");
    }
}
