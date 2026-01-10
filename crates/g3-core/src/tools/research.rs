//! Research tool: spawns a scout agent to perform web-based research.

use anyhow::Result;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::ui_writer::UiWriter;
use crate::ToolCall;
use g3_config::WebDriverBrowser;

use super::executor::ToolContext;

/// Delimiter markers for scout report extraction
const REPORT_START_MARKER: &str = "---SCOUT_REPORT_START---";
const REPORT_END_MARKER: &str = "---SCOUT_REPORT_END---";

/// Translate scout agent output lines into friendly progress messages.
///
/// Parses tool call headers from the scout output and returns human-readable
/// progress messages. Returns None for lines that should be suppressed.
fn translate_progress(line: &str) -> Option<String> {
    // Strip ANSI codes first for pattern matching
    let clean_line = strip_ansi_codes(line);
    let trimmed = clean_line.trim();
    
    // Tool call header pattern: "┌─ tool_name" or "┌─ tool_name | args"
    if !trimmed.starts_with("┌─") {
        return None;
    }
    
    // Extract tool name and optional args after the box drawing char
    let after_prefix = trimmed.trim_start_matches("┌─").trim();
    
    // Split on " | " to separate tool name from args
    let (tool_name, args) = if let Some(pipe_pos) = after_prefix.find(" | ") {
        let name = after_prefix[..pipe_pos].trim();
        let arg = after_prefix[pipe_pos + 3..].trim();
        (name, Some(arg))
    } else {
        (after_prefix.trim(), None)
    };
    
    // Translate tool names to friendly messages
    match tool_name {
        "webdriver_start" => Some("🌐 Launching browser...".to_string()),
        
        "webdriver_navigate" => {
            if let Some(url) = args {
                // Extract domain from URL for cleaner display
                let display_url = extract_domain(url).unwrap_or(url);
                Some(format!("🔗 Navigating to {}...", display_url))
            } else {
                Some("🔗 Navigating...".to_string())
            }
        }
        
        "webdriver_get_page_source" => {
            if let Some(arg) = args {
                // arg might be max_length or file path
                if arg.contains('/') || arg.ends_with(".html") || arg.ends_with(".md") {
                    let filename = arg.rsplit('/').next().unwrap_or(arg);
                    Some(format!("📥 Downloading {}...", filename))
                } else {
                    Some("📄 Reading page content...".to_string())
                }
            } else {
                Some("📄 Reading page content...".to_string())
            }
        }
        
        "webdriver_find_element" | "webdriver_find_elements" => {
            Some("🔍 Searching page...".to_string())
        }
        
        "webdriver_click" => Some("👆 Clicking element...".to_string()),
        
        "webdriver_quit" => Some("✅ Closing browser...".to_string()),
        
        "read_file" => {
            if let Some(path) = args {
                // Check if there's a range specified (format: "filename [start..end]")
                if let Some(bracket_pos) = path.find(" [") {
                    let filename = path[..bracket_pos].rsplit('/').next().unwrap_or(&path[..bracket_pos]);
                    let range = &path[bracket_pos + 1..]; // includes "[start..end]"
                    Some(format!("📖 Reading {} slice {}...", filename, range.trim_end_matches(']').trim_start_matches('[')))
                } else {
                    let filename = path.rsplit('/').next().unwrap_or(path);
                    Some(format!("📖 Reading {}...", filename))
                }
            } else {
                Some("📖 Reading file...".to_string())
            }
        }
        
        "write_file" => {
            if let Some(path) = args {
                let filename = path.rsplit('/').next().unwrap_or(path);
                Some(format!("💾 Writing {}...", filename))
            } else {
                Some("💾 Writing file...".to_string())
            }
        }
        
        "shell" => {
            if let Some(cmd) = args {
                // Show a truncated snippet of the command with wider display
                let snippet = truncate_command_snippet(cmd, 60);
                Some(format!(" > `{}` ...", snippet))
            } else {
                Some("⚙️ Running command...".to_string())
            }
        }
        
        // Suppress unknown tools - don't show raw output
        _ => None,
    }
}

