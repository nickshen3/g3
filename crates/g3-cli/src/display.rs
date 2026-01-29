//! Display utilities for G3 CLI.
//!
//! Provides shared display functions used by both interactive mode and agent mode.

use crossterm::style::{Color, ResetColor, SetForegroundColor};
use std::path::Path;

/// Format a workspace path for display, replacing home directory with ~.
pub fn format_workspace_path(workspace_path: &Path) -> String {
    let path_str = workspace_path.display().to_string();
    dirs::home_dir()
        .and_then(|home| {
            path_str
                .strip_prefix(&home.display().to_string())
                .map(|s| format!("~{}", s))
        })
        .unwrap_or(path_str)
}

/// Shorten a path string for display by:
/// 1. Replacing project directory prefix with `<project_name>/` (if project is active)
/// 2. Replacing workspace directory prefix with `./`
/// 3. Replacing home directory prefix with `~`
///
/// This is useful for tool output where paths should be concise.
/// The project check happens first (most specific), then workspace, then home.
pub fn shorten_path(path: &str, workspace_path: Option<&std::path::Path>, project: Option<(&std::path::Path, &str)>) -> String {
    // First, try to make it relative to project (most specific)
    if let Some((project_path, project_name)) = project {
        let project_str = project_path.display().to_string();
        if let Some(relative) = path.strip_prefix(&project_str) {
            // Handle both "/subpath" and "" (exact match) cases
            if relative.is_empty() {
                return format!("{}/", project_name);
            } else if let Some(stripped) = relative.strip_prefix('/') {
                return format!("{}/{}", project_name, stripped);
            }
        }
    }

    // First, try to make it relative to workspace
    if let Some(workspace) = workspace_path {
        let workspace_str = workspace.display().to_string();
        if let Some(relative) = path.strip_prefix(&workspace_str) {
            // Handle both "/subpath" and "" (exact match) cases
            if relative.is_empty() {
                return "./".to_string();
            } else if let Some(stripped) = relative.strip_prefix('/') {
                return format!("./{}", stripped);
            }
        }
    }

    // Fall back to replacing home directory with ~
    if let Some(home) = dirs::home_dir() {
        let home_str = home.display().to_string();
        if let Some(relative) = path.strip_prefix(&home_str) {
            return format!("~{}", relative);
        }
    }

    path.to_string()
}

/// Shorten any paths found within a shell command string.
/// This replaces project paths with `<project_name>/`, workspace paths with `./`, and home paths with `~`.
pub fn shorten_paths_in_command(command: &str, workspace_path: Option<&std::path::Path>, project: Option<(&std::path::Path, &str)>) -> String {
    let mut result = command.to_string();

    // First, replace project paths (most specific)
    if let Some((project_path, project_name)) = project {
        let project_str = project_path.display().to_string();
        // Replace project path followed by / with project_name/
        result = result.replace(&format!("{}/", project_str), &format!("{}/", project_name));
        // Replace exact project path
        result = result.replace(&project_str, project_name);
    }

    // Then, replace workspace paths
    if let Some(workspace) = workspace_path {
        let workspace_str = workspace.display().to_string();
        // Replace workspace path followed by / with ./
        result = result.replace(&format!("{}/", workspace_str), "./");
        // Replace exact workspace path at word boundary
        result = result.replace(&workspace_str, ".");
    }

    // Then replace home directory paths
    if let Some(home) = dirs::home_dir() {
        let home_str = home.display().to_string();
        result = result.replace(&home_str, "~");
    }

    result
}

/// Print the workspace path in a consistent format.
pub fn print_workspace_path(workspace_path: &Path) {
    let display = format_workspace_path(workspace_path);
    print!(
        "{}-> {}{}",
        SetForegroundColor(Color::DarkGrey),
        display,
        ResetColor
    );
    println!();
}

/// Information about what project files were loaded.
#[derive(Default)]
pub struct LoadedContent {
    pub has_agents: bool,
    pub has_memory: bool,
    pub include_prompt_filename: Option<String>,
}

