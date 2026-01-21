use crate::filter_json::{filter_json_tool_calls, reset_json_tool_state, ToolParsingHint};
use crate::display::{shorten_path, shorten_paths_in_command};
use crate::streaming_markdown::StreamingMarkdownFormatter;
use g3_core::ui_writer::UiWriter;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU8, Ordering}};
use termimad::MadSkin;

/// Padding width for tool names in compact display (longest tool: "str_replace" = 11 chars)
const TOOL_NAME_PADDING: usize = 11;

/// ANSI escape codes
mod ansi {
    pub const YELLOW: &str = "\x1b[33m";
    pub const ORANGE: &str = "\x1b[38;5;208m";
    pub const RED: &str = "\x1b[31m";
}

/// Colorize a str_replace summary (e.g., "+5 | -3" -> green "+5" | red "-3")
fn colorize_str_replace_summary(summary: &str) -> String {
    // Parse patterns like "+5 | -3", "+5", "-3"
    if summary.contains(" | ") {
        let parts: Vec<&str> = summary.split(" | ").collect();
        if parts.len() == 2 {
            return format!("\x1b[32m{}\x1b[0m \x1b[2m|\x1b[0m \x1b[31m{}\x1b[0m", parts[0], parts[1]);
        }
    } else if summary.starts_with('+') {
        return format!("\x1b[32m{}\x1b[0m", summary);
    } else if summary.starts_with('-') {
        return format!("\x1b[31m{}\x1b[0m", summary);
    }
    summary.to_string()
}

/// ANSI color codes for tool names
const TOOL_COLOR_NORMAL: &str = "\x1b[32m";
const TOOL_COLOR_NORMAL_BOLD: &str = "\x1b[1;32m";
const TOOL_COLOR_AGENT: &str = "\x1b[38;5;250m";
const TOOL_COLOR_AGENT_BOLD: &str = "\x1b[1;38;5;250m";

/// Blink state values for the streaming indicator
const BLINK_INACTIVE: u8 = 0;
const BLINK_SHOW_PIPE: u8 = 1;
const BLINK_SHOW_SPACE: u8 = 2;

/// Shared state for tool parsing hints that can be used in callbacks.
/// This is separate from ConsoleUiWriter so it can be captured by Arc in closures.
#[derive(Clone)]
struct ParsingHintState {
    parsing_indicator_printed: Arc<AtomicBool>,
    last_output_was_text: Arc<AtomicBool>,
    last_output_was_tool: Arc<AtomicBool>,
    is_agent_mode: Arc<AtomicBool>,
    /// Blink state: 0 = inactive, 1 = show pipe, 2 = show space
    blink_state: Arc<AtomicU8>,
}

impl ParsingHintState {
    fn new() -> Self {
        Self {
            parsing_indicator_printed: Arc::new(AtomicBool::new(false)),
            last_output_was_text: Arc::new(AtomicBool::new(false)),
            last_output_was_tool: Arc::new(AtomicBool::new(false)),
            is_agent_mode: Arc::new(AtomicBool::new(false)),
            blink_state: Arc::new(AtomicU8::new(BLINK_INACTIVE)),
        }
    }

    fn clear(&self) {
        self.parsing_indicator_printed.store(false, Ordering::Relaxed);
        self.blink_state.store(BLINK_INACTIVE, Ordering::Relaxed);
    }

