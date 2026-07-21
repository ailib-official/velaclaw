//! CR-HOST-002: local observe-only aggregate of L4 M3c/d/e log fields.
//!
//! Parses operator logs (tracing text or JSONL) for `m3c_pass` / `m3d_category` /
//! `m3e_fallback`. Does **not** enable default-on L4 or require Prometheus/Grafana.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

/// Event target names emitted by `agent::candidate_dag` (CR-L4-004).
const EVENT_TARGETS: &[&str] = &[
    "candidate_dag_run",
    "candidate_dag_fallback",
    "candidate_dag_shadow_run",
    "candidate_dag_schema_fail",
];

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct L4ShadowAggregate {
    pub lines_scanned: u64,
    pub events_matched: u64,
    pub m3c_pass: u64,
    pub m3c_fail: u64,
    pub m3e_fallback: u64,
    pub by_m3d_category: BTreeMap<String, u64>,
    pub by_event: BTreeMap<String, u64>,
}

impl L4ShadowAggregate {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events_matched == 0
    }
}

/// Aggregate M3 fields from a UTF-8 log blob.
#[must_use]
pub fn aggregate_log_text(text: &str) -> L4ShadowAggregate {
    let mut agg = L4ShadowAggregate::default();
    for line in text.lines() {
        agg.lines_scanned += 1;
        if let Some(event) = match_event_line(line) {
            agg.events_matched += 1;
            *agg.by_event.entry(event.to_string()).or_default() += 1;
            match parse_bool_field(line, "m3c_pass") {
                Some(true) => agg.m3c_pass += 1,
                Some(false) => agg.m3c_fail += 1,
                None => {}
            }
            if parse_bool_field(line, "m3e_fallback") == Some(true) {
                agg.m3e_fallback += 1;
            }
            if let Some(cat) = parse_string_field(line, "m3d_category") {
                *agg.by_m3d_category.entry(cat).or_default() += 1;
            }
        }
    }
    agg
}

fn match_event_line(line: &str) -> Option<&'static str> {
    for target in EVENT_TARGETS {
        if line.contains(target) {
            return Some(*target);
        }
    }
    // JSONL may put the target in a `"message"` / `"target"` field without bare substring
    // of the const next to fields — still count if M3 fields are present together.
    if line.contains("m3c_pass") && line.contains("m3d_category") {
        return Some("m3_fields");
    }
    None
}

fn parse_bool_field(line: &str, key: &str) -> Option<bool> {
    // tracing: key=true / key=false
    let eq_true = format!("{key}=true");
    let eq_false = format!("{key}=false");
    if line.contains(&eq_true) {
        return Some(true);
    }
    if line.contains(&eq_false) {
        return Some(false);
    }
    // JSON: "key":true / "key": false
    let json_true = format!("\"{key}\":true");
    let json_true_sp = format!("\"{key}\": true");
    let json_false = format!("\"{key}\":false");
    let json_false_sp = format!("\"{key}\": false");
    if line.contains(&json_true) || line.contains(&json_true_sp) {
        return Some(true);
    }
    if line.contains(&json_false) || line.contains(&json_false_sp) {
        return Some(false);
    }
    None
}

