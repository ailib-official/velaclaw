//! ORCH-DAG-VIS-001: doctor DAG graph view + reachable model picker listing.

use crate::capability_index::{
    filter_reachable, load_or_rebuild_for_config, lookup_tag, CAPABILITY_TAGS,
};
use crate::config::Config;
use crate::execution::provider_has_usable_key;
use crate::orchestration::dag_view::graph_from_value;
use crate::orchestration::host_decide::SessionModelOverride;
use crate::orchestration::session_override;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Render a DAG fixture as a text view-model and list reachable models for picker UX.
///
/// Node-level ModelSelector product UI is a non-goal; capabilities are shown read-only.
pub fn run_dag_view(
    config: &Config,
    fixture: &Path,
    tag: Option<&str>,
    set_override: Option<&str>,
    session_key: &str,
) -> Result<()> {
    let raw = fs::read_to_string(fixture)
        .with_context(|| format!("read DAG fixture {}", fixture.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw).context("parse DAG JSON")?;
    let view = graph_from_value(&value);

    println!("🩺 VelaClaw Doctor — DAG View (ORCH-DAG-VIS-001)");
    println!("  fixture:         {}", fixture.display());
    println!("  id:              {}", view.id);
    println!("  entry:           {}", view.entry);
    println!(
        "  schema_version:  {}",
        view.schema_version.as_deref().unwrap_or("(none)")
    );
    println!("  valid_shape:     {}", view.valid_shape);
    for n in &view.notes {
        println!("  note:            {n}");
    }
    println!();
    println!("Nodes (read-only; node ModelSelector UI is non-goal):");
    for n in &view.nodes {
        let caps = if n.capabilities.is_empty() {
            String::from("(none)")
        } else {
            n.capabilities.join(", ")
        };
        println!(
            "  • {}  task={:?}  next={:?}  caps=[{caps}]",
            n.id, n.task_type, n.next
        );
    }
    println!();
    println!("Edges:");
    for (a, b) in &view.edges {
        println!("  {a} → {b}");
    }

    let config_dir = config
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let (index, _) = load_or_rebuild_for_config(config_dir, false)?;
    let declared = if let Some(t) = tag {
        lookup_tag(&index, t)?
    } else {
        let mut found: &[crate::capability_index::CapabilityCandidate] = &[];
        for t in CAPABILITY_TAGS {
            if let Ok(c) = lookup_tag(&index, t) {
                if !c.is_empty() {
                    found = c;
                    break;
                }
            }
        }
        found
    };
    let reachable = filter_reachable(declared, provider_has_usable_key);

    println!();
    println!("Model picker options (= CAP reachable set):");
    if reachable.is_empty() {
        println!("  (empty — no usable keys ∩ declared)");
    }
    for c in &reachable {
        let mid = c
            .logical_model_id
            .as_deref()
            .unwrap_or("(no logical_model_id)");
        println!("  • {} / {}", c.provider_id, mid);
    }

    if let Some(spec) = set_override {
        let (provider_id, model) = spec
            .split_once('/')
            .map(|(p, m)| (p.to_string(), m.to_string()))
            .ok_or_else(|| anyhow::anyhow!("override must be provider/model"))?;
        let ok = reachable.iter().any(|c| {
            c.provider_id == provider_id
                && c.logical_model_id.as_deref().is_some_and(|id| {
                    id == model || id.rsplit_once('/').map(|(_, m)| m).unwrap_or(id) == model
                })
        });
        if !ok {
            anyhow::bail!(
                "picker rejects unreachable model id '{provider_id}/{model}' (not in CAP reachable set)"
            );
        }
        session_override::set_override(
            session_key,
            Some(SessionModelOverride {
                provider_id: provider_id.clone(),
                model: model.clone(),
            }),
        );
        println!();
        println!("session override set → {provider_id}/{model} (session_key={session_key})");
    }

    Ok(())
}
