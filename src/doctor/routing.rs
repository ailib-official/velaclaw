//! OmniRoute L-V1 / VL-DR-001: explain execution routing (provider_mode + BYOK hygiene).
//! 路径可解释：当前 provider_mode、配置模型 vs BYOK 有效模型、已检测 env 名（无密钥）。

use crate::config::{Config, ProviderRoutingMode};
use crate::execution::{
    detected_byok_env_names, diagnose_byok_routing, logical_model_id_from_config,
};
use anyhow::Result;

/// Print operator-facing routing diagnosis (no LLM; no secrets).
pub fn run_routing(config: &Config) -> Result<()> {
    let mode = config.routing.provider_mode;
    let mode_label = match mode {
        ProviderRoutingMode::Byok => "byok",
        ProviderRoutingMode::Prism => "prism",
    };
    let configured = logical_model_id_from_config(config);

    println!("🩺 VelaClaw Doctor — Routing (execution path)");
    println!("  provider_mode:     {mode_label}");
    println!("  configured_model:  {configured}");
    println!(
        "  default_provider:  {}",
        config.default_provider.as_deref().unwrap_or("(unset)")
    );
    println!(
        "  default_model:     {}",
        config
            .default_model
            .as_deref()
            .unwrap_or("(unset → protocol default)")
    );
    println!("  {}", super::cap_pipeline::CAP_PIPELINE_LINE);
    println!();

    match mode {
        ProviderRoutingMode::Byok => print_byok(config, &configured),
        ProviderRoutingMode::Prism => print_prism(&configured),
    }

    println!();
    println!("{}", super::cap_pipeline::CAP_RELATED_DOCTOR);
    println!();
    println!("Also:");
    println!("  docs/providers-reference.md — BYOK hygiene (VL-RT-003)");
    println!("  velaclaw doctor maintenance — config vs rebuild");
    println!("  Non-goals: Catalog meat / default-on L4 / HOST-002 M3 aggregate");
    Ok(())
}

fn print_byok(config: &Config, configured: &str) {
    let diagnosis = diagnose_byok_routing(config);
    let detected = detected_byok_env_names(config);
    let detected_msg = if detected.is_empty() {
        "none".to_string()
    } else {
        detected.join(", ")
    };

    println!("  [byok hygiene]");
    match diagnosis.effective {
        Ok(ref effective) if effective == configured => {
            println!(
                "    status:            keep configured model (provider key present or keyless)"
            );
            println!("    effective_model:   {effective}");
        }
        Ok(ref effective) => {
            println!("    status:            remapped (configured provider has no usable key)");
            println!("    effective_model:   {effective}");
            println!(
                "    note:              set the missing provider env key or pin default_model"
            );
        }
        Err(ref err) => {
            println!("    status:            fail-closed (no usable provider key)");
            println!("    effective_model:   (none)");
            for line in err.lines() {
                println!("    error:             {line}");
            }
        }
    }
    println!("    detected_env_keys: {detected_msg}");
    println!("    note:              env names only — secret values are never printed");
}

fn print_prism(configured: &str) {
    println!("  [prism routing]");
    println!("    effective_model:   {configured} (no BYOK remap; prism uses this logical id)");
    println!(
        "    keys:              require at least one PRISM_*_API_KEY (e.g. PRISM_GROQ_API_KEY)"
    );
    println!("    note:              restart process after changing routing.provider_mode");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DEFAULT_PROTOCOL_MODEL_ID;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn doctor_routing_byok_keep_when_key_present() {
        let _lock = env_lock().lock().unwrap();
        let _openai = EnvGuard::set("OPENAI_API_KEY", Some("sk-test"));
        let mut config = Config::default();
        config.default_provider = Some("openai".into());
        config.default_model = Some("gpt-4o-mini".into());
        config.routing.provider_mode = ProviderRoutingMode::Byok;

        let d = diagnose_byok_routing(&config);
        assert_eq!(d.configured, "openai/gpt-4o-mini");
        assert_eq!(d.effective.as_deref().ok(), Some("openai/gpt-4o-mini"));
    }

    #[test]
    fn doctor_routing_byok_remaps_when_default_unkeyed() {
        let _lock = env_lock().lock().unwrap();
        let _openai = EnvGuard::set("OPENAI_API_KEY", Some("sk-test"));
        let _nvidia = EnvGuard::set("NVIDIA_API_KEY", None);
        let mut config = Config::default();
        // Protocol default is nvidia; only openai keyed → remap.
        assert_eq!(
            logical_model_id_from_config(&config),
            DEFAULT_PROTOCOL_MODEL_ID
        );
        config.routing.provider_mode = ProviderRoutingMode::Byok;

        let d = diagnose_byok_routing(&config);
        let effective = d.effective.expect("should remap");
        assert!(
            effective.starts_with("openai/"),
            "expected openai remap, got {effective}"
        );
        assert_ne!(d.configured, effective);
    }
}
