//! Default values shared across onboarding, config, and CLI fallbacks.

/// Canonical example `provider/model` id for fresh installs (upstream ai-protocol naming).
/// Uses NVIDIA NIM's free-tier Nemotron model — publicly available without payment.
pub const DEFAULT_PROTOCOL_MODEL_ID: &str = "nvidia/nemotron-4-340b-instruct";
