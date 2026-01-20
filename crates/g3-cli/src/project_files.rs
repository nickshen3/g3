//! Project file reading utilities.
//!
//! Reads AGENTS.md, README.md, and project memory files from the workspace.

use std::path::Path;
use tracing::error;

use crate::template::process_template;

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

/// Read project memory from analysis/memory.md in the workspace directory.
/// Returns formatted content with emoji prefix and size info, or None if not found.
pub fn read_project_memory(workspace_dir: &Path) -> Option<String> {
    let memory_path = workspace_dir.join("analysis").join("memory.md");

    if !memory_path.exists() {
        return None;
    }

    match std::fs::read_to_string(&memory_path) {
        Ok(content) => {
            let size = format_size(content.len());
            Some(format!(
                "=== Project Memory (read from analysis/memory.md, {}) ===\n{}\n=== End Project Memory ===",
                size,
                content
            ))
        }
        Err(_) => None,
    }
}

/// Read include prompt content from a specified file path.
/// Returns formatted content with emoji prefix, or None if path is None or file doesn't exist.
pub fn read_include_prompt(path: Option<&std::path::Path>) -> Option<String> {
    let path = path?;
    
    if !path.exists() {
        tracing::error!("Include prompt file not found: {}", path.display());
        return None;
    }

    match std::fs::read_to_string(path) {
        Ok(content) => {
            let processed = process_template(&content);
            Some(format!("📎 Included Prompt (from {}):\n{}", path.display(), processed))
        }
        Err(e) => {
            tracing::error!("Failed to read include prompt file {}: {}", path.display(), e);
            None
        }
    }
}

