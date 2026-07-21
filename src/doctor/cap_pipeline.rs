//! CR-CAP-007: shared capability-index routing narrative for doctor surfaces.
//!
//! Product story: declared facts → reachable (keys) → optional host select.
//! Not an "intent routing" product mainline; CAP-003 remains trial wire.

/// One-line pipeline shared by `routing` / `capabilities` / `capability-route`.
pub const CAP_PIPELINE_LINE: &str =
    "pipeline: protocol facts (declared) → reachable (local keys) → optional host select (opt-in)";

/// Related doctor commands (operator navigation).
pub const CAP_RELATED_DOCTOR: &str = "\
Related (capability-index; default-off live select):\n\
  velaclaw doctor capabilities [--tag <Tag>] [--reachable-only]  — declared vs reachable\n\
  velaclaw doctor capability-route --tag <Tag> --force           — opt-in select observe (alias: intent-route)\n\
  velaclaw doctor routing                                        — BYOK/prism execution path\n\
Prefer explicit --tag over NL classification. Live chat unchanged unless\n\
`[agent].intent_capability_route = true` (alias capability_index_route).";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_line_mentions_reachable_not_intent_product() {
        assert!(CAP_PIPELINE_LINE.contains("reachable"));
        assert!(CAP_PIPELINE_LINE.contains("declared"));
        assert!(!CAP_PIPELINE_LINE
            .to_ascii_lowercase()
            .contains("intent product"));
    }

    #[test]
    fn related_doctor_prefers_capability_route_and_tag() {
        assert!(CAP_RELATED_DOCTOR.contains("capability-route"));
        assert!(CAP_RELATED_DOCTOR.contains("--tag"));
        assert!(CAP_RELATED_DOCTOR.contains("intent_capability_route"));
        assert!(
            CAP_RELATED_DOCTOR.contains("default-off") || CAP_RELATED_DOCTOR.contains("unchanged")
        );
    }
}
