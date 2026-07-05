//! E2E tests for VelaClaw's tool-calling system.
//!
//! These tests validate the complete tool-calling lifecycle without external
//! service dependencies:
//!
//! - **Trait conformance**: Every tool type implements the `Tool` trait correctly
//!   (name, description, schema, spec generation).
//! - **Execution**: Each default tool executes correctly on its happy path and
//!   handles errors gracefully (bad input, missing params, security blocks).
//! - **Serialization**: `ToolResult`, `ToolSpec` roundtrip through JSON.
//! - **Schema cleaning**: Provider-specific schema cleaning strategies produce
//!   valid, provider-compatible schemas.
//! - **Registry assembly**: `default_tools()` and `all_tools()` assemble the
//!   correct set of tools under different configurations.
//! - **Agent dispatch**: The full agent→provider→tool pipeline works with
//!   mock providers and the `NativeToolDispatcher`.
//! - **Security enforcement**: Rate limiting, path sandboxing, command blocking,
//!   and autonomy levels are enforced consistently across tools.
//!
//! Ref: AGENTS.md §7.3 — Adding a Tool
//! Related: tests/agent_e2e.rs, tests/agent_loop_robustness.rs

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use velaclaw::agent::agent::Agent;
use velaclaw::agent::dispatcher::NativeToolDispatcher;
use velaclaw::config::{MemoryConfig, WebSearchConfig};
use velaclaw::memory;
use velaclaw::memory::Memory;
use velaclaw::observability::{NoopObserver, Observer};
use velaclaw::providers::{ChatRequest, ChatResponse, Provider, ToolCall};
use velaclaw::security::{AutonomyLevel, SecurityPolicy};
use velaclaw::tools::{self, Tool, ToolResult, ToolSpec};

// ═════════════════════════════════════════════════════════════════════════════
// Helpers
// ═════════════════════════════════════════════════════════════════════════════

fn test_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::Supervised,
        workspace_dir: workspace,
        allowed_commands: vec![
            "git".into(),
            "npm".into(),
            "cargo".into(),
            "ls".into(),
            "cat".into(),
            "grep".into(),
            "find".into(),
            "echo".into(),
            "pwd".into(),
            "wc".into(),
            "head".into(),
            "tail".into(),
            "date".into(),
            "touch".into(),
            "mkdir".into(),
            "env".into(),
        ],
        ..SecurityPolicy::default()
    })
}

fn permissive_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::Full,
        workspace_dir: workspace,
        max_actions_per_hour: 10_000,
        require_approval_for_medium_risk: false,
        block_high_risk_commands: false,
        workspace_only: false,
        forbidden_paths: vec![],
        allowed_commands: vec![
            "echo".into(),
            "ls".into(),
            "cat".into(),
            "grep".into(),
            "find".into(),
            "pwd".into(),
            "wc".into(),
            "head".into(),
            "tail".into(),
            "date".into(),
            "env".into(),
            "touch".into(),
            "mkdir".into(),
            "git".into(),
        ],
        ..SecurityPolicy::default()
    })
}

fn limited_security(workspace: std::path::PathBuf, max_actions: u32) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::Supervised,
        workspace_dir: workspace,
        max_actions_per_hour: max_actions,
        allowed_commands: vec!["echo".into(), "ls".into(), "cat".into(), "env".into()],
        ..SecurityPolicy::default()
    })
}

fn readonly_security(workspace: std::path::PathBuf) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy {
        autonomy: AutonomyLevel::ReadOnly,
        workspace_dir: workspace,
        max_actions_per_hour: 1000,
        ..SecurityPolicy::default()
    })
}

fn make_memory(tmp: &TempDir) -> Arc<dyn Memory> {
    let cfg = MemoryConfig {
        backend: "none".into(),
        ..MemoryConfig::default()
    };
    Arc::from(memory::create_memory(&cfg, tmp.path(), None).unwrap())
}

fn make_observer() -> Arc<dyn Observer> {
    Arc::from(NoopObserver {})
}

// ═════════════════════════════════════════════════════════════════════════════
// Mock Provider for agent-level tests
// ═════════════════════════════════════════════════════════════════════════════

struct MockProvider {
    responses: Mutex<Vec<ChatResponse>>,
}

impl MockProvider {
    fn new(responses: Vec<ChatResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn chat_with_system(
        &self,
        _system_prompt: Option<&str>,
        _message: &str,
        _model: &str,
        _temperature: f64,
    ) -> Result<String> {
        Ok("fallback".into())
    }

    async fn chat(
        &self,
        _request: ChatRequest<'_>,
        _model: &str,
        _temperature: f64,
    ) -> Result<ChatResponse> {
        let mut guard = self.responses.lock().unwrap();
        if guard.is_empty() {
            return Ok(ChatResponse {
                text: Some("done".into()),
                tool_calls: vec![],
            });
        }
        Ok(guard.remove(0))
    }
}

fn text_response(text: &str) -> ChatResponse {
    ChatResponse {
        text: Some(text.into()),
        tool_calls: vec![],
    }
}

fn tool_response(calls: Vec<ToolCall>) -> ChatResponse {
    ChatResponse {
        text: Some(String::new()),
        tool_calls: calls,
    }
}

fn build_agent(provider: Box<dyn Provider>, tools: Vec<Box<dyn Tool>>, tmp: &TempDir) -> Agent {
    Agent::builder()
        .provider(provider)
        .tools(tools)
        .memory(make_memory(tmp))
        .observer(make_observer())
        .tool_dispatcher(Box::new(NativeToolDispatcher::default()))
        .workspace_dir(tmp.path().to_path_buf())
        .build()
        .unwrap()
}

// ═════════════════════════════════════════════════════════════════════════════
// §1 — Tool trait conformance (all registry tools)
// ═════════════════════════════════════════════════════════════════════════════

mod trait_conformance {
    use super::*;
    use velaclaw::tools;

