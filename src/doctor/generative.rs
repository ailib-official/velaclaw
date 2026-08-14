//! Experimental doctor view of PT-GEN declared / L-Exec / key reachability (VL-GEN-002).
//! 生成式能力一览：复用 inspect_loaded，reachable = allowed ∧ 本地密钥存在（不读密钥值）。

use crate::execution::provider_has_usable_key;
use crate::protocol_registry::{
    list_generative_capabilities, resolve_local_protocol_root, GenerativeCapabilityInspect,
};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
struct GenerativeDoctorRow {
    #[serde(flatten)]
    inspect: GenerativeCapabilityInspect,
    key_present: bool,
    reachable: bool,
}

#[derive(Debug, Serialize)]
struct GenerativeDoctorReport {
    protocol_root: String,
    capability_filter: Option<String>,
    reachable_only: bool,
    listed: usize,
    allowed: usize,
    reachable: usize,
    rows: Vec<GenerativeDoctorRow>,
}

fn row_from_inspect(inspect: GenerativeCapabilityInspect) -> GenerativeDoctorRow {
    let key_present = provider_has_usable_key(&inspect.provider);
    let reachable = inspect.allowed && key_present;
    GenerativeDoctorRow {
        inspect,
        key_present,
        reachable,
    }
}

/// Print PT-GEN inspect rows plus query-time key reachability (no secrets).
pub fn run_generative(capability: Option<&str>, reachable_only: bool, json: bool) -> Result<()> {
    let Some(root) = resolve_local_protocol_root() else {
        anyhow::bail!("Set AI_PROTOCOL_DIR to a local ai-protocol checkout (not a URL).");
    };
    let listed = list_generative_capabilities(&root, capability)?;
    let allowed_count = listed.iter().filter(|r| r.allowed).count();
    let mut rows: Vec<GenerativeDoctorRow> = listed.into_iter().map(row_from_inspect).collect();
    let reachable_count = rows.iter().filter(|r| r.reachable).count();
    if reachable_only {
        rows.retain(|r| r.reachable);
    }
    let report = GenerativeDoctorReport {
        protocol_root: root.display().to_string(),
        capability_filter: capability.map(str::to_string),
        reachable_only,
        listed: rows.len(),
        allowed: allowed_count,
        reachable: reachable_count,
        rows,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("🩺 VelaClaw Doctor — Generative capabilities (Experimental, VL-GEN-002)");
    println!("  protocol_root: {}", report.protocol_root);
    println!(
        "  filter:        {}",
        report
            .capability_filter
            .as_deref()
            .unwrap_or("(all PT-GEN keys)")
    );
    println!(
        "  counts:        {} listed / {} allowed / {} reachable",
        report.listed, report.allowed, report.reachable
    );
    println!("  reachable:     allowed ∧ local key presence (no secrets)");
    println!("  note:          not a CR-CAP Tag; does not mutate CAPABILITY_TAGS");
    println!();
    if report.rows.is_empty() {
        if reachable_only {
            println!("  (empty — no allowed+keyed generative rows)");
        } else {
            println!("  (empty — no metadata.models under AI_PROTOCOL_DIR)");
        }
        return Ok(());
    }
    for row in &report.rows {
        let mark = if row.reachable {
            "reachable"
        } else if row.inspect.allowed {
            "no-key"
        } else {
            "blocked"
        };
        println!(
            "  - [{mark}] {}  {}  declared={}  path={}  adapter={}",
            row.inspect.logical_id,
            row.inspect.capability,
            row.inspect.capability_declared,
            row.inspect.endpoint_path.as_deref().unwrap_or("-"),
            row.inspect.adapter.as_deref().unwrap_or("-"),
        );
        if let Some(reason) = &row.inspect.fail_closed_reason {
            println!("      reason: {reason}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_index::PROTOCOL_ENV_LOCK;
    use std::fs;

    #[test]
    fn doctor_generative_lists_declared_and_omit() {
        let _guard = PROTOCOL_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let providers = dir.path().join("v2").join("providers");
        fs::create_dir_all(&providers).unwrap();
        fs::write(
            providers.join("genprov.yaml"),
            r#"
id: genprov
protocol_version: v2-alpha
provider_id: genprov
name: Gen
version: v2
status: stable
category: ai_provider
official_url: https://example.com
support_contact: support@example.com
capabilities: [chat]
endpoint:
  base_url: https://example.com/v1
  auth:
    type: bearer
    token_env: VELACLAW_GEN_TOKEN
endpoints:
  image_generation:
    path: /images/generations
    method: POST
    adapter: openai
metadata:
  models:
    img-1:
      model_capabilities:
        image_generation: true
    chat-1:
      context_window: 128
"#,
        )
        .unwrap();
        std::env::set_var("AI_PROTOCOL_DIR", dir.path());
        run_generative(Some("image_generation"), false, false).expect("doctor");
        run_generative(Some("image_generation"), true, true).expect("reachable-only json");
        std::env::remove_var("AI_PROTOCOL_DIR");
    }
}
