//! Shared turn progress mapping for CLI and Web (VL-UX-STEP-001/002 / GOV-007).
//! CLI 与 Web 共用 caption；展开正文同一 `progress_expand_body`，默认不刷 stdout。

use crate::observability::traits::ObserverMetric;
use crate::observability::{Observer, ObserverEvent};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::Sender;
use velaclaw_agent_runtime::scrub_credentials;

const CAPTION_MAX_CHARS: usize = 80;
const EXPAND_MAX_CHARS: usize = 4000;

/// Session-scoped store for folded CLI payloads (`/expand <id>`).
pub type FoldCache = Arc<Mutex<HashMap<u64, String>>>;

/// Allocate the next fold id and store `payload` for `/expand`.
pub fn store_fold_payload(cache: &FoldCache, payload: &str) -> u64 {
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let id = guard.len() as u64 + 1;
    guard.insert(id, payload.to_string());
    id
}

/// Look up a previously stored expand payload.
pub fn get_fold_payload(cache: &FoldCache, id: u64) -> Option<String> {
    let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    guard.get(&id).cloned()
}

/// One node in a live bounded DAG (Web rail + CLI observe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DagNodeProgress {
    pub id: String,
    pub label: String,
    pub task_type: String,
    pub caps: String,
    /// Resolved provider/model for this hop (`hint:` already mapped).
    pub contact: String,
    /// `pending` | `running` | `ok` | `error`
    pub status: String,
}

/// User-facing progress during a tool-loop turn (not the final assistant reply).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnProgress {
    Status {
        phase: String,
        detail: String,
    },
    Step {
        kind: String,
        tool: String,
        ok: bool,
        summary: String,
        /// Scrubbed output for on-demand expand; omitted when empty.
        expand: Option<String>,
    },
    /// Structured DAG + node state (not dumped as chat Markdown).
    Dag {
        dag_id: String,
        fallback: bool,
        outline: String,
        nodes: Vec<DagNodeProgress>,
    },
}

/// Truncate a display string (no secrets; caller should scrub first).
#[must_use]
pub fn truncate_summary(text: &str) -> String {
    truncate_chars(text, CAPTION_MAX_CHARS)
}

