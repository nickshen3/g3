//! Git worktree management for studio sessions

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::session::Session;

/// Manages git worktrees for studio sessions
pub struct GitWorktree {
    repo_root: PathBuf,
}

impl GitWorktree {
    pub fn new(repo_root: &Path) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
        }
    }

    /// Get the base directory for all worktrees
    fn worktrees_base(&self) -> PathBuf {
        self.repo_root.join(".worktrees").join("sessions")
    }

    /// Get the worktree path for a session
    pub fn worktree_path(&self, session: &Session) -> PathBuf {
        self.worktrees_base()
            .join(&session.agent)
            .join(&session.id)
    }

    /// Create a new worktree for a session
    pub fn create(&self, session: &Session) -> Result<PathBuf> {
        let worktree_path = self.worktree_path(session);
        let branch_name = session.branch_name();

        // Ensure parent directory exists
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent)
                .context("Failed to create worktree parent directory")?;
        }

        // Create the worktree with a new branch
        // git worktree add -b <branch> <path>
        let output = Command::new("git")
            .current_dir(&self.repo_root)
            .args([
                "worktree",
                "add",
                "-b",
                &branch_name,
                worktree_path.to_str().unwrap(),
            ])
            .output()
            .context("Failed to run git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create worktree: {}", stderr);
        }

        Ok(worktree_path)
    }

    /// Remove a worktree and its branch
    pub fn remove(&self, session: &Session) -> Result<()> {
        let worktree_path = self.worktree_path(session);
        let branch_name = session.branch_name();

        // Remove the worktree (force to handle uncommitted changes)
        if worktree_path.exists() {
            let output = Command::new("git")
                .current_dir(&self.repo_root)
                .args([
                    "worktree",
                    "remove",
                    "--force",
                    worktree_path.to_str().unwrap(),
                ])
                .output()
                .context("Failed to run git worktree remove")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Don't fail if worktree is already gone
                if !stderr.contains("is not a working tree") {
                    bail!("Failed to remove worktree: {}", stderr);
                }
            }
        }

        // Prune worktrees to clean up any stale entries
        let _ = Command::new("git")
            .current_dir(&self.repo_root)
            .args(["worktree", "prune"])
            .output();

        // Delete the branch
        let output = Command::new("git")
            .current_dir(&self.repo_root)
            .args(["branch", "-D", &branch_name])
            .output()
            .context("Failed to run git branch -D")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Don't fail if branch doesn't exist
            if !stderr.contains("not found") {
                bail!("Failed to delete branch: {}", stderr);
            }
        }

        // Clean up empty directories
        let agent_dir = self.worktrees_base().join(&session.agent);
        if agent_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&agent_dir) {
                if entries.count() == 0 {
                    let _ = std::fs::remove_dir(&agent_dir);
                }
            }
        }

        Ok(())
    }

    /// Merge a branch to main
    pub fn merge_to_main(&self, branch_name: &str) -> Result<()> {
        // First, checkout main
        let output = Command::new("git")
            .current_dir(&self.repo_root)
            .args(["checkout", "main"])
            .output()
            .context("Failed to checkout main")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to checkout main: {}", stderr);
        }

        // Merge the branch (allow merge commits)
        let output = Command::new("git")
            .current_dir(&self.repo_root)
            .args(["merge", branch_name, "-m", &format!("Merge {}", branch_name)])
            .output()
            .context("Failed to merge branch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to merge branch: {}", stderr);
        }

        Ok(())
    }

    /// List all worktrees
    #[allow(dead_code)]
    pub fn list(&self) -> Result<Vec<String>> {
        let output = Command::new("git")
            .current_dir(&self.repo_root)
            .args(["worktree", "list", "--porcelain"])
            .output()
            .context("Failed to list worktrees")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to list worktrees: {}", stderr);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();

        for line in stdout.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                worktrees.push(path.to_string());
            }
        }

        Ok(worktrees)
    }
}