    /// Every tool in `default_tools()` must return a non-empty name,
    /// description, and valid JSON schema with `type: object` and `properties`.
    #[test]
    fn default_tools_have_valid_specs() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);

        assert!(
            !tools.is_empty(),
            "default_tools should produce at least 4 tools"
        );

        let mut names = std::collections::HashSet::new();
        for tool in &tools {
            let name = tool.name();
            assert!(!name.is_empty(), "Tool name must not be empty");
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "Tool name '{}' must be lowercase snake_case",
                name
            );
            assert!(
                names.insert(name.to_string()),
                "Duplicate tool name: {}",
                name
            );

            assert!(
                !tool.description().is_empty(),
                "Tool '{}' has empty description",
                name
            );

            let schema = tool.parameters_schema();
            assert!(
                schema.is_object(),
                "Tool '{}' schema is not an object",
                name
            );
            assert_eq!(
                schema["type"], "object",
                "Tool '{}' schema type is not 'object'",
                name
            );
            assert!(
                schema["properties"].is_object(),
                "Tool '{}' schema has no properties object",
                name
            );

            // spec() must match individual getters
            let spec = tool.spec();
            assert_eq!(spec.name, tool.name());
            assert_eq!(spec.description, tool.description());
            assert_eq!(spec.parameters, tool.parameters_schema());
        }
    }

    /// Tool names must be stable — changing them breaks LLM function calling.
    #[test]
    fn default_tool_names_are_stable() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

        let expected: &[&str] = &["shell", "file_read", "file_write", "glob_search"];
        for &name in expected {
            assert!(
                names.contains(&name),
                "Expected tool '{}' in default registry. Found: {:?}",
                name,
                names
            );
        }
    }

    /// Every tool's schema `required` field must reference known properties.
    #[test]
    fn tools_with_required_params_reference_valid_properties() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);

        for tool in &tools {
            let schema = tool.parameters_schema();
            if let Some(required) = schema["required"].as_array() {
                let props = schema["properties"]
                    .as_object()
                    .expect("schema must have properties");
                for req in required {
                    let key = req.as_str().expect("required items must be strings");
                    assert!(
                        props.contains_key(key),
                        "Tool '{}': required param '{}' not in properties",
                        tool.name(),
                        key
                    );
                }
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §2 — Tool execution (happy paths and error paths)
// ═════════════════════════════════════════════════════════════════════════════

mod tool_execution {
    use super::*;

    // ── Shell tool ──────────────────────────────────────────────────────

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_executes_command() {
        let tmp = TempDir::new().unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool
            .execute(json!({"command": "echo hello-world-42"}))
            .await
            .unwrap();
        assert!(result.success, "echo should succeed: {:?}", result.error);
        assert!(result.output.contains("hello-world-42"));
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn shell_missing_command_param() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool.execute(json!({})).await;
        assert!(result.is_err(), "Missing command should error");
        assert!(result.unwrap_err().to_string().contains("command"));
    }

    #[tokio::test]
    async fn shell_wrong_param_type() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool.execute(json!({"command": 123})).await;
        assert!(result.is_err(), "Non-string command should error");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_captures_stderr() {
        let tmp = TempDir::new().unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool
            .execute(json!({"command": "ls /nonexistent_xyz_123"}))
            .await
            .unwrap();
        assert!(!result.success, "Command to nonexistent path should fail");
    }

    #[tokio::test]
    async fn shell_blocks_readonly_autonomy() {
        let tmp = TempDir::new().unwrap();
        let sec = readonly_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool.execute(json!({"command": "ls"})).await.unwrap();
        assert!(!result.success, "ReadOnly autonomy should block shell");
        assert!(result.error.unwrap().contains("not allowed"));
    }

    // ── File read tool ───────────────────────────────────────────────────

    #[tokio::test]
    async fn file_read_existing_file() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("hello.txt"), "hello world\n")
            .await
            .unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::FileReadTool::new(sec);

        let result = tool.execute(json!({"path": "hello.txt"})).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("1: hello world"));
        assert!(result.output.contains("lines total"));
    }

    #[tokio::test]
    async fn file_read_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::FileReadTool::new(sec);

        let result = tool.execute(json!({"path": "nope.txt"})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("Failed to resolve"));
    }

    #[tokio::test]
    async fn file_read_missing_path_param() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::FileReadTool::new(sec);

        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_read_blocks_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::FileReadTool::new(sec);

        let result = tool
            .execute(json!({"path": "../../../etc/passwd"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("not allowed"));
    }

    #[tokio::test]
    async fn file_read_empty_file() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("empty.txt"), "")
            .await
            .unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::FileReadTool::new(sec);

        let result = tool.execute(json!({"path": "empty.txt"})).await.unwrap();
        assert!(result.success);
        assert_eq!(result.output, "");
    }

    // ── File write tool ──────────────────────────────────────────────────

    #[tokio::test]
    async fn file_write_creates_file() {
        let tmp = TempDir::new().unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let tool = tools::FileWriteTool::new(sec);

        let result = tool
            .execute(json!({
                "path": "new_file.txt",
                "content": "created by test"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let content = tokio::fs::read_to_string(tmp.path().join("new_file.txt"))
            .await
            .unwrap();
        assert_eq!(content, "created by test");
    }

    #[tokio::test]
    async fn file_write_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("existing.txt"), "old content")
            .await
            .unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let tool = tools::FileWriteTool::new(sec);

        let result = tool
            .execute(json!({
                "path": "existing.txt",
                "content": "new content"
            }))
            .await
            .unwrap();
        assert!(result.success);

        let content = tokio::fs::read_to_string(tmp.path().join("existing.txt"))
            .await
            .unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn file_write_missing_params() {
        let tmp = TempDir::new().unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let tool = tools::FileWriteTool::new(sec);

        let result = tool.execute(json!({"path": "f.txt"})).await;
        assert!(result.is_err());

        let result = tool.execute(json!({"content": "x"})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn file_write_blocks_readonly_autonomy() {
        let tmp = TempDir::new().unwrap();
        let sec = readonly_security(tmp.path().to_path_buf());
        let tool = tools::FileWriteTool::new(sec);

        let result = tool
            .execute(json!({
                "path": "blocked.txt",
                "content": "should not write"
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("read-only"));
    }

    #[tokio::test]
    async fn file_write_blocks_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let tool = tools::FileWriteTool::new(sec);

        let result = tool
            .execute(json!({
                "path": "../../../etc/hacked",
                "content": "malware"
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("not allowed"));
    }

    // ── Glob search tool ─────────────────────────────────────────────────

    #[tokio::test]
    async fn glob_search_finds_files() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("a.rs"), "").await.unwrap();
        tokio::fs::write(tmp.path().join("b.rs"), "").await.unwrap();
        tokio::fs::write(tmp.path().join("c.txt"), "")
            .await
            .unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GlobSearchTool::new(sec);

        let result = tool.execute(json!({"pattern": "*.rs"})).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("a.rs"));
        assert!(result.output.contains("b.rs"));
        assert!(!result.output.contains("c.txt"));
    }

    #[tokio::test]
    async fn glob_search_recursive() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::create_dir_all(tmp.path().join("sub/deep"))
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("root.txt"), "")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("sub/mid.txt"), "")
            .await
            .unwrap();
        tokio::fs::write(tmp.path().join("sub/deep/bottom.txt"), "")
            .await
            .unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GlobSearchTool::new(sec);

        let result = tool.execute(json!({"pattern": "**/*.txt"})).await.unwrap();
        assert!(result.success);
        assert!(result.output.contains("root.txt"));
        assert!(result.output.contains("sub/mid.txt"));
        assert!(result.output.contains("sub/deep/bottom.txt"));
    }

    #[tokio::test]
    async fn glob_search_no_matches() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GlobSearchTool::new(sec);

        let result = tool
            .execute(json!({"pattern": "nonexistent*"}))
            .await
            .unwrap();
        assert!(result.success);
        assert!(
            result.output.contains("No files")
                || result.output.is_empty()
                || result.output.contains("0")
        );
    }

    #[tokio::test]
    async fn glob_search_blocks_absolute_path() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GlobSearchTool::new(sec);

        let result = tool
            .execute(json!({"pattern": "/etc/passwd"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Absolute paths are not allowed"));
    }

    #[tokio::test]
    async fn glob_search_blocks_path_traversal() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GlobSearchTool::new(sec);

        let result = tool
            .execute(json!({"pattern": "../outside"}))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("Path traversal"));
    }

    #[tokio::test]
    async fn glob_search_missing_pattern_param() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GlobSearchTool::new(sec);

        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    // ── Roundtrip: write → read → verify ────────────────────────────────

    #[tokio::test]
    async fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let sec = permissive_security(tmp.path().to_path_buf());
        let writer = tools::FileWriteTool::new(sec.clone());
        let reader = tools::FileReadTool::new(sec);

        let content = "Write-then-read integration test content.\nLine 2.\nLine 3.";
        let w = writer
            .execute(json!({"path": "roundtrip.txt", "content": content}))
            .await
            .unwrap();
        assert!(w.success, "write failed: {:?}", w.error);

        let r = reader
            .execute(json!({"path": "roundtrip.txt"}))
            .await
            .unwrap();
        assert!(r.success, "read failed: {:?}", r.error);
        assert!(r
            .output
            .contains("Write-then-read integration test content"));
        assert!(r.output.contains("Line 2"));
        assert!(r.output.contains("Line 3"));
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §3 — Serialization roundtrips
// ═════════════════════════════════════════════════════════════════════════════

