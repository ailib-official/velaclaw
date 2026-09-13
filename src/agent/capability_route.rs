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

/// True when this node should run on the session picker (tier 1).
#[must_use]
pub fn is_work_cognition_node(capabilities: &[String]) -> bool {
    if capabilities.is_empty() {
        return true;
    }
    capabilities.iter().any(|c| !is_non_session_capability(c))
}

/// Planner/judge cheap default: `fast` route if set, else session default; never picker.
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
        assert!(!is_work_cognition_node(&["embed".into()]));
        assert!(is_work_cognition_node(&["embed".into(), "coding".into()]));
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
    fn node_identity_is_capabilities_not_provider() {
        let dag = parse_dag_json(crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON).unwrap();
        let locate = dag.nodes.iter().find(|n| n.id == "locate").unwrap();
        let id = node_capability_identity(locate);
        assert!(!id.is_empty());
        assert!(!id.iter().any(|c| c.contains('/')));
    }
}
