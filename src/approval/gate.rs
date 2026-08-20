//! Unified approval gate — re-exports runtime gate with app backends (VL-UR-002).
//! 统一批准门：runtime 契约 + app 薄封装。

pub use velaclaw_agent_runtime::GateDecision;

use super::backend::{ChannelApprovalSession, ManagerApprovalBackend, PolicyHandleShellHook};
use super::{ApprovalHub, ApprovalManager};
use crate::agent::dispatcher::ParsedToolCall;
use crate::security::PolicyHandle;
use std::sync::Arc;
use velaclaw_agent_runtime::ApprovalGate as InnerGate;

/// Channel-aware approval gate (app wrapper over runtime [`InnerGate`]).
pub struct ApprovalGate<'a> {
    backend: ManagerApprovalBackend<'a>,
    shell_hook: Option<PolicyHandleShellHook>,
}

impl<'a> ApprovalGate<'a> {
    pub fn new(
        manager: &'a ApprovalManager,
        channel: &'a str,
        security: Option<PolicyHandle>,
    ) -> Self {
        Self {
            backend: ManagerApprovalBackend::new(manager, channel),
            shell_hook: security.map(PolicyHandleShellHook),
        }
    }

    pub fn with_hub(mut self, hub: Arc<ApprovalHub>) -> Self {
        self.backend = self.backend.with_hub(hub);
        self
    }

    pub fn with_channel_session(mut self, session: ChannelApprovalSession) -> Self {
        self.backend = self.backend.with_channel_session(session);
        self
    }

    pub fn needs_approval(&self, tool_name: &str) -> bool {
        self.inner().needs_approval(tool_name)
    }

    pub fn decide_sync(&self, call: &ParsedToolCall) -> GateDecision {
        self.inner().decide_sync(call)
    }

    pub async fn decide_async(&self, call: &ParsedToolCall) -> GateDecision {
        self.inner().decide_async(call).await
    }

    pub async fn decide_elevation_async(&self, call: &ParsedToolCall) -> GateDecision {
        self.inner().decide_elevation_async(call).await
    }

    fn inner(&self) -> InnerGate<'_, ManagerApprovalBackend<'a>> {
        let hook_ref = self
            .shell_hook
            .as_ref()
            .map(|h| h as &dyn velaclaw_agent_runtime::ShellPolicyHook);
        InnerGate::new(&self.backend, hook_ref)
    }
}
