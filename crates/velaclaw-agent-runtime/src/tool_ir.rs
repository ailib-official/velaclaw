//! Unwrapped canonical tool IR (VL-TTC-015).
//! 无信封 IR：行级孤立 `{name, arguments}`，仅本轮 assistant；非用户内容扫描。
//!
//! Decode order in the host loop: native `tool_calls` → envelope codec (XML/DSML/invoke)
//! → this isolated IR. Execution always uses the same tool loop. Display must not keep
//! the carrier. Unregistered isolated IR is stripped and counted so the turn can continue
//! with a notice instead of aborting.

use crate::dispatcher::ParsedToolCall;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Result of scanning assistant text for line-isolated canonical IR JSON.
#[derive(Debug, Clone, Default)]
pub struct UnwrappedIrDecode {
    pub remaining: String,
    pub calls: Vec<ParsedToolCall>,
    /// Isolated IR objects whose `name` was not in the registry (not executed).
    pub unknown_isolated: usize,
}

/// True when JSON is a tool-call payload for a registered name (channel sanitizer).
#[must_use]
pub fn is_tool_call_payload(value: &Value, known_tool_names: &HashSet<String>) -> bool {
    let Some(name) = ir_tool_name(value) else {
        return false;
    };
    has_ir_args(value) && known_tool_names.contains(&name.to_ascii_lowercase())
}

#[must_use]
pub fn is_tool_result_payload(
    object: &serde_json::Map<String, Value>,
    saw_tool_call_payload: bool,
) -> bool {
    if !saw_tool_call_payload || !object.contains_key("result") {
        return false;
    }
    object.keys().all(|key| {
        matches!(
            key.as_str(),
            "result" | "id" | "tool_call_id" | "name" | "tool"
        )
    })
}

pub fn sanitize_tool_json_value(
    value: &Value,
    known_tool_names: &HashSet<String>,
    saw_tool_call_payload: bool,
) -> Option<(String, bool)> {
    if is_tool_call_payload(value, known_tool_names) {
        return Some((String::new(), true));
    }

    if let Some(array) = value.as_array() {
        if !array.is_empty()
            && array
                .iter()
                .all(|item| is_tool_call_payload(item, known_tool_names))
        {
            return Some((String::new(), true));
        }
        return None;
    }

    let object = value.as_object()?;

    if let Some(tool_calls) = object.get("tool_calls").and_then(|value| value.as_array()) {
        if !tool_calls.is_empty()
            && tool_calls
                .iter()
                .all(|call| is_tool_call_payload(call, known_tool_names))
        {
            let content = object
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            return Some((content, true));
        }
    }

    if is_tool_result_payload(object, saw_tool_call_payload) {
        return Some((String::new(), false));
    }

    None
}

#[must_use]
pub fn is_line_isolated_json_segment(message: &str, start: usize, end: usize) -> bool {
    let line_start = message[..start].rfind('\n').map_or(0, |idx| idx + 1);
    let line_end = message[end..]
        .find('\n')
        .map_or(message.len(), |idx| end + idx);

    message[line_start..start].trim().is_empty() && message[end..line_end].trim().is_empty()
}

/// Strip registered-tool JSON artifacts (Telegram/channel display). Unknown JSON stays.
#[must_use]
pub fn strip_isolated_tool_json_artifacts(
    message: &str,
    known_tool_names: &HashSet<String>,
) -> String {
    let mut cleaned = String::with_capacity(message.len());
    let mut cursor = 0usize;
    let mut saw_tool_call_payload = false;

    while cursor < message.len() {
        let Some(rel_start) = message[cursor..].find(|ch: char| ['{', '['].contains(&ch)) else {
            cleaned.push_str(&message[cursor..]);
            break;
        };

        let start = cursor + rel_start;
        cleaned.push_str(&message[cursor..start]);

        let candidate = &message[start..];
        let mut stream = serde_json::Deserializer::from_str(candidate).into_iter::<Value>();

        if let Some(Ok(value)) = stream.next() {
            let consumed = stream.byte_offset();
            if consumed > 0 {
                let end = start + consumed;
                if is_line_isolated_json_segment(message, start, end) {
                    if let Some((replacement, marks_tool_call)) =
                        sanitize_tool_json_value(&value, known_tool_names, saw_tool_call_payload)
                    {
                        if marks_tool_call {
                            saw_tool_call_payload = true;
                        }
                        if !replacement.trim().is_empty() {
                            cleaned.push_str(replacement.trim());
                        }
                        cursor = end;
                        continue;
                    }
                }
            }
        }

        let Some(ch) = message[start..].chars().next() else {
            break;
        };
        cleaned.push(ch);
        cursor = start + ch.len_utf8();
    }

    normalize_stripped(&cleaned)
}

