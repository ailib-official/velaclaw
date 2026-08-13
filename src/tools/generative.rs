//! Experimental generative capability inspect (VL-GEN-001).
//! 显式生成式能力探查：只声明需求 + 读 manifest，不调厂商 HTTP。

use super::traits::{Tool, ToolExecutionContext, ToolResult};
use crate::protocol_registry::{inspect_generative_capability, resolve_local_protocol_root};
use crate::security::{PolicyHandle, ToolOperation};
use async_trait::async_trait;
use serde_json::json;

/// Agent-facing inspect of PT-GEN capability keys against local `AI_PROTOCOL_DIR`.
pub struct GenerativeCapabilityTool {
    security: PolicyHandle,
}

impl GenerativeCapabilityTool {
    pub fn new(security: PolicyHandle) -> Self {
        Self { security }
    }
}

#[async_trait]
impl Tool for GenerativeCapabilityTool {
    fn name(&self) -> &str {
        "generative_capability"
    }

    fn description(&self) -> &str {
        "Inspect whether a protocol model declares an explicit generative capability \
(image_generation, speech_to_text, or text_to_speech) and report the L-Exec endpoint. \
Does not call vendor APIs. Fail-closed when the capability key is omitted."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "model": {
                    "type": "string",
                    "description": "Logical id provider/model (e.g. openai/gpt-image-1)"
                },
                "capability": {
                    "type": "string",
                    "description": "image_generation | speech_to_text | text_to_speech"
                }
            },
            "required": ["model", "capability"]
        })
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolExecutionContext,
    ) -> anyhow::Result<ToolResult> {
        self.security
            .enforce_tool_operation(ToolOperation::Read, self.name())
            .map_err(|e| anyhow::anyhow!(e))?;

        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("model is required (provider/model)"))?;
        let capability = args
            .get("capability")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("capability is required"))?;

        let Some(root) = resolve_local_protocol_root() else {
            return Ok(ToolResult {
                success: false,
                output: String::new(),
                error: Some(
                    "Set AI_PROTOCOL_DIR to a local ai-protocol checkout (not a URL).".into(),
                ),
            });
        };

        match inspect_generative_capability(&root, model, capability) {
            Ok(info) => Ok(ToolResult {
                success: true,
                output: serde_json::to_string_pretty(&info)?,
                error: None,
            }),
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
    use crate::security::{PolicyHandle, SecurityPolicy};

    #[tokio::test]
    async fn missing_protocol_dir_fails_closed() {
        let _guard = crate::capability_index::PROTOCOL_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let prev_dir = std::env::var_os("AI_PROTOCOL_DIR");
        let prev_path = std::env::var_os("AI_PROTOCOL_PATH");
        std::env::remove_var("AI_PROTOCOL_DIR");
        std::env::remove_var("AI_PROTOCOL_PATH");
        let tool = GenerativeCapabilityTool::new(PolicyHandle::new(SecurityPolicy::default()));
        let out = tool
            .execute(
                json!({"model": "openai/gpt-image-1", "capability": "image_generation"}),
                &ToolExecutionContext::default(),
            )
            .await
            .expect("tool result");
        match prev_dir {
            Some(v) => std::env::set_var("AI_PROTOCOL_DIR", v),
            None => std::env::remove_var("AI_PROTOCOL_DIR"),
        }
        match prev_path {
            Some(v) => std::env::set_var("AI_PROTOCOL_PATH", v),
            None => std::env::remove_var("AI_PROTOCOL_PATH"),
        }
        assert!(!out.success);
        assert!(out
            .error
            .as_deref()
            .unwrap_or("")
            .contains("AI_PROTOCOL_DIR"));
    }
}
