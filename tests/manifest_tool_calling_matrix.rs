//! Multi-provider manifest → runtime tool-calling chain (identify → call → run).
//!
//! Scans `ai-protocol/v2/providers/*.yaml` for `capabilities.tool_calling`, then for each
//! provider asserts: `ExecutionHandle::from_config`, manifest policy, dispatcher build,
//! `EffectivePolicy::resolve`, and optional text-fallback parse smoke.
#![cfg(feature = "ai-protocol")]

use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use velaclaw::agent::dispatcher::{build_tool_dispatcher, ToolDispatcher};
use velaclaw::config::{Config, EffectivePolicy};
use velaclaw::execution::ExecutionHandle;
use velaclaw::providers::ChatResponse;

/// Providers whose manifests declare `tool_calling` but lack `metadata.models` chat entries.
const FALLBACK_PROBE_MODELS: &[(&str, &str)] =
    &[("cohere", "command-r-plus"), ("doubao", "doubao-pro-32k")];

fn ai_protocol_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("AI_PROTOCOL_DIR") {
        let path = PathBuf::from(&dir);
        if path.join("v2/providers").is_dir() {
            return Some(path);
        }
    }
    for candidate in ["/home/alex/ai-protocol", r"d:\ai-protocol"] {
        let path = PathBuf::from(candidate);
        if path.join("v2/providers").is_dir() {
            return Some(path);
        }
    }
    None
}

fn fallback_model(provider_id: &str) -> Option<&'static str> {
    FALLBACK_PROBE_MODELS
        .iter()
        .find_map(|(id, model)| (*id == provider_id).then_some(*model))
}

fn deprecated_models(manifest: &YamlValue) -> HashSet<String> {
    manifest
        .get("metadata")
        .and_then(|m| m.get("deprecated"))
        .and_then(YamlValue::as_mapping)
        .map(|map| {
            map.keys()
                .filter_map(|k| k.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn probe_model_id(provider_id: &str, manifest: &YamlValue) -> Option<String> {
    if let Some(model) = fallback_model(provider_id) {
        return Some(model.to_string());
    }

    let deprecated = deprecated_models(manifest);
    let models = manifest
        .get("metadata")
        .and_then(|m| m.get("models"))
        .and_then(YamlValue::as_mapping)?;

    for key in models.keys() {
        if let Some(name) = key.as_str() {
            if !deprecated.contains(name) {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn shell_parse_sample(manifest_tc: &JsonValue) -> Option<String> {
    let tag = manifest_tc
        .get("text_fallback")
        .and_then(|tf| tf.get("known_dialects"))
        .and_then(JsonValue::as_array)?
        .iter()
        .find_map(|d| {
            let tag = d.get("tag")?.as_str()?;
            let map_to = d.get("map_to").and_then(|v| v.as_str()).unwrap_or("");
            if tag == "shell" || map_to == "shell" {
                Some(tag.to_string())
            } else {
                None
            }
        })?;

    Some(format!("probe\n<{tag}>\necho tool-matrix-ok\n</{tag}>\n"))
}

fn config_for_provider(logical_model_id: &str) -> Config {
    Config {
        default_provider: Some(logical_model_id.into()),
        default_model: Some(logical_model_id.into()),
        ..Default::default()
    }
}

fn assert_provider_tool_chain(provider_id: &str, model_id: &str) {
    let logical_model_id = format!("{provider_id}/{model_id}");
    let config = config_for_provider(&logical_model_id);

    let handle =
        ExecutionHandle::from_config(&config).unwrap_or_else(|e| panic!("{logical_model_id}: {e}"));

    let manifest_tc = handle
        .manifest_tool_calling()
        .expect("manifest tool_calling block must be present");
    assert!(
        manifest_tc.get("native").is_some() || manifest_tc.get("text_fallback").is_some(),
        "{logical_model_id}: tool_calling must declare native and/or text_fallback"
    );

    let policy = handle.tool_calling_policy();
    assert!(
        policy.prefer_native_dispatcher() || manifest_tc.get("text_fallback").is_some(),
        "{logical_model_id}: tool_calling_policy must enable native and/or text fallback"
    );

    let provider = handle
        .provider_adapter()
        .unwrap_or_else(|e| panic!("{logical_model_id} provider_adapter: {e}"));

    let native_supported = manifest_tc
        .get("native")
        .and_then(|n| n.get("supported"))
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    if native_supported {
        assert!(
            provider.supports_native_tools(),
            "{logical_model_id}: manifest native.supported=true requires Provider::supports_native_tools"
        );
    }

    let effective = EffectivePolicy::resolve(
        config.agent.tool_dispatcher.as_str(),
        None,
        None,
        policy.clone(),
    );
    assert_eq!(effective.tool_dispatcher, "auto");

    let dispatcher = effective.build_dispatcher(provider.as_ref());
    let auto_dispatcher = build_tool_dispatcher("auto", provider.as_ref(), policy.clone());

    let expect_native_specs = provider.supports_native_tools() && policy.prefer_native_dispatcher();
    assert_eq!(
        ToolDispatcher::should_send_tool_specs(&*dispatcher),
        expect_native_specs,
        "{logical_model_id}: EffectivePolicy dispatcher native-spec expectation"
    );
    assert_eq!(
        ToolDispatcher::should_send_tool_specs(&*auto_dispatcher),
        ToolDispatcher::should_send_tool_specs(&*dispatcher),
        "{logical_model_id}: auto dispatcher must match effective policy"
    );

    if let Some(sample) = shell_parse_sample(manifest_tc) {
        let response = ChatResponse {
            text: Some(sample),
            tool_calls: vec![],
        };
        let (_, calls) = ToolDispatcher::parse_response(&*dispatcher, &response);
        assert!(
            !calls.is_empty(),
            "{logical_model_id}: text_fallback shell dialect should parse at least one tool call"
        );
        let tool_name = calls[0].name.as_str();
        assert!(
            tool_name == "shell" || !tool_name.is_empty(),
            "{logical_model_id}: parsed tool name must be non-empty (got {tool_name:?})"
        );
    }
}

#[test]
fn all_manifest_tool_calling_providers_identify_call_and_run() {
    let Some(protocol_dir) = ai_protocol_dir() else {
        eprintln!("SKIP: AI_PROTOCOL_DIR not set or v2/providers missing");
        return;
    };
    std::env::set_var("AI_PROTOCOL_DIR", protocol_dir.as_os_str());

    let providers_dir = protocol_dir.join("v2/providers");
    let mut probed = 0usize;

    for entry in fs::read_dir(&providers_dir).expect("read providers dir") {
        let entry = entry.expect("provider dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }

        let raw = fs::read_to_string(&path).expect("read provider yaml");
        let manifest: YamlValue = serde_yaml::from_str(&raw).expect("parse provider yaml");

        if manifest
            .get("capabilities")
            .and_then(|c| c.get("tool_calling"))
            .is_none()
        {
            continue;
        }

        let provider_id = manifest
            .get("id")
            .and_then(YamlValue::as_str)
            .unwrap_or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .expect("provider yaml stem")
            });

        let model_id = probe_model_id(provider_id, &manifest).unwrap_or_else(|| {
            panic!("{provider_id}: no probe model (add metadata.models or FALLBACK_PROBE_MODELS)");
        });

        assert_provider_tool_chain(provider_id, &model_id);
        probed += 1;
    }

    assert!(
        probed >= 11,
        "expected at least 11 tool_calling providers in ai-protocol v2, probed {probed}"
    );
}
