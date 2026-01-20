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
    pub has_readme: bool,
    pub has_agents: bool,
    pub has_memory: bool,
    pub include_prompt_filename: Option<String>,
}

impl LoadedContent {
    /// Create from explicit boolean flags.
    pub fn new(has_readme: bool, has_agents: bool, has_memory: bool, include_prompt_filename: Option<String>) -> Self {
        Self {
            has_readme,
            has_agents,
            has_memory,
            include_prompt_filename,
        }
    }

    /// Create from combined content string by detecting markers.
    pub fn from_combined_content(content: &str) -> Self {
        Self {
            has_readme: content.contains("Project README"),
            has_agents: content.contains("Agent Configuration"),
            has_memory: content.contains("=== Project Memory"),
            include_prompt_filename: if content.contains("Included Prompt") {
                Some("prompt".to_string()) // Default name when we can't determine the actual filename
            } else {
                None
            },
        }
    }

    /// Create with explicit include prompt filename.
    pub fn with_include_prompt_filename(mut self, filename: Option<String>) -> Self {
        if self.include_prompt_filename.is_some() {
            self.include_prompt_filename = filename;
        }
        self
    }

    /// Check if any content was loaded.
    pub fn has_any(&self) -> bool {
        self.has_readme || self.has_agents || self.has_memory || self.include_prompt_filename.is_some()
    }

    /// Build a list of loaded item names in load order.
    pub fn to_loaded_items(&self) -> Vec<String> {
        let mut items = Vec::new();
        if self.has_readme {
            items.push("README".to_string());
        }
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
        let content = "Project README\nAgent Configuration\n=== Project Memory";
        let loaded = LoadedContent::from_combined_content(content);
        assert!(loaded.has_readme);
        assert!(loaded.has_agents);
        assert!(loaded.has_memory);
        assert!(loaded.include_prompt_filename.is_none());
    }

    #[test]
    fn test_loaded_content_with_include_prompt() {
        let content = "Project README\nIncluded Prompt";
        let loaded = LoadedContent::from_combined_content(content)
            .with_include_prompt_filename(Some("custom.md".to_string()));
        assert!(loaded.has_readme);
        assert_eq!(loaded.include_prompt_filename, Some("custom.md".to_string()));
    }

    #[test]
    fn test_loaded_content_to_items_order() {
        let loaded = LoadedContent {
            has_readme: true,
            has_agents: true,
            has_memory: true,
            include_prompt_filename: Some("prompt.md".to_string()),
        };
        let items = loaded.to_loaded_items();
        assert_eq!(items, vec!["README", "AGENTS.md", "prompt.md", "Memory"]);
    }

    #[test]
    fn test_loaded_content_has_any() {
        let empty = LoadedContent::default();
        assert!(!empty.has_any());

        let with_readme = LoadedContent {
            has_readme: true,
            ..Default::default()
        };
        assert!(with_readme.has_any());
    }
}