fn parse_string_field(line: &str, key: &str) -> Option<String> {
    // tracing: key=value (value until whitespace)
    let needle = format!("{key}=");
    if let Some(idx) = line.find(&needle) {
        let rest = &line[idx + needle.len()..];
        let value = rest.split_whitespace().next().unwrap_or("");
        let value = value.trim_matches(|c| c == '"' || c == '\'' || c == ',');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    // JSON: "key":"value"
    let json_key = format!("\"{key}\":\"");
    if let Some(idx) = line.find(&json_key) {
        let rest = &line[idx + json_key.len()..];
        if let Some(end) = rest.find('"') {
            let value = &rest[..end];
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Load log from path, or stdin when `log` is `None` / `-`.
pub fn load_log_text(log: Option<&Path>) -> Result<String> {
    match log {
        None => {
            bail!(
                "pass --log <path> (or --log - to read stdin). \
                 Tip: capture with `RUST_LOG=info` while running shadow probes, \
                 then aggregate — Grafana is not required."
            )
        }
        Some(path) if path.as_os_str() == "-" => {
            let mut buf = String::new();
            io::stdin()
                .read_to_string(&mut buf)
                .context("read stdin for l4-shadow-summary")?;
            Ok(buf)
        }
        Some(path) => fs::read_to_string(path)
            .with_context(|| format!("read L4 shadow log {}", path.display())),
    }
}

/// Print human or JSON summary. Returns Ok even when aggregate is empty (observe-only).
pub fn run_l4_shadow_summary(log: Option<&Path>, json: bool) -> Result<()> {
    let text = load_log_text(log)?;
    let agg = aggregate_log_text(&text);
    if json {
        println!("{}", serde_json::to_string_pretty(&agg)?);
        return Ok(());
    }

    println!("🩺 L4 shadow M3 aggregate (CR-HOST-002; observe-only)");
    println!("  candidate_dag_shadow stays default-off; no Prometheus/Grafana gate.");
    println!();
    println!("  lines_scanned:   {}", agg.lines_scanned);
    println!("  events_matched:  {}", agg.events_matched);
    println!("  m3c_pass:        {}", agg.m3c_pass);
    println!("  m3c_fail:        {}", agg.m3c_fail);
    println!("  m3e_fallback:    {}", agg.m3e_fallback);
    if agg.by_m3d_category.is_empty() {
        println!("  m3d_category:    (none)");
    } else {
        println!("  m3d_category:");
        for (cat, n) in &agg.by_m3d_category {
            println!("    {cat}: {n}");
        }
    }
    if !agg.by_event.is_empty() {
        println!("  by_event:");
        for (ev, n) in &agg.by_event {
            println!("    {ev}: {n}");
        }
    }
    if agg.is_empty() {
        println!();
        println!("  ⚠️  No M3 events found. Generate sample traffic with:");
        println!("      RUST_LOG=info velaclaw doctor candidate-dag --candidate <path>");
        println!(
            "      (optional) enable `[agent].candidate_dag_shadow = true` for host shadow only."
        );
    }
    println!();
    println!("  Docs: docs/commands-reference.md · docs/troubleshooting.md");
    Ok(())
}

/// Read all lines from a reader (tests / stdin helpers).
#[cfg(test)]
fn aggregate_reader(reader: impl std::io::BufRead) -> L4ShadowAggregate {
    let text: String = reader
        .lines()
        .map_while(Result::ok)
        .collect::<Vec<_>>()
        .join("\n");
    aggregate_log_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_tracing_style_lines() {
        let text = "\
2026-07-21T12:00:00Z INFO candidate_dag_run dag_id=x m3c_pass=true m3d_category=ok m3e_fallback=false
2026-07-21T12:00:01Z INFO candidate_dag_fallback m3c_pass=false m3d_category=unknown_capability m3e_fallback=true fallback_reason=schema_fail
2026-07-21T12:00:02Z INFO unrelated line
";
        let agg = aggregate_log_text(text);
        assert_eq!(agg.lines_scanned, 3);
        assert_eq!(agg.events_matched, 2);
        assert_eq!(agg.m3c_pass, 1);
        assert_eq!(agg.m3c_fail, 1);
        assert_eq!(agg.m3e_fallback, 1);
        assert_eq!(agg.by_m3d_category.get("ok"), Some(&1));
        assert_eq!(agg.by_m3d_category.get("unknown_capability"), Some(&1));
        assert_eq!(agg.by_event.get("candidate_dag_run"), Some(&1));
        assert_eq!(agg.by_event.get("candidate_dag_fallback"), Some(&1));
    }

    #[test]
    fn aggregates_jsonl_style_lines() {
        let text = r#"{"message":"candidate_dag_shadow_run","m3c_pass":true,"m3d_category":"ok","m3e_fallback":false}
{"m3c_pass":false,"m3d_category":"schema_validation","m3e_fallback":false}
"#;
        let agg = aggregate_log_text(text);
        assert_eq!(agg.events_matched, 2);
        assert_eq!(agg.m3c_pass, 1);
        assert_eq!(agg.m3c_fail, 1);
        assert_eq!(agg.m3e_fallback, 0);
        assert_eq!(agg.by_m3d_category.get("schema_validation"), Some(&1));
    }

    #[test]
    fn empty_log_is_ok_observe_only() {
        let agg = aggregate_log_text("hello\nworld\n");
        assert!(agg.is_empty());
        assert_eq!(agg.lines_scanned, 2);
    }

    #[test]
    fn aggregate_reader_counts() {
        let data = b"INFO candidate_dag_schema_fail m3c_pass=false m3d_category=parse_error m3e_fallback=false\n";
        let agg = aggregate_reader(data.as_slice());
        assert_eq!(agg.events_matched, 1);
        assert_eq!(agg.by_m3d_category.get("parse_error"), Some(&1));
    }
}
