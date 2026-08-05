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
pub use host_wire::{try_host_decide_model, HostDecideHost};