impl LoadedContent {
    /// Create from explicit boolean flags.
    pub fn new(has_agents: bool, has_memory: bool, include_prompt_filename: Option<String>) -> Self {
        Self {
            has_agents,
            has_memory,
            include_prompt_filename,
        }
    }

    /// Create from combined content string by detecting markers.
    pub fn from_combined_content(content: &str) -> Self {
        Self {
            has_agents: content.contains("Agent Configuration"),
            has_memory: content.contains("=== Workspace Memory"),
            include_prompt_filename: if content.contains("Included Prompt") {
                Some("prompt".to_string()) // Default name when we can't determine the actual filename
            } else {
                None
            },
        }
    }

    /// Create with explicit include prompt filename.
    #[allow(dead_code)] // Used in tests, may be useful for future callers
    pub fn with_include_prompt_filename(mut self, filename: Option<String>) -> Self {
        if self.include_prompt_filename.is_some() {
            self.include_prompt_filename = filename;
        }
        self
    }

    /// Check if any content was loaded.
    pub fn has_any(&self) -> bool {
        self.has_agents || self.has_memory || self.include_prompt_filename.is_some()
    }

    /// Build a list of loaded item names in load order.
    pub fn to_loaded_items(&self) -> Vec<String> {
        let mut items = Vec::new();
        if self.has_agents {
            items.push("AGENTS.md".to_string());
        }
        if let Some(ref filename) = self.include_prompt_filename {
            items.push(filename.clone());
        }
        if self.has_memory {
            items.push("Memory".to_string());
        }
        items
    }
}

/// Print a status line showing what project files were loaded.
/// Format: "   ✓ README  ✓ AGENTS.md  ✓ Memory"
pub fn print_loaded_status(loaded: &LoadedContent) {
    if !loaded.has_any() {
        return;
    }

    let items = loaded.to_loaded_items();
    let status_str = items
        .iter()
        .map(|s| format!("✓ {}", s))
        .collect::<Vec<_>>()
        .join("  ");

    print!(
        "{}   {}{}",
        SetForegroundColor(Color::DarkGrey),
        status_str,
        ResetColor
    );
    println!();
}

