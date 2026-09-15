//! Host capability contract for planner nodes (VL-APE-011 / I11).
//! 规划期 Σ + locus；工具形直调；入闸不安全的 I 不得进入 G。

use super::dag_runner::{DagManifest, DagNode};
use super::graph_scheduler::{node_sigma, NodeSigma};
use crate::security::SecurityPolicy;
use anyhow::{bail, Result};

const STOP_HOST_WORDS: &[&str] = &[
    "the",
    "this",
    "that",
    "my",
    "our",
    "a",
    "an",
    "local",
    "workspace",
    "host",
    "server",
    "machine",
    "node",
    "box",
    "git",
    "ssh",
    "ls",
    "disk",
    "file",
    "files",
    "repo",
    "origin",
];

/// True when the user asked for inspect/list/status that allowed tools can finish.
#[must_use]
pub fn user_task_is_tool_shaped(user_task: &str) -> bool {
    let t = user_task.to_ascii_lowercase();
    if cognition_override(&t) {
        return false;
    }
    t.contains("list ")
        || t.contains("inspect")
        || t.contains("status")
        || t.contains("uptime")
        || t.contains("what files")
        || t.contains("ls ")
        || t.contains("directory")
        || t.contains("which services")
}

fn cognition_override(t: &str) -> bool {
    t.contains("patch")
        || t.contains("fix the")
        || t.contains("implement")
        || t.contains("refactor")
        || t.contains("compiler error")
        || t.contains("write a report")
}

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

/// Named remote from user text or configured aliases (never a product-default hostname).
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
    if let Some(tok) = locative_adjacent_host(user_task) {
        return Some(tok);
    }
    if let Some(idx) = user_task.find('@') {
        if let Some(tok) = first_host_token(user_task[idx + 1..].trim_start()) {
            if is_named_host_token(tok) {
                return Some(tok);
            }
        }
    }
    for marker in [" on ", " at ", " via "] {
        if let Some(idx) = lower.find(marker) {
            let after = user_task[idx + marker.len()..].trim_start();
            if let Some(tok) = first_host_token(after) {
                if is_named_host_token(tok) {
                    return Some(tok);
                }
            }
        }
    }
    None
}

fn first_host_token(s: &str) -> Option<&str> {
    let tok = s
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == ':')
        .next()?
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '.');
    if tok.is_empty() {
        return None;
    }
    if !tok
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return None;
    }
    Some(tok)
}

fn is_named_host_token(tok: &str) -> bool {
    let lower = tok.to_ascii_lowercase();
    if STOP_HOST_WORDS.contains(&lower.as_str()) {
        return false;
    }
    let mut chars = tok.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    tok.len() >= 2
        && tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// ASCII host immediately before CJK 上 or after CJK 在 (script-agnostic locative).
fn locative_adjacent_host(s: &str) -> Option<&str> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let (byte_start, ch) = chars[i];
        if ch.is_ascii_alphabetic() {
            let mut j = i + 1;
            while j < chars.len() {
                let c = chars[j].1;
                if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                    j += 1;
                } else {
                    break;
                }
            }
            let byte_end = if j < chars.len() { chars[j].0 } else { s.len() };
            let tok = &s[byte_start..byte_end];
            if is_named_host_token(tok) {
                if j < chars.len() && chars[j].1 == '上' {
                    return Some(tok);
                }
                if i > 0 && chars[i - 1].1 == '在' {
                    return Some(tok);
                }
            }
            i = j;
            continue;
        }
        i += 1;
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

/// Rewrite tool-shaped plans to ToolDirect I; reject unsafe constructs before E.
pub fn admit_capability_contract(
    dag: &mut DagManifest,
    user_task: &str,
    policy: &SecurityPolicy,
    extra_aliases: &[String],
) -> Result<()> {
    let tool_shaped = user_task_is_tool_shaped(user_task);
    let remote = remote_alias_from_user(user_task, extra_aliases).map(str::to_string);
    for node in &mut dag.nodes {
        if tool_shaped {
            rewrite_tool_direct_node(node, remote.as_deref());
        } else {
            fill_defaults(node, remote.as_deref());
            if node_sigma(node) == NodeSigma::ToolDirect && artifact_command(node).is_none() {
                apply_admit_safe_i(node, remote.as_deref());
            }
        }
        if node_sigma(node) == NodeSigma::ToolDirect {
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
        }
    }
    admit_graph_shape(dag, remote.as_deref())?;
    Ok(())
}

fn node_declares_readonly_sigma(node: &DagNode) -> bool {
    !crate::agent::artifact_contract::required_evidence_layers(node).is_empty()
}

/// I19: all-LLM graphs that declare readonly evidence layers must get ToolDirect I.
fn admit_graph_shape(dag: &mut DagManifest, remote: Option<&str>) -> Result<()> {
    if dag.nodes.is_empty() {
        return Ok(());
    }
    let all_llm = dag
        .nodes
        .iter()
        .all(|n| node_sigma(n) == NodeSigma::LlmWork);
    if !all_llm {
        return Ok(());
    }
    let readonly: Vec<String> = dag
        .nodes
        .iter()
        .filter(|n| node_declares_readonly_sigma(n))
        .map(|n| n.id.clone())
        .collect();
    if readonly.is_empty() {
        return Ok(());
    }
    for node in &mut dag.nodes {
        if readonly.iter().any(|id| id == &node.id) {
            rewrite_tool_direct_node(node, remote);
        }
    }
    if dag
        .nodes
        .iter()
        .all(|n| node_sigma(n) == NodeSigma::LlmWork)
    {
        bail!("plan rejected: readonly evidence-layer nodes cannot all be LLM hops");
    }
    Ok(())
}

