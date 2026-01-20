//! Embedded agent prompts - compiled into the binary for portability.
//!
//! Agent prompts are embedded at compile time using `include_str!`.
//! This allows g3 to run on any repository without needing the agents/ directory.
//!
//! Priority order for loading agent prompts:
//! 1. Workspace `agents/<name>.md` (allows per-project customization)
//! 2. Embedded prompts (fallback, always available)

use std::collections::HashMap;
use std::path::Path;

use crate::template::process_template;

/// Embedded agent prompts, keyed by agent name.
static EMBEDDED_AGENTS: &[(&str, &str)] = &[
    ("breaker", include_str!("../../../agents/breaker.md")),
    ("carmack", include_str!("../../../agents/carmack.md")),
    ("euler", include_str!("../../../agents/euler.md")),
    ("fowler", include_str!("../../../agents/fowler.md")),
    ("hopper", include_str!("../../../agents/hopper.md")),
    ("lamport", include_str!("../../../agents/lamport.md")),
    ("scout", include_str!("../../../agents/scout.md")),
];

/// Get an embedded agent prompt by name.
pub fn get_embedded_agent(name: &str) -> Option<&'static str> {
    EMBEDDED_AGENTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, content)| *content)
}

/// Get all available embedded agent names.
pub fn list_embedded_agents() -> Vec<&'static str> {
    EMBEDDED_AGENTS.iter().map(|(name, _)| *name).collect()
}

/// Load an agent prompt, checking workspace first, then falling back to embedded.
///
/// Returns the prompt content and a boolean indicating if it was loaded from disk (true)
/// or embedded (false).
pub fn load_agent_prompt(name: &str, workspace_dir: &Path) -> Option<(String, bool)> {
    // First, try workspace agents/<name>.md
    let workspace_path = workspace_dir.join("agents").join(format!("{}.md", name));
    if workspace_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&workspace_path) {
            let processed = process_template(&content);
            return Some((processed, true));
        }
    }

    // Fall back to embedded prompt
    get_embedded_agent(name).map(|content| (process_template(content), false))
}

/// Get a map of all available agents (both embedded and from workspace).
pub fn get_available_agents(workspace_dir: &Path) -> HashMap<String, bool> {
    let mut agents = HashMap::new();

    // Add all embedded agents
    for name in list_embedded_agents() {
        agents.insert(name.to_string(), false); // false = embedded
    }

    // Check for workspace agents (these override embedded)
    let agents_dir = workspace_dir.join("agents");
    if agents_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&agents_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "md") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        agents.insert(stem.to_string(), true); // true = from disk
                    }
                }
            }
        }
    }

    agents
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_agents_exist() {
        // Verify all expected agents are embedded
        let expected = ["breaker", "carmack", "euler", "fowler", "hopper", "lamport", "scout"];
        for name in expected {
            assert!(
                get_embedded_agent(name).is_some(),
                "Agent '{}' should be embedded",
                name
            );
        }
    }

    #[test]
    fn test_list_embedded_agents() {
        let agents = list_embedded_agents();
        assert!(agents.len() >= 7, "Should have at least 7 embedded agents");
        assert!(agents.contains(&"carmack"));
        assert!(agents.contains(&"hopper"));
    }

    #[test]
    fn test_embedded_agent_content() {
        // Verify the content looks reasonable
        let carmack = get_embedded_agent("carmack").unwrap();
        assert!(carmack.contains("Carmack"), "Carmack prompt should mention Carmack");
        
        let hopper = get_embedded_agent("hopper").unwrap();
        assert!(hopper.contains("Hopper"), "Hopper prompt should mention Hopper");
    }
}
