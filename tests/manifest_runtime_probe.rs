//! Runtime assertions: manifest tool_calling wiring and dispatcher selection for DeepSeek.
#![cfg(feature = "ai-protocol")]

use ai_lib_rust::NativeStrategy;
use std::path::Path;
use velaclaw::agent::dispatcher::{build_tool_dispatcher, ToolDispatcher};
use velaclaw::config::Config;
use velaclaw::execution::ExecutionHandle;

fn ai_protocol_dir() -> Option<String> {
    if let Ok(dir) = std::env::var("AI_PROTOCOL_DIR") {
        if Path::new(&dir).join("v2/providers/deepseek.yaml").exists() {
            return Some(dir);
        }
    }
    for candidate in ["/home/alex/ai-protocol", r"d:\ai-protocol"] {
        if Path::new(candidate)
            .join("v2/providers/deepseek.yaml")
            .exists()
        {
            return Some(candidate.to_string());
        }
    }
    None
}

fn deepseek_config() -> Config {
    Config {
        default_provider: Some("deepseek/deepseek-chat".into()),
        default_model: Some("deepseek-chat".into()),
        ..Default::default()
    }
}

#[test]
fn deepseek_runtime_manifest_and_dispatcher_probe() {
    let Some(protocol_dir) = ai_protocol_dir() else {
        eprintln!("SKIP: AI_PROTOCOL_DIR not set or deepseek.yaml missing");
        return;
    };
    std::env::set_var("AI_PROTOCOL_DIR", &protocol_dir);

    let config = deepseek_config();
    let handle = ExecutionHandle::from_config(&config).expect("ExecutionHandle::from_config");
    let manifest_tc = handle.manifest_tool_calling();
    assert!(
        manifest_tc.is_some(),
        "expected capabilities.tool_calling from deepseek manifest"
    );

    let policy = handle.tool_calling_policy();
    assert_eq!(
        policy.native_strategy,
        NativeStrategy::Hybrid,
        "deepseek partial + text_fallback should be Hybrid"
    );
    assert!(policy.prefer_native_dispatcher());

    let provider = handle.provider_adapter().expect("provider_adapter");
    assert!(provider.supports_native_tools());

    let sample =
        "让我检查一下。\n<shell>\nwhich opencode 2>/dev/null || echo \"not found\"\n</shell>\n";
    let (_, calls) = policy.parser.parse(sample);
    assert_eq!(
        calls.len(),
        1,
        "plain <shell> body should parse to one tool call"
    );
    assert_eq!(calls[0].name, "shell");
    assert_eq!(
        calls[0].arguments["command"],
        "which opencode 2>/dev/null || echo \"not found\""
    );

    let dispatcher = build_tool_dispatcher(
        config.agent.tool_dispatcher.as_str(),
        provider.as_ref(),
        policy.clone(),
    );
    assert!(
        dispatcher.should_send_tool_specs(),
        "auto mode should select NativeToolDispatcher for deepseek hybrid"
    );

    let response = velaclaw::providers::ChatResponse {
        text: Some(sample.to_string()),
        tool_calls: vec![],
    };
    let (_, native_calls) = dispatcher.parse_response(&response);
    assert_eq!(
        native_calls.len(),
        1,
        "NativeToolDispatcher hybrid should parse plain shell from text"
    );
}

#[test]
fn deepseek_xml_dispatcher_override_from_config() {
    let Some(protocol_dir) = ai_protocol_dir() else {
        eprintln!("SKIP: AI_PROTOCOL_DIR not set or deepseek.yaml missing");
        return;
    };
    std::env::set_var("AI_PROTOCOL_DIR", &protocol_dir);

    let mut config = deepseek_config();
    config.agent.tool_dispatcher = "xml".into();

    let handle = ExecutionHandle::from_config(&config).expect("ExecutionHandle::from_config");
    let provider = handle.provider_adapter().expect("provider_adapter");
    let policy = handle.tool_calling_policy();

    let dispatcher = build_tool_dispatcher("xml", provider.as_ref(), policy);
    assert!(
        !dispatcher.should_send_tool_specs(),
        "xml override must not send native tool specs even for hybrid manifest"
    );
}