fn fill_defaults(node: &mut DagNode, remote: Option<&str>) {
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
        node.locus = Some(match remote {
            Some(alias) => format!("remote:{alias}"),
            None => "workspace".into(),
        });
    }
}

fn rewrite_tool_direct_node(node: &mut DagNode, remote: Option<&str>) {
    node.model_selector.capabilities = vec!["shell.exec".into()];
    node.sigma = Some("tool_direct".into());
    apply_admit_safe_i(node, remote);
}

fn apply_admit_safe_i(node: &mut DagNode, remote: Option<&str>) {
    let simple = simple_status_command("");
    let cmd = match remote {
        Some(alias) if !simple.trim_start().to_ascii_lowercase().starts_with("ssh ") => {
            format!("ssh {alias} {simple}")
        }
        _ => simple,
    };
    node.artifact = Some(cmd);
    node.locus = Some(match remote {
        Some(alias) => format!("remote:{alias}"),
        None => "workspace".into(),
    });
}

fn simple_status_command(_user_task: &str) -> String {
    "ls".into()
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
    fn tool_shaped_plan_is_tool_direct() {
        let mut dag = parse_dag_json(&coding_node_json("echo hi")).unwrap();
        admit_capability_contract(&mut dag, "list files in the workspace", &policy(), &[]).unwrap();
        let n = &dag.nodes[0];
        assert_eq!(node_sigma(n), NodeSigma::ToolDirect);
        assert_eq!(n.sigma.as_deref(), Some("tool_direct"));
        assert_eq!(n.locus.as_deref(), Some("workspace"));
        assert!(!artifact_command(n).unwrap().contains("$("));
    }

    #[test]
    fn remote_locus_prefixes_ssh_alias() {
        let mut dag = parse_dag_json(&coding_node_json("ls")).unwrap();
        admit_capability_contract(&mut dag, "list services on lab-host", &policy(), &[]).unwrap();
        let n = &dag.nodes[0];
        assert_eq!(n.locus.as_deref(), Some("remote:lab-host"));
        let cmd = artifact_command(n).unwrap();
        assert!(cmd.starts_with("ssh lab-host "), "{cmd}");
        assert!(!cmd.contains("$("));
        assert!(!cmd.contains('>'));
        assert_eq!(node_sigma(n), NodeSigma::ToolDirect);
    }

    #[test]
    fn extra_alias_without_on_token_sets_remote_locus() {
        let mut dag = parse_dag_json(&coding_node_json("ls")).unwrap();
        admit_capability_contract(
            &mut dag,
            "list files on lab-host please",
            &policy(),
            &["lab-host".into()],
        )
        .unwrap();
        assert_eq!(dag.nodes[0].locus.as_deref(), Some("remote:lab-host"));
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
    fn empty_tool_direct_i_fills_locative_ssh() {
        let mut dag = parse_dag_json(&empty_tool_direct_json()).unwrap();
        admit_capability_contract(
            &mut dag,
            "inspect disk lab-host上 and continue",
            &policy(),
            &[],
        )
        .unwrap();
        let n = &dag.nodes[0];
        assert_eq!(n.locus.as_deref(), Some("remote:lab-host"));
        assert_eq!(artifact_command(n).as_deref(), Some("ssh lab-host ls"));
    }

    #[test]
    fn empty_tool_direct_i_fills_workspace_ls() {
        let mut dag = parse_dag_json(&empty_tool_direct_json()).unwrap();
        admit_capability_contract(
            &mut dag,
            "summarize the architecture in prose",
            &policy(),
            &[],
        )
        .unwrap();
        let n = &dag.nodes[0];
        assert_eq!(n.locus.as_deref(), Some("workspace"));
        assert_eq!(artifact_command(n).as_deref(), Some("ls"));
    }

    #[test]
    fn locative_host_is_token_before_place_particle() {
        assert_eq!(
            remote_alias_from_user("lab-host上git", &[]),
            Some("lab-host")
        );
        assert_eq!(
            remote_alias_from_user("check 在lab-host please", &[]),
            Some("lab-host")
        );
        assert_eq!(
            remote_alias_from_user("inspect @lab-host now", &[]),
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
        let mut dag = parse_dag_json(&coding_node_json("ls")).unwrap();
        admit_capability_contract(&mut dag, "list files lab-host please", &policy(), &aliases)
            .unwrap();
        assert_eq!(dag.nodes[0].locus.as_deref(), Some("remote:lab-host"));
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
        assert!(artifact_command(n).is_some());
    }

    #[test]
    fn remote_locus_ssh_prefix_unchanged() {
        let mut dag = parse_dag_json(&coding_node_json("ls")).unwrap();
        admit_capability_contract(&mut dag, "list services on lab-host", &policy(), &[]).unwrap();
        let cmd = artifact_command(&dag.nodes[0]).unwrap();
        assert!(cmd.starts_with("ssh lab-host "), "{cmd}");
    }
}