mod serialization {
    use super::*;

    #[test]
    fn tool_result_roundtrip_success() {
        let original = ToolResult {
            success: true,
            output: "hello world".into(),
            error: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.output, "hello world");
        assert!(parsed.error.is_none());
    }

    #[test]
    fn tool_result_roundtrip_error() {
        let original = ToolResult {
            success: false,
            output: String::new(),
            error: Some("something went wrong".into()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(!parsed.success);
        assert_eq!(parsed.error.unwrap(), "something went wrong");
    }

    #[test]
    fn tool_spec_roundtrip() {
        let original = ToolSpec {
            name: "my_tool".into(),
            description: "A test tool".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "x": { "type": "integer" }
                },
                "required": ["x"]
            }),
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: ToolSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "my_tool");
        assert_eq!(parsed.description, "A test tool");
        assert_eq!(parsed.parameters["type"], "object");
        assert_eq!(parsed.parameters["properties"]["x"]["type"], "integer");
    }

    #[test]
    fn tool_result_with_unicode() {
        let result = ToolResult {
            success: true,
            output: "こんにちは世界 🌍\n你好世界\nПривет мир".into(),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert!(parsed.output.contains("こんにちは"));
        assert!(parsed.output.contains("你好"));
        assert!(parsed.output.contains("Привет"));
    }

    #[test]
    fn tool_result_with_newlines_and_special_chars() {
        let result = ToolResult {
            success: true,
            output: "line1\nline2\n{\"key\": \"value\"}\n".into(),
            error: Some("error with \"quotes\" and \\backslashes".into()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert!(parsed.output.contains("line1"));
        assert!(parsed.output.contains("{\"key\": \"value\"}"));
        assert!(parsed.error.unwrap().contains("quotes"));
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §4 — Schema cleaning for provider compatibility
// ═════════════════════════════════════════════════════════════════════════════

mod schema_cleaning {
    use super::*;
    use velaclaw::tools;
    use velaclaw::tools::schema::{CleaningStrategy, SchemaCleanr};

    #[test]
    fn gemini_removes_constraint_keywords() {
        let schema = serde_json::json!({
            "type": "string",
            "minLength": 1,
            "maxLength": 100,
            "pattern": "^[a-z]+$",
            "format": "email",
            "description": "test desc"
        });
        let cleaned = SchemaCleanr::clean_for_gemini(schema);
        assert_eq!(cleaned["type"], "string");
        assert_eq!(cleaned["description"], "test desc");
        assert!(cleaned.get("minLength").is_none());
        assert!(cleaned.get("maxLength").is_none());
        assert!(cleaned.get("pattern").is_none());
        assert!(cleaned.get("format").is_none());
    }

    #[test]
    fn anthropic_preserves_constraint_keywords() {
        let schema = serde_json::json!({
            "type": "string",
            "minLength": 1,
            "description": "test desc"
        });
        let cleaned = SchemaCleanr::clean_for_anthropic(schema);
        // Anthropic allows constraint keywords; only removes ref/defs
        assert_eq!(cleaned["type"], "string");
        assert_eq!(cleaned["minLength"], 1);
        assert_eq!(cleaned["description"], "test desc");
    }

    #[test]
    fn openai_preserves_all() {
        let schema = serde_json::json!({
            "type": "string",
            "minLength": 1,
            "pattern": "^[a-z]+$",
            "description": "test desc"
        });
        let cleaned = SchemaCleanr::clean_for_openai(schema);
        assert_eq!(cleaned["minLength"], 1);
        assert_eq!(cleaned["pattern"], "^[a-z]+$");
    }

    #[test]
    fn resolves_refs() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "age": { "$ref": "#/$defs/Age" }
            },
            "$defs": { "Age": { "type": "integer", "minimum": 0 } }
        });
        let cleaned = SchemaCleanr::clean_for_anthropic(schema);
        assert_eq!(cleaned["properties"]["age"]["type"], "integer");
        assert!(cleaned.get("$defs").is_none());
    }

    #[test]
    fn const_converted_to_enum() {
        let schema = serde_json::json!({"const": "fixed"});
        let cleaned = SchemaCleanr::clean_for_gemini(schema);
        assert_eq!(cleaned["enum"], serde_json::json!(["fixed"]));
        assert!(cleaned.get("const").is_none());
    }

    #[test]
    fn nullable_type_arrays_simplified() {
        let schema = serde_json::json!({"type": ["string", "null"]});
        let cleaned = SchemaCleanr::clean_for_gemini(schema);
        assert_eq!(cleaned["type"], "string");
    }

    #[test]
    fn literal_union_flattened_to_enum() {
        let schema = serde_json::json!({
            "anyOf": [
                {"const": "admin", "type": "string"},
                {"const": "user", "type": "string"},
                {"const": "guest", "type": "string"}
            ]
        });
        let cleaned = SchemaCleanr::clean_for_gemini(schema);
        assert_eq!(cleaned["type"], "string");
        assert!(cleaned["enum"].is_array());
        let values: Vec<&str> = cleaned["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(values.contains(&"admin"));
        assert!(values.contains(&"user"));
        assert!(values.contains(&"guest"));
    }

    #[test]
    fn strategy_enum_matches_all_variants() {
        // Ensure CleaningStrategy variants are all covered
        let strategies = [
            CleaningStrategy::Gemini,
            CleaningStrategy::Anthropic,
            CleaningStrategy::OpenAI,
            CleaningStrategy::Conservative,
        ];
        for s in strategies {
            let kw = s.unsupported_keywords();
            assert!(
                !kw.is_empty() || matches!(s, CleaningStrategy::OpenAI),
                "{:?} should have empty or non-empty keywords as expected",
                s
            );
        }
    }

    #[test]
    fn validate_rejects_missing_type() {
        let schema = serde_json::json!({"properties": {"x": {"type": "string"}}});
        assert!(SchemaCleanr::validate(&schema).is_err());
    }

    #[test]
    fn validate_accepts_valid_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"x": {"type": "string"}}
        });
        assert!(SchemaCleanr::validate(&schema).is_ok());
    }

    #[test]
    fn default_tool_schemas_validate() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);
        for tool in &tools {
            let schema = tool.parameters_schema();
            assert!(
                SchemaCleanr::validate(&schema).is_ok(),
                "Tool '{}' schema failed validation",
                tool.name()
            );
        }
    }

    #[test]
    fn default_tool_schemas_survive_gemini_cleaning() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);
        for tool in &tools {
            let schema = tool.parameters_schema();
            let cleaned = SchemaCleanr::clean_for_gemini(schema);
            // After Gemini cleaning, every schema must still validate
            assert!(
                SchemaCleanr::validate(&cleaned).is_ok(),
                "Tool '{}' schema failed validation after Gemini cleaning",
                tool.name()
            );
            // After cleaning, the schema must still have type and properties
            assert!(
                cleaned.get("type").is_some(),
                "Tool '{}' lost 'type' after Gemini cleaning",
                tool.name()
            );
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §5 — Registry assembly
// ═════════════════════════════════════════════════════════════════════════════

mod registry {
    use super::*;

    #[test]
    fn default_tools_count() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);
        assert_eq!(tools.len(), 4, "default_tools must return exactly 4 tools");
    }

    #[test]
    fn default_tools_have_unique_names() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tools = tools::default_tools(sec);
        let mut seen = std::collections::HashSet::new();
        for tool in &tools {
            assert!(
                seen.insert(tool.name().to_string()),
                "Duplicate tool name in registry: {}",
                tool.name()
            );
        }
    }

    #[test]
    fn all_tools_excludes_browser_when_disabled() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..MemoryConfig::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());
        let browser = velaclaw::config::BrowserConfig {
            enabled: false,
            ..Default::default()
        };
        let http = velaclaw::config::HttpRequestConfig::default();
        let cfg = velaclaw::config::Config {
            web_search: WebSearchConfig {
                enabled: false,
                ..Default::default()
            },
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };

        let tool_list = tools::all_tools(
            Arc::new(velaclaw::config::Config::default()),
            &sec,
            mem,
            None,
            None,
            &browser,
            &http,
            tmp.path(),
            &std::collections::HashMap::new(),
            None,
            &cfg,
        );
        let names: Vec<&str> = tool_list.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"browser_open"));
        assert!(!names.contains(&"browser"));
    }

    #[test]
    fn all_tools_unique_names() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..Default::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());
        let browser = velaclaw::config::BrowserConfig {
            enabled: true,
            ..Default::default()
        };
        let http = velaclaw::config::HttpRequestConfig::default();
        let cfg = velaclaw::config::Config {
            web_search: WebSearchConfig {
                enabled: false,
                ..Default::default()
            },
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };

        let tool_list = tools::all_tools(
            Arc::new(velaclaw::config::Config::default()),
            &sec,
            mem,
            None,
            None,
            &browser,
            &http,
            tmp.path(),
            &std::collections::HashMap::new(),
            None,
            &cfg,
        );
        let mut seen = std::collections::HashSet::new();
        for tool in &tool_list {
            assert!(
                seen.insert(tool.name().to_string()),
                "Duplicate tool name in all_tools: {}",
                tool.name()
            );
        }
    }

    /// All tools in the registry (default + all) must have valid, non-empty schemas.
    #[test]
    fn all_registry_tools_have_valid_specs() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let default = tools::default_tools(sec.clone());
        for tool in &default {
            let spec = tool.spec();
            assert!(!spec.name.is_empty());
            assert!(!spec.description.is_empty());
            assert!(spec.parameters.is_object());
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §6 — Agent-level tool dispatch (full pipeline with mock provider)
// ═════════════════════════════════════════════════════════════════════════════

mod agent_dispatch {
    use super::*;

    /// Simple tool that records invocation for verification.
    struct RecordTool {
        name: &'static str,
        count: Arc<Mutex<usize>>,
    }

    impl RecordTool {
        fn new(name: &'static str) -> (Self, Arc<Mutex<usize>>) {
            let count = Arc::new(Mutex::new(0));
            (
                Self {
                    name,
                    count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Tool for RecordTool {
        fn name(&self) -> &str {
            self.name
        }
        fn description(&self) -> &str {
            "Records invocations"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
            let mut c = self.count.lock().unwrap();
            *c += 1;
            Ok(ToolResult {
                success: true,
                output: format!("call #{}", *c),
                error: None,
            })
        }
    }

    #[tokio::test]
    async fn agent_single_tool_dispatch() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let provider = Box::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "tc1".into(),
                name: "record".into(),
                arguments: "{}".into(),
            }]),
            text_response("done"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);
        let response = agent.turn("test").await.unwrap();
        assert!(!response.is_empty());
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn agent_parallel_tool_dispatch() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let provider = Box::new(MockProvider::new(vec![
            tool_response(vec![
                ToolCall {
                    id: "a".into(),
                    name: "record".into(),
                    arguments: "{}".into(),
                },
                ToolCall {
                    id: "b".into(),
                    name: "record".into(),
                    arguments: "{}".into(),
                },
                ToolCall {
                    id: "c".into(),
                    name: "record".into(),
                    arguments: "{}".into(),
                },
            ]),
            text_response("all done"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);
        let response = agent.turn("test").await.unwrap();
        assert!(!response.is_empty());
        assert_eq!(
            *count.lock().unwrap(),
            3,
            "All 3 parallel calls should execute"
        );
    }

    #[tokio::test]
    async fn agent_unknown_tool_recovery() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let provider = Box::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "tc1".into(),
                name: "nonexistent".into(),
                arguments: "{}".into(),
            }]),
            text_response("recovered"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);
        let response = agent.turn("call missing tool").await.unwrap();
        assert!(!response.is_empty());
        // Unknown tool should not increment the real tool's counter
        assert_eq!(*count.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn agent_multi_step_tool_chain() {
        let tmp = TempDir::new().unwrap();
        let (tool_a, count_a) = RecordTool::new("tool_a");
        let (tool_b, count_b) = RecordTool::new("tool_b");
        let provider = Box::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "t1".into(),
                name: "tool_a".into(),
                arguments: "{}".into(),
            }]),
            tool_response(vec![ToolCall {
                id: "t2".into(),
                name: "tool_b".into(),
                arguments: "{}".into(),
            }]),
            text_response("finished chain"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool_a), Box::new(tool_b)], &tmp);
        let response = agent.turn("chain test").await.unwrap();
        assert!(!response.is_empty());
        assert_eq!(*count_a.lock().unwrap(), 1);
        assert_eq!(*count_b.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn agent_text_only_no_tools() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let provider = Box::new(MockProvider::new(vec![text_response(
            "just text, no tools",
        )]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);
        let response = agent.turn("chat").await.unwrap();
        assert_eq!(response, "just text, no tools");
        assert_eq!(*count.lock().unwrap(), 0, "Tool should not be called");
    }

    #[tokio::test]
    async fn agent_multiple_turns_maintain_state() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let provider = Box::new(MockProvider::new(vec![
            text_response("turn 1"),
            tool_response(vec![ToolCall {
                id: "t1".into(),
                name: "record".into(),
                arguments: "{}".into(),
            }]),
            text_response("turn 2 after tool"),
            text_response("turn 3"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);

        let r1 = agent.turn("hello").await.unwrap();
        assert_eq!(r1, "turn 1");
        assert_eq!(*count.lock().unwrap(), 0);

        let r2 = agent.turn("use tool").await.unwrap();
        assert_eq!(r2, "turn 2 after tool");
        assert_eq!(*count.lock().unwrap(), 1);

        let r3 = agent.turn("bye").await.unwrap();
        assert_eq!(r3, "turn 3");
        assert_eq!(
            *count.lock().unwrap(),
            1,
            "No additional tool calls in turn 3"
        );
    }

    /// When a tool returns an error result (success: false),
    /// the error is propagated to the tool result message.
    #[tokio::test]
    async fn agent_tool_error_propagation() {
        struct FailTool;
        #[async_trait]
        impl Tool for FailTool {
            fn name(&self) -> &str {
                "fail_tool"
            }
            fn description(&self) -> &str {
                "Always fails"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                json!({"type": "object", "properties": {}})
            }
            async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
                Ok(ToolResult {
                    success: false,
                    output: String::new(),
                    error: Some("deliberate tool failure".into()),
                })
            }
        }

        let tmp = TempDir::new().unwrap();
        let provider = Box::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "t1".into(),
                name: "fail_tool".into(),
                arguments: "{}".into(),
            }]),
            text_response("handled failure"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(FailTool)], &tmp);
        let response = agent.turn("test failure").await.unwrap();
        // Agent should recover and continue after tool error
        assert!(!response.is_empty());
    }

    /// DeepSeek-style DSML text tool calls must execute when native tool_calls are empty.
    #[tokio::test]
    #[cfg(feature = "ai-protocol")]
    async fn agent_dispatches_deepseek_dsml_text_tool_calls() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let tag = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";
        let dsml = format!(
            "Running check.\n\
             <{tag}tool_calls>\n\
             <{tag}invoke name=\"record\">\n\
             <{tag}parameter name=\"note\" string=\"true\">via-dsml</{tag}parameter>\n\
             </{tag}invoke>\n\
             </{tag}tool_calls>"
        );
        let provider = Box::new(MockProvider::new(vec![
            ChatResponse {
                text: Some(dsml),
                tool_calls: vec![],
            },
            text_response("done after dsml"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);
        let response = agent.turn("check dsml").await.unwrap();
        assert_eq!(response, "done after dsml");
        assert_eq!(
            *count.lock().unwrap(),
            1,
            "DSML tool call should execute once"
        );
    }

    /// Malformed native JSON should not block DSML text fallback.
    #[tokio::test]
    #[cfg(feature = "ai-protocol")]
    async fn agent_falls_back_to_dsml_when_native_json_is_malformed() {
        let tmp = TempDir::new().unwrap();
        let (tool, count) = RecordTool::new("record");
        let tag = "\u{FF5C}\u{FF5C}DSML\u{FF5C}\u{FF5C}";
        let dsml = format!(
            "<{tag}tool_calls>\n\
             <{tag}invoke name=\"record\">\n\
             <{tag}parameter name=\"note\" string=\"true\">fallback</{tag}parameter>\n\
             </{tag}invoke>\n\
             </{tag}tool_calls>"
        );
        let provider = Box::new(MockProvider::new(vec![
            ChatResponse {
                text: Some(dsml),
                tool_calls: vec![ToolCall {
                    id: "broken".into(),
                    name: "record".into(),
                    arguments: "not-json".into(),
                }],
            },
            text_response("recovered via dsml"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(tool)], &tmp);
        let response = agent.turn("fallback test").await.unwrap();
        assert_eq!(response, "recovered via dsml");
        assert_eq!(
            *count.lock().unwrap(),
            1,
            "Malformed native JSON must not prevent DSML execution"
        );
    }

    /// When a tool panics (returns Err), the agent should catch it and continue.
    #[tokio::test]
    async fn agent_tool_panic_recovery() {
        struct PanicTool;
        #[async_trait]
        impl Tool for PanicTool {
            fn name(&self) -> &str {
                "panic_tool"
            }
            fn description(&self) -> &str {
                "Always panics"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                json!({"type": "object", "properties": {}})
            }
            async fn execute(&self, _args: serde_json::Value) -> Result<ToolResult> {
                anyhow::bail!("deliberate tool panic")
            }
        }

        let tmp = TempDir::new().unwrap();
        let provider = Box::new(MockProvider::new(vec![
            tool_response(vec![ToolCall {
                id: "t1".into(),
                name: "panic_tool".into(),
                arguments: "{}".into(),
            }]),
            text_response("recovered from panic"),
        ]));

        let mut agent = build_agent(provider, vec![Box::new(PanicTool)], &tmp);
        let response = agent.turn("test panic").await.unwrap();
        assert!(
            !response.is_empty(),
            "Agent should recover after tool error"
        );
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §7 — Security policy enforcement (rate limiting, path sandboxing)
// ═════════════════════════════════════════════════════════════════════════════

mod security_enforcement {
    use super::*;

    #[tokio::test]
    async fn file_read_rate_limits_exhausted() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("f.txt"), "data")
            .await
            .unwrap();
        let sec = limited_security(tmp.path().to_path_buf(), 2);
        let tool = tools::FileReadTool::new(sec);

        // First two: should succeed
        let r1 = tool.execute(json!({"path": "f.txt"})).await.unwrap();
        assert!(r1.success, "First read should succeed");
        let r2 = tool.execute(json!({"path": "f.txt"})).await.unwrap();
        assert!(r2.success, "Second read should succeed");

        // Third: rate limited
        let r3 = tool.execute(json!({"path": "f.txt"})).await.unwrap();
        assert!(!r3.success, "Third read should be rate limited");
        assert!(r3.error.unwrap().contains("Rate limit"));
    }

    #[tokio::test]
    async fn file_write_blocked_in_readonly_mode() {
        let tmp = TempDir::new().unwrap();
        let sec = readonly_security(tmp.path().to_path_buf());
        let tool = tools::FileWriteTool::new(sec);

        let result = tool
            .execute(json!({
                "path": "blocked.txt",
                "content": "should not write"
            }))
            .await
            .unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("read-only"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_blocked_in_readonly_mode() {
        let tmp = TempDir::new().unwrap();
        let sec = readonly_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool.execute(json!({"command": "echo test"})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not allowed"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shell_blocks_disallowed_command() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let runtime = Arc::new(velaclaw::runtime::NativeRuntime::new());
        let tool = tools::ShellTool::new(sec, runtime);

        let result = tool.execute(json!({"command": "rm -rf /"})).await.unwrap();
        assert!(!result.success);
        let error = result.error.unwrap();
        assert!(
            error.contains("not allowed") || error.contains("high-risk"),
            "Expected block message, got: {}",
            error
        );
    }

    #[tokio::test]
    async fn file_read_blocks_absolute_paths() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::FileReadTool::new(sec);

        let result = tool.execute(json!({"path": "/etc/passwd"})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not allowed"));
    }

    #[tokio::test]
    async fn multiple_tools_share_same_rate_limit() {
        let tmp = TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("shared.txt"), "data")
            .await
            .unwrap();
        let sec = limited_security(tmp.path().to_path_buf(), 3);
        let reader = tools::FileReadTool::new(sec.clone());
        let globber = tools::GlobSearchTool::new(sec);

        // Consume budget with reader
        let _ = reader.execute(json!({"path": "shared.txt"})).await.unwrap();
        let _ = reader.execute(json!({"path": "shared.txt"})).await.unwrap();

        // Glob should still have budget
        let r = globber.execute(json!({"pattern": "*.txt"})).await.unwrap();
        assert!(r.success, "Glob should succeed within budget");

        // 4th action should fail
        let r4 = globber.execute(json!({"pattern": "*.txt"})).await.unwrap();
        assert!(!r4.success, "Should be rate limited on 4th action");
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §8 — Git operations tool (read-only subset)
// ═════════════════════════════════════════════════════════════════════════════

mod git_operations {
    use super::*;

    fn init_test_repo(tmp: &TempDir) {
        let status = std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .status()
            .unwrap();
        assert!(status.success(), "git init failed");

        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@test.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ])
            .current_dir(tmp.path())
            .status()
            .unwrap();
    }

    #[tokio::test]
    async fn git_log_read_only() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(&tmp);
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GitOperationsTool::new(sec, tmp.path().to_path_buf());

        let result = tool
            .execute(json!({"operation": "log", "limit": 1}))
            .await
            .unwrap();
        assert!(result.success, "git log failed: {:?}", result.error);
    }

    #[tokio::test]
    async fn git_status_read_only() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(&tmp);
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GitOperationsTool::new(sec, tmp.path().to_path_buf());

        let result = tool.execute(json!({"operation": "status"})).await.unwrap();
        assert!(result.success, "git status failed: {:?}", result.error);
    }

    #[tokio::test]
    async fn git_branch_read_only() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(&tmp);
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GitOperationsTool::new(sec, tmp.path().to_path_buf());

        let result = tool.execute(json!({"operation": "branch"})).await.unwrap();
        assert!(result.success, "git branch failed: {:?}", result.error);
    }

    #[tokio::test]
    async fn git_blocks_write_operations_in_readonly_mode() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(&tmp);
        let sec = readonly_security(tmp.path().to_path_buf());
        let tool = tools::GitOperationsTool::new(sec, tmp.path().to_path_buf());

        let result = tool.execute(json!({"operation": "commit"})).await.unwrap();
        assert!(!result.success);
        let error = result.error.unwrap();
        assert!(
            error.contains("not allowed")
                || error.contains("read-only")
                || error.contains("higher autonomy"),
            "Expected write-block message, got: {}",
            error
        );
    }

    #[tokio::test]
    async fn git_rejects_unknown_operations() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(&tmp);
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GitOperationsTool::new(sec, tmp.path().to_path_buf());

        let ops = ["reset", "rebase", "push", "pull", "fetch"];
        for op in ops {
            let result = tool.execute(json!({"operation": op})).await.unwrap();
            assert!(
                !result.success,
                "Operation '{}' should be rejected as unknown or unsafe, got: {:?}",
                op, result.error
            );
        }
    }

    #[tokio::test]
    async fn git_missing_operation_param_returns_error_result() {
        let tmp = TempDir::new().unwrap();
        init_test_repo(&tmp);
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::GitOperationsTool::new(sec, tmp.path().to_path_buf());

        let result = tool.execute(json!({"args": ""})).await.unwrap();
        assert!(!result.success);
        assert!(result.error.as_deref().unwrap_or("").contains("operation"));
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §9 — Config-dependent tools (smoke test availability)
// ═════════════════════════════════════════════════════════════════════════════

mod config_dependent {
    use super::*;

    /// Verify Pushover tool parses its config file when present.
    #[tokio::test]
    async fn pushover_missing_config_returns_graceful_error() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let tool = tools::PushoverTool::new(sec, tmp.path().to_path_buf());

        match tool
            .execute(json!({"title": "Test", "message": "Hello"}))
            .await
        {
            Ok(result) => {
                assert!(!result.success);
                let error = result.error.unwrap();
                assert!(
                    error.contains("not found")
                        || error.contains("config")
                        || error.contains("Pushover"),
                    "Expected config-related error, got: {}",
                    error
                );
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("not found")
                        || msg.contains("config")
                        || msg.contains("Pushover")
                        || msg.contains(".env"),
                    "Expected config-related error, got: {}",
                    msg
                );
            }
        }
    }

    /// Verify schedule tool returns available schedules (even if empty).
    #[tokio::test]
    async fn schedule_list_works() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let cfg = velaclaw::config::Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };
        let tool = tools::ScheduleTool::new(sec, cfg);

        let result = tool.execute(json!({"action": "list"})).await.unwrap();
        assert!(result.success);
        // Should return a list (possibly empty)
        assert!(!result.output.is_empty() || result.output.contains("No scheduled"));
    }

    #[tokio::test]
    async fn proxy_config_get_returns_result() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let cfg = velaclaw::config::Config {
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };
        let tool = tools::ProxyConfigTool::new(Arc::new(cfg), sec);

        let result = tool.execute(json!({"action": "get"})).await;
        assert!(
            result.is_ok(),
            "proxy get should not panic: {:?}",
            result.err()
        );
    }

    /// Verify memory tools are instantiatable and have valid schemas.
    #[test]
    fn memory_tools_have_valid_schemas() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..Default::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());

        let store = tools::MemoryStoreTool::new(mem.clone(), sec.clone());
        assert_eq!(store.name(), "memory_store");
        assert!(!store.description().is_empty());
        assert!(store.parameters_schema().is_object());

        let recall = tools::MemoryRecallTool::new(mem);
        assert_eq!(recall.name(), "memory_recall");
        assert!(!recall.description().is_empty());
        assert!(recall.parameters_schema().is_object());
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §10 — Stress: many tools in registry, large schema generation
// ═════════════════════════════════════════════════════════════════════════════