    /// Handle a tool parsing hint - this is the core logic extracted for use in callbacks
    fn handle_hint(&self, hint: ToolParsingHint) {
        match hint {
            ToolParsingHint::Detected(tool_name) => {
                // Stop any previous blinking
                self.blink_state.store(BLINK_INACTIVE, Ordering::Relaxed);
                
                // Check if we've already printed an indicator (this is an update)
                let already_printed = self.parsing_indicator_printed.load(Ordering::Relaxed);
                
                if already_printed {
                    // Update in place: clear line and reprint with new name
                    print!("\r\x1b[2K");
                } else {
                    // First time: add blank line if last output was text
                    if self.last_output_was_text.load(Ordering::Relaxed) {
                        println!();
                    }
                    self.last_output_was_text.store(false, Ordering::Relaxed);
                    self.last_output_was_tool.store(true, Ordering::Relaxed);
                }

                // Get color based on agent mode
                let tool_color = if self.is_agent_mode.load(Ordering::Relaxed) {
                    TOOL_COLOR_AGENT
                } else {
                    TOOL_COLOR_NORMAL
                };
                
                // Print the indicator: " ● tool_name |"
                print!(" \x1b[2m●\x1b[0m {}{:<width$}\x1b[0m \x1b[2m|\x1b[0m", tool_color, tool_name, width = TOOL_NAME_PADDING);
                let _ = io::stdout().flush();
                
                self.parsing_indicator_printed.store(true, Ordering::Relaxed);
                self.blink_state.store(BLINK_SHOW_PIPE, Ordering::Relaxed);
            }
            ToolParsingHint::Active => {
                // Toggle blink state for visual feedback
                let current = self.blink_state.load(Ordering::Relaxed);
                if current != BLINK_INACTIVE {
                    let new_state = if current == BLINK_SHOW_PIPE { BLINK_SHOW_SPACE } else { BLINK_SHOW_PIPE };
                    self.blink_state.store(new_state, Ordering::Relaxed);
                    let indicator = if new_state == BLINK_SHOW_PIPE { "|" } else { " " };
                    // Move back one char and reprint
                    print!("\x1b[1D\x1b[2m{}\x1b[0m", indicator);
                    let _ = io::stdout().flush();
                }
            }
            ToolParsingHint::Complete => {
                // Stop blinking
                self.blink_state.store(BLINK_INACTIVE, Ordering::Relaxed);
                // Clear the parsing indicator line - the actual tool output will follow
                if self.parsing_indicator_printed.load(Ordering::Relaxed) {
                    // Clear the current line and move to start
                    print!("\r\x1b[2K");
                    let _ = io::stdout().flush();
                }
                self.clear();
            }
        }
    }
}

/// Console implementation of UiWriter that prints to stdout
pub struct ConsoleUiWriter {
    current_tool_name: std::sync::Mutex<Option<String>>,
    current_tool_args: std::sync::Mutex<Vec<(String, String)>>,
    /// Workspace path for shortening displayed paths
    workspace_path: std::sync::Mutex<Option<std::path::PathBuf>>,
    /// Project path for shortening displayed paths (takes priority over workspace)
    project_path: std::sync::Mutex<Option<std::path::PathBuf>>,
    /// Project name for display (e.g., "appa_estate")
    project_name: std::sync::Mutex<Option<String>>,
    current_output_line: std::sync::Mutex<Option<String>>,
    output_line_printed: std::sync::Mutex<bool>,
    /// Track if we're in shell compact mode (for appending timing to output line)
    is_shell_compact: std::sync::Mutex<bool>,
    /// Streaming markdown formatter for agent responses
    markdown_formatter: Mutex<Option<StreamingMarkdownFormatter>>,
    /// Track the last read_file path for continuation display
    last_read_file_path: std::sync::Mutex<Option<String>>,
    /// Shared state for tool parsing hints (used by real-time callback)
    hint_state: ParsingHintState,
}

/// ANSI color code for duration display based on elapsed time.
/// Returns empty string for fast operations, yellow/orange/red for slower ones.
fn duration_color(duration_str: &str) -> &'static str {
    if duration_str.ends_with("ms") {
        return "";
    }

    if let Some(m_pos) = duration_str.find('m') {
        if let Ok(minutes) = duration_str[..m_pos].trim().parse::<u32>() {
            return match minutes {
                5.. => ansi::RED,
                1.. => ansi::ORANGE,
                _ => "",
            };
        }
    } else if let Some(s_value) = duration_str.strip_suffix('s') {
        if let Ok(seconds) = s_value.trim().parse::<f64>() {
            if seconds >= 1.0 {
                return ansi::YELLOW;
            }
        }
    }

    ""
}

