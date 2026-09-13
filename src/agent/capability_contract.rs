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
    let marker = " on ";
    let idx = lower.find(marker)?;
    let after = user_task[idx + marker.len()..].trim_start();
    let tok = first_host_token(after)?;
    let tok_lower = tok.to_ascii_lowercase();
    if STOP_HOST_WORDS.contains(&tok_lower.as_str()) {
        return None;
    }
    Some(tok)
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
            rewrite_tool_direct_node(node, user_task, remote.as_deref());
        } else {
            fill_defaults(node, remote.as_deref());
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

fn rewrite_tool_direct_node(node: &mut DagNode, user_task: &str, remote: Option<&str>) {
    node.model_selector.capabilities = vec!["shell.exec".into()];
    node.sigma = Some("tool_direct".into());
    let simple = simple_status_command(user_task);
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
}