mod stress {
    use super::*;

    /// Generate specs for all tools in the full registry; verify each is valid JSON.
    #[test]
    fn all_tools_generate_valid_specs() {
        let tmp = TempDir::new().unwrap();
        let sec = test_security(tmp.path().to_path_buf());
        let mem_cfg = MemoryConfig {
            backend: "markdown".into(),
            ..Default::default()
        };
        let mem: Arc<dyn Memory> =
            Arc::from(memory::create_memory(&mem_cfg, tmp.path(), None).unwrap());
        let browser = velaclaw::config::BrowserConfig {
            enabled: true,
            ..Default::default()
        };
        let http = velaclaw::config::HttpRequestConfig::default();
        let cfg = velaclaw::config::Config {
            web_search: WebSearchConfig {
                enabled: false,
                ..Default::default()
            },
            workspace_dir: tmp.path().join("workspace"),
            config_path: tmp.path().join("config.toml"),
            ..Default::default()
        };

        let tool_list = tools::all_tools(
            Arc::new(velaclaw::config::Config::default()),
            &sec,
            mem,
            None,
            None,
            &browser,
            &http,
            tmp.path(),
            &std::collections::HashMap::new(),
            None,
            &cfg,
        );

        assert!(
            tool_list.len() >= 20,
            "Expected 20+ tools in full registry, got {}",
            tool_list.len()
        );

        for tool in &tool_list {
            let spec = tool.spec();
            // Every spec must be valid JSON and have required fields
            let json_str = serde_json::to_string(&spec).unwrap();
            let _parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

            assert!(
                !spec.name.is_empty(),
                "Empty name for tool with desc: {}",
                spec.description
            );
            assert!(
                !spec.description.is_empty(),
                "Empty description for tool: {}",
                spec.name
            );
            assert!(
                spec.parameters.is_object(),
                "Non-object parameters for tool: {}",
                spec.name
            );
        }
    }

