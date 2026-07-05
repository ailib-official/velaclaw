//! Runtime assertions: manifest tool_calling wiring and dispatcher selection for DeepSeek.
#![cfg(feature = "ai-protocol")]

use ai_lib_rust::{NativeStrategy, TextToolParser};
use std::path::Path;
use velaclaw::agent::dispatcher::{NativeToolDispatcher, ToolDispatcher, XmlToolDispatcher};
use velaclaw::config::Config;
use velaclaw::execution::ExecutionHandle;

fn ai_protocol_dir() -> Option<String> {
    if let Ok(dir) = std::env::var("AI_PROTOCOL_DIR") {
        if Path::new(&dir).join("v2/providers/deepseek.yaml").exists() {
            return Some(dir);
        }
    }
    let default = "/home/alex/ai-protocol";
    if Path::new(default)
        .join("v2/providers/deepseek.yaml")
        .exists()
    {
        return Some(default.to_string());
    }
    None
}

#[test]
fn deepseek_runtime_manifest_and_dispatcher_probe() {
    let Some(protocol_dir) = ai_protocol_dir() else {
        eprintln!("SKIP: AI_PROTOCOL_DIR not set or deepseek.yaml missing");
        return;
    };
    std::env::set_var("AI_PROTOCOL_DIR", &protocol_dir);

    let mut config = Config::default();
    config.default_provider = Some("deepseek/deepseek-chat".into());
    config.default_model = Some("deepseek-chat".into());
    config.agent.tool_dispatcher = "auto".into();

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

    let dispatcher_kind = if config.agent.tool_dispatcher.as_str() == "native" {
        "native"
    } else if config.agent.tool_dispatcher.as_str() == "xml" {
        "xml"
    } else if provider.supports_native_tools() && policy.prefer_native_dispatcher() {
        "native"
    } else {
        "xml"
    };
    assert_eq!(
        dispatcher_kind, "native",
        "auto mode should select NativeToolDispatcher for deepseek hybrid"
    );

    let native = NativeToolDispatcher::new(policy.parser.clone());
    assert!(native.should_send_tool_specs());

    let response = velaclaw::providers::ChatResponse {
        text: Some(sample.to_string()),
        tool_calls: vec![],
    };
    let (_, native_calls) = native.parse_response(&response);
    assert_eq!(
        native_calls.len(),
        1,
        "NativeToolDispatcher hybrid should parse plain shell from text"
    );

    let xml = XmlToolDispatcher::new(policy.parser);
    assert!(!xml.should_send_tool_specs());
}
