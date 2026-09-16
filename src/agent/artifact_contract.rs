//! Hop / graph artifact contract gates (VL-APE-016 / I16).
//! 制品合同门：空制品与 evidence_layer 未满足时不得假装成功。

use super::dag_runner::DagNode;
use super::graph_scheduler::hop_text_is_user_visible;

/// Outcome of evaluating a stored hop artifact against node Σ / locus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopArtifactVerdict {
    Ok,
    Empty,
    InsufficientEvidenceLayer,
}

/// Graph-level contract before parlor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphArtifactVerdict {
    Ok,
    AllEmpty,
    InsufficientEvidenceLayer,
}

/// Evidence layers a node may require beyond mere non-emptiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceLayer {
    ProtocolDist,
    UpstreamLive,
}

/// Layers declared by node locus, artifact name, and capability tags (generic).
#[must_use]
pub fn required_evidence_layers(node: &DagNode) -> Vec<EvidenceLayer> {
    let mut out = Vec::new();
    let locus = node.locus.as_deref().unwrap_or("").to_ascii_lowercase();
    if locus.starts_with("remote:") {
        out.push(EvidenceLayer::UpstreamLive);
    }
    let blob = format!(
        "{} {} {}",
        node.id.to_ascii_lowercase(),
        node.artifact.as_deref().unwrap_or(""),
        node.model_selector.capabilities.join(" ")
    )
    .to_ascii_lowercase();
    if blob.contains("manifest")
        || blob.contains("protocol")
        || blob.contains("catalog")
        || blob.contains("provider")
        || blob.contains("dist/")
    {
        out.push(EvidenceLayer::ProtocolDist);
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn signals_protocol_dist(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("protocol-dist")
        || t.contains(".yaml")
        || t.contains(".yml")
        || t.contains("manifest")
        || t.contains("providers/")
        || t.contains("ai-protocol")
        || t.contains("schema_version")
}

fn signals_upstream_live(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("upstream-live")
        || t.contains("http://")
        || t.contains("https://")
        || t.contains("curl ")
        || t.contains("ssh ")
        || t.contains("api.")
        || t.contains("status code")
        || t.contains(" 200")
        || t.contains(" 404")
        || t.contains(" 410")
}

fn artifact_signals_layer(text: &str, layer: EvidenceLayer) -> bool {
    match layer {
        EvidenceLayer::ProtocolDist => signals_protocol_dist(text),
        EvidenceLayer::UpstreamLive => signals_upstream_live(text),
    }
}

/// True when findings are only workspace listing (pwd/ls/find) without higher layers.
#[must_use]
pub fn is_workspace_only_listing(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return true;
    }
    if signals_protocol_dist(t) || signals_upstream_live(t) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    let has_listing = lower.contains("pwd")
        || lower.contains(" ls")
        || lower.starts_with("ls ")
        || lower.contains("find ")
        || lower.contains("total ");
    has_listing && !hop_text_is_user_visible(t)
}

/// Workspace listing without protocol/upstream signals (I20).
#[must_use]
pub fn is_off_goal_listing(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || advances_declared_evidence(t) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    lower.contains("pwd")
        || lower.contains(" ls")
        || lower.starts_with("ls ")
        || lower.contains("find ")
        || lower.contains("total ")
}

/// True when text shows protocol-dist or upstream-live evidence (generic layers).
#[must_use]
pub fn advances_declared_evidence(text: &str) -> bool {
    signals_protocol_dist(text) || signals_upstream_live(text)
}

/// Per-hop gate after [`hop_contract_body`]. `tool_evidence` is the accumulator;
/// visible assistant prose does not waive listing-only tools (VL-APE-022 / I16).
#[must_use]
pub fn hop_artifact_contract(
    node: &DagNode,
    artifact: &str,
    tool_evidence: &str,
) -> HopArtifactVerdict {
    if artifact.trim().is_empty() {
        return HopArtifactVerdict::Empty;
    }
    if !tool_evidence.trim().is_empty()
        && is_off_goal_listing(tool_evidence)
        && !advances_declared_evidence(tool_evidence)
    {
        return HopArtifactVerdict::InsufficientEvidenceLayer;
    }
    let probe = if tool_evidence.trim().is_empty() {
        artifact
    } else {
        tool_evidence
    };
    for layer in required_evidence_layers(node) {
        if !artifact_signals_layer(probe, layer) {
            return HopArtifactVerdict::InsufficientEvidenceLayer;
        }
    }
    HopArtifactVerdict::Ok
}

/// Graph-end gate over all stored node artifacts (topology order).
#[must_use]
pub fn graph_artifact_contract(
    nodes: &[DagNode],
    artifacts: &[(String, String)],
) -> GraphArtifactVerdict {
    if artifacts.is_empty() || artifacts.iter().all(|(_, b)| b.trim().is_empty()) {
        return GraphArtifactVerdict::AllEmpty;
    }
    for node in nodes {
        let body = artifacts
            .iter()
            .find(|(id, _)| id == &node.id)
            .map(|(_, b)| b.as_str())
            .unwrap_or("");
        match hop_artifact_contract(node, body, "") {
            HopArtifactVerdict::Ok => {}
            HopArtifactVerdict::Empty => {
                if required_evidence_layers(node).is_empty() {
                    continue;
                }
                return GraphArtifactVerdict::AllEmpty;
            }
            HopArtifactVerdict::InsufficientEvidenceLayer => {
                return GraphArtifactVerdict::InsufficientEvidenceLayer;
            }
        }
    }
    GraphArtifactVerdict::Ok
}

/// Operator-visible stop when a hop cannot be stored (I16).
#[must_use]
pub fn hop_contract_stop_reason(user_task: &str, verdict: HopArtifactVerdict) -> String {
    let cjk = crate::agent::bounded_dag_live::user_prefers_cjk(user_task);
    match verdict {
        HopArtifactVerdict::Ok => String::new(),
        HopArtifactVerdict::Empty => {
            if cjk {
                "本跳没有可写入报告的制品：助手正文为空且未留下工具摘要。".into()
            } else {
                "This hop produced no storable artifact (empty assistant text and no tool gist)."
                    .into()
            }
        }
        HopArtifactVerdict::InsufficientEvidenceLayer => {
            if cjk {
                "本跳制品未满足声明的证据层，不能当作完成。".into()
            } else {
                "This hop artifact did not satisfy the declared evidence layers.".into()
            }
        }
    }
}

/// Operator-visible stop when the graph cannot deliver (I16).
#[must_use]
pub fn graph_contract_stop_reason(user_task: &str, verdict: GraphArtifactVerdict) -> String {
    let cjk = crate::agent::bounded_dag_live::user_prefers_cjk(user_task);
    match verdict {
        GraphArtifactVerdict::Ok => String::new(),
        GraphArtifactVerdict::AllEmpty => {
            if cjk {
                "本图没有可写入报告的节点制品：助手正文为空且未留下工具摘要，无法完成交付。".into()
            } else {
                "This graph produced no node artifacts to deliver (empty assistant text and no tool gist).".into()
            }
        }
        GraphArtifactVerdict::InsufficientEvidenceLayer => {
            if cjk {
                "节点制品未满足声明的证据层（例如协议清单或上游可用性），无法完成交付。".into()
            } else {
                "Node artifacts did not satisfy the declared evidence layers (e.g. protocol catalog or upstream reachability).".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dag_runner::{ContextRequirements, DagNode, ModelSelector};

    fn node(id: &str, artifact: Option<&str>, locus: Option<&str>) -> DagNode {
        DagNode {
            id: id.into(),
            task_type: "work".into(),
            model_selector: ModelSelector {
                capabilities: vec!["tools".into()],
            },
            context_requirements: ContextRequirements::default(),
            max_steps: None,
            next: None,
            fork: vec![],
            artifact: artifact.map(str::to_string),
            sigma: None,
            locus: locus.map(str::to_string),
        }
    }

    #[test]
    fn artifact_contract_empty_hop_fails() {
        let n = node("locate", None, None);
        assert_eq!(hop_artifact_contract(&n, "", ""), HopArtifactVerdict::Empty);
    }

    #[test]
    fn contract_gate_rejects_workspace_only_when_sigma_requires_protocol() {
        let n = node("read-manifest", Some("provider-manifest"), None);
        assert!(required_evidence_layers(&n).contains(&EvidenceLayer::ProtocolDist));
        let listing = "pwd\n./src\n./docs";
        assert_eq!(
            hop_artifact_contract(&n, listing, ""),
            HopArtifactVerdict::InsufficientEvidenceLayer
        );
    }

    #[test]
    fn yaml_body_satisfies_protocol_layer() {
        let n = node("read-manifest", Some("manifest"), None);
        let body = "providers/nvidia.yaml\nschema_version: 1";
        assert_eq!(hop_artifact_contract(&n, body, ""), HopArtifactVerdict::Ok);
    }

    #[test]
    fn listing_accumulator_plus_prose_is_not_ok() {
        let n = node("locate", None, None);
        let prose = "The repository layout is complete and the task is done.";
        let listing = "pwd\n./src\n./docs\ntotal 12";
        assert!(hop_text_is_user_visible(prose));
        assert_eq!(
            hop_artifact_contract(&n, prose, listing),
            HopArtifactVerdict::InsufficientEvidenceLayer
        );
        let n2 = node("read-manifest", Some("provider-manifest"), None);
        assert_eq!(
            hop_artifact_contract(&n2, prose, listing),
            HopArtifactVerdict::InsufficientEvidenceLayer
        );
    }

    #[test]
    fn graph_end_all_empty_artifacts_not_completed() {
        let n = node("a", None, None);
        assert_eq!(
            graph_artifact_contract(&[n], &[("a".into(), String::new())]),
            GraphArtifactVerdict::AllEmpty
        );
    }
}