    /// Very large tool output should serialize fine.
    #[test]
    fn massive_tool_result_serialization() {
        let big_output = "x".repeat(100_000);
        let result = ToolResult {
            success: true,
            output: big_output.clone(),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        // Verify it deserializes back correctly
        let parsed: ToolResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.success);
        assert_eq!(parsed.output.len(), 100_000);
        assert!(parsed.output.starts_with('x'));
    }

    /// Many tool specs in a single JSON array.
    #[test]
    fn many_tool_specs_serialize_as_array() {
        let specs: Vec<ToolSpec> = (0..50)
            .map(|i| ToolSpec {
                name: format!("tool_{}", i),
                description: format!("Tool number {}", i),
                parameters: json!({"type": "object", "properties": {"value": {"type": "integer"}}}),
            })
            .collect();

        let json = serde_json::to_string(&specs).unwrap();
        let parsed: Vec<ToolSpec> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 50);
        for (i, spec) in parsed.iter().enumerate() {
            assert_eq!(spec.name, format!("tool_{}", i));
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// §11 — Manifest auto dispatcher (VL-TTC-005: from_config + build_tool_dispatcher)
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "ai-protocol")]
mod manifest_auto_dispatcher {
    use std::path::Path;
    use velaclaw::agent::dispatcher::build_tool_dispatcher;
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
    fn from_config_auto_dispatcher_prefers_native_for_deepseek() {
        let Some(protocol_dir) = ai_protocol_dir() else {
            eprintln!("SKIP: AI_PROTOCOL_DIR not set or deepseek.yaml missing");
            return;
        };
        std::env::set_var("AI_PROTOCOL_DIR", &protocol_dir);

        let config = deepseek_config();
        assert_eq!(config.agent.tool_dispatcher, "auto");

        let handle = ExecutionHandle::from_config(&config).expect("from_config");
        let provider = handle.provider_adapter().expect("provider_adapter");
        let policy = handle.tool_calling_policy();

        let dispatcher = build_tool_dispatcher(
            config.agent.tool_dispatcher.as_str(),
            provider.as_ref(),
            policy,
        );
        assert!(
            velaclaw::agent::dispatcher::ToolDispatcher::should_send_tool_specs(&*dispatcher),
            "auto + deepseek hybrid should use native dispatcher"
        );
    }

