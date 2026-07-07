//! Tool trait surface — re-exported from `velaclaw-agent-runtime` (VL-ARCH-007).
//! 工具 trait：由 agent-runtime crate 提供。

pub use velaclaw_agent_runtime::{Tool, ToolExecutionContext, ToolResult, ToolSpec};

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy_tool"
        }

        fn description(&self) -> &str {
            "A deterministic test tool"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string" }
                }
            })
        }

        async fn execute(&self, args: serde_json::Value, _ctx: &ToolExecutionContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                success: true,
                output: args
                    .get("value")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                error: None,
            })
        }
    }

    #[test]
    fn tool_spec_includes_name_and_schema() {
        let tool = DummyTool;
        let spec = tool.spec();
        assert_eq!(spec.name, "dummy_tool");
        assert_eq!(spec.description, "A deterministic test tool");
        assert!(spec.parameters.get("properties").is_some());
    }

    #[tokio::test]
    async fn tool_execute_returns_output() {
        let tool = DummyTool;
        let result = tool
            .execute(serde_json::json!({ "value": "hello" }), &ToolExecutionContext::default())
            .await
            .expect("execute");
        assert!(result.success);
        assert_eq!(result.output, "hello");
    }
}