/// Print the project name/heading from README content.
pub fn print_project_heading(heading: &str) {
    print!(
        "{}>> {}{}",
        SetForegroundColor(Color::DarkGrey),
        heading,
        ResetColor
    );
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_format_workspace_path_with_home() {
        // This test depends on having a home directory
        if let Some(home) = dirs::home_dir() {
            let test_path = home.join("projects").join("myapp");
            let formatted = format_workspace_path(&test_path);
            assert!(formatted.starts_with("~/"), "Expected ~/ prefix, got: {}", formatted);
            assert!(formatted.contains("projects/myapp"));
        }
    }

    #[test]
    fn test_format_workspace_path_without_home() {
        let test_path = PathBuf::from("/tmp/workspace");
        let formatted = format_workspace_path(&test_path);
        assert_eq!(formatted, "/tmp/workspace");
    }

    #[test]
    fn test_loaded_content_from_combined() {
        let content = "Agent Configuration\n=== Workspace Memory";
        let loaded = LoadedContent::from_combined_content(content);
        assert!(loaded.has_agents);
        assert!(loaded.has_memory);
        assert!(loaded.include_prompt_filename.is_none());
    }

    #[test]
    fn test_loaded_content_with_include_prompt() {
        let content = "Agent Configuration\nIncluded Prompt";
        let loaded = LoadedContent::from_combined_content(content)
            .with_include_prompt_filename(Some("custom.md".to_string()));
        assert!(loaded.has_agents);
        assert_eq!(loaded.include_prompt_filename, Some("custom.md".to_string()));
    }

    #[test]
    fn test_loaded_content_to_items_order() {
        let loaded = LoadedContent {
            has_agents: true,
            has_memory: true,
            include_prompt_filename: Some("prompt.md".to_string()),
        };
        let items = loaded.to_loaded_items();
        assert_eq!(items, vec!["AGENTS.md", "prompt.md", "Memory"]);
    }

    #[test]
    fn test_loaded_content_has_any() {
        let empty = LoadedContent::default();
        assert!(!empty.has_any());

        let with_agents = LoadedContent {
            has_agents: true,
            ..Default::default()
        };
        assert!(with_agents.has_any());
    }

    #[test]
    fn test_shorten_path_workspace_relative() {
        let workspace = PathBuf::from("/Users/test/projects/myapp");
        let path = "/Users/test/projects/myapp/src/main.rs";
        let shortened = shorten_path(path, Some(&workspace), None);
        assert_eq!(shortened, "./src/main.rs");
    }

    #[test]
    fn test_shorten_path_workspace_exact() {
        let workspace = PathBuf::from("/Users/test/projects/myapp");
        let path = "/Users/test/projects/myapp";
        let shortened = shorten_path(path, Some(&workspace), None);
        assert_eq!(shortened, "./");
    }

    #[test]
    fn test_shorten_path_home_relative() {
        // This test depends on having a home directory
        if let Some(home) = dirs::home_dir() {
            let path = format!("{}/other/project/file.rs", home.display());
            let shortened = shorten_path(&path, None, None);
            assert_eq!(shortened, "~/other/project/file.rs");
        }
    }

    #[test]
    fn test_shorten_path_no_match() {
        let workspace = PathBuf::from("/Users/test/projects/myapp");
        let path = "/tmp/other/file.rs";
        let shortened = shorten_path(path, Some(&workspace), None);
        assert_eq!(shortened, "/tmp/other/file.rs");
    }

    #[test]
    fn test_shorten_path_project_relative() {
        let workspace = PathBuf::from("/Users/test/projects");
        let project_path = PathBuf::from("/Users/test/projects/appa_estate");
        let path = "/Users/test/projects/appa_estate/status.md";
        let shortened = shorten_path(path, Some(&workspace), Some((&project_path, "appa_estate")));
        assert_eq!(shortened, "appa_estate/status.md");
    }

    #[test]
    fn test_shorten_path_project_takes_priority() {
        // Project path is under workspace, but project shortening should take priority
        let workspace = PathBuf::from("/Users/test/projects");
        let project_path = PathBuf::from("/Users/test/projects/appa_estate");
        let path = "/Users/test/projects/appa_estate/src/main.rs";
        let shortened = shorten_path(path, Some(&workspace), Some((&project_path, "appa_estate")));
        assert_eq!(shortened, "appa_estate/src/main.rs");
    }

    #[test]
    fn test_shorten_paths_in_command_workspace() {
        let workspace = PathBuf::from("/Users/test/projects/myapp");
        let command = "cat /Users/test/projects/myapp/src/main.rs";
        let shortened = shorten_paths_in_command(command, Some(&workspace), None);
        assert_eq!(shortened, "cat ./src/main.rs");
    }

    #[test]
    fn test_shorten_paths_in_command_home() {
        if let Some(home) = dirs::home_dir() {
            let command = format!("ls {}/Documents", home.display());
            let shortened = shorten_paths_in_command(&command, None, None);
            assert_eq!(shortened, "ls ~/Documents");
        }
    }

    #[test]
    fn test_shorten_paths_in_command_multiple() {
        let workspace = PathBuf::from("/Users/test/projects/myapp");
        let command = "diff /Users/test/projects/myapp/a.rs /Users/test/projects/myapp/b.rs";
        let shortened = shorten_paths_in_command(command, Some(&workspace), None);
        assert_eq!(shortened, "diff ./a.rs ./b.rs");
    }

    #[test]
    fn test_shorten_paths_in_command_project() {
        let workspace = PathBuf::from("/Users/test/projects");
        let project_path = PathBuf::from("/Users/test/projects/appa_estate");
        let command = "cat /Users/test/projects/appa_estate/status.md";
        let shortened = shorten_paths_in_command(command, Some(&workspace), Some((&project_path, "appa_estate")));
        assert_eq!(shortened, "cat appa_estate/status.md");
    }
}