impl ConsoleUiWriter {
    /// Clear all stored tool state after output is complete.
    fn clear_tool_state(&self) {
        *self.current_tool_name.lock().unwrap() = None;
        self.current_tool_args.lock().unwrap().clear();
        *self.current_output_line.lock().unwrap() = None;
        *self.output_line_printed.lock().unwrap() = false;
    }

}

impl ConsoleUiWriter {
    pub fn new() -> Self {
        Self {
            current_tool_name: std::sync::Mutex::new(None),
            current_tool_args: std::sync::Mutex::new(Vec::new()),
            workspace_path: std::sync::Mutex::new(None),
            project_path: std::sync::Mutex::new(None),
            project_name: std::sync::Mutex::new(None),
            current_output_line: std::sync::Mutex::new(None),
            output_line_printed: std::sync::Mutex::new(false),
            is_shell_compact: std::sync::Mutex::new(false),
            markdown_formatter: Mutex::new(None),
            last_read_file_path: std::sync::Mutex::new(None),
            hint_state: ParsingHintState::new(),
        }
    }
}

impl ConsoleUiWriter {
    fn get_workspace_path(&self) -> Option<std::path::PathBuf> {
        self.workspace_path.lock().unwrap().clone()
    }

    fn get_project_info(&self) -> Option<(std::path::PathBuf, String)> {
        let path = self.project_path.lock().unwrap().clone()?;
        let name = self.project_name.lock().unwrap().clone()?;
        Some((path, name))
    }
}

impl UiWriter for ConsoleUiWriter {
    fn print(&self, message: &str) {
        print!("{}", message);
    }

    fn println(&self, message: &str) {
        println!("{}", message);
    }

    fn print_inline(&self, message: &str) {
        print!("{}", message);
        let _ = io::stdout().flush();
    }

    fn print_system_prompt(&self, prompt: &str) {
        println!("🔍 System Prompt:");
        println!("================");
        println!("{}", prompt);
        println!("================");
        println!();
    }

    fn print_context_status(&self, message: &str) {
        println!("{}", message);
    }

    fn print_g3_progress(&self, message: &str) {
        crate::g3_status::G3Status::progress(message);
    }

    fn print_g3_status(&self, message: &str, status: &str) {
        use crate::g3_status::Status;
        let _ = message; // unused now - progress already printed the message
        crate::g3_status::G3Status::status(&Status::parse(status));
    }

    fn print_thin_result(&self, result: &g3_core::ThinResult) {
        // Use centralized G3Status formatting
        crate::g3_status::G3Status::thin_result(result);
    }

    fn print_tool_header(&self, tool_name: &str, _tool_args: Option<&serde_json::Value>) {
        // Store the tool name and clear args for collection
        *self.current_tool_name.lock().unwrap() = Some(tool_name.to_string());
        self.current_tool_args.lock().unwrap().clear();
    }

    fn print_tool_arg(&self, key: &str, value: &str) {
        // Collect arguments instead of printing immediately
        // Filter out any keys that look like they might be agent message content
        // (e.g., keys that are suspiciously long or contain message-like content)
        let is_valid_arg_key = key.len() < 50
            && !key.contains('\n')
            && !key.contains("I'll")
            && !key.contains("Let me")
            && !key.contains("Here's")
            && !key.contains("I can");

        if is_valid_arg_key {
            self.current_tool_args
                .lock()
                .unwrap()
                .push((key.to_string(), value.to_string()));
        }
    }

