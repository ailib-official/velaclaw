//! Invoke a WIT-documented WASM plugin module (VL-MA-006).
//! 调用 WIT 合同下的 WASM 插件模块。

use super::traits::{Tool, ToolExecutionContext, ToolResult};
use crate::runtime::wasm::WasmRuntime;
use crate::security::{PolicyHandle, ToolOperation};
use async_trait::async_trait;
use serde_json::json;
use std::path::PathBuf;

/// Run a named `*.wasm` module from `runtime.wasm.tools_dir`.
pub struct WasmInvokeTool {
    security: PolicyHandle,
    runtime: WasmRuntime,
    workspace_dir: PathBuf,
}

impl WasmInvokeTool {
    pub fn new(security: PolicyHandle, runtime: WasmRuntime, workspace_dir: PathBuf) -> Self {
        Self {
            security,
            runtime,
            workspace_dir,
        }
    }
}

#[async_trait]
impl Tool for WasmInvokeTool {
    fn name(&self) -> &str {
        "wasm_invoke"
    }

    fn description(&self) -> &str {
        "Run a sandboxed WASM plugin module (WIT world `tool`, export `run() -> s32`). \
         Module files live under workspace runtime.wasm.tools_dir. Requires \
         `runtime.wasm.enabled = true` and a build with `--features runtime-wasm`."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "module": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Module stem (file is <module>.wasm). ASCII alphanumeric, '_' or '-' only."
                }
            },
            "required": ["module"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        if let Err(error) = self
            .security
            .enforce_tool_operation(ToolOperation::Act, "wasm_invoke")
        {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(error),
            });
        }

        let module = args
            .get("module")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if !WasmRuntime::is_safe_module_name(module) {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "'module' must be 1..=64 ASCII alphanumeric, '_' or '-' characters".into(),
                ),
            });
        }

        if let Err(e) = self.runtime.validate_config() {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            });
        }

        let caps = self.runtime.default_capabilities();
        match self
            .runtime
            .execute_module(module, &self.workspace_dir, &caps)
        {
            Ok(result) => {
                let success = result.stderr.is_empty();
                Ok(ToolResult {
                    success,
                    output: format!(
                        "wasm module={module} result={} fuel={}",
                        result.exit_code, result.fuel_consumed
                    ),
                    error: if success { None } else { Some(result.stderr) },
                })
            }
            Err(e) => Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(e.to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WasmRuntimeConfig;
    use crate::security::SecurityPolicy;

    #[tokio::test]
    async fn rejects_path_like_module_name() {
        let tool = WasmInvokeTool::new(
            PolicyHandle::new(SecurityPolicy::default()),
            WasmRuntime::new(WasmRuntimeConfig::default()),
            PathBuf::from("/tmp"),
        );
        let result = tool
            .execute(
                json!({"module": "../secret"}),
                &ToolExecutionContext::default(),
            )
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("alphanumeric"));
    }
}
