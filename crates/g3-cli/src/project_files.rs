//! Project file reading utilities.
//!
//! Reads AGENTS.md, README.md, and project memory files from the workspace.

use std::path::Path;
use tracing::error;

/// Read AGENTS.md configuration from the workspace directory.
/// Returns formatted content with emoji prefix, or None if not found.
pub fn read_agents_config(workspace_dir: &Path) -> Option<String> {
    // Try AGENTS.md first, then agents.md
    let paths = [
        (workspace_dir.join("AGENTS.md"), "AGENTS.md"),
        (workspace_dir.join("agents.md"), "agents.md"),
    ];

    for (path, name) in &paths {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    return Some(format!("🤖 Agent Configuration (from {}):{}\n{}", name, "\n", content));
                }
                Err(e) => {
                    error!("Failed to read {}: {}", name, e);
                }
            }
        }
    }
    None
}

/// Read README from the workspace directory if it's a project directory.
/// Returns formatted content with emoji prefix, or None if not found.
pub fn read_project_readme(workspace_dir: &Path) -> Option<String> {
    // Only read README if we're in a project directory
    let is_project_dir = workspace_dir.join(".g3").exists() || workspace_dir.join(".git").exists();
    if !is_project_dir {
        return None;
    }

    const README_NAMES: &[&str] = &[
        "README.md",
        "README.MD",
        "readme.md",
        "Readme.md",
        "README",
        "README.txt",
        "README.rst",
    ];

    for name in README_NAMES {
        let path = workspace_dir.join(name);
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    return Some(format!("📚 Project README (from {}):{}\n{}", name, "\n", content));
                }
                Err(e) => {
                    error!("Failed to read {}: {}", path.display(), e);
                }
            }
        }
    }
    None
}

/// Read project memory from .g3/memory.md in the workspace directory.
/// Returns formatted content with emoji prefix and size info, or None if not found.
pub fn read_project_memory(workspace_dir: &Path) -> Option<String> {
    let memory_path = workspace_dir.join(".g3").join("memory.md");

    if !memory_path.exists() {
        return None;
    }

    match std::fs::read_to_string(&memory_path) {
        Ok(content) => {
            let size = format_size(content.len());
            Some(format!("🧠 Project Memory ({}):{}\n{}", size, "\n", content))
        }
        Err(_) => None,
    }
}

/// Combine AGENTS.md, README, and memory content into a single string.
///
/// Returns None if all inputs are None, otherwise joins non-None parts with double newlines.
pub fn combine_project_content(
    agents_content: Option<String>,
    readme_content: Option<String>,
    memory_content: Option<String>,
) -> Option<String> {
    let parts: Vec<String> = [agents_content, readme_content, memory_content]
        .into_iter()
        .flatten()
        .collect();

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Format a byte size for display.
fn format_size(len: usize) -> String {
    if len < 1000 {
        format!("{} chars", len)
    } else {
        format!("{:.1}k chars", len as f64 / 1000.0)
    }
}

/// Extract the first H1 heading from README content for display.
pub fn extract_readme_heading(readme_content: &str) -> Option<String> {
    // Find where the actual README content starts (after any prefix markers)
    let readme_start = readme_content.find("📚 Project README (from");

    let content_to_search = match readme_start {
        Some(pos) => &readme_content[pos..],
        None => readme_content,
    };

    // Skip the prefix line and collect content
    let content: String = content_to_search
        .lines()
        .filter(|line| !line.starts_with("📚 Project README"))
        .collect::<Vec<_>>()
        .join("\n");

    // Look for H1 heading
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix("# ") {
            let title = stripped.trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }

    // Fallback: first non-empty, non-metadata line
    find_fallback_title(&content)
}

/// Find a fallback title from the first few lines of content.
fn find_fallback_title(content: &str) -> Option<String> {
    for line in content.lines().take(5) {
        let trimmed = line.trim();
        if !trimmed.is_empty()
            && !trimmed.starts_with("📚")
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("==")
            && !trimmed.starts_with("--")
        {
            return Some(truncate_for_display(trimmed, 100));
        }
    }
    None
}

/// Truncate a string for display, adding ellipsis if needed.
fn truncate_for_display(s: &str, max_len: usize) -> String {
    if s.len() > max_len {
        format!("{}...", &s[..max_len - 3])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_readme_heading() {
        let content = "# My Project\n\nSome description";
        assert_eq!(extract_readme_heading(content), Some("My Project".to_string()));
    }

    #[test]
    fn test_extract_readme_heading_with_prefix() {
        let content = "📚 Project README (from README.md):\n# Cool App\n\nDescription";
        assert_eq!(extract_readme_heading(content), Some("Cool App".to_string()));
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(500), "500 chars");
        assert_eq!(format_size(1500), "1.5k chars");
    }

    #[test]
    fn test_truncate_for_display() {
        assert_eq!(truncate_for_display("short", 100), "short");
        let long = "a".repeat(150);
        let truncated = truncate_for_display(&long, 100);
        assert!(truncated.ends_with("..."));
        assert_eq!(truncated.len(), 100);
    }

    #[test]
    fn test_combine_project_content_all_some() {
        let result = combine_project_content(
            Some("agents".to_string()),
            Some("readme".to_string()),
            Some("memory".to_string()),
        );
        assert_eq!(result, Some("agents\n\nreadme\n\nmemory".to_string()));
    }

    #[test]
    fn test_combine_project_content_partial() {
        let result = combine_project_content(None, Some("readme".to_string()), None);
        assert_eq!(result, Some("readme".to_string()));
    }

    #[test]
    fn test_combine_project_content_all_none() {
        let result = combine_project_content(None, None, None);
        assert_eq!(result, None);
    }
}