fn truncate_chars(text: &str, max: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Logical model id once — do not wrap `{provider}/` when `model` already has `/`.
#[must_use]
pub fn model_status_detail(provider: &str, model: &str) -> String {
    let model = model.trim();
    let provider = provider.trim();
    if model.contains('/') {
        return model.to_string();
    }
    if provider.is_empty() {
        return model.to_string();
    }
    if provider == model {
        return model.to_string();
    }
    format!("{provider}/{model}")
}

/// Semantic caption: verb + object. Never the full script or tool stdout.
#[must_use]
pub fn progress_caption(tool: &str, args: &Value) -> String {
    let raw = match tool {
        "shell" => shell_caption(args),
        "file_read" | "pdf_read" => verb_path("read", args),
        "file_write" => verb_path("write", args),
        "glob_search" => verb_arg("glob", args, &["pattern", "glob"]),
        "web_search_tool" => verb_arg("search", args, &["query", "q"]),
        "http_request" => http_caption(args),
        "git_operations" => git_ops_caption(args),
        "memory_store" => verb_arg("memory store", args, &["key"]),
        "memory_recall" => verb_arg("memory recall", args, &["query", "key"]),
        "memory_forget" => verb_arg("memory forget", args, &["key"]),
        "browser_open" => verb_arg("open", args, &["url", "path"]),
        "browser" => verb_arg("browser", args, &["action", "url"]),
        "screenshot" => "screenshot".into(),
        "cron_list" => "cron list".into(),
        "cron_add" => verb_arg("cron add", args, &["name", "schedule"]),
        "cron_remove" => verb_arg("cron remove", args, &["name", "id"]),
        "cron_run" => verb_arg("cron run", args, &["name", "id"]),
        "cron_update" => verb_arg("cron update", args, &["name", "id"]),
        "cron_runs" => "cron runs".into(),
        "request_human_input" => verb_arg("ask", args, &["kind", "prompt"]),
        "delegate" => verb_arg("delegate", args, &["task", "goal"]),
        "generative_capability" => verb_arg("generative", args, &["capability"]),
        other => default_caption(other, args),
    };
    truncate_chars(&scrub_credentials(&raw), CAPTION_MAX_CHARS)
}

/// Scrubbed tool output for on-demand expand. Same string CLI and Web show.
#[must_use]
pub fn progress_expand_body(raw: &str) -> Option<String> {
    let scrubbed = scrub_credentials(raw);
    if scrubbed.trim().is_empty() {
        return None;
    }
    Some(truncate_expand(&scrubbed, EXPAND_MAX_CHARS))
}

fn truncate_expand(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn first_str<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    let obj = args.as_object()?;
    for key in keys {
        if let Some(s) = obj.get(*key).and_then(Value::as_str) {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn first_any_string(args: &Value) -> Option<&str> {
    let obj = args.as_object()?;
    for (k, v) in obj {
        if k == "secret_slot" || k == "content" || k == "body" || k == "headers" {
            continue;
        }
        if let Some(s) = v.as_str() {
            let t = s.trim();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn basename_or_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
}

fn verb_path(verb: &str, args: &Value) -> String {
    match first_str(args, &["path", "file", "filename"]) {
        Some(p) => format!("{verb} {}", basename_or_path(p)),
        None => verb.to_string(),
    }
}

fn verb_arg(verb: &str, args: &Value, keys: &[&str]) -> String {
    match first_str(args, keys) {
        Some(v) => format!("{verb} {v}"),
        None => verb.to_string(),
    }
}

fn first_substantive_shell_segment(cmd: &str) -> &str {
    for part in cmd.split("&&") {
        for seg in part.split(';') {
            let t = seg.trim();
            if t.is_empty() {
                continue;
            }
            let bin = t
                .split_whitespace()
                .next()
                .map(basename_or_path)
                .unwrap_or("");
            if matches!(bin, "cd" | "export" | "true" | ":" | "shift") {
                continue;
            }
            return t;
        }
    }
    cmd.trim()
}

fn shell_caption(args: &Value) -> String {
    let Some(cmd) = first_str(args, &["command"]) else {
        return "shell".into();
    };
    let head = first_substantive_shell_segment(cmd);
    let head = head.split('|').next().unwrap_or(head).trim();
    let mut tokens = head.split_whitespace().filter(|t| !t.contains('='));
    let Some(bin_raw) = tokens.next() else {
        return "shell".into();
    };
    let bin = basename_or_path(bin_raw);
    let obj = tokens.find(|t| !t.starts_with('-'));
    match obj {
        Some(o) => format!("{bin} {o}"),
        None => bin.to_string(),
    }
}

fn http_caption(args: &Value) -> String {
    let method = first_str(args, &["method"]).unwrap_or("GET").to_uppercase();
    let Some(url) = first_str(args, &["url"]) else {
        return method;
    };
    let stripped = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host_path = stripped.split('?').next().unwrap_or(stripped);
    format!("{method} {host_path}")
}

fn git_ops_caption(args: &Value) -> String {
    match first_str(args, &["operation"]) {
        Some(op) => format!("git {op}"),
        None => "git".into(),
    }
}

fn default_caption(tool: &str, args: &Value) -> String {
    match first_any_string(args) {
        Some(v) => format!("{tool} {}", basename_or_path(v)),
        None => tool.to_string(),
    }
}

/// Map a runtime observer event to a compact progress item.
pub fn event_to_progress(event: &ObserverEvent) -> Option<TurnProgress> {
    match event {
        ObserverEvent::LlmRequest {
            provider, model, ..
        } => Some(TurnProgress::Status {
            phase: "model".into(),
            detail: model_status_detail(provider, model),
        }),
        ObserverEvent::ToolCallStart { tool, caption } => {
            let cap = caption
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(tool.as_str());
            Some(TurnProgress::Status {
                phase: "run".into(),
                detail: cap.to_string(),
            })
        }
        ObserverEvent::ToolCall {
            tool,
            success,
            summary,
            detail,
            ..
        } => {
            let cap = summary
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(tool.as_str());
            Some(TurnProgress::Step {
                kind: "tool_result".into(),
                tool: tool.clone(),
                ok: *success,
                summary: cap.to_string(),
                expand: detail.clone(),
            })
        }
        _ => None,
    }
}

/// Fan-out observer: keep the configured backend, plus optional progress sink.
pub struct ProgressObserver {
    inner: Arc<dyn Observer>,
    tx: Option<Sender<TurnProgress>>,
    print_cli: bool,
    fold_cache: Option<FoldCache>,
}

impl ProgressObserver {
    pub fn forwarding(inner: Arc<dyn Observer>, tx: Sender<TurnProgress>) -> Self {
        Self {
            inner,
            tx: Some(tx),
            print_cli: false,
            fold_cache: None,
        }
    }

    pub fn cli(inner: Arc<dyn Observer>) -> Self {
        Self {
            inner,
            tx: None,
            print_cli: true,
            fold_cache: None,
        }
    }

    pub fn cli_with_fold(inner: Arc<dyn Observer>, fold_cache: FoldCache) -> Self {
        Self {
            inner,
            tx: None,
            print_cli: true,
            fold_cache: Some(fold_cache),
        }
    }
}

impl Observer for ProgressObserver {
    fn record_event(&self, event: &ObserverEvent) {
        self.inner.record_event(event);
        if let Some(progress) = event_to_progress(event) {
            if let Some(tx) = &self.tx {
                let _ = tx.try_send(progress.clone());
            }
            if self.print_cli {
                print_cli_progress(&progress, self.fold_cache.as_ref());
            }
        }
    }

    fn record_metric(&self, metric: &ObserverMetric) {
        self.inner.record_metric(metric);
    }

    fn flush(&self) {
        self.inner.flush();
    }

    fn name(&self) -> &str {
        "progress"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Dim status / distinct step lines (not Markdown assistant output).
/// Caption is the default line; expand body is stored for `/expand` when a cache is present.
pub fn print_cli_progress(progress: &TurnProgress, fold_cache: Option<&FoldCache>) {
    match progress {
        TurnProgress::Status { detail, .. } => {
            eprintln!("{}", console::style(format!("· {detail}")).dim());
        }
        TurnProgress::Step {
            ok,
            summary,
            expand,
            ..
        } => {
            let tag = if *ok { "ok" } else { "fail" };
            let line = match (expand.as_deref(), fold_cache) {
                (Some(body), Some(cache)) => {
                    let id = store_fold_payload(cache, body);
                    format!("  [{tag}] {summary}  (/expand {id})")
                }
                _ => format!("  [{tag}] {summary}"),
            };
            eprintln!("{}", console::style(line).cyan());
        }
        TurnProgress::Dag { dag_id, nodes, .. } => {
            let running = nodes
                .iter()
                .find(|n| n.status == "running")
                .map(|n| n.id.as_str())
                .unwrap_or("-");
            eprintln!(
                "{}",
                console::style(format!("· dag `{dag_id}` running={running}")).dim()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn truncate_summary_caps_length() {
        let long = "a".repeat(400);
        let out = truncate_summary(&long);
        assert!(out.chars().count() <= CAPTION_MAX_CHARS);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn model_status_skips_double_prefix() {
        assert_eq!(
            model_status_detail("deepseek", "deepseek/deepseek-v4-flash"),
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(
            model_status_detail("deepseek/deepseek-v4-flash", "deepseek/deepseek-v4-flash"),
            "deepseek/deepseek-v4-flash"
        );
        assert_eq!(
            model_status_detail("deepseek", "deepseek-v4-flash"),
            "deepseek/deepseek-v4-flash"
        );
    }

    #[test]
    fn llm_request_maps_to_status() {
        let p = event_to_progress(&ObserverEvent::LlmRequest {
            provider: "deepseek".into(),
            model: "deepseek/deepseek-v4-flash".into(),
            messages_count: 2,
        })
        .expect("mapped");
        assert_eq!(
            p,
            TurnProgress::Status {
                phase: "model".into(),
                detail: "deepseek/deepseek-v4-flash".into(),
            }
        );
    }

    #[test]
    fn shell_caption_is_verb_and_object() {
        let cap = progress_caption(
            "shell",
            &json!({"command": "git status -sb && echo secret"}),
        );
        assert_eq!(cap, "git status");
        let pipe = progress_caption("shell", &json!({"command": "cat Cargo.toml | wc -l"}));
        assert_eq!(pipe, "cat Cargo.toml");
        let cargo = progress_caption("shell", &json!({"command": "cargo test --lib repairs"}));
        assert_eq!(cargo, "cargo test");
        let cd = progress_caption("shell", &json!({"command": "cd /tmp && git status -sb"}));
        assert_eq!(cd, "git status");
    }

    #[test]
    fn file_read_caption_uses_basename() {
        let cap = progress_caption("file_read", &json!({"path": "src/agent/turn_progress.rs"}));
        assert_eq!(cap, "read turn_progress.rs");
    }

    #[test]
    fn unknown_tool_falls_back_without_body() {
        let cap = progress_caption(
            "custom_tool",
            &json!({"path": "a/b.txt", "content": "huge body"}),
        );
        assert_eq!(cap, "custom_tool b.txt");
    }

    #[test]
    fn caption_scrubs_key_like_tokens() {
        let cap = progress_caption(
            "http_request",
            &json!({"url": "https://example.com/api_key=sk-secretvalue123"}),
        );
        assert!(!cap.contains("sk-secretvalue123"));
    }

    #[test]
    fn tool_start_and_result_share_caption() {
        let start = event_to_progress(&ObserverEvent::ToolCallStart {
            tool: "shell".into(),
            caption: Some("git status".into()),
        })
        .expect("start");
        assert_eq!(
            start,
            TurnProgress::Status {
                phase: "run".into(),
                detail: "git status".into(),
            }
        );
        let step = event_to_progress(&ObserverEvent::ToolCall {
            tool: "shell".into(),
            duration: Duration::from_millis(1),
            success: true,
            summary: Some("git status".into()),
            detail: Some("On branch main".into()),
        })
        .expect("step");
        match step {
            TurnProgress::Step {
                summary,
                ok,
                expand,
                ..
            } => {
                assert_eq!(summary, "git status");
                assert!(ok);
                assert_eq!(expand.as_deref(), Some("On branch main"));
            }
            TurnProgress::Status { .. } | TurnProgress::Dag { .. } => panic!("expected step"),
        }
    }

    #[test]
    fn expand_body_scrubs_and_caps() {
        let out = progress_expand_body("api_key=sk-secretvalue123\nrest").expect("body");
        assert!(!out.contains("sk-secretvalue123"));
        let long = "a".repeat(5000);
        let capped = progress_expand_body(&long).expect("long");
        assert!(capped.chars().count() <= EXPAND_MAX_CHARS);
        assert!(capped.ends_with('…'));
        assert!(progress_expand_body("   ").is_none());
    }
}
