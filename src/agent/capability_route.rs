//! Host Route(C, Context) (VL-APE-002). Three tiers; E does not pick models.
//! 宿主能力路由：级 1 会话认知；级 2 能力索引；级 3 失败 peer。

use crate::agent::bounded_dag_context::NodeContact;
use crate::agent::dag_runner::DagNode;

/// Caps that are not session work-cognition: picker must not steal them.
#[must_use]
pub fn is_non_session_capability(cap: &str) -> bool {
    let t = cap.trim().to_ascii_lowercase();
    t.contains("embed")
        || t == "translate"
        || t.contains("speech")
        || t.contains("tts")
        || t.contains("stt")
        || t.contains("image_gen")
        || t.contains("vision_embed")
}

/// Cheap Route hints (speed/tools/document): do not pin the session picker (VL-APE-022/023).
/// Planner DAGs write `tool_calling`; `[[model_routes]]` uses hint `tools` — same cheap tier.
#[must_use]
pub fn is_cheap_capability_route(cap: &str) -> bool {
    let raw = cap.trim().to_ascii_lowercase();
    if raw == "tools"
        || raw == "tool_calling"
        || raw == "fast"
        || raw == "document"
        || raw == "speed"
        || raw == "shell.exec"
        || raw == "file.read"
        || raw == "glob.search"
        || raw == "shell"
        || raw == "file"
    {
        return true;
    }
    let tag = crate::agent::intent_route::hint_to_tag(cap)
        .unwrap_or(cap.trim())
        .to_ascii_lowercase();
    matches!(
        tag.as_str(),
        "speed" | "document_understanding" | "tool_calling"
    )
}

/// Caps that mean the hop must have a shell/tool I (not document/speed/coding).
#[must_use]
pub fn is_tool_invoke_capability(cap: &str) -> bool {
    let t = cap.trim().to_ascii_lowercase();
    t == "tools"
        || t == "tool_calling"
        || t == "shell.exec"
        || t == "file.read"
        || t == "glob.search"
        || t == "shell"
        || t == "file"
}

/// Tool-invoke hops without a coding/cognition cap (empty cap is cognition, not this).
#[must_use]
pub fn node_is_tool_invoke_without_cognition(capabilities: &[String]) -> bool {
    if capabilities.is_empty() {
        return false;
    }
    if is_work_cognition_node(capabilities) {
        return false;
    }
    capabilities.iter().any(|c| is_tool_invoke_capability(c))
}

/// True when this node should run on the session picker (tier 1).
#[must_use]
pub fn is_work_cognition_node(capabilities: &[String]) -> bool {
    if capabilities.is_empty() {
        return true;
    }
    capabilities
        .iter()
        .any(|c| !is_non_session_capability(c) && !is_cheap_capability_route(c))
}

/// Planner/judge cheap default: `fast` route if set, else session default; never picker.
/// Live planning does not use this. Title refine may still prefer the fast route.
#[must_use]
pub fn cheap_planner_model<'a>(
    session_default: &'a str,
    fast_route: Option<&'a str>,
    _picker: Option<&str>,
) -> &'a str {
    fast_route
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(session_default)
}

/// Shown when the live planner has no cognition model distinct from the fast route.
pub const PLANNER_MODEL_STOP: &str = "The planner needs a cognition model that is not the fast route. Configure that model and send the task again on this session. The host will not plan on the fast route, and it will not ask for a shell command.";

/// Live planner: the session cognition model, once. Never the fast route, even as a fallback.
#[must_use]
pub fn strong_planner_model<'a>(
    session_default: &'a str,
    fast_route: Option<&str>,
) -> Option<&'a str> {
    let session = session_default.trim();
    if session.is_empty() {
        return None;
    }
    if fast_route
        .map(str::trim)
        .is_some_and(|fast| !fast.is_empty() && fast == session)
    {
        return None;
    }
    Some(session)
}

/// SoT identity on a planner node is the capability list, not a provider id.
#[must_use]
pub fn node_capability_identity(node: &DagNode) -> &[String] {
    &node.model_selector.capabilities
}

/// Tier-1 preference for a live work hop. Non-session-only nodes skip picker.
#[must_use]
pub fn work_preference<'a>(
    capabilities: &[String],
    picker_or_session: Option<&'a str>,
) -> Option<&'a str> {
    if is_work_cognition_node(capabilities) {
        picker_or_session.map(str::trim).filter(|s| !s.is_empty())
    } else {
        None
    }
}

/// Apply Route after a typed provider fail (tier 3): stay on cheap default.
#[must_use]
pub fn fail_peer_default(default_model: &str, capabilities: Vec<String>) -> NodeContact {
    NodeContact {
        model: default_model.to_string(),
        reason: "fail_strategy:default_model".into(),
        capabilities,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dag_runner::parse_dag_json;

    #[test]
    fn coding_caps_are_work_cognition() {
        assert!(is_work_cognition_node(&["coding".into()]));
        assert!(!is_work_cognition_node(&["tool_calling".into()]));
        assert!(is_cheap_capability_route("tool_calling"));
        assert!(!is_work_cognition_node(&["embed".into()]));
        assert!(!is_work_cognition_node(&["speed".into()]));
        assert!(!is_work_cognition_node(&["tools".into()]));
        assert!(is_work_cognition_node(&["embed".into(), "coding".into()]));
        assert!(is_work_cognition_node(&[]));
    }

    #[test]
    fn planner_ignores_picker() {
        assert_eq!(
            cheap_planner_model("deepseek/deepseek-v4-flash", None, Some("nvidia/ultra"),),
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(
            cheap_planner_model(
                "nvidia/super",
                Some("groq/openai/gpt-oss-20b"),
                Some("nvidia/ultra"),
            ),
            "groq/openai/gpt-oss-20b"
        );
    }

    #[test]
    fn live_planner_uses_cognition_model_not_fast_route() {
        assert_eq!(
            strong_planner_model(
                "deepseek/deepseek-v4-flash",
                Some("groq/openai/gpt-oss-20b")
            ),
            Some("deepseek/deepseek-v4-flash")
        );
        assert_eq!(
            strong_planner_model("groq/openai/gpt-oss-20b", Some("groq/openai/gpt-oss-20b")),
            None
        );
        assert_eq!(
            strong_planner_model("  ", Some("groq/openai/gpt-oss-20b")),
            None
        );
        assert_eq!(
            strong_planner_model("deepseek/deepseek-v4-flash", None),
            Some("deepseek/deepseek-v4-flash")
        );
    }

    #[test]
    fn node_identity_is_capabilities_not_provider() {
        let dag = parse_dag_json(crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON).unwrap();
        let locate = dag.nodes.iter().find(|n| n.id == "locate").unwrap();
        let id = node_capability_identity(locate);
        assert!(!id.is_empty());
        assert!(!id.iter().any(|c| c.contains('/')));
    }
}