/// Extract domain from a URL for cleaner display.
fn extract_domain(url: &str) -> Option<&str> {
    // Remove protocol
    let without_protocol = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    
    // Get just the domain (before any path)
    without_protocol.split('/').next()
}

/// Truncate a command to a maximum length for display.
/// Preserves the beginning of the command and adds "..." if truncated.
fn truncate_command_snippet(cmd: &str, max_len: usize) -> String {
    // Take just the first line if multi-line
    let first_line = cmd.lines().next().unwrap_or(cmd);
    
    if first_line.chars().count() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

pub async fn execute_research<W: UiWriter>(
    tool_call: &ToolCall,
    ctx: &mut ToolContext<'_, W>,
) -> Result<String> {
    let query = tool_call
        .args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required 'query' parameter"))?;

    // Find the g3 executable path
    let g3_path = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("g3"));

    // Build the command with appropriate webdriver flags
    let mut cmd = Command::new(&g3_path);
    cmd
        .arg("--agent")
        .arg("scout")
        .arg("--new-session")  // Always start fresh for research
        .arg("--quiet");  // Suppress log file creation

    // Propagate the webdriver browser choice from the parent g3 instance
    match ctx.config.webdriver.browser {
        WebDriverBrowser::ChromeHeadless => { cmd.arg("--chrome-headless"); }
        WebDriverBrowser::Safari => { cmd.arg("--webdriver"); }
    }

    let mut child = cmd.arg(query)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn scout agent: {}", e))?;

    // Capture stdout to find the report content
    let stdout = child.stdout.take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture scout agent stdout"))?;
    
    let mut reader = BufReader::new(stdout).lines();
    let mut all_output = Vec::new();

    // Collect all lines, showing only translated progress messages
    while let Some(line) = reader.next_line().await? {
        all_output.push(line.clone());
        
        // Show translated progress for tool calls
        if let Some(progress_msg) = translate_progress(&line) {
            // Update the status line in-place (no spinner)
            ctx.ui_writer.update_tool_output_line(&progress_msg);
        }
    }

    // Wait for the process to complete
    let status = child.wait().await
        .map_err(|e| anyhow::anyhow!("Failed to wait for scout agent: {}", e))?;

    if !status.success() {
        return Ok(format!("❌ Scout agent failed with exit code: {:?}", status.code()));
    }

    // Join all output and extract the report between markers
    let full_output = all_output.join("\n");
    
    let report = extract_report(&full_output)?;
    
    // Print the research brief to the console for scrollback reference
    // The report is printed without stripping ANSI codes to preserve formatting
    ctx.ui_writer.println("");
    ctx.ui_writer.println(&report);
    ctx.ui_writer.println("");
    
    Ok(report)
}

/// Extract the research report from scout output.
/// 
/// Looks for content between SCOUT_REPORT_START and SCOUT_REPORT_END markers.
/// Preserves ANSI escape codes in the extracted content for terminal formatting.
fn extract_report(output: &str) -> Result<String> {
    // Strip ANSI codes only for finding markers, but preserve them in the output
    let clean_output = strip_ansi_codes(output);
    
    // Find the start marker
    let start_pos = clean_output.find(REPORT_START_MARKER)
        .ok_or_else(|| anyhow::anyhow!(
            "Scout agent did not output a properly formatted report. Expected {} marker.",
            REPORT_START_MARKER
        ))?;
    
    // Find the end marker
    let end_pos = clean_output.find(REPORT_END_MARKER)
        .ok_or_else(|| anyhow::anyhow!(
            "Scout agent report is incomplete. Expected {} marker.",
            REPORT_END_MARKER
        ))?;
    
    if end_pos <= start_pos {
        return Err(anyhow::anyhow!("Invalid report format: end marker before start marker"));
    }
    
    // Now find the same markers in the original output to preserve ANSI codes
    // We need to find the marker positions accounting for ANSI codes
    let original_start = find_marker_position(output, REPORT_START_MARKER)
        .ok_or_else(|| anyhow::anyhow!("Could not find start marker in original output"))?;
    let original_end = find_marker_position(output, REPORT_END_MARKER)
        .ok_or_else(|| anyhow::anyhow!("Could not find end marker in original output"))?;
    
    // Extract content between markers from original (with ANSI codes)
    let report_start = original_start + REPORT_START_MARKER.len();
    let report_content = output[report_start..original_end].trim();
    
    if report_content.is_empty() {
        return Ok("❌ Scout agent returned an empty report.".to_string());
    }
    
    Ok(format!("📋 Research Report:\n\n{}", report_content))
}

