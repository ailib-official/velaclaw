//! Per-hop probe fingerprinting: skip repeat shells and script_vN ladders.
//! 同一 hop 内跳过重复探测与 script_vN 梯子。

use serde_json::Value;

pub const REPEAT_PROBE_NOTICE: &str = "Host skipped a repeat probe (same fingerprint as an earlier call this hop). Use INPUTS; compound remaining work or HANDOFF.";

pub const SHELL_ROUND_CAP_NOTICE: &str = "Host capped this hop at four shell rounds. HANDOFF with current INPUTS; do not start script_v2/v3.";

pub const MAX_SHELL_ROUNDS_PER_HOP: u32 = 4;

/// Fingerprint a tool call so equivalent probes collapse (whitespace + script version).
#[must_use]
pub fn tool_probe_fingerprint(name: &str, arguments: &Value) -> String {
    let n = name.trim().to_ascii_lowercase();
    if n == "shell" {
        let cmd = arguments
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("");
        format!("shell:{}", normalize_shell_command(cmd))
    } else {
        format!("{n}:{arguments}")
    }
}

#[must_use]
pub fn normalize_shell_command(command: &str) -> String {
    command
        .split_whitespace()
        .map(normalize_path_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_path_token(token: &str) -> String {
    let Some(stem) = token.strip_suffix(".py") else {
        return token.to_string();
    };
    let (dir, file) = match stem.rfind('/') {
        Some(i) => (&stem[..=i], &stem[i + 1..]),
        None => ("", stem),
    };
    format!("{dir}{}.py", strip_script_version(file))
}

/// `xray_audit2` / `script_v3` / `confirm_v2` → unversioned stem.
#[must_use]
pub fn strip_script_version(stem: &str) -> String {
    if let Some(i) = stem.rfind("_v") {
        let rest = &stem[i + 2..];
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            return stem[..i].to_string();
        }
    }
    let stripped = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    if stripped.len() >= 2
        && stripped.len() < stem.len()
        && stripped
            .chars()
            .last()
            .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return stripped.to_string();
    }
    stem.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn collapses_script_version_ladder() {
        let a = tool_probe_fingerprint(
            "shell",
            &json!({"command": "python3 /tmp/.velaclaw/xray_audit.py"}),
        );
        let b = tool_probe_fingerprint(
            "shell",
            &json!({"command": "python3 /tmp/.velaclaw/xray_audit2.py"}),
        );
        let c = tool_probe_fingerprint(
            "shell",
            &json!({"command": "python3 /tmp/.velaclaw/xray_audit_v3.py"}),
        );
        assert_eq!(a, b);
        assert_eq!(a, c);
    }

    #[test]
    fn collapses_whitespace() {
        let a = tool_probe_fingerprint("shell", &json!({"command": "arp -an"}));
        let b = tool_probe_fingerprint("shell", &json!({"command": "  arp   -an  "}));
        assert_eq!(a, b);
    }

    #[test]
    fn leaves_python3_token_alone() {
        assert_eq!(normalize_path_token("python3"), "python3");
    }

    #[test]
    fn repeat_set() {
        let mut seen = HashSet::new();
        let fp = tool_probe_fingerprint("shell", &json!({"command": "pwd"}));
        assert!(!seen.contains(&fp));
        seen.insert(fp.clone());
        assert!(seen.contains(&fp));
    }
}
