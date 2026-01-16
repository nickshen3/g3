//! Session management for studio

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Session status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    Running,
    Complete,
    Paused,
    Failed,
}

/// Session type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionType {
    OneShot,
    Interactive,
}

/// A studio session representing a g3 agent run in a worktree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Short unique identifier
    pub id: String,
    /// Agent name
    pub agent: String,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// Current status
    pub status: SessionStatus,
    /// Process ID if running
    pub pid: Option<u32>,
    /// Path to the worktree
    pub worktree_path: Option<PathBuf>,
    /// Type of session
    #[serde(default = "default_session_type")]
    pub session_type: SessionType,
}

impl Session {
    /// Create a new session with a short UUID
    pub fn new(agent: &str) -> Self {
        // Generate a short UUID (first 8 chars of a UUID v4)
        let full_uuid = Uuid::new_v4();
        let short_id = full_uuid.to_string()[..8].to_string();

        Self {
            id: short_id,
            agent: agent.to_string(),
            created_at: Utc::now(),
            status: SessionStatus::Running,
            pid: None,
            worktree_path: None,
            session_type: SessionType::OneShot,
        }
    }

    /// Create a new interactive session
    pub fn new_interactive() -> Self {
        let full_uuid = Uuid::new_v4();
        let short_id = full_uuid.to_string()[..8].to_string();

        Self {
            id: short_id,
            agent: "interactive".to_string(),
            created_at: Utc::now(),
            status: SessionStatus::Running,
            pid: None,
            worktree_path: None,
            session_type: SessionType::Interactive,
        }
    }

    /// Get the git branch name for this session
    pub fn branch_name(&self) -> String {
        format!("sessions/{}/{}", self.agent, self.id)
    }

    /// Get the sessions metadata directory
    fn sessions_dir(repo_root: &Path) -> PathBuf {
        repo_root.join(".worktrees").join(".sessions")
    }

    /// Get the path to this session's metadata file
    fn metadata_path(&self, repo_root: &Path) -> PathBuf {
        Self::sessions_dir(repo_root).join(format!("{}.json", self.id))
    }

    /// Save session metadata
    pub fn save(&self, repo_root: &Path, worktree_path: &Path) -> Result<()> {
        let mut session = self.clone();
        session.worktree_path = Some(worktree_path.to_path_buf());

        let sessions_dir = Self::sessions_dir(repo_root);
        fs::create_dir_all(&sessions_dir).context("Failed to create sessions directory")?;

        let path = session.metadata_path(repo_root);
        let json = serde_json::to_string_pretty(&session)?;
        fs::write(&path, json).context("Failed to write session metadata")?;

        Ok(())
    }

    /// Update session with PID
    pub fn update_pid(&self, repo_root: &Path, pid: u32) -> Result<()> {
        let path = self.metadata_path(repo_root);
        let content = fs::read_to_string(&path).context("Failed to read session metadata")?;
        let mut session: Session = serde_json::from_str(&content)?;
        session.pid = Some(pid);

        let json = serde_json::to_string_pretty(&session)?;
        fs::write(&path, json).context("Failed to write session metadata")?;

        Ok(())
    }

    /// Mark session as complete
    pub fn mark_complete(&self, repo_root: &Path, success: bool) -> Result<()> {
        let path = self.metadata_path(repo_root);
        let content = fs::read_to_string(&path).context("Failed to read session metadata")?;
        let mut session: Session = serde_json::from_str(&content)?;
        session.status = if success {
            SessionStatus::Complete
        } else {
            SessionStatus::Failed
        };
        session.pid = None;

        let json = serde_json::to_string_pretty(&session)?;
        fs::write(&path, json).context("Failed to write session metadata")?;

        Ok(())
    }

    /// Mark session as paused (for interactive sessions)
    pub fn mark_paused(&self, repo_root: &Path) -> Result<()> {
        let path = self.metadata_path(repo_root);
        let content = fs::read_to_string(&path).context("Failed to read session metadata")?;
        let mut session: Session = serde_json::from_str(&content)?;
        session.status = SessionStatus::Paused;
        session.pid = None;

        let json = serde_json::to_string_pretty(&session)?;
        fs::write(&path, json).context("Failed to write session metadata")?;

        Ok(())
    }

    /// Load a session by ID
    pub fn load(repo_root: &Path, session_id: &str) -> Result<Session> {
        let path = Self::sessions_dir(repo_root).join(format!("{}.json", session_id));

        if !path.exists() {
            bail!("Session '{}' not found", session_id);
        }

        let content = fs::read_to_string(&path).context("Failed to read session metadata")?;
        let session: Session = serde_json::from_str(&content)?;

        Ok(session)
    }

    /// List all sessions
    pub fn list_all(repo_root: &Path) -> Result<Vec<Session>> {
        let sessions_dir = Self::sessions_dir(repo_root);

        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for entry in fs::read_dir(&sessions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<Session>(&content) {
                        sessions.push(session);
                    }
                }
            }
        }

        // Sort by creation time, newest first
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(sessions)
    }

    /// Delete session metadata
    pub fn delete(&self, repo_root: &Path) -> Result<()> {
        let path = self.metadata_path(repo_root);

        if path.exists() {
            fs::remove_file(&path).context("Failed to delete session metadata")?;
        }

        Ok(())
    }
}

/// Default session type for backwards compatibility with existing sessions
fn default_session_type() -> SessionType {
    SessionType::OneShot
}