/// Find the position of a marker in text that may contain ANSI codes.
/// Searches by stripping ANSI codes character by character to find the true position.
fn find_marker_position(text: &str, marker: &str) -> Option<usize> {
    // Simple approach: search for the marker directly first
    // The markers themselves shouldn't contain ANSI codes
    if let Some(pos) = text.find(marker) {
        return Some(pos);
    }
    
    // If not found directly, the marker might be split by ANSI codes
    // This is unlikely for our use case, but handle it gracefully
    None
}

/// Strip ANSI escape codes from a string.
/// 
/// Handles common ANSI sequences like:
/// - CSI sequences: \x1b[...m (colors, styles)
/// - OSC sequences: \x1b]...\x07 (terminal titles, etc.)
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Start of escape sequence
            match chars.peek() {
                Some('[') => {
                    // CSI sequence: \x1b[...X where X is a letter
                    chars.next(); // consume '['
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC sequence: \x1b]...\x07
                    chars.next(); // consume ']'
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '\x07' {
                            break;
                        }
                    }
                }
                _ => {
                    // Unknown escape, skip just the ESC
                }
            }
        } else {
            result.push(c);
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes() {
        // Simple color code
        assert_eq!(strip_ansi_codes("\x1b[31mred\x1b[0m"), "red");
        
        // RGB color code (like the bug we saw)
        assert_eq!(
            strip_ansi_codes("\x1b[38;2;216;177;114mtmp/file.md\x1b[0m"),
            "tmp/file.md"
        );
        
        // Multiple codes
        assert_eq!(
            strip_ansi_codes("\x1b[1m\x1b[32mbold green\x1b[0m normal"),
            "bold green normal"
        );
        
        // No codes
        assert_eq!(strip_ansi_codes("plain text"), "plain text");
        
        // Empty string
        assert_eq!(strip_ansi_codes(""), "");
    }

    #[test]
    fn test_extract_report_success() {
        let output = r#"Some preamble text
---SCOUT_REPORT_START---
# Research Brief

This is the report content.
---SCOUT_REPORT_END---
Some trailing text"#;
        
        let result = extract_report(output).unwrap();
        assert!(result.contains("Research Brief"));
        assert!(result.contains("This is the report content."));
        assert!(!result.contains("preamble"));
        assert!(!result.contains("trailing"));
    }

    #[test]
    fn test_extract_report_with_ansi_codes() {
        let output = "\x1b[32m---SCOUT_REPORT_START---\x1b[0m\n# Report\n\x1b[31m---SCOUT_REPORT_END---\x1b[0m";
        
        let result = extract_report(output).unwrap();
        assert!(result.contains("# Report"));
    }

    #[test]
    fn test_extract_report_missing_start() {
        let output = "No markers here";
        let result = extract_report(output);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SCOUT_REPORT_START"));
    }

    #[test]
    fn test_extract_report_missing_end() {
        let output = "---SCOUT_REPORT_START---\nContent but no end";
        let result = extract_report(output);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("SCOUT_REPORT_END"));
    }

    #[test]
    fn test_extract_report_empty_content() {
        let output = "---SCOUT_REPORT_START---\n---SCOUT_REPORT_END---";
        let result = extract_report(output).unwrap();
        assert!(result.contains("empty report"));
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://www.rust-lang.org/"), Some("www.rust-lang.org"));
        assert_eq!(extract_domain("https://python.org/downloads"), Some("python.org"));
        assert_eq!(extract_domain("http://example.com"), Some("example.com"));
        assert_eq!(extract_domain("example.com/path"), Some("example.com"));
    }

    #[test]
    fn test_translate_progress_webdriver_start() {
        let line = "┌─ webdriver_start";
        assert_eq!(translate_progress(line), Some("🌐 Launching browser...".to_string()));
    }

    #[test]
    fn test_translate_progress_webdriver_navigate() {
        let line = "┌─ webdriver_navigate | https://www.rust-lang.org/";
        assert_eq!(translate_progress(line), Some("🔗 Navigating to www.rust-lang.org...".to_string()));
    }

    #[test]
    fn test_translate_progress_webdriver_get_page_source() {
        // With max_length arg (number)
        let line = "┌─ webdriver_get_page_source | 15000";
        assert_eq!(translate_progress(line), Some("📄 Reading page content...".to_string()));
        
        // With file path
        let line = "┌─ webdriver_get_page_source | tmp/rust_release.html";
        assert_eq!(translate_progress(line), Some("📥 Downloading rust_release.html...".to_string()));
    }

    #[test]
    fn test_translate_progress_webdriver_find_elements() {
        let line = "┌─ webdriver_find_elements | .download-os-source, .download-for-current-os";
        assert_eq!(translate_progress(line), Some("🔍 Searching page...".to_string()));
    }

    #[test]
    fn test_translate_progress_webdriver_quit() {
        let line = "┌─ webdriver_quit";
        assert_eq!(translate_progress(line), Some("✅ Closing browser...".to_string()));
    }

    #[test]
    fn test_translate_progress_read_file() {
        // Without range
        let line = "┌─ read_file | /path/to/file.rs";
        assert_eq!(translate_progress(line), Some("📖 Reading file.rs...".to_string()));
        
        // With range (file slice)
        let line = "┌─ read_file | /path/to/file.rs [1000..2000]";
        assert_eq!(translate_progress(line), Some("📖 Reading file.rs slice 1000..2000...".to_string()));
    }

    #[test]
    fn test_translate_progress_write_file() {
        let line = "┌─ write_file | output.md";
        assert_eq!(translate_progress(line), Some("💾 Writing output.md...".to_string()));
    }

    #[test]
    fn test_translate_progress_shell() {
        let line = "┌─ shell | ls -la";
        assert_eq!(translate_progress(line), Some(" > `ls -la` ...".to_string()));
    }

    #[test]
    fn test_translate_progress_with_ansi_codes() {
        // Real output from scout agent has ANSI codes
        let line = "\x1b[1;38;5;69m┌─ webdriver_start\x1b[0m";
        assert_eq!(translate_progress(line), Some("🌐 Launching browser...".to_string()));
        
        let line = "\x1b[1;38;5;69m┌─ webdriver_navigate\x1b[0m\x1b[35m | https://www.python.org/\x1b[0m";
        assert_eq!(translate_progress(line), Some("🔗 Navigating to www.python.org...".to_string()));
    }

    #[test]
    fn test_translate_progress_suppresses_non_tool_lines() {
        assert_eq!(translate_progress("Some random output"), None);
        assert_eq!(translate_progress("│ Page source (59851 chars)"), None);
        assert_eq!(translate_progress("└─ ⚡️ 1.5s"), None);
        assert_eq!(translate_progress(""), None);
    }

    #[test]
    fn test_truncate_command_snippet() {
        // Short command - no truncation
        assert_eq!(truncate_command_snippet("ls -la", 40), "ls -la");
        
        // Long command - truncated
        let long_cmd = "grep -r 'some very long search pattern' --include='*.rs' /path/to/directory";
        let result = truncate_command_snippet(long_cmd, 40);
        assert!(result.len() <= 40);
        assert!(result.ends_with("..."));
        
        // Multi-line command - only first line
        let multi_line = "echo 'line1'\necho 'line2'";
        assert_eq!(truncate_command_snippet(multi_line, 40), "echo 'line1'");
    }

    #[test]
    fn test_translate_progress_shell_long_command() {
        let line = "┌─ shell | grep -r 'some very long search pattern that exceeds the limit' --include='*.rs'";
        let result = translate_progress(line).unwrap();
        assert!(result.starts_with(" > `grep"));
        assert!(result.contains("..."));
    }
}
