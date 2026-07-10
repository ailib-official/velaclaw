//! Channel system-prompt construction (VL-REVIEW-003).
//!
//! Extracted from `channels/mod.rs` to keep orchestration and prompt building separate.

use super::BOOTSTRAP_MAX_CHARS;
use crate::agent::prompt_composer::{
    build_channel_capabilities_section, build_hardware_section, build_safety_section,
    build_task_section, compose, load_openclaw_bootstrap_section, PromptMode, PromptTier,
    TieredSection,
};
use crate::identity;
use std::fmt::Write;
use std::path::Path;

/// Load workspace identity files and build a system prompt.
///
/// Follows the `OpenClaw` framework structure by default:
/// 1. Tooling — tool list + descriptions
/// 2. Safety — guardrail reminder
/// 3. Skills — full skill instructions and tool metadata
/// 4. Workspace — working directory
/// 5. Bootstrap files — AGENTS, SOUL, TOOLS, IDENTITY, USER, BOOTSTRAP, MEMORY
/// 6. Date & Time — timezone for cache stability
/// 7. Runtime — host, OS, model
///
/// When `identity_config` is set to AIEOS format, the bootstrap files section
/// is replaced with the AIEOS identity data loaded from file or inline JSON.
///
/// Daily memory files (`memory/*.md`) are NOT injected — they are accessed
/// on-demand via `memory_recall` / `memory_search` tools.
pub fn build_system_prompt(
    workspace_dir: &Path,
    model_name: &str,
    tools: &[(&str, &str)],
    skills: &[crate::skills::Skill],
    identity_config: Option<&crate::config::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
) -> String {
    build_system_prompt_with_mode_inner(
        workspace_dir,
        model_name,
        tools,
        skills,
        identity_config,
        bootstrap_max_chars,
        false,
        crate::config::SkillsPromptInjectionMode::Full,
        PromptMode::Full,
        None,
    )
}

pub fn build_system_prompt_with_mode(
    workspace_dir: &Path,
    model_name: &str,
    tools: &[(&str, &str)],
    skills: &[crate::skills::Skill],
    identity_config: Option<&crate::config::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
    native_tools: bool,
    skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
) -> String {
    build_system_prompt_with_mode_inner(
        workspace_dir,
        model_name,
        tools,
        skills,
        identity_config,
        bootstrap_max_chars,
        native_tools,
        skills_prompt_mode,
        PromptMode::Full,
        None,
    )
}

/// Pyramid assembly with explicit mode and optional total character budget.
#[must_use]
pub fn build_system_prompt_pyramid(
    workspace_dir: &Path,
    model_name: &str,
    tools: &[(&str, &str)],
    skills: &[crate::skills::Skill],
    identity_config: Option<&crate::config::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
    native_tools: bool,
    skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
    prompt_mode: PromptMode,
    max_chars: Option<usize>,
) -> String {
    build_system_prompt_with_mode_inner(
        workspace_dir,
        model_name,
        tools,
        skills,
        identity_config,
        bootstrap_max_chars,
        native_tools,
        skills_prompt_mode,
        prompt_mode,
        max_chars,
    )
}

