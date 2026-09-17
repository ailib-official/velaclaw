//! Host capability contract for planner nodes (VL-APE-011 / I11, VL-APE-018).
//! 规划器填 DAG；入闸只验结构合同。不按用户原文语种/关键词分流。

use super::dag_runner::{DagManifest, DagNode};
use super::graph_scheduler::{direct_tool_call, node_sigma, NodeSigma};
use crate::security::SecurityPolicy;
use anyhow::{bail, Result};

/// `deploy.servers` `id` and `host` as extra aliases (no product-default hostname).
#[must_use]
pub fn host_aliases_from_deploy(servers: &[crate::config::DeploymentTargetConfig]) -> Vec<String> {
    let mut out = Vec::new();
    for s in servers {
        for raw in [&s.id, &s.host] {
            let t = raw.trim();
            if t.is_empty() {
                continue;
            }
            if !out.iter().any(|e: &String| e.eq_ignore_ascii_case(t)) {
                out.push(t.to_string());
            }
        }
    }
    out
}

/// Configured remote alias if it appears as a token in `user_task` (not NLP).
#[must_use]
pub fn remote_alias_from_user<'a>(
    user_task: &'a str,
    extra_aliases: &'a [String],
) -> Option<&'a str> {
    let lower = user_task.to_ascii_lowercase();
    for alias in extra_aliases {
        let a = alias.trim();
        if a.is_empty() {
            continue;
        }
        if lower
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '.')
            .any(|w| w.eq_ignore_ascii_case(a))
        {
            return Some(alias.trim());
        }
    }
    None
}

fn artifact_command(node: &DagNode) -> Option<String> {
    let raw = node.artifact.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(c) = v.get("command").and_then(|x| x.as_str()) {
            return Some(c.to_string());
        }
    }
    Some(raw.to_string())
}

