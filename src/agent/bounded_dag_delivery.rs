//! Host parlor (VL-NA-031): internodal HANDOFF never becomes the user-visible reply.
//!
//! 会客厅：节点信封只进 artifact；用户出口由宿主整理或模板降级。

use crate::providers::{ChatMessage, ChatRequest, Provider};
use anyhow::Result;

/// System prompt for the host Delivery rewrite (not a planner node, not a work-node card).
pub const DELIVERY_SYSTEM_PROMPT: &str = "\
You write the operator-visible conclusion for USER TASK.\n\
Use the node artifacts as evidence. Be direct.\n\
Do not use internodal envelope headers: HANDOFF, verdict:, findings:, pointers:, gaps:.\n\
Do not tell the operator to hand off to another node.\n\
If evidence is incomplete, say what is known and the single next action.\n";

/// True when `text` is a work-node internodal envelope, not a parlor reply.
#[must_use]
pub fn looks_like_internodal_envelope(text: &str) -> bool {
    let trimmed = text.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    let head = trimmed.lines().next().unwrap_or("").trim();
    if head.eq_ignore_ascii_case("handoff") || head.starts_with("HANDOFF") {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    let has_verdict = lower.contains("verdict:");
    let has_pointers = ["\npointers:", "\n## pointers", "\npointers\n"]
        .iter()
        .any(|k| lower.contains(k));
    let has_gaps = ["\ngaps:", "\n## gaps", "\ngaps\n"]
        .iter()
        .any(|k| lower.contains(k));
    has_verdict && has_pointers && has_gaps
}

/// Host template when Delivery rewrite is skipped or still internodal (no replan).
#[must_use]
pub fn parlor_fallback(user_task: &str, internodal: &str) -> String {
    let findings = section_after(internodal, &["findings:", "findings"]);
    let next = section_after(internodal, &["pointers:", "pointers"])
        .or_else(|| section_after(internodal, &["gaps:", "gaps"]));
    let mut out = String::new();
    let task = user_task.trim();
    if !task.is_empty() {
        out.push_str(task);
        out.push_str("\n\n");
    }
    if let Some(body) = findings {
        out.push_str(body.trim());
        out.push('\n');
    } else {
        let stripped = internodal
            .lines()
            .filter(|line| {
                let t = line.trim();
                !t.eq_ignore_ascii_case("handoff")
                    && !t.to_ascii_lowercase().starts_with("verdict:")
                    && !t.to_ascii_lowercase().starts_with("findings:")
                    && !t.to_ascii_lowercase().starts_with("pointers:")
                    && !t.to_ascii_lowercase().starts_with("gaps:")
            })
            .collect::<Vec<_>>()
            .join("\n");
        out.push_str(stripped.trim());
        out.push('\n');
    }
    if let Some(n) = next {
        let one: String = n
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .into();
        if !one.is_empty() {
            out.push_str("\n下一步：");
            out.push_str(one.trim_start_matches('-').trim());
            out.push('\n');
        }
    }
    let visible = out.trim().to_string();
    if looks_like_internodal_envelope(&visible) {
        "任务已完成部分核查。请根据会话中的步骤记录确认下一步；详细信封未向操作者展示。".into()
    } else {
        visible
    }
}

fn section_after(text: &str, headers: &[&str]) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let mut start = None;
    for h in headers {
        if let Some(idx) = lower.find(h) {
            start = Some(idx + h.len());
            break;
        }
    }
    let start = start?;
    let rest = &text[start..];
    let rest_lower = rest.to_ascii_lowercase();
    let mut end = rest.len();
    for stop in ["\npointers:", "\ngaps:", "\nverdict:"] {
        if let Some(i) = rest_lower.find(stop) {
            end = end.min(i);
        }
    }
    let body = rest[..end].trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

/// Hard gate: internodal skeleton never leaves as the chat body.
#[must_use]
pub fn ensure_user_visible(user_task: &str, body: &str) -> String {
    if !looks_like_internodal_envelope(body) {
        return body.to_string();
    }
    parlor_fallback(user_task, body)
}

/// Last hop: internodal envelope is parlor material, not a reason to replan the graph.
#[must_use]
pub fn skip_replan_for_parlor(remaining_nodes: usize, last_body: &str) -> bool {
    remaining_nodes == 0 && looks_like_internodal_envelope(last_body)
}

/// Per-hop RAO budget is the configured tool-iteration cap (not DAG hop count).
#[must_use]
pub fn per_hop_tool_iteration_budget(configured: usize) -> usize {
    if configured == 0 {
        10
    } else {
        configured
    }
}
/// Host Delivery: optional no-tool rewrite, then [`ensure_user_visible`].
/// Same provider as the turn (planner-style `chat`, empty tools). Not a second tool-loop.
pub async fn host_delivery(
    provider: &dyn Provider,
    model: &str,
    temperature: f64,
    user_task: &str,
    last_node_body: &str,
) -> Result<String> {
    if !looks_like_internodal_envelope(last_node_body) {
        return Ok(last_node_body.to_string());
    }
    let rewritten =
        match delivery_chat(provider, model, temperature, user_task, last_node_body).await {
            Ok(text) if !text.trim().is_empty() && !looks_like_internodal_envelope(&text) => text,
            Ok(_) | Err(_) => parlor_fallback(user_task, last_node_body),
        };
    Ok(ensure_user_visible(user_task, &rewritten))
}

async fn delivery_chat(
    provider: &dyn Provider,
    model: &str,
    temperature: f64,
    user_task: &str,
    internodal: &str,
) -> Result<String> {
    let clip: String = internodal.chars().take(6_000).collect();
    let messages = [
        ChatMessage::system(DELIVERY_SYSTEM_PROMPT),
        ChatMessage::user(format!("USER TASK\n{user_task}\n\nNODE ARTIFACT\n{clip}")),
    ];
    let request = ChatRequest {
        messages: &messages,
        tools: None,
    };
    let response = provider.chat(request, model, temperature).await?;
    Ok(response.text_or_empty().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMART_TUBE: &str = "HANDOFF\nverdict: partial\nfindings:\n- issue #5917 fixed in 32.38s\npointers:\n- upgrade to 32.38s\ngaps:\n- device version unknown";

    #[test]
    fn envelope_detected() {
        assert!(looks_like_internodal_envelope(SMART_TUBE));
        assert!(looks_like_internodal_envelope(
            "**Verdict: partial**\n## Findings(node 3/3,x)\nfoo\n## Pointers\na\n## Gaps\nb"
        ));
        assert!(!looks_like_internodal_envelope(
            "升级到 32.38s，若仍卡在 1 分钟再切 Cronet。"
        ));
    }

    #[test]
    fn fallback_strips_envelope_headers() {
        let out = parlor_fallback("电视 SmartTube 只播一分钟", SMART_TUBE);
        assert!(!looks_like_internodal_envelope(&out));
        assert!(out.contains("5917") || out.contains("32.38"));
        assert!(!out.trim_start().to_ascii_lowercase().starts_with("handoff"));
    }

    #[test]
    fn ensure_passes_clean_text() {
        let clean = "Google 路由当前可用。";
        assert_eq!(ensure_user_visible("check", clean), clean);
    }

    #[test]
    fn last_hop_envelope_skips_replan() {
        assert!(skip_replan_for_parlor(0, SMART_TUBE));
        assert!(!skip_replan_for_parlor(2, SMART_TUBE));
        assert!(!skip_replan_for_parlor(0, "升级到 32.38s。"));
    }

    #[test]
    fn per_hop_budget_is_not_dag_hop_count() {
        assert_eq!(per_hop_tool_iteration_budget(0), 10);
        assert_eq!(per_hop_tool_iteration_budget(64), 64);
    }
}