/// Extract allowlisted isolated IR for execution; strip unregistered isolated IR from remaining.
#[must_use]
pub fn decode_unwrapped_ir(
    message: &str,
    allow_lower_to_name: &HashMap<String, String>,
) -> UnwrappedIrDecode {
    let known: HashSet<String> = allow_lower_to_name.keys().cloned().collect();
    let mut remaining = String::with_capacity(message.len());
    let mut calls = Vec::new();
    let mut unknown_isolated = 0usize;
    let mut cursor = 0usize;

    while cursor < message.len() {
        let Some(rel_start) = message[cursor..].find(|ch: char| ['{', '['].contains(&ch)) else {
            remaining.push_str(&message[cursor..]);
            break;
        };

        let start = cursor + rel_start;
        remaining.push_str(&message[cursor..start]);

        let candidate = &message[start..];
        let mut stream = serde_json::Deserializer::from_str(candidate).into_iter::<Value>();

        if let Some(Ok(value)) = stream.next() {
            let consumed = stream.byte_offset();
            if consumed > 0 {
                let end = start + consumed;
                if is_line_isolated_json_segment(message, start, end) {
                    if let Some(extracted) = parsed_calls_from_value(&value, allow_lower_to_name) {
                        calls.extend(extracted);
                        cursor = end;
                        continue;
                    }
                    if is_canonical_ir_shape(&value) {
                        unknown_isolated = unknown_isolated.saturating_add(1);
                        cursor = end;
                        continue;
                    }
                    if sanitize_tool_json_value(&value, &known, !calls.is_empty()).is_some() {
                        cursor = end;
                        continue;
                    }
                }
            }
        }

        let Some(ch) = message[start..].chars().next() else {
            break;
        };
        remaining.push(ch);
        cursor = start + ch.len_utf8();
    }

    UnwrappedIrDecode {
        remaining: drop_envelope_noise(&normalize_stripped(&remaining)),
        calls,
        unknown_isolated,
    }
}

/// Notice when isolated IR was dropped because the name is not registered.
/// Conversation continues (`Ok` path); the user is not left with a silent empty turn.
#[must_use]
pub fn append_unregistered_ir_notice(reply: &str) -> String {
    let notice = "\n\n---\nVelaClaw notice: ignored tool-shaped JSON that did not match a \
         registered tool. The conversation continues; no tool was run for that payload.";
    if reply.trim().is_empty() {
        notice.trim_start().to_string()
    } else {
        format!("{reply}{notice}")
    }
}

fn ir_tool_name(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    if let Some(function) = object.get("function").and_then(|f| f.as_object()) {
        function
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| object.get("name").and_then(|v| v.as_str()))
    } else {
        object.get("name").and_then(|v| v.as_str())
    }
    .map(str::trim)
    .filter(|name| !name.is_empty())
}

fn has_ir_args(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if let Some(function) = object.get("function").and_then(|f| f.as_object()) {
        function.contains_key("arguments")
            || function.contains_key("parameters")
            || object.contains_key("arguments")
            || object.contains_key("parameters")
    } else {
        object.contains_key("arguments") || object.contains_key("parameters")
    }
}

fn is_canonical_ir_shape(value: &Value) -> bool {
    if let Some(array) = value.as_array() {
        return !array.is_empty() && array.iter().all(is_canonical_ir_shape);
    }
    ir_tool_name(value).is_some() && has_ir_args(value)
}

