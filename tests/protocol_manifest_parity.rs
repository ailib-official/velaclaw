//! ZS-ML-014 — protocol-first resolution without enabling `legacy-providers`.
#![cfg(feature = "ai-protocol")]

use std::path::PathBuf;
use std::sync::Mutex;

static AI_PROTOCOL_ENV_LOCK: Mutex<()> = Mutex::new(());

/// Restores prior `AI_PROTOCOL_DIR` after the test finishes.
struct AiProtocolDirGuard {
    previous: Option<std::ffi::OsString>,
}

impl AiProtocolDirGuard {
    fn set(dir: &str) -> Self {
        let previous = std::env::var_os("AI_PROTOCOL_DIR");
        std::env::set_var("AI_PROTOCOL_DIR", dir);
        Self { previous }
    }
}

impl Drop for AiProtocolDirGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var("AI_PROTOCOL_DIR", v),
            None => std::env::remove_var("AI_PROTOCOL_DIR"),
        }
    }
}

/// Temporarily clears `AI_PROTOCOL_DIR` / `AI_PROTOCOL_PATH`; restores on drop.
///
/// Clearing these variables does **not** force resolution to fail: `ai-lib-rust` still probes
/// default filesystem roots (`../ai-protocol`, …) and canonical GitHub manifest fallbacks. For a
/// deterministic negative assertion, combine this guard with a **bogus** `provider/model` id.
struct ProtocolRootsClearedGuard {
    previous_dir: Option<std::ffi::OsString>,
    previous_path: Option<std::ffi::OsString>,
}

impl ProtocolRootsClearedGuard {
    fn unset() -> Self {
        let previous_dir = std::env::var_os("AI_PROTOCOL_DIR");
        let previous_path = std::env::var_os("AI_PROTOCOL_PATH");
        std::env::remove_var("AI_PROTOCOL_DIR");
        std::env::remove_var("AI_PROTOCOL_PATH");
        Self {
            previous_dir,
            previous_path,
        }
    }
}

impl Drop for ProtocolRootsClearedGuard {
    fn drop(&mut self) {
        match &self.previous_dir {
            Some(v) => std::env::set_var("AI_PROTOCOL_DIR", v),
            None => std::env::remove_var("AI_PROTOCOL_DIR"),
        }
        match &self.previous_path {
            Some(v) => std::env::set_var("AI_PROTOCOL_PATH", v),
            None => std::env::remove_var("AI_PROTOCOL_PATH"),
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

    let _env = AiProtocolDirGuard::set(root.to_str().expect("UTF-8 path"));

    let out = zerospider::providers::create_provider("openai/gpt-5.2", Some("sk-zsml014-teststub"));
    assert!(
        out.is_ok(),
        "expected protocol-backed provider, got {:?}",
        out.as_ref().err().map(ToString::to_string)
    );
}

#[test]
fn protocol_unknown_provider_surfaces_hint_without_protocol_env() {
    let _lane = AI_PROTOCOL_ENV_LOCK
        .lock()
        .expect("protocol env mutex poisoned");

    let _cleared = ProtocolRootsClearedGuard::unset();

    let result = zerospider::providers::create_provider(
        "zsml014_unknown_provider_gs99/no-such-model",
        Some("sk-zsml014-negative"),
    );
    assert!(
        result.is_err(),
        "bogus provider id must not pretend to succeed"
    );

    let err = result.err().expect("checked is_err");
    let raw = err.to_string();
    let msg = raw.to_ascii_lowercase();
    assert!(
        msg.contains("ai_protocol_dir")
            || msg.contains("ai-protocol")
            || msg.contains("hint")
            || msg.contains("migration-legacy"),
        "protocol resolution failures should mention checkout/docs guidance, got:\n{raw}",
    );
}
