//! Host orchestration surfaces (ORCH-WAVE) — decide / DAG UX helpers.
//!
//! Strategy layer only; not `velaclaw-agent-runtime` ownership.

#[cfg(feature = "ai-protocol")]
pub mod host_decide;

#[cfg(feature = "ai-protocol")]
pub mod dag_view;

#[cfg(feature = "ai-protocol")]
pub mod session_override;

#[cfg(feature = "ai-protocol")]
pub mod dag_emit;

#[cfg(feature = "ai-protocol")]
mod pricing;

#[cfg(feature = "ai-protocol")]
pub mod host_wire;

#[cfg(feature = "ai-protocol")]
pub mod turn_model;

#[cfg(feature = "ai-protocol")]
pub use host_wire::{
    finalize_tool_format_exhausted, map_provider_limit_error, maybe_apply_host_decide_failover,
    try_host_decide_model, try_host_decide_selection, HostDecideHost, HostDecideSelection,
};

#[cfg(feature = "ai-protocol")]
pub use turn_model::{resolve_turn_model, TurnModelDecision, TurnModelRequest, TurnModelSource};