fn build_system_prompt_with_mode_inner(
    workspace_dir: &Path,
    model_name: &str,
    tools: &[(&str, &str)],
    skills: &[crate::skills::Skill],
    identity_config: Option<&crate::config::IdentityConfig>,
    bootstrap_max_chars: Option<usize>,
    native_tools: bool,
    skills_prompt_mode: crate::config::SkillsPromptInjectionMode,
    prompt_mode: PromptMode,
    max_chars: Option<usize>,
) -> String {
    let mut sections: Vec<TieredSection> = Vec::with_capacity(10);

    // P0 — mission + safety (headline first)
    sections.push(TieredSection::new(
        PromptTier::P0Critical,
        build_task_section(native_tools),
    ));
    sections.push(TieredSection::new(
        PromptTier::P0Critical,
        build_safety_section(),
    ));

    // P1 — tools / hardware / skills
    if !tools.is_empty() {
        let mut tools_body =
            String::from("## Tools\n\nYou have access to the following tools:\n\n");
        for (name, desc) in tools {
            let _ = writeln!(tools_body, "- **{name}**: {desc}");
        }
        tools_body.push('\n');
        sections.push(TieredSection::new(PromptTier::P1Operational, tools_body));
    }

    let has_hardware = tools.iter().any(|(name, _)| {
        *name == "gpio_read"
            || *name == "gpio_write"
            || *name == "arduino_upload"
            || *name == "hardware_memory_map"
            || *name == "hardware_board_info"
            || *name == "hardware_memory_read"
            || *name == "hardware_capabilities"
    });
    if has_hardware {
        sections.push(TieredSection::new(
            PromptTier::P1Operational,
            build_hardware_section(),
        ));
    }

    if !skills.is_empty() {
        let skills_body = format!(
            "{}\n\n",
            crate::skills::skills_to_prompt_with_mode(skills, workspace_dir, skills_prompt_mode,)
        );
        sections.push(TieredSection::new(PromptTier::P1Operational, skills_body));
    }

    // P2 — project bootstrap / identity
    let max_chars_per_file = bootstrap_max_chars.unwrap_or(BOOTSTRAP_MAX_CHARS);
    let mut project_context = String::from("## Project Context\n\n");
    if let Some(config) = identity_config {
        if identity::is_aieos_configured(config) {
            match identity::load_aieos_identity(config, workspace_dir) {
                Ok(Some(aieos_identity)) => {
                    let aieos_prompt = identity::aieos_to_system_prompt(&aieos_identity);
                    if !aieos_prompt.is_empty() {
                        project_context.push_str(&aieos_prompt);
                        project_context.push_str("\n\n");
                    }
                }
                Ok(None) => {
                    project_context.push_str(&load_openclaw_bootstrap_section(
                        workspace_dir,
                        max_chars_per_file,
                    ));
                }
                Err(e) => {
                    eprintln!(
                        "Warning: Failed to load AIEOS identity: {e}. Using OpenClaw format."
                    );
                    project_context.push_str(&load_openclaw_bootstrap_section(
                        workspace_dir,
                        max_chars_per_file,
                    ));
                }
            }
        } else {
            project_context.push_str(&load_openclaw_bootstrap_section(
                workspace_dir,
                max_chars_per_file,
            ));
        }
    } else {
        project_context.push_str(&load_openclaw_bootstrap_section(
            workspace_dir,
            max_chars_per_file,
        ));
    }
    sections.push(TieredSection::new(PromptTier::P2Context, project_context));

    // P3 — ambient metadata (dropped first under budget pressure)
    let mut workspace = String::new();
    let _ = writeln!(
        workspace,
        "## Workspace\n\nWorking directory: `{}`\n",
        workspace_dir.display()
    );
    sections.push(TieredSection::new(PromptTier::P3Ambient, workspace));

    let now = chrono::Local::now();
    let tz = now.format("%Z").to_string();
    let mut datetime = String::new();
    let _ = writeln!(datetime, "## Current Date & Time\n\nTimezone: {tz}\n");
    sections.push(TieredSection::new(PromptTier::P3Ambient, datetime));

    let host =
        hostname::get().map_or_else(|_| "unknown".into(), |h| h.to_string_lossy().to_string());
    let mut runtime = String::new();
    let _ = writeln!(
        runtime,
        "## Runtime\n\nHost: {host} | OS: {} | Model: {model_name}\n",
        std::env::consts::OS,
    );
    sections.push(TieredSection::new(PromptTier::P3Ambient, runtime));

    sections.push(TieredSection::new(
        PromptTier::P3Ambient,
        build_channel_capabilities_section(),
    ));

    compose(sections, prompt_mode, max_chars)
}

// Re-export for tests and channel modules that referenced inject via prompt.rs internals.
pub use crate::agent::prompt_composer::inject_workspace_file;
