//! Project loading and management for the /project command.
//!
//! Projects allow loading context from a specific project directory that persists
//! in the system message and survives compaction/dehydration.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

/// Represents an active project with its loaded content.
#[derive(Debug, Clone)]
pub struct Project {
    /// Absolute path to the project directory
    pub path: PathBuf,
    /// Combined content blob to append to system message
    pub content: String,
    /// List of files that were successfully loaded
    pub loaded_files: Vec<String>,
}

impl Project {
    /// Load a project from the given absolute path.
    ///
    /// Loads the following files if present (skips missing silently):
    /// - brief.md
    /// - contacts.yaml
    /// - status.md
    ///
    /// Also loads projects.md from the workspace root if present.
    pub fn load(project_path: &Path, workspace_dir: &Path) -> Option<Self> {
        let mut content_parts = Vec::new();
        let mut loaded_files = Vec::new();

        // Load workspace-level projects.md if present
        let projects_md_path = workspace_dir.join("projects.md");
        if projects_md_path.exists() {
            if let Ok(projects_content) = std::fs::read_to_string(&projects_md_path) {
                content_parts.push(format!(
                    "=== PROJECT INSTRUCTIONS ===\n{}\n=== END PROJECT INSTRUCTIONS ===",
                    projects_content.trim()
                ));
                loaded_files.push("projects.md".to_string());
            }
        }

        // Load project-specific files
        let project_files = ["brief.md", "contacts.yaml", "status.md"];
        let mut project_content_parts = Vec::new();

        for filename in &project_files {
            let file_path = project_path.join(filename);
            if file_path.exists() {
                if let Ok(file_content) = std::fs::read_to_string(&file_path) {
                    let section_name = match *filename {
                        "brief.md" => "Brief",
                        "contacts.yaml" => "Contacts",
                        "status.md" => "Status",
                        _ => filename,
                    };
                    project_content_parts.push(format!(
                        "## {}\n{}",
                        section_name,
                        file_content.trim()
                    ));
                    loaded_files.push(filename.to_string());
                }
            }
        }

        // If we loaded any project-specific files, add the active project header
        if !project_content_parts.is_empty() {
            content_parts.push(format!(
                "=== ACTIVE PROJECT: {} ===\n{}",
                project_path.display(),
                project_content_parts.join("\n\n")
            ));
        }

        // Only return a project if we loaded something
        if loaded_files.is_empty() {
            return None;
        }

        Some(Project {
            path: project_path.to_path_buf(),
            content: content_parts.join("\n\n"),
            loaded_files,
        })
    }

    /// Format the loaded files status message (e.g., "✓ brief.md  ✓ status.md")
    pub fn format_loaded_status(&self) -> String {
        self.loaded_files
            .iter()
            .map(|f| format!("✓ {}", f))
            .collect::<Vec<_>>()
            .join("  ")
    }
}