/// Combine AGENTS.md, README, and memory content into a single string.
///
/// Returns None if all inputs are None, otherwise joins non-None parts with double newlines.
/// Prepends the current working directory to help the LLM avoid path hallucinations.
/// 
/// Order: Working Directory → AGENTS.md → README → Language prompts → Include prompt → Memory
pub fn combine_project_content(
    agents_content: Option<String>,
    readme_content: Option<String>,
    memory_content: Option<String>,
    language_content: Option<String>,
    include_prompt: Option<String>,
    workspace_dir: &Path,
) -> Option<String> {
    // Always include working directory to prevent LLM from hallucinating paths
    let cwd_info = format!("📂 Working Directory: {}", workspace_dir.display());
    
    // Order: cwd → agents → readme → language → include_prompt → memory
    // Include prompt comes BEFORE memory so memory is always last (most recent context)
    let parts: Vec<String> = [
        Some(cwd_info), agents_content, readme_content, language_content, include_prompt, memory_content
    ]
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
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        // Truncate at character boundary, not byte boundary
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
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
    fn test_truncate_for_display_utf8() {
        // Multi-byte characters should not cause panics
        let emoji_text = "Hello 👋 World 🌍 Test ✨ More text here and more";
        let truncated = truncate_for_display(emoji_text, 15);
        assert!(truncated.ends_with("..."));
        assert!(truncated.chars().count() <= 15);
    }

    #[test]
    fn test_combine_project_content_all_some() {
        let workspace = std::path::PathBuf::from("/test/workspace");
        let result = combine_project_content(
            Some("agents".to_string()),
            Some("readme".to_string()),
            Some("memory".to_string()),
            Some("language".to_string()),
            None, // include_prompt
            &workspace,
        );
        assert!(result.is_some());
        let content = result.unwrap();
        assert!(content.contains("📂 Working Directory: /test/workspace"));
        assert!(content.contains("agents"));
        assert!(content.contains("readme"));
        assert!(content.contains("memory"));
        assert!(content.contains("language"));
    }

    #[test]
    fn test_combine_project_content_partial() {
        let workspace = std::path::PathBuf::from("/test/workspace");
        let result = combine_project_content(None, Some("readme".to_string()), None, None, None, &workspace);
        assert!(result.is_some());
        let content = result.unwrap();
        assert!(content.contains("📂 Working Directory: /test/workspace"));
        assert!(content.contains("readme"));
    }

    #[test]
    fn test_combine_project_content_all_none() {
        let workspace = std::path::PathBuf::from("/test/workspace");
        let result = combine_project_content(None, None, None, None, None, &workspace);
        // Now always returns Some because we always include the working directory
        assert!(result.is_some());
        assert!(result.unwrap().contains("📂 Working Directory: /test/workspace"));
    }

    #[test]
    fn test_combine_project_content_with_include_prompt() {
        let workspace = std::path::PathBuf::from("/test/workspace");
        let result = combine_project_content(
            Some("agents".to_string()),
            Some("readme".to_string()),
            Some("memory".to_string()),
            Some("language".to_string()),
            Some("include_prompt".to_string()),
            &workspace,
        );
        assert!(result.is_some());
        let content = result.unwrap();
        assert!(content.contains("include_prompt"));
    }

    #[test]
    fn test_combine_project_content_order_include_before_memory() {
        // Verify that include_prompt appears BEFORE memory in the combined content
        let workspace = std::path::PathBuf::from("/test/workspace");
        let result = combine_project_content(
            Some("AGENTS_CONTENT".to_string()),
            Some("README_CONTENT".to_string()),
            Some("MEMORY_CONTENT".to_string()),
            Some("LANGUAGE_CONTENT".to_string()),
            Some("INCLUDE_PROMPT_CONTENT".to_string()),
            &workspace,
        );
        let content = result.unwrap();
        
        // Find positions of each section
        let agents_pos = content.find("AGENTS_CONTENT").expect("agents not found");
        let readme_pos = content.find("README_CONTENT").expect("readme not found");
        let language_pos = content.find("LANGUAGE_CONTENT").expect("language not found");
        let include_pos = content.find("INCLUDE_PROMPT_CONTENT").expect("include_prompt not found");
        let memory_pos = content.find("MEMORY_CONTENT").expect("memory not found");
        
        // Verify order: agents < readme < language < include_prompt < memory
        assert!(agents_pos < readme_pos, "agents should come before readme");
        assert!(readme_pos < language_pos, "readme should come before language");
        assert!(language_pos < include_pos, "language should come before include_prompt");
        assert!(include_pos < memory_pos, "include_prompt should come before memory");
    }

    #[test]
    fn test_combine_project_content_order_memory_last() {
        // Verify memory is always last even when include_prompt is None
        let workspace = std::path::PathBuf::from("/test/workspace");
        let result = combine_project_content(
            Some("AGENTS".to_string()),
            Some("README".to_string()),
            Some("MEMORY".to_string()),
            Some("LANGUAGE".to_string()),
            None, // no include_prompt
            &workspace,
        );
        let content = result.unwrap();
        
        // Memory should still be last
        let language_pos = content.find("LANGUAGE").expect("language not found");
        let memory_pos = content.find("MEMORY").expect("memory not found");
        assert!(language_pos < memory_pos, "memory should come after language");
    }

    #[test]
    fn test_read_include_prompt_none_path() {
        // None path should return None
        let result = read_include_prompt(None);
        assert!(result.is_none());
    }

    #[test]
    fn test_read_include_prompt_nonexistent_file() {
        // Non-existent file should return None
        let path = std::path::Path::new("/nonexistent/path/to/file.md");
        let result = read_include_prompt(Some(path));
        assert!(result.is_none());
    }

    #[test]
    fn test_read_include_prompt_valid_file() {
        // Create a temp file and read it
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_include_prompt.md");
        std::fs::write(&temp_file, "Test prompt content").unwrap();
        
        let result = read_include_prompt(Some(&temp_file));
        assert!(result.is_some());
        let content = result.unwrap();
        assert!(content.contains("📎 Included Prompt"));
        assert!(content.contains("Test prompt content"));
        
        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }

    #[test]
    fn test_read_include_prompt_with_template_variables() {
        // Create a temp file with template variables
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join("test_include_prompt_template.md");
        std::fs::write(&temp_file, "Today is {{today}} and {{unknown}} stays").unwrap();
        
        let result = read_include_prompt(Some(&temp_file));
        assert!(result.is_some());
        let content = result.unwrap();
        
        // {{today}} should be replaced with a date, {{unknown}} should remain
        assert!(!content.contains("{{today}}"));
        assert!(content.contains("{{unknown}}"));
        
        // Cleanup
        let _ = std::fs::remove_file(&temp_file);
    }
}
