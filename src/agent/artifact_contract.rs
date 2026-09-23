//! Hop / graph artifact contract gates (VL-APE-016 / I16).
//! 制品合同门：空制品与 evidence_layer 未满足时不得假装成功。
//! P9：YAML 与 JSON internodal 的 verdict 同等；JSON 解析失败不算声明不足。

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
    PartialUnsatisfied,
}

/// P9 Ask prefix: gap in evidence/coverage, not EMPTY_I / R23, session stays open.
pub const PARTIAL_STOP_ASK: &str = "Ask: this graph still has an evidence or coverage gap. Name the missing layer or deliverable in a follow-up on this same session. This turn is not completed.";

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

/// Agent diagnostic tree: not a task locus and not evidence_layer (R20).
#[must_use]
pub fn is_agent_diagnostic_path(text: &str) -> bool {
    let t = text.replace('\\', "/").to_ascii_lowercase();
    t.contains(".velaclaw/chat_sessions")
        || t.contains(".velaclaw/tmp/graphs")
        || t.contains(".velaclaw/tmp/")
        || t.contains("tool_receipts.jsonl")
}

/// Tool output whose only paths are diagnostic (R20 / P8).
#[must_use]
pub fn is_diagnostic_only_evidence(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    is_agent_diagnostic_path(t)
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
        if crate::agent::graph_scheduler::node_sigma(node)
            == crate::agent::graph_scheduler::NodeSigma::ToolDirect
        {
            return HopArtifactVerdict::Ok;
        }
        return HopArtifactVerdict::Empty;
    }
    if is_diagnostic_only_evidence(tool_evidence) {
        return HopArtifactVerdict::InsufficientEvidenceLayer;
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
    let all_blank = artifacts.iter().all(|(_, body)| body.trim().is_empty());
    let any_tool_record = nodes.iter().any(|node| {
        crate::agent::graph_scheduler::node_sigma(node)
            == crate::agent::graph_scheduler::NodeSigma::ToolDirect
            && artifacts.iter().any(|(id, _)| id == &node.id)
    });
    if artifacts.is_empty() || (all_blank && !any_tool_record) {
        return GraphArtifactVerdict::AllEmpty;
    }
    if artifacts
        .iter()
        .any(|(_, b)| internodal_declares_unsatisfied_sigma(b))
    {
        return GraphArtifactVerdict::PartialUnsatisfied;
    }
    for node in nodes {
        if crate::agent::graph_scheduler::node_sigma(node)
            == crate::agent::graph_scheduler::NodeSigma::ToolDirect
            && !artifacts.iter().any(|(id, _)| id == &node.id)
        {
            return GraphArtifactVerdict::PartialUnsatisfied;
        }
        let body = artifacts
            .iter()
            .find(|(id, _)| id == &node.id)
            .map(|(_, b)| b.as_str())
            .unwrap_or("");
        match hop_artifact_contract(node, body, "") {
            HopArtifactVerdict::Ok => {}
            HopArtifactVerdict::Empty => {
                if crate::agent::graph_scheduler::node_sigma(node)
                    == crate::agent::graph_scheduler::NodeSigma::ToolDirect
                {
                    let present = artifacts.iter().any(|(id, _)| id == &node.id);
                    if present {
                        continue;
                    }
                    return GraphArtifactVerdict::PartialUnsatisfied;
                }
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

/// Internodal hop body declared an unsatisfied Σ (P9). Not empty-I.
///
/// YAML (`verdict: partial` and the same tokens for `failed` / `unsatisfied`)
/// and a parsed JSON object (`"verdict": "partial"`) are one gate. A JSON
/// value that parses decides only from its `verdict` field. A parse failure
/// is not a declaration — truncated JSON stays silent here (budget is not
/// this gate). Callers run this on every `dag_art` before Completed, including
/// after A20 skip-parlor.
#[must_use]
pub fn internodal_declares_unsatisfied_sigma(text: &str) -> bool {
    let trimmed = text.trim();
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => json_object_verdict_unsatisfied(&value),
        Err(_) => yaml_declares_unsatisfied_sigma(&trimmed.to_ascii_lowercase()),
    }
}

fn yaml_declares_unsatisfied_sigma(lower: &str) -> bool {
    lower.contains("verdict: partial")
        || lower.contains("verdict:partial")
        || lower.contains("**verdict: partial**")
        || lower.contains("coverage: partial")
        || lower.contains("verdict: failed")
        || lower.contains("verdict:failed")
        || lower.contains("verdict: unsatisfied")
        || lower.contains("verdict:unsatisfied")
}

fn json_object_verdict_unsatisfied(value: &serde_json::Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };
    obj.iter().any(|(key, val)| {
        key.eq_ignore_ascii_case("verdict")
            && val
                .as_str()
                .is_some_and(|s| verdict_token_unsatisfied(s.trim()))
    })
}

fn verdict_token_unsatisfied(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "partial" | "failed" | "unsatisfied"
    )
}

/// True when a stop reply is an Ask/short-reason, not a completed turn.
#[must_use]
pub fn honest_stop_keeps_session_open(stop: &str) -> bool {
    let t = stop.to_ascii_lowercase();
    stop.contains(PARTIAL_STOP_ASK)
        && t.contains("same session")
        && !t.contains("new session")
        && !t.contains("empty i")
}

/// Operator-visible stop when the graph cannot deliver (I16 / P9).
#[must_use]
pub fn graph_contract_stop_reason(user_task: &str, verdict: GraphArtifactVerdict) -> String {
    let cjk = crate::agent::bounded_dag_live::user_prefers_cjk(user_task);
    match verdict {
        GraphArtifactVerdict::Ok => String::new(),
        GraphArtifactVerdict::AllEmpty => {
            if cjk {
                format!(
                    "{PARTIAL_STOP_ASK} 本图没有可写入报告的节点制品（助手正文为空且未留下工具摘要）。"
                )
            } else {
                format!(
                    "{PARTIAL_STOP_ASK} This graph produced no node artifacts to deliver (empty assistant text and no tool gist)."
                )
            }
        }
        GraphArtifactVerdict::InsufficientEvidenceLayer => {
            if cjk {
                format!(
                    "{PARTIAL_STOP_ASK} 节点制品未满足声明的证据层（例如协议清单或上游可用性）。"
                )
            } else {
                format!(
                    "{PARTIAL_STOP_ASK} Node artifacts did not satisfy the declared evidence layers (for example protocol catalog or upstream reachability)."
                )
            }
        }
        GraphArtifactVerdict::PartialUnsatisfied => {
            if cjk {
                format!("{PARTIAL_STOP_ASK} 制品仍是 Σ/覆盖缺口（partial），不是完成。")
            } else {
                format!(
                    "{PARTIAL_STOP_ASK} Artifacts still show an unsatisfied Σ or partial coverage gap."
                )
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
    fn diagnostic_path_is_not_task_evidence() {
        assert!(is_agent_diagnostic_path(
            ".velaclaw/chat_sessions/abcd.json: session notes"
        ));
        assert!(is_agent_diagnostic_path(
            "workspace/.velaclaw/tmp/graphs/sess/one-node/admit.json"
        ));
        assert!(is_diagnostic_only_evidence(
            "grep hit .velaclaw/chat_sessions/old.json\nprotocol-looking prose"
        ));
        let n = node("work", None, None);
        let prose = "Architecture share is feasible at the protocol layer.";
        let diag = "grep -r notes .velaclaw/chat_sessions/foo.json | head -20";
        assert_eq!(
            hop_artifact_contract(&n, prose, diag),
            HopArtifactVerdict::InsufficientEvidenceLayer
        );
        assert!(!is_agent_diagnostic_path("workspace/src/lib.rs"));
    }

    #[test]
    fn graph_end_all_empty_artifacts_not_completed() {
        let n = node("a", None, None);
        assert_eq!(
            graph_artifact_contract(&[n], &[("a".into(), String::new())]),
            GraphArtifactVerdict::AllEmpty
        );
    }

    #[test]
    fn nonempty_dag_art_unsatisfied_sigma_is_not_completed() {
        let n = node("a", None, None);
        let body = "HANDOFF\nverdict: partial\nfindings:\n- listing only\ngaps:\n- protocol layer";
        assert!(internodal_declares_unsatisfied_sigma(body));
        assert_eq!(
            graph_artifact_contract(std::slice::from_ref(&n), &[("a".into(), body.into())]),
            GraphArtifactVerdict::PartialUnsatisfied
        );
        let stop = graph_contract_stop_reason(
            "check the protocol catalog",
            GraphArtifactVerdict::PartialUnsatisfied,
        );
        assert!(honest_stop_keeps_session_open(&stop));
        assert!(stop.to_ascii_lowercase().contains("not completed"));
        assert!(!stop.contains(crate::agent::graph_scheduler::EMPTY_I_ASK));
    }

    #[test]
    fn yaml_and_json_internodal_declare_unsatisfied_equally() {
        let yaml = "HANDOFF\nverdict: partial\nfindings:\n- gap\n";
        let json = "{\n  \"verdict\": \"Partial\"\n}\n";
        assert!(internodal_declares_unsatisfied_sigma(yaml));
        assert!(internodal_declares_unsatisfied_sigma(json));
        assert!(internodal_declares_unsatisfied_sigma("verdict: failed"));
        assert!(internodal_declares_unsatisfied_sigma("verdict:unsatisfied"));
        assert!(internodal_declares_unsatisfied_sigma(
            r#"{"Verdict":"failed"}"#
        ));
        assert!(internodal_declares_unsatisfied_sigma(
            r#"{"verdict":"unsatisfied"}"#
        ));
        assert!(!internodal_declares_unsatisfied_sigma(
            r#"{"verdict":"ok","note":"verdict: partial"}"#
        ));
        assert!(!internodal_declares_unsatisfied_sigma(r#"{"note":"done"}"#));
    }

    #[test]
    fn truncated_json_is_not_declared_unsatisfied() {
        let cut = r#"{"verdict": "partial", "gap": "still ope"#;
        assert!(serde_json::from_str::<serde_json::Value>(cut).is_err());
        assert!(!internodal_declares_unsatisfied_sigma(cut));
    }

    #[test]
    fn skip_parlor_still_gates_json_internodal_before_complete() {
        let mid = r#"{"verdict":"partial","note":"gap remains"}"#;
        let last = "The catalog check is finished.";
        assert!(crate::agent::graph_scheduler::skip_parlor_llm(
            2, last, false
        ));
        assert_eq!(
            graph_artifact_contract(
                &[node("mid", None, None), node("last", None, None)],
                &[("mid".into(), mid.into()), ("last".into(), last.into())],
            ),
            GraphArtifactVerdict::PartialUnsatisfied
        );
        let stop = graph_contract_stop_reason(
            "check the catalog",
            GraphArtifactVerdict::PartialUnsatisfied,
        );
        assert!(honest_stop_keeps_session_open(&stop));
        assert!(stop.to_ascii_lowercase().contains("not completed"));
    }

    #[test]
    fn missing_tooldirect_receipt_is_ask_not_completed() {
        let n = DagNode {
            id: "list".into(),
            task_type: "shell.exec".into(),
            model_selector: ModelSelector {
                capabilities: vec!["shell.exec".into()],
            },
            context_requirements: ContextRequirements::default(),
            max_steps: None,
            next: None,
            fork: vec![],
            artifact: Some("gh repo list".into()),
            sigma: Some("tool_direct".into()),
            locus: None,
        };
        assert_eq!(
            crate::agent::graph_scheduler::node_sigma(&n),
            crate::agent::graph_scheduler::NodeSigma::ToolDirect
        );
        let cog = node("cmp", None, None);
        assert_eq!(
            graph_artifact_contract(
                &[n.clone(), cog.clone()],
                &[
                    ("list".into(), String::new()),
                    ("cmp".into(), "comparison without the planned list".into()),
                ]
            ),
            GraphArtifactVerdict::Ok
        );
        assert_eq!(
            graph_artifact_contract(
                &[n, cog],
                &[("cmp".into(), "comparison without the planned list".into())]
            ),
            GraphArtifactVerdict::PartialUnsatisfied
        );
        let stop =
            graph_contract_stop_reason("list the repos", GraphArtifactVerdict::PartialUnsatisfied);
        assert!(honest_stop_keeps_session_open(&stop));
        assert!(stop.to_ascii_lowercase().contains("not completed"));
    }

    #[test]
    fn partial_stop_keeps_session_continuable() {
        let stop = graph_contract_stop_reason(
            "continue here",
            GraphArtifactVerdict::InsufficientEvidenceLayer,
        );
        assert!(honest_stop_keeps_session_open(&stop));
    }
}