    #[test]
    fn from_config_xml_override_disables_native_specs() {
        let Some(protocol_dir) = ai_protocol_dir() else {
            eprintln!("SKIP: AI_PROTOCOL_DIR not set or deepseek.yaml missing");
            return;
        };
        std::env::set_var("AI_PROTOCOL_DIR", &protocol_dir);

        let mut config = deepseek_config();
        config.agent.tool_dispatcher = "xml".into();

        let handle = ExecutionHandle::from_config(&config).expect("from_config");
        let provider = handle.provider_adapter().expect("provider_adapter");
        let policy = handle.tool_calling_policy();

        let dispatcher = build_tool_dispatcher("xml", provider.as_ref(), policy);
        assert!(
            !ToolDispatcher::should_send_tool_specs(&*dispatcher),
            "xml override must force text dispatcher"
        );
    }

    #[test]
    fn from_config_native_override_enables_native_specs() {
        let Some(protocol_dir) = ai_protocol_dir() else {
            eprintln!("SKIP: AI_PROTOCOL_DIR not set or deepseek.yaml missing");
            return;
        };
        std::env::set_var("AI_PROTOCOL_DIR", &protocol_dir);

        let mut config = deepseek_config();
        config.agent.tool_dispatcher = "native".into();

        let handle = ExecutionHandle::from_config(&config).expect("from_config");
        let provider = handle.provider_adapter().expect("provider_adapter");
        let policy = handle.tool_calling_policy();

        let dispatcher = build_tool_dispatcher("native", provider.as_ref(), policy);
        assert!(
            ToolDispatcher::should_send_tool_specs(&*dispatcher),
            "native override must send tool specs"
        );
    }
}