fn parsed_calls_from_value(
    value: &Value,
    allow_lower_to_name: &HashMap<String, String>,
) -> Option<Vec<ParsedToolCall>> {
    if let Some(array) = value.as_array() {
        if array.is_empty() {
            return None;
        }
        let mut out = Vec::new();
        for item in array {
            out.push(parsed_call_from_object(item, allow_lower_to_name)?);
        }
        return Some(out);
    }
    parsed_call_from_object(value, allow_lower_to_name).map(|c| vec![c])
}

fn parsed_call_from_object(
    value: &Value,
    allow_lower_to_name: &HashMap<String, String>,
) -> Option<ParsedToolCall> {
    let name_raw = ir_tool_name(value)?;
    let canonical = allow_lower_to_name
        .get(&name_raw.to_ascii_lowercase())?
        .clone();
    let object = value.as_object()?;
    let args_src = if let Some(function) = object.get("function").and_then(|f| f.as_object()) {
        function
            .get("arguments")
            .or_else(|| function.get("parameters"))
            .or_else(|| object.get("arguments"))
            .or_else(|| object.get("parameters"))
    } else {
        object.get("arguments").or_else(|| object.get("parameters"))
    };
    let arguments = match args_src {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| {
            let mut wrap = serde_json::Map::new();
            wrap.insert("value".into(), Value::String(s.clone()));
            Value::Object(wrap)
        }),
        Some(v) => v.clone(),
        None => Value::Object(serde_json::Map::new()),
    };
    Some(ParsedToolCall {
        name: canonical,
        arguments,
        tool_call_id: object
            .get("id")
            .or_else(|| object.get("tool_call_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

fn normalize_stripped(cleaned: &str) -> String {
    let mut result = cleaned.replace("\r\n", "\n");
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    result.trim().to_string()
}

/// Drop leftover carrier tokens (e.g. 入入 / 出出) after IR JSON is removed.
fn drop_envelope_noise(s: &str) -> String {
    let kept: Vec<&str> = s
        .lines()
        .filter(|line| {
            let t = line.trim();
            if t.is_empty() {
                return false;
            }
            t.chars().any(|c| c.is_ascii_alphanumeric()) || t.chars().count() > 8
        })
        .collect();
    kept.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow_shell() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("shell".into(), "shell".into());
        m
    }

    #[test]
    fn decode_unwrapped_ir_with_carrier_noise() {
        let raw = "入入\n{\"name\": \"shell\", \"arguments\": {\"command\": \"ls -d /tmp\"}}\n出出";
        let out = decode_unwrapped_ir(raw, &allow_shell());
        assert_eq!(out.calls.len(), 1);
        assert_eq!(out.calls[0].name, "shell");
        assert_eq!(
            out.calls[0]
                .arguments
                .get("command")
                .and_then(|v| v.as_str()),
            Some("ls -d /tmp")
        );
        assert!(out.remaining.is_empty());
        assert_eq!(out.unknown_isolated, 0);
    }

    #[test]
    fn decode_unwrapped_ir_skips_inline_json() {
        let raw = "see {\"name\": \"shell\", \"arguments\": {\"command\": \"rm\"}} please";
        let out = decode_unwrapped_ir(raw, &allow_shell());
        assert!(out.calls.is_empty());
        assert!(out.remaining.contains("shell"));
    }

    #[test]
    fn decode_unwrapped_ir_counts_unknown_name() {
        let raw = "{\"name\": \"not_a_tool\", \"arguments\": {\"x\": 1}}";
        let out = decode_unwrapped_ir(raw, &allow_shell());
        assert!(out.calls.is_empty());
        assert_eq!(out.unknown_isolated, 1);
        assert!(out.remaining.is_empty());
    }

    #[test]
    fn strip_preserves_unrelated_json() {
        let mut known = HashSet::new();
        known.insert("shell".into());
        let input =
            "{\"name\":\"profile\",\"parameters\":{\"timezone\":\"UTC\"}}\nThis is an example.";
        assert_eq!(strip_isolated_tool_json_artifacts(input, &known), input);
    }
}