fn locus_remote_alias(node: &DagNode) -> Option<&str> {
    node.locus
        .as_deref()
        .map(str::trim)
        .and_then(|l| l.strip_prefix("remote:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn ssh_alias<'a>(node: &'a DagNode, configured: Option<&'a str>) -> Option<&'a str> {
    locus_remote_alias(node).or(configured)
}

/// Admit: verify DAG fields; never rewrite the whole graph from user-language heuristics.
pub fn admit_capability_contract(
    dag: &mut DagManifest,
    user_task: &str,
    policy: &SecurityPolicy,
    extra_aliases: &[String],
) -> Result<()> {
    let configured = remote_alias_from_user(user_task, extra_aliases).map(str::to_string);
    for node in &mut dag.nodes {
        fill_defaults(node);
        if node_sigma(node) == NodeSigma::ToolDirect {
            normalize_tool_direct_invoke(node, configured.as_deref())?;
        }
        gate_tool_direct(node, policy)?;
    }
    admit_graph_shape(dag, configured.as_deref(), policy)?;
    Ok(())
}

fn gate_tool_direct(node: &DagNode, policy: &SecurityPolicy) -> Result<()> {
    if node_sigma(node) != NodeSigma::ToolDirect {
        return Ok(());
    }
    let Some(cmd) = artifact_command(node) else {
        bail!(
            "tool_direct node `{}` missing admit-safe I (artifact)",
            node.id
        );
    };
    if !policy.passes_shell_safety_gates(&cmd) {
        bail!(
            "plan rejected: node `{}` I matches unsafe_construct (substitution, redirect, or find -exec)",
            node.id
        );
    }
    if let Err(err) = direct_tool_call(node) {
        bail!("plan rejected: {err}");
    }
    Ok(())
}

fn node_declares_readonly_sigma(node: &DagNode) -> bool {
    !crate::agent::artifact_contract::required_evidence_layers(node).is_empty()
}

/// I19: all-LLM graphs that declare readonly evidence layers must get ToolDirect I.
fn admit_graph_shape(
    dag: &mut DagManifest,
    configured: Option<&str>,
    policy: &SecurityPolicy,
) -> Result<()> {
    if dag.nodes.is_empty() {
        return Ok(());
    }
    let all_llm = dag
        .nodes
        .iter()
        .all(|n| node_sigma(n) == NodeSigma::LlmWork);
    if all_llm {
        let any_i = dag.nodes.iter().any(|n| artifact_command(n).is_some());
        if !any_i {
            let tool_shaped = dag.nodes.iter().all(|n| {
                crate::agent::capability_route::node_is_tool_invoke_without_cognition(
                    &n.model_selector.capabilities,
                )
            });
            if tool_shaped {
                bail!(
                    "plan rejected: empty I on LLM hops; Ask for a command or permission (do not invent ls)"
                );
            }
        }
        let readonly: Vec<String> = dag
            .nodes
            .iter()
            .filter(|n| node_declares_readonly_sigma(n))
            .map(|n| n.id.clone())
            .collect();
        if !readonly.is_empty() {
            for node in &mut dag.nodes {
                if readonly.iter().any(|id| id == &node.id) {
                    normalize_tool_direct_invoke(node, configured)?;
                }
            }
            if dag
                .nodes
                .iter()
                .all(|n| node_sigma(n) == NodeSigma::LlmWork)
            {
                bail!("plan rejected: readonly evidence-layer nodes cannot all be LLM hops");
            }
        }
    }
    for node in &dag.nodes {
        gate_tool_direct(node, policy)?;
    }
    Ok(())
}

fn fill_defaults(node: &mut DagNode) {
    if node.sigma.is_none() {
        node.sigma = Some(
            if node_sigma(node) == NodeSigma::ToolDirect {
                "tool_direct"
            } else {
                "llm_cognition"
            }
            .into(),
        );
    }
    if node.locus.is_none() {
        node.locus = Some("workspace".into());
    }
}

/// Make ToolDirect invocable without replacing a planner-supplied I.
fn normalize_tool_direct_invoke(node: &mut DagNode, configured: Option<&str>) -> Result<()> {
    node.sigma = Some("tool_direct".into());
    if direct_tool_call(node).is_err() {
        node.model_selector.capabilities = vec!["shell.exec".into()];
        node.task_type = "shell.exec".into();
    }
    let alias = ssh_alias(node, configured).map(str::to_string);
    if artifact_command(node).is_none() {
        let layers = crate::agent::artifact_contract::required_evidence_layers(node);
        if layers.contains(&crate::agent::artifact_contract::EvidenceLayer::ProtocolDist) {
            bail!(
                "tool_direct node `{}` missing admit-safe I (artifact)",
                node.id
            );
        }
        apply_admit_safe_i(node, alias.as_deref());
    } else if node.locus.is_none() {
        node.locus = Some(match alias {
            Some(a) => format!("remote:{a}"),
            None => "workspace".into(),
        });
    }
    Ok(())
}

fn apply_admit_safe_i(node: &mut DagNode, remote: Option<&str>) {
    let simple = "ls";
    let cmd = match remote {
        Some(alias) if !simple.trim_start().to_ascii_lowercase().starts_with("ssh ") => {
            format!("ssh {alias} {simple}")
        }
        _ => simple.to_string(),
    };
    node.artifact = Some(cmd);
    node.locus = Some(match remote {
        Some(alias) => format!("remote:{alias}"),
        None => "workspace".into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::dag_runner::parse_dag_json;
    use crate::security::AutonomyLevel;

    fn policy() -> SecurityPolicy {
        SecurityPolicy {
            autonomy: AutonomyLevel::Full,
            ..SecurityPolicy::default()
        }
    }

    fn coding_node_json(artifact: &str) -> String {
        format!(
            r#"{{"schema_version":"0.1.0","id":"t","entry":"n1","max_steps":8,"nodes":[{{"id":"n1","task_type":"ops","model_selector":{{"capabilities":["coding"]}},"next":null,"artifact":{}}}]}}"#,
            serde_json::to_string(artifact).unwrap()
        )
    }

    #[test]
    fn user_keywords_do_not_force_tool_direct() {
        let mut dag = parse_dag_json(&coding_node_json("echo hi")).unwrap();
        admit_capability_contract(&mut dag, "list files in the workspace", &policy(), &[]).unwrap();
        let n = &dag.nodes[0];
        assert_eq!(node_sigma(n), NodeSigma::LlmWork);
        assert_eq!(n.sigma.as_deref(), Some("llm_cognition"));
        assert_eq!(artifact_command(n).as_deref(), Some("echo hi"));
    }

    #[test]
    fn english_on_host_without_alias_does_not_invent_remote() {
        let mut dag = parse_dag_json(&coding_node_json("ls")).unwrap();
        admit_capability_contract(&mut dag, "list services on lab-host", &policy(), &[]).unwrap();
        let n = &dag.nodes[0];
        assert_eq!(node_sigma(n), NodeSigma::LlmWork);
        assert_eq!(n.locus.as_deref(), Some("workspace"));
        assert_eq!(artifact_command(n).as_deref(), Some("ls"));
    }

    #[test]
    fn extra_alias_token_sets_remote_locus_on_empty_tool_direct() {
        let mut dag = parse_dag_json(&empty_tool_direct_json()).unwrap();
        admit_capability_contract(
            &mut dag,
            "please check lab-host whenever convenient",
            &policy(),
            &["lab-host".into()],
        )
        .unwrap();
        assert_eq!(dag.nodes[0].locus.as_deref(), Some("remote:lab-host"));
        assert_eq!(
            artifact_command(&dag.nodes[0]).as_deref(),
            Some("ssh lab-host ls")
        );
        direct_tool_call(&dag.nodes[0]).unwrap();
    }

    #[test]
    fn unsafe_i_rejected_before_execute() {
        let mut dag = parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"t","entry":"n1","max_steps":8,"nodes":[{"id":"n1","task_type":"ops","model_selector":{"capabilities":["shell.exec"]},"next":null,"artifact":"echo $(whoami) > /tmp/x"}]}"#,
        )
        .unwrap();
        let err = admit_capability_contract(
            &mut dag,
            "summarize the architecture in prose",
            &policy(),
            &[],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unsafe_construct"), "{err}");
    }

    #[test]
    fn cognition_task_remains_llm_work() {
        let mut dag = parse_dag_json(crate::agent::dag_runner::CODE_FIX_TEMPLATE_JSON).unwrap();
        admit_capability_contract(
            &mut dag,
            "fix the compiler error in src/main.rs",
            &policy(),
            &[],
        )
        .unwrap();
        assert_eq!(node_sigma(&dag.nodes[0]), NodeSigma::LlmWork);
        assert_eq!(dag.nodes[0].sigma.as_deref(), Some("llm_cognition"));
        assert_eq!(dag.nodes[0].locus.as_deref(), Some("workspace"));
    }

    fn empty_tool_direct_json() -> String {
        r#"{"schema_version":"0.1.0","id":"t","entry":"svc_status","max_steps":8,"nodes":[{"id":"svc_status","task_type":"ops","model_selector":{"capabilities":["shell.exec"]},"sigma":"tool_direct","next":null}]}"#.into()
    }

    #[test]
    fn empty_tool_direct_i_fills_workspace_ls() {
        let mut dag = parse_dag_json(&empty_tool_direct_json()).unwrap();
        admit_capability_contract(&mut dag, "any natural language task text", &policy(), &[])
            .unwrap();
        let n = &dag.nodes[0];
        assert_eq!(n.locus.as_deref(), Some("workspace"));
        assert_eq!(artifact_command(n).as_deref(), Some("ls"));
        direct_tool_call(n).unwrap();
    }

    #[test]
    fn unconfigured_host_token_is_not_parsed_from_user_text() {
        assert_eq!(remote_alias_from_user("lab-host上git", &[]), None);
        assert_eq!(remote_alias_from_user("check 在lab-host please", &[]), None);
        assert_eq!(remote_alias_from_user("inspect @lab-host now", &[]), None);
        let aliases = ["lab-host".to_string()];
        assert_eq!(
            remote_alias_from_user("inspect lab-host now", &aliases),
            Some("lab-host")
        );
    }

    #[test]
    fn deploy_aliases_match_without_english_on() {
        let aliases = host_aliases_from_deploy(&[crate::config::DeploymentTargetConfig {
            id: "lab".into(),
            host: "lab-host".into(),
            port: 22,
            user: "ops".into(),
            ssh_key: None,
            labels: vec![],
        }]);
        let mut dag = parse_dag_json(&empty_tool_direct_json()).unwrap();
        admit_capability_contract(&mut dag, "list files lab-host please", &policy(), &aliases)
            .unwrap();
        assert_eq!(dag.nodes[0].locus.as_deref(), Some("remote:lab-host"));
        direct_tool_call(&dag.nodes[0]).unwrap();
    }

    #[test]
    fn admit_rejects_all_llm_readonly_sigma() {
        let mut dag = parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"g","entry":"a","max_steps":4,"nodes":[{"id":"a","task_type":"ops","model_selector":{"capabilities":["tools"]},"artifact":"provider-manifest","next":"b"},{"id":"b","task_type":"ops","model_selector":{"capabilities":["tools"]},"artifact":"catalog","next":null}]}"#,
        )
        .unwrap();
        admit_capability_contract(&mut dag, "audit provider manifests", &policy(), &[]).unwrap();
        assert!(
            dag.nodes
                .iter()
                .any(|n| node_sigma(n) == NodeSigma::ToolDirect),
            "readonly graph must not stay all-LLM"
        );
        assert_eq!(
            artifact_command(&dag.nodes[0]).as_deref(),
            Some("provider-manifest")
        );
        direct_tool_call(&dag.nodes[0]).unwrap();
    }

    #[test]
    fn tool_direct_keeps_planner_i_and_closes_invoke() {
        let mut dag = parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"g","entry":"n1","max_steps":2,"nodes":[{"id":"n1","task_type":"ops","model_selector":{"capabilities":["coding"]},"sigma":"tool_direct","artifact":"pwd","next":null}]}"#,
        )
        .unwrap();
        admit_capability_contract(&mut dag, "arbitrary user text", &policy(), &[]).unwrap();
        let n = &dag.nodes[0];
        assert_eq!(node_sigma(n), NodeSigma::ToolDirect);
        assert_eq!(artifact_command(n).as_deref(), Some("pwd"));
        let call = direct_tool_call(n).unwrap();
        assert_eq!(call.name, "shell");
    }

    #[test]
    fn tool_direct_read_manifest_shape() {
        let mut dag = parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"g","entry":"read","max_steps":2,"nodes":[{"id":"read","task_type":"ops","model_selector":{"capabilities":["tools"]},"artifact":"manifest","next":null}]}"#,
        )
        .unwrap();
        admit_capability_contract(&mut dag, "summarize this code patch", &policy(), &[]).unwrap();
        let n = &dag.nodes[0];
        assert_eq!(node_sigma(n), NodeSigma::ToolDirect);
        assert_eq!(n.sigma.as_deref(), Some("tool_direct"));
        assert_eq!(artifact_command(n).as_deref(), Some("manifest"));
        direct_tool_call(n).unwrap();
    }

    #[test]
    fn remote_locus_ssh_prefix_from_configured_alias() {
        let mut dag = parse_dag_json(&empty_tool_direct_json()).unwrap();
        admit_capability_contract(
            &mut dag,
            "list services on lab-host",
            &policy(),
            &["lab-host".into()],
        )
        .unwrap();
        let cmd = artifact_command(&dag.nodes[0]).unwrap();
        assert!(cmd.starts_with("ssh lab-host "), "{cmd}");
        direct_tool_call(&dag.nodes[0]).unwrap();
    }

    #[test]
    fn empty_tool_direct_protocol_layer_does_not_invent_ls() {
        let mut dag = parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"t","entry":"read-manifest","max_steps":2,"nodes":[{"id":"read-manifest","task_type":"ops","model_selector":{"capabilities":["shell.exec"]},"sigma":"tool_direct","next":null}]}"#,
        )
        .unwrap();
        let err = admit_capability_contract(&mut dag, "any user text", &policy(), &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("missing admit-safe I"), "{err}");
    }

    #[test]
    fn empty_i_llm_graph_asks_or_rejects_without_ls() {
        let mut dag = parse_dag_json(
            r#"{"schema_version":"0.1.0","id":"g","entry":"a","max_steps":4,"nodes":[{"id":"a","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"sigma":"llm_cognition","next":"b"},{"id":"b","task_type":"ops-check","model_selector":{"capabilities":["tool_calling"]},"sigma":"llm_cognition","next":null}]}"#,
        )
        .unwrap();
        let err = admit_capability_contract(&mut dag, "any natural language task", &policy(), &[])
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty I"), "{err}");
        assert!(err.contains("Ask"), "{err}");
        assert!(
            dag.nodes.iter().all(|n| artifact_command(n).is_none()),
            "must not invent ls"
        );
    }
}
