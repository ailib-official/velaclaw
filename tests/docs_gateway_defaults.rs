//! Runtime-contract docs must match schema defaults (VL-DOC-002).
//! 运行时合同文档必须与 schema 默认端口/模型一致。

use velaclaw::config::DEFAULT_PROTOCOL_MODEL_ID;

#[test]
fn commands_reference_gateway_default_is_port_3000() {
    let doc = include_str!("../docs/commands-reference.md");
    assert!(
        doc.contains("default `http://127.0.0.1:3000`"),
        "commands-reference must advertise gateway default 3000"
    );
    assert!(
        !doc.contains("default `http://127.0.0.1:8080`"),
        "commands-reference must not advertise 8080 as the gateway default"
    );
    assert!(
        doc.contains("http://127.0.0.1:3000/pair"),
        "pairing example must use port 3000"
    );
}

#[test]
fn config_reference_core_keys_match_protocol_default() {
    let doc = include_str!("../docs/config-reference.md");
    assert!(
        doc.contains(DEFAULT_PROTOCOL_MODEL_ID),
        "config-reference core keys must name {DEFAULT_PROTOCOL_MODEL_ID}"
    );
    assert!(
        !doc.contains("| `default_provider` | `openrouter`"),
        "stale openrouter default must not remain in the core keys table"
    );
}

#[test]
fn readme_web_chat_uses_gateway_port_3000() {
    let en = include_str!("../README.md");
    assert!(en.contains("http://127.0.0.1:3000/chat"));
    assert!(!en.contains("http://127.0.0.1:8080/chat"));
}