    fn print_tool_output_header(&self) {
        // Clear any streaming hint that might be showing
        // This ensures we don't duplicate the tool name on the line
        self.hint_state.handle_hint(ToolParsingHint::Complete);

        // Add blank line if last output was text (for visual separation)
        let last_was_text = self.hint_state.last_output_was_text.load(Ordering::Relaxed);
        if last_was_text {
            println!();
        }
        self.hint_state.last_output_was_text.store(false, Ordering::Relaxed);
        self.hint_state.last_output_was_tool.store(true, Ordering::Relaxed);

        // Reset output_line_printed at the start of a new tool output
        // This ensures the header isn't cleared by update_tool_output_line
        *self.output_line_printed.lock().unwrap() = false;
        // Reset shell compact mode
        *self.is_shell_compact.lock().unwrap() = false;
        // Now print the tool header with the most important arg
        // Use light gray/silver in agent mode, bold green otherwise
        let is_agent_mode = self.hint_state.is_agent_mode.load(Ordering::Relaxed);
        // Light gray/silver: \x1b[38;5;250m, Bold green: \x1b[1;32m
        let tool_color = if is_agent_mode {
            TOOL_COLOR_AGENT_BOLD
        } else {
            TOOL_COLOR_NORMAL_BOLD
        };
        if let Some(tool_name) = self.current_tool_name.lock().unwrap().as_ref() {
            let args = self.current_tool_args.lock().unwrap();

            // Find the most important argument - prioritize file_path if available
            let important_arg = args
                .iter()
                .find(|(k, _)| k == "file_path")
                .or_else(|| args.iter().find(|(k, _)| k == "command" || k == "path"))
                .or_else(|| args.first());

            if let Some((_, value)) = important_arg {
                // For multi-line values, only show the first line
                let first_line = value.lines().next().unwrap_or("");

                // Get workspace path for shortening
                let workspace = self.get_workspace_path();
                let workspace_ref = workspace.as_deref();
                
                // Get project info for shortening
                let project_info = self.get_project_info();
                let project_ref = project_info.as_ref().map(|(p, n)| (p.as_path(), n.as_str()));

                // Shorten paths in the value (handles both file paths and shell commands)
                let shortened = shorten_paths_in_command(first_line, workspace_ref, project_ref);

                // Truncate long values for display (after shortening)
                let display_value = if shortened.chars().count() > 80 {
                    // Use char_indices to safely truncate at character boundary
                    let truncate_at = shortened
                        .char_indices()
                        .nth(77)
                        .map(|(i, _)| i)
                        .unwrap_or(shortened.len());
                    format!("{}...", &shortened[..truncate_at])
                } else {
                    shortened
                };

                // Add range information for read_file tool calls
                let header_suffix = if tool_name == "read_file" {
                    // Check if start or end parameters are present
                    let has_start = args.iter().any(|(k, _)| k == "start");
                    let has_end = args.iter().any(|(k, _)| k == "end");

                    if has_start || has_end {
                        let start_val = args
                            .iter()
                            .find(|(k, _)| k == "start")
                            .map(|(_, v)| v.as_str())
                            .unwrap_or("0");
                        let end_val = args
                            .iter()
                            .find(|(k, _)| k == "end")
                            .map(|(_, v)| v.as_str())
                            .unwrap_or("end");
                        format!(" [{}..{}]", start_val, end_val)
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                // Check if this is a shell command - use compact format
                if tool_name == "shell" {
                    *self.is_shell_compact.lock().unwrap() = true;
                    // Print compact shell header: "● shell      | command"
                    // Pad to align with longest compact tool (str_replace = 11 chars)
                    println!(
                        " \x1b[2m●\x1b[0m {}{:<11}\x1b[0m \x1b[2m|\x1b[0m \x1b[35m{}\x1b[0m",
                        tool_color, tool_name, display_value
                    );
                    return;
                }

                // Print with tool name in color (royal blue for agent mode, green otherwise)
                println!(
                    "┌─{} {}\x1b[0m\x1b[35m | {}{}\x1b[0m",
                    tool_color, tool_name, display_value, header_suffix
                );
            } else {
                // Print with tool name in color
                println!("┌─{} {}\x1b[0m", tool_color, tool_name);
            }
        }
    }

    fn update_tool_output_line(&self, line: &str) {
        // Truncate long lines to prevent terminal wrapping issues
        // When lines wrap, the cursor-up escape code only moves up one visual line
        const MAX_LINE_WIDTH: usize = 120;
        let mut current_line = self.current_output_line.lock().unwrap();
        let mut line_printed = self.output_line_printed.lock().unwrap();
        let is_shell = *self.is_shell_compact.lock().unwrap();

        // If we've already printed a line, clear it first
        if *line_printed {
            if is_shell {
                // For shell, we printed without newline, so just clear the line
                print!("\r\x1b[2K");
            } else {
                // Move cursor up one line and clear it
                print!("\x1b[1A\x1b[2K");
            }
        }

        // Truncate line if needed to prevent wrapping
        let display_line = if line.chars().count() > MAX_LINE_WIDTH {
            let truncated: String = line.chars().take(MAX_LINE_WIDTH - 3).collect();
            format!("{}...", truncated)
        } else {
            line.to_string()
        };

        // Use different prefix for shell (└─) vs other tools (│)
        if is_shell {
            // For shell, print without newline so timing can be appended
            print!("   \x1b[2m└─ {}\x1b[0m", display_line);
        } else {
            println!("│ \x1b[2m{}\x1b[0m", display_line);
        }
        let _ = io::stdout().flush();

        // Update state
        *current_line = Some(line.to_string());
        *line_printed = true;
    }

    fn print_tool_output_line(&self, line: &str) {
        // Skip the TODO list header line
        if line.starts_with("📝 TODO list:") {
            return;
        }
        println!("│ \x1b[2m{}\x1b[0m", line);
    }

    fn print_tool_output_summary(&self, count: usize) {
        let is_shell = *self.is_shell_compact.lock().unwrap();
        if is_shell {
            // For shell, append to the same line (no newline)
            print!(" \x1b[2m({} line{})\x1b[0m", count, if count == 1 { "" } else { "s" });
            let _ = io::stdout().flush();
        } else {
            println!(
                "│ \x1b[2m({} line{})\x1b[0m",
                count,
                if count == 1 { "" } else { "s" }
            );
        }
    }

    fn print_tool_compact(&self, tool_name: &str, summary: &str, duration_str: &str, tokens_delta: u32, _context_percentage: f32) -> bool {
        // Clear any streaming hint that might be showing
        // This ensures we don't duplicate the tool name on the line
        self.hint_state.handle_hint(ToolParsingHint::Complete);

        // Handle file operation tools and other compact tools
        let is_compact_tool = matches!(tool_name, "read_file" | "write_file" | "str_replace" | "remember" | "screenshot" | "coverage" | "rehydrate" | "code_search");
        if !is_compact_tool {
            // Reset continuation tracking for non-compact tools
            *self.last_read_file_path.lock().unwrap() = None;
            return false;
        }

        // Add blank line if last output was text (for visual separation)
        if self.hint_state.last_output_was_text.load(Ordering::Relaxed) {
            println!();
        }
        self.hint_state.last_output_was_text.store(false, Ordering::Relaxed);
        self.hint_state.last_output_was_tool.store(true, Ordering::Relaxed);

        let args = self.current_tool_args.lock().unwrap();
        let is_agent_mode = self.hint_state.is_agent_mode.load(Ordering::Relaxed);

        // Get file path (for file operation tools)
        let file_path = args
            .iter()
            .find(|(k, _)| k == "file_path")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");

        // Check if this is a continuation of reading the same file
        let mut last_read_path = self.last_read_file_path.lock().unwrap();
        let is_continuation = tool_name == "read_file" && !file_path.is_empty() && last_read_path.as_deref() == Some(file_path);

        // For tools without file_path, get other relevant args
        let display_arg = if file_path.is_empty() {
            // For code_search, extract language and name from searches
            if tool_name == "code_search" {
                // searches arg is JSON array, try to extract first search's language and name
                if let Some((_, searches_json)) = args.iter().find(|(k, _)| k == "searches") {
                    if let Ok(searches) = serde_json::from_str::<serde_json::Value>(searches_json) {
                        if let Some(first_search) = searches.as_array().and_then(|arr| arr.first()) {
                            let lang = first_search.get("language").and_then(|v| v.as_str()).unwrap_or("?");
                            let name = first_search.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                            // Truncate name if too long
                            let display_name = if name.len() > 30 {
                                let truncate_at = name.char_indices().nth(27).map(|(i, _)| i).unwrap_or(name.len());
                                format!("{}...", &name[..truncate_at])
                            } else {
                                name.to_string()
                            };
                            format!("{}:\"{}\"", lang, display_name)
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            } else {
                // For remember, screenshot, etc. - no path to show
                String::new()
            }
        } else {
            // Shorten path (project -> name/, workspace -> ./, home -> ~) then truncate if still long
            let workspace = self.get_workspace_path();
            let project_info = self.get_project_info();
            let project_ref = project_info.as_ref().map(|(p, n)| (p.as_path(), n.as_str()));
            let shortened = shorten_path(file_path, workspace.as_deref(), project_ref);
            
            if shortened.chars().count() > 60 {
                let truncate_at = shortened
                    .char_indices()
                    .nth(57)
                    .map(|(i, _)| i)
                    .unwrap_or(shortened.len());
                format!("{}...", &shortened[..truncate_at])
            } else {
                shortened
            }
        };

        // Build range suffix for read_file
        let range_suffix = if tool_name == "read_file" {
            let has_start = args.iter().any(|(k, _)| k == "start");
            let has_end = args.iter().any(|(k, _)| k == "end");
            if has_start || has_end {
                let start_val = args
                    .iter()
                    .find(|(k, _)| k == "start")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("0");
                let end_val = args
                    .iter()
                    .find(|(k, _)| k == "end")
                    .map(|(_, v)| v.as_str())
                    .unwrap_or("end");
                format!(" [{}..{}]", start_val, end_val)
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Color for tool name
        let tool_color = if is_agent_mode { TOOL_COLOR_AGENT } else { TOOL_COLOR_NORMAL };

        // Colorize summary for str_replace (green insertions, red deletions)
        let display_summary = if tool_name == "str_replace" {
            colorize_str_replace_summary(summary)
        } else {
            summary.to_string()
        };

        // Print compact single line
        if is_continuation {
            // Continuation line for consecutive read_file on same file:
            // "   └─ reading further [range] | summary | tokens ◉ time"
            println!(
                "   \x1b[2m└─ reading further\x1b[0m\x1b[35m{}\x1b[0m \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
                range_suffix,
                display_summary,
                tokens_delta,
                duration_str
            );
        } else if display_arg.is_empty() {
            // Tools without file path: " ● tool_name | summary | tokens ◉ time"
            // Pad to align with longest compact tool (str_replace = 11 chars)
            println!(
                " \x1b[2m●\x1b[0m {}{:<11}\x1b[0m \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
                tool_color, tool_name, display_summary, tokens_delta, duration_str
            );
        } else {
            // Tools with file path: " ● tool_name | path [range] | summary | tokens ◉ time"
            // Pad to align with longest compact tool (str_replace = 11 chars)
            println!(
                " \x1b[2m●\x1b[0m {}{:<11}\x1b[0m \x1b[2m|\x1b[0m \x1b[35m{}{}\x1b[0m \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
                tool_color, tool_name, display_arg, range_suffix, display_summary, tokens_delta, duration_str
            );
        }

        // Update last_read_file_path for continuation tracking
        if tool_name == "read_file" && !file_path.is_empty() {
            *last_read_path = Some(file_path.to_string());
        } else {
            // Reset for non-read_file tools
            *last_read_path = None;
        }

        // Clear the stored tool info
        drop(args); // Release the lock before clearing
        drop(last_read_path); // Release this lock too
        self.clear_tool_state();

        true
    }

    fn print_todo_compact(&self, content: Option<&str>, is_write: bool) -> bool {
        let tool_name = if is_write { "todo_write" } else { "todo_read" };
        // Clear any streaming hint that might be showing
        // This ensures we don't duplicate the tool name on the line
        self.hint_state.handle_hint(ToolParsingHint::Complete);

        let is_agent_mode = self.hint_state.is_agent_mode.load(Ordering::Relaxed);
        let tool_color = if is_agent_mode { TOOL_COLOR_AGENT } else { TOOL_COLOR_NORMAL };

        // Add blank line if last output was text (for visual separation)
        if self.hint_state.last_output_was_text.load(Ordering::Relaxed) {
            println!();
        }
        self.hint_state.last_output_was_text.store(false, Ordering::Relaxed);
        self.hint_state.last_output_was_tool.store(true, Ordering::Relaxed);
        // Reset read_file continuation tracking
        *self.last_read_file_path.lock().unwrap() = None;

        match content {
            None => {
                // Empty TODO
                println!(" \x1b[2m●\x1b[0m {}{:<width$}\x1b[0m \x1b[2m|\x1b[0m \x1b[35mempty\x1b[0m", tool_color, tool_name, width = TOOL_NAME_PADDING);
            }
            Some(text) => {
                // Header
                println!(" \x1b[2m●\x1b[0m {}{:<width$}\x1b[0m", tool_color, tool_name, width = TOOL_NAME_PADDING);
                
                let lines: Vec<&str> = text.lines().collect();
                let last_idx = lines.len().saturating_sub(1);
                
                for (i, line) in lines.iter().enumerate() {
                    let is_last = i == last_idx;
                    let prefix = if is_last { "└" } else { "│" };
                    
                    // Convert checkboxes to styled symbols and strikethrough completed items
                    let is_completed = line.contains("- [x]") || line.contains("- [X]");
                    let styled_line = if is_completed {
                        // Replace checkbox and apply strikethrough to the task text
                        let task_text = line
                            .replace("- [x]", "")
                            .replace("- [X]", "")
                            .trim_start()
                            .to_string();
                        format!("■ \x1b[9m{}\x1b[0m\x1b[2m", task_text)  // \x1b[9m is strikethrough
                    } else {
                        line.replace("- [ ]", "□")
                    };

                    // Dim the line content
                    println!("   \x1b[2m{}  {}\x1b[0m", prefix, styled_line);
                }
                // Add blank line after content for readability
                println!();
            }
        }

        // Clear tool state
        self.clear_tool_state();
        
        true
    }

    fn print_tool_timing(&self, duration_str: &str, tokens_delta: u32, context_percentage: f32) {
        let color_code = duration_color(duration_str);

        // Reset read_file continuation tracking for non-read_file tools
        // (read_file tools handle this in print_tool_compact)
        if let Some(tool_name) = self.current_tool_name.lock().unwrap().as_ref() {
            if tool_name != "read_file" {
                *self.last_read_file_path.lock().unwrap() = None;
            }
        }

        // Add blank line before footer for research tool (its output is a full report)
        if let Some(tool_name) = self.current_tool_name.lock().unwrap().as_ref() {
            if tool_name == "research" {
                println!();
            }
        }
        
        // Check if we're in shell compact mode - append timing to the output line
        let is_shell = *self.is_shell_compact.lock().unwrap();
        if is_shell {
            // Append timing to the same line as shell output
            println!(" \x1b[2m| {} ◉ {}{}\x1b[0m", tokens_delta, color_code, duration_str);
            println!();
        } else {
            println!("└─ ⚡️ {}{}\x1b[0m  \x1b[2m{} ◉ | {:.0}%\x1b[0m", color_code, duration_str, tokens_delta, context_percentage);
            println!();
        }
        
        // Clear the stored tool info
        self.clear_tool_state();
        *self.is_shell_compact.lock().unwrap() = false;
    }

    fn print_agent_prompt(&self) {
        let _ = io::stdout().flush();
    }

    fn print_agent_response(&self, content: &str) {
        let mut formatter_guard = self.markdown_formatter.lock().unwrap();
        
        // Initialize formatter if not already done
        if formatter_guard.is_none() {
            let mut skin = MadSkin::default();
            skin.bold.set_fg(termimad::crossterm::style::Color::Green);
            skin.italic.set_fg(termimad::crossterm::style::Color::Cyan);
            skin.inline_code.set_fg(termimad::crossterm::style::Color::Rgb { r: 216, g: 177, b: 114 });
            *formatter_guard = Some(StreamingMarkdownFormatter::new(skin));
        }
        
        // Process the chunk through the formatter
        if let Some(ref mut formatter) = *formatter_guard {
            // Add blank line if last output was a tool call (for visual separation)
            // Only do this once at the start of new text content
            let last_was_tool = self.hint_state.last_output_was_tool.load(Ordering::Relaxed);
            if last_was_tool && !content.trim().is_empty() {
                println!();
                self.hint_state.last_output_was_tool.store(false, Ordering::Relaxed);
            }

            let formatted = formatter.process(content);
            print!("{}", formatted);
            // Track that we just output text (only if non-empty)
            if !content.trim().is_empty() {
                self.hint_state.last_output_was_text.store(true, Ordering::Relaxed);
                // Reset read_file continuation tracking when text is output between tool calls
                *self.last_read_file_path.lock().unwrap() = None;
            }
            let _ = io::stdout().flush();
        }
    }

    fn finish_streaming_markdown(&self) {
        let mut formatter_guard = self.markdown_formatter.lock().unwrap();
        
        if let Some(ref mut formatter) = *formatter_guard {
            // Flush any remaining buffered content
            let remaining = formatter.finish();
            print!("{}", remaining);
            let _ = io::stdout().flush();
        }
        
        // Reset the formatter for the next response
        *formatter_guard = None;
    }

    fn notify_sse_received(&self) {
        // No-op for console - we don't track SSEs in console mode
    }

    fn print_tool_streaming_hint(&self, tool_name: &str) {
        // Use the hint state to show the streaming indicator
        self.hint_state.handle_hint(ToolParsingHint::Detected(tool_name.to_string()));
    }

    fn print_tool_streaming_active(&self) {
        // Trigger the blink animation
        self.hint_state.handle_hint(ToolParsingHint::Active);
    }

    fn flush(&self) {
        let _ = io::stdout().flush();
    }

    fn prompt_user_yes_no(&self, message: &str) -> bool {
        print!("{} [y/N] ", message);
        let _ = io::stdout().flush();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_ok() {
            let trimmed = input.trim().to_lowercase();
            trimmed == "y" || trimmed == "yes"
        } else {
            false
        }
    }

    fn prompt_user_choice(&self, message: &str, options: &[&str]) -> usize {
        println!("{} ", message);
        for (i, option) in options.iter().enumerate() {
            println!("  [{}] {}", i + 1, option);
        }
        print!("Select an option (1-{}): ", options.len());
        let _ = io::stdout().flush();

        loop {
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                if let Ok(choice) = input.trim().parse::<usize>() {
                    if choice > 0 && choice <= options.len() {
                        return choice - 1;
                    }
                }
            }
            print!("Invalid choice. Please select (1-{}): ", options.len());
            let _ = io::stdout().flush();
        }
    }


    fn filter_json_tool_calls(&self, content: &str) -> String {
        // Filter the content to remove JSON tool calls from display.
        // Tool streaming hints are now handled via the provider's tool_call_streaming
        // field in CompletionChunk, not via callbacks during JSON filtering.
        filter_json_tool_calls(content)
    }

    fn reset_json_filter(&self) {
        // Reset the filter state for a new response
        reset_json_tool_state();
    }

    fn set_agent_mode(&self, is_agent_mode: bool) {
        self.hint_state.is_agent_mode.store(is_agent_mode, Ordering::Relaxed);
    }

    fn set_workspace_path(&self, path: std::path::PathBuf) {
        *self.workspace_path.lock().unwrap() = Some(path);
    }

    fn set_project_path(&self, path: std::path::PathBuf, name: String) {
        *self.project_path.lock().unwrap() = Some(path);
        *self.project_name.lock().unwrap() = Some(name);
    }

    fn clear_project(&self) {
        *self.project_path.lock().unwrap() = None;
        *self.project_name.lock().unwrap() = None;
    }
}
