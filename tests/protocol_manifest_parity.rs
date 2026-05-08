//! ZS-ML-014 — protocol-first resolution without enabling `legacy-providers`.
#![cfg(feature = "ai-protocol")]

use std::path::PathBuf;
use std::sync::Mutex;

static AI_PROTOCOL_ENV_LOCK: Mutex<()> = Mutex::new(());

/// Restores previous `AI_PROTOCOL_DIR` after the test finishes.
struct AiProtocolEnvGuard {
    previous: Option<std::ffi::OsString>,
}

impl AiProtocolEnvGuard {
    fn set(dir: &str) -> Self {
        let previous = std::env::var_os("AI_PROTOCOL_DIR");
        std::env::set_var("AI_PROTOCOL_DIR", dir);
        Self { previous }
    }
}

impl Drop for AiProtocolEnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("AI_PROTOCOL_DIR", v),
            None => std::env::remove_var("AI_PROTOCOL_DIR"),
        }
    }
}

#[test]
fn protocol_fixture_resolves_openai_without_legacy() {
    let _lane = AI_PROTOCOL_ENV_LOCK
        .lock()
        .expect("protocol env mutex poisoned");

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ai-protocol-min");
    assert!(
        root.join("v2/providers/openai.yaml").is_file(),
        "fixture missing: {}",
        root.display()
    );

    let _env = AiProtocolEnvGuard::set(root.to_str().expect("UTF-8 path"));

    let out = zerospider::providers::create_provider("openai/gpt-5.2", Some("sk-zsml014-teststub"));
    assert!(
        out.is_ok(),
        "expected protocol-backed provider, got {:?}",
        out.as_ref().err().map(ToString::to_string)
    );
}
