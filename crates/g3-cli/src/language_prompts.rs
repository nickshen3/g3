//! Language-specific prompt injection.
//!
//! Detects programming languages in the workspace and injects relevant
//! toolchain guidance into the system prompt.
//!
//! Language prompts are embedded at compile time from `prompts/langs/*.md`.

use std::path::Path;

/// Embedded language prompts, keyed by language name.
/// The key should match common file extensions or language identifiers.
static LANGUAGE_PROMPTS: &[(&str, &[&str], &str)] = &[
    // (language_name, file_extensions, prompt_content)
    (
        "racket",
        &[".rkt", ".rktl", ".rktd", ".scrbl"],
        include_str!("../../../prompts/langs/racket.md"),
    ),
];

/// Embedded agent-specific language prompts.
/// Format: (agent_name, language_name, prompt_content)
static AGENT_LANGUAGE_PROMPTS: &[(&str, &str, &str)] = &[
    // (agent_name, language_name, prompt_content)
    ("carmack", "racket", include_str!("../../../prompts/langs/carmack.racket.md")),
];

/// Detect languages present in the workspace by scanning for file extensions.
/// Returns a list of detected language names.
pub fn detect_languages(workspace_dir: &Path) -> Vec<&'static str> {
    let mut detected = Vec::new();

    for (lang_name, extensions, _) in LANGUAGE_PROMPTS {
        if has_files_with_extensions(workspace_dir, extensions) {
            detected.push(*lang_name);
        }
    }

    detected
}

/// Check if the workspace contains files with any of the given extensions.
/// Scans up to a reasonable depth to avoid slow startup on large repos.
fn has_files_with_extensions(workspace_dir: &Path, extensions: &[&str]) -> bool {
    // Quick check: scan top-level and one level deep
    // This avoids slow startup on large repos while catching most projects
    scan_directory_for_extensions(workspace_dir, extensions, 2)
}

/// Recursively scan a directory for files with given extensions, up to max_depth.
fn scan_directory_for_extensions(dir: &Path, extensions: &[&str], max_depth: usize) -> bool {
    if max_depth == 0 {
        return false;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        
        // Skip hidden directories and common non-source directories
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "vendor" {
                continue;
            }
        }

        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                for ext in extensions {
                    if name.ends_with(ext) {
                        return true;
                    }
                }
            }
        } else if path.is_dir() {
            if scan_directory_for_extensions(&path, extensions, max_depth - 1) {
                return true;
            }
        }
    }

    false
}

/// Get the prompt content for a specific language.
pub fn get_language_prompt(lang: &str) -> Option<&'static str> {
    LANGUAGE_PROMPTS
        .iter()
        .find(|(name, _, _)| *name == lang)
        .map(|(_, _, content)| *content)
}

/// Get all language prompts for detected languages in the workspace.
/// Returns formatted content ready for injection into the system prompt.
pub fn get_language_prompts_for_workspace(workspace_dir: &Path) -> Option<String> {
    let detected = detect_languages(workspace_dir);
    
    if detected.is_empty() {
        return None;
    }

    let mut prompts = Vec::new();
    for lang in detected {
        if let Some(content) = get_language_prompt(lang) {
            prompts.push(content);
        }
    }

    if prompts.is_empty() {
        return None;
    }

    Some(format!(
        "🔧 Language-Specific Guidance:\n\n{}",
        prompts.join("\n\n---\n\n")
    ))
}

/// List all available language prompts.
pub fn list_available_languages() -> Vec<&'static str> {
    LANGUAGE_PROMPTS.iter().map(|(name, _, _)| *name).collect()
}

/// Get agent-specific language prompt for a specific agent and language.
pub fn get_agent_language_prompt(agent_name: &str, lang: &str) -> Option<&'static str> {
    AGENT_LANGUAGE_PROMPTS
        .iter()
        .find(|(agent, language, _)| *agent == agent_name && *language == lang)
        .map(|(_, _, content)| *content)
}

/// Get agent-specific language prompts for detected languages in the workspace.
/// Returns formatted content ready for injection into the agent's system prompt.
#[allow(dead_code)]
pub fn get_agent_language_prompts_for_workspace(
    workspace_dir: &Path,
    agent_name: &str,
) -> Option<String> {
    let (content, _) = get_agent_language_prompts_for_workspace_with_langs(workspace_dir, agent_name);
    content
}

/// Get agent-specific language prompts for detected languages in the workspace.
/// Returns both the formatted content and the list of languages that had matching prompts.
pub fn get_agent_language_prompts_for_workspace_with_langs(
    workspace_dir: &Path,
    agent_name: &str,
) -> (Option<String>, Vec<&'static str>) {
    let detected = detect_languages(workspace_dir);
    let mut prompts = Vec::new();
    let mut matched_langs = Vec::new();

    for lang in detected {
        if let Some(content) = get_agent_language_prompt(agent_name, lang) {
            prompts.push(content.to_string());
            matched_langs.push(lang);
        }
    }

    let content = if prompts.is_empty() { None } else { Some(prompts.join("\n\n---\n\n")) };
    (content, matched_langs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_racket_prompt_embedded() {
        let prompt = get_language_prompt("racket");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("raco"));
    }

    #[test]
    fn test_list_available_languages() {
        let langs = list_available_languages();
        assert!(langs.contains(&"racket"));
    }

    #[test]
    fn test_detect_racket_files() {
        let temp_dir = TempDir::new().unwrap();
        let rkt_file = temp_dir.path().join("main.rkt");
        fs::write(&rkt_file, "#lang racket\n").unwrap();

        let detected = detect_languages(temp_dir.path());
        assert!(detected.contains(&"racket"));
    }

    #[test]
    fn test_no_detection_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let detected = detect_languages(temp_dir.path());
        assert!(detected.is_empty());
    }

    #[test]
    fn test_get_prompts_for_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let rkt_file = temp_dir.path().join("main.rkt");
        fs::write(&rkt_file, "#lang racket\n").unwrap();

        let prompts = get_language_prompts_for_workspace(temp_dir.path());
        assert!(prompts.is_some());
        let content = prompts.unwrap();
        assert!(content.contains("🔧 Language-Specific Guidance"));
        assert!(content.contains("raco"));
    }

    #[test]
    fn test_carmack_racket_prompt_embedded() {
        let prompt = get_agent_language_prompt("carmack", "racket");
        assert!(prompt.is_some());
        assert!(prompt.unwrap().contains("RACKET-SPECIFIC GUIDANCE"));
    }

    #[test]
    fn test_agent_language_prompt_not_found() {
        let prompt = get_agent_language_prompt("nonexistent", "racket");
        assert!(prompt.is_none());
    }

    #[test]
    fn test_get_agent_prompts_for_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let rkt_file = temp_dir.path().join("main.rkt");
        fs::write(&rkt_file, "#lang racket\n").unwrap();

        let prompts = get_agent_language_prompts_for_workspace(temp_dir.path(), "carmack");
        assert!(prompts.is_some());
        let content = prompts.unwrap();
        assert!(content.contains("RACKET-SPECIFIC GUIDANCE"));
    }
}
