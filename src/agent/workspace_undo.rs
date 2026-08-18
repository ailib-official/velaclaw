//! Workspace undo: restore tracked git files when `.git` already exists (VL-MA-004 ACK).
//! 工作区 undo：仅在已有 `.git` 时 restore 已跟踪文件；禁止 git init。

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoAvailability {
    pub available: bool,
    pub mechanism: &'static str,
    pub detail: String,
}

pub fn describe_undo(workspace_dir: &Path) -> UndoAvailability {
    if workspace_has_git(workspace_dir) {
        UndoAvailability {
            available: true,
            mechanism: "git-restore-tracked",
            detail: "workspace has .git; undo restores tracked files to HEAD (no git init, no untracked clean)".into(),
        }
    } else {
        UndoAvailability {
            available: false,
            mechanism: "unavailable",
            detail: "no .git in workspace; undo refuses (will not git init; MEMORY_SNAPSHOT is not workspace undo)".into(),
        }
    }
}

pub fn workspace_has_git(workspace_dir: &Path) -> bool {
    workspace_dir.join(".git").exists()
}

/// Restore tracked worktree files to HEAD. Does not `git init`, `git revert`, or `git clean`.
pub fn restore_tracked_if_git(workspace_dir: &Path) -> Result<String> {
    if !workspace_has_git(workspace_dir) {
        bail!(
            "undo unavailable: workspace has no .git (will not git init). \
             MEMORY_SNAPSHOT is not a workspace file undo."
        );
    }
    let status = Command::new("git")
        .args([
            "restore",
            "--source=HEAD",
            "--worktree",
            "--staged",
            "--",
            ".",
        ])
        .current_dir(workspace_dir)
        .status()
        .context("spawn git restore")?;
    if !status.success() {
        bail!("git restore failed with status {status}");
    }
    Ok("restored tracked files to HEAD".into())
}

/// Doctor one-liner.
pub fn doctor_line(workspace_dir: &Path) -> String {
    let report = describe_undo(workspace_dir);
    format!(
        "undo={} available={} ({})",
        report.mechanism,
        if report.available { "yes" } else { "no" },
        report.detail
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    #[test]
    fn describe_unavailable_without_git() {
        let dir = tempfile::tempdir().unwrap();
        let report = describe_undo(dir.path());
        assert!(!report.available);
        assert_eq!(report.mechanism, "unavailable");
        let err = restore_tracked_if_git(dir.path()).unwrap_err();
        assert!(err.to_string().contains("no .git"));
        assert!(!dir.path().join(".git").exists());
    }

    #[test]
    fn restore_tracked_file_when_git_present() {
        let dir = tempfile::tempdir().unwrap();
        let init = Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(init.success());
        let _ = Command::new("git")
            .args(["config", "user.email", "velaclaw_test@example.com"])
            .current_dir(dir.path())
            .status();
        let _ = Command::new("git")
            .args(["config", "user.name", "VelaClawAgent"])
            .current_dir(dir.path())
            .status();
        fs::write(dir.path().join("tracked.txt"), "orig\n").unwrap();
        assert!(Command::new("git")
            .args(["add", "tracked.txt"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        fs::write(dir.path().join("tracked.txt"), "dirty\n").unwrap();
        fs::write(dir.path().join("untracked.txt"), "keep\n").unwrap();
        let msg = restore_tracked_if_git(dir.path()).unwrap();
        assert!(msg.contains("restored"));
        assert_eq!(
            fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
            "orig\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("untracked.txt")).unwrap(),
            "keep\n"
        );
        assert!(dir.path().join(".git").exists());
    }
}
