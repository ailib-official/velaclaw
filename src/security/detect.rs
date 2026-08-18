//! Auto-detection of available security features

use crate::config::{SandboxBackend, SecurityConfig};
use crate::security::traits::{FailClosedSandbox, NoopSandbox, Sandbox};
use std::path::Path;
use std::sync::Arc;

/// Honest description of the sandbox `create_sandbox` will install on the
/// production shell path (`all_tools_with_runtime`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveSandbox {
    pub name: String,
    pub source: &'static str,
    /// True when wrap_command applies OS isolation (not none / fail-closed).
    pub production_path: bool,
}

/// Create a sandbox based on config. Linux Auto is Landlock or fail-closed.
///
/// YOLO opt-out: `sandbox.enabled = false` or `backend = none`.
/// Autonomy Full does not change this selection.
pub fn create_sandbox(config: &SecurityConfig, workspace_dir: Option<&Path>) -> Arc<dyn Sandbox> {
    let backend = &config.sandbox.backend;

    if matches!(backend, SandboxBackend::None) || config.sandbox.enabled == Some(false) {
        return Arc::new(NoopSandbox);
    }

    let workspace = workspace_dir.map(Path::to_path_buf);

    match backend {
        SandboxBackend::Landlock => landlock_or_fail_closed(workspace),
        SandboxBackend::Firejail => {
            #[cfg(target_os = "linux")]
            {
                if let Ok(sandbox) = super::firejail::FirejailSandbox::new() {
                    return Arc::new(sandbox);
                }
            }
            tracing::warn!("Firejail requested but not available; fail-closed");
            Arc::new(FailClosedSandbox)
        }
        SandboxBackend::Bubblewrap => {
            #[cfg(feature = "sandbox-bubblewrap")]
            {
                #[cfg(any(target_os = "linux", target_os = "macos"))]
                {
                    if let Ok(sandbox) = super::bubblewrap::BubblewrapSandbox::new() {
                        return Arc::new(sandbox);
                    }
                }
            }
            tracing::warn!("Bubblewrap requested but not available; fail-closed");
            Arc::new(FailClosedSandbox)
        }
        SandboxBackend::Docker => {
            if let Ok(sandbox) = super::docker::DockerSandbox::new() {
                return Arc::new(sandbox);
            }
            tracing::warn!("Docker requested but not available; fail-closed");
            Arc::new(FailClosedSandbox)
        }
        SandboxBackend::Auto => detect_best_sandbox(workspace),
        SandboxBackend::None => Arc::new(NoopSandbox),
    }
}

fn landlock_or_fail_closed(workspace: Option<std::path::PathBuf>) -> Arc<dyn Sandbox> {
    #[cfg(all(feature = "sandbox-landlock", target_os = "linux"))]
    {
        match super::landlock::LandlockSandbox::with_workspace(workspace) {
            Ok(sandbox) => {
                tracing::info!("Landlock sandbox enabled (child pre_exec)");
                return Arc::new(sandbox);
            }
            Err(e) => {
                tracing::warn!("Landlock unavailable ({e}); fail-closed");
            }
        }
    }
    let _ = workspace;
    Arc::new(FailClosedSandbox)
}

/// Linux Auto: Landlock or fail-closed. Other OS: Noop until those backends ship.
fn detect_best_sandbox(workspace: Option<std::path::PathBuf>) -> Arc<dyn Sandbox> {
    #[cfg(target_os = "linux")]
    {
        landlock_or_fail_closed(workspace)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = workspace;
        tracing::info!("Non-Linux Auto sandbox: application-layer only (no Landlock)");
        Arc::new(NoopSandbox)
    }
}

/// Map config to the sandbox that production `ShellTool` wiring will use.
pub fn describe_effective_sandbox(config: &SecurityConfig) -> EffectiveSandbox {
    let yolo = matches!(config.sandbox.backend, SandboxBackend::None)
        || config.sandbox.enabled == Some(false);
    let source = if yolo {
        "explicit_yolo"
    } else if matches!(config.sandbox.backend, SandboxBackend::Auto) {
        #[cfg(target_os = "linux")]
        {
            "linux_auto"
        }
        #[cfg(not(target_os = "linux"))]
        {
            "non_linux_auto"
        }
    } else {
        "config_explicit"
    };
    let sandbox = create_sandbox(config, None);
    let name = sandbox.name().to_string();
    let production_path = matches!(
        name.as_str(),
        "landlock" | "firejail" | "bubblewrap" | "docker"
    );
    EffectiveSandbox {
        name,
        source,
        production_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{SandboxConfig, SecurityConfig};

    fn auto_config() -> SecurityConfig {
        SecurityConfig {
            sandbox: SandboxConfig {
                enabled: None,
                backend: SandboxBackend::Auto,
                firejail_args: Vec::new(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn detect_best_sandbox_returns_something() {
        let sandbox = detect_best_sandbox(None);
        assert!(sandbox.is_available());
    }

    #[test]
    fn explicit_none_returns_noop() {
        let config = SecurityConfig {
            sandbox: SandboxConfig {
                enabled: Some(false),
                backend: SandboxBackend::None,
                firejail_args: Vec::new(),
            },
            ..Default::default()
        };
        let sandbox = create_sandbox(&config, None);
        assert_eq!(sandbox.name(), "none");
        let report = describe_effective_sandbox(&config);
        assert_eq!(report.name, "none");
        assert_eq!(report.source, "explicit_yolo");
        assert!(!report.production_path);
    }

    #[test]
    fn linux_auto_is_landlock_or_fail_closed_not_silent_noop() {
        let sandbox = create_sandbox(&auto_config(), None);
        #[cfg(target_os = "linux")]
        {
            assert!(
                sandbox.name() == "landlock" || sandbox.name() == "fail-closed",
                "unexpected auto sandbox {}",
                sandbox.name()
            );
            assert_ne!(sandbox.name(), "none");
            assert_ne!(sandbox.name(), "firejail");
            assert_ne!(sandbox.name(), "docker");
        }
        #[cfg(not(target_os = "linux"))]
        {
            assert_eq!(sandbox.name(), "none");
        }
    }

    #[test]
    fn auto_mode_detects_something() {
        let sandbox = create_sandbox(&auto_config(), None);
        assert!(sandbox.is_available());
    }

    #[test]
    fn full_autonomy_does_not_select_noop_on_linux_auto() {
        // Autonomy is not a field of SecurityConfig; Auto must stay isolated/fail-closed.
        let sandbox = create_sandbox(&auto_config(), None);
        #[cfg(target_os = "linux")]
        {
            assert_ne!(sandbox.name(), "none");
        }
        let _ = sandbox;
    }
}