/// Load and validate a project from a path string.
///
/// This is the shared logic used by both `--project` CLI flag and `/project` command.
/// It handles:
/// - Tilde expansion for home directory
/// - Validation that path is absolute
/// - Validation that path exists
/// - Loading project files
///
/// Returns the loaded Project or an error with a user-friendly message.
pub fn load_and_validate_project(project_path_str: &str, workspace_dir: &Path) -> Result<Project> {
    // Expand tilde if present
    let project_path = if project_path_str.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(&project_path_str[2..])
        } else {
            PathBuf::from(project_path_str)
        }
    } else {
        PathBuf::from(project_path_str)
    };

    // Validate path is absolute
    if !project_path.is_absolute() {
        return Err(anyhow!(
            "Project path must be absolute (e.g., /Users/name/projects/myproject)"
        ));
    }

    // Validate path exists
    if !project_path.exists() {
        return Err(anyhow!("Project path does not exist: {}", project_path.display()));
    }

    // Load the project
    Project::load(&project_path, workspace_dir)
        .ok_or_else(|| anyhow!("No project files found (brief.md, contacts.yaml, status.md)"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_format_loaded_status() {
        let project = Project {
            path: PathBuf::from("/test/project"),
            content: String::new(),
            loaded_files: vec!["brief.md".to_string(), "status.md".to_string()],
        };
        assert_eq!(project.format_loaded_status(), "✓ brief.md  ✓ status.md");
    }

    #[test]
    fn test_format_loaded_status_single_file() {
        let project = Project {
            path: PathBuf::from("/test/project"),
            content: String::new(),
            loaded_files: vec!["brief.md".to_string()],
        };
        assert_eq!(project.format_loaded_status(), "✓ brief.md");
    }

    #[test]
    fn test_load_project_with_all_files() {
        let workspace = TempDir::new().unwrap();
        let project_dir = TempDir::new().unwrap();

        // Create project files
        fs::write(project_dir.path().join("brief.md"), "Project brief").unwrap();
        fs::write(project_dir.path().join("contacts.yaml"), "contacts: []").unwrap();
        fs::write(project_dir.path().join("status.md"), "In progress").unwrap();

        let project = Project::load(project_dir.path(), workspace.path()).unwrap();

        assert_eq!(project.loaded_files.len(), 3);
        assert!(project.loaded_files.contains(&"brief.md".to_string()));
        assert!(project.loaded_files.contains(&"contacts.yaml".to_string()));
        assert!(project.loaded_files.contains(&"status.md".to_string()));
        assert!(project.content.contains("=== ACTIVE PROJECT:"));
        assert!(project.content.contains("## Brief"));
        assert!(project.content.contains("## Contacts"));
        assert!(project.content.contains("## Status"));
    }

    #[test]
    fn test_load_project_with_workspace_projects_md() {
        let workspace = TempDir::new().unwrap();
        let project_dir = TempDir::new().unwrap();

        // Create workspace projects.md
        fs::write(workspace.path().join("projects.md"), "Global project instructions").unwrap();

        // Create one project file
        fs::write(project_dir.path().join("brief.md"), "Project brief").unwrap();

        let project = Project::load(project_dir.path(), workspace.path()).unwrap();

        assert_eq!(project.loaded_files.len(), 2);
        assert!(project.loaded_files.contains(&"projects.md".to_string()));
        assert!(project.loaded_files.contains(&"brief.md".to_string()));
        assert!(project.content.contains("=== PROJECT INSTRUCTIONS ==="));
        assert!(project.content.contains("=== END PROJECT INSTRUCTIONS ==="));
        assert!(project.content.contains("=== ACTIVE PROJECT:"));
    }

    #[test]
    fn test_load_project_missing_files() {
        let workspace = TempDir::new().unwrap();
        let project_dir = TempDir::new().unwrap();

        // Create only one file
        fs::write(project_dir.path().join("status.md"), "Status only").unwrap();

        let project = Project::load(project_dir.path(), workspace.path()).unwrap();

        assert_eq!(project.loaded_files.len(), 1);
        assert!(project.loaded_files.contains(&"status.md".to_string()));
        assert!(!project.content.contains("## Brief"));
        assert!(project.content.contains("## Status"));
    }

    #[test]
    fn test_load_project_no_files() {
        let workspace = TempDir::new().unwrap();
        let project_dir = TempDir::new().unwrap();

        // No files created
        let project = Project::load(project_dir.path(), workspace.path());

        assert!(project.is_none());
    }

    #[test]
    fn test_load_and_validate_project_success() {
        let workspace = TempDir::new().unwrap();
        let project_dir = TempDir::new().unwrap();

        // Create project files
        fs::write(project_dir.path().join("brief.md"), "Project brief").unwrap();

        let result = load_and_validate_project(
            project_dir.path().to_str().unwrap(),
            workspace.path(),
        );

        assert!(result.is_ok());
        let project = result.unwrap();
        assert!(project.loaded_files.contains(&"brief.md".to_string()));
    }

    #[test]
    fn test_load_and_validate_project_relative_path_error() {
        let workspace = TempDir::new().unwrap();

        let result = load_and_validate_project("relative/path", workspace.path());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("must be absolute"));
    }

    #[test]
    fn test_load_and_validate_project_nonexistent_path_error() {
        let workspace = TempDir::new().unwrap();

        let result = load_and_validate_project("/nonexistent/path/12345", workspace.path());

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn test_load_and_validate_project_no_files_error() {
        let workspace = TempDir::new().unwrap();
        let project_dir = TempDir::new().unwrap();

        // No project files created
        let result = load_and_validate_project(
            project_dir.path().to_str().unwrap(),
            workspace.path(),
        );

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("No project files found"));
    }
}
