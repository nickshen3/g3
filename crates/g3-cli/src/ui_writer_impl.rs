use crate::filter_json::{filter_json_tool_calls, reset_json_tool_state};
use crate::streaming_markdown::StreamingMarkdownFormatter;
use g3_core::ui_writer::UiWriter;
use std::io::{self, Write};
use std::sync::Mutex;
use termimad::MadSkin;

/// Console implementation of UiWriter that prints to stdout
pub struct ConsoleUiWriter {
    current_tool_name: std::sync::Mutex<Option<String>>,
    current_tool_args: std::sync::Mutex<Vec<(String, String)>>,
    current_output_line: std::sync::Mutex<Option<String>>,
    output_line_printed: std::sync::Mutex<bool>,
    is_agent_mode: std::sync::Mutex<bool>,
    /// Track if we're in shell compact mode (for appending timing to output line)
    is_shell_compact: std::sync::Mutex<bool>,
    /// Streaming markdown formatter for agent responses
    markdown_formatter: Mutex<Option<StreamingMarkdownFormatter>>,
    /// Track if the last output was text (for spacing between text and tool calls)
    last_output_was_text: std::sync::Mutex<bool>,
    /// Track if the last output was a tool call (for spacing between tool calls and text)
    last_output_was_tool: std::sync::Mutex<bool>,
    /// Track the last read_file path for continuation display
    last_read_file_path: std::sync::Mutex<Option<String>>,
}

/// ANSI color code for duration display based on elapsed time.
/// Returns empty string for fast operations, yellow/orange/red for slower ones.
fn duration_color(duration_str: &str) -> &'static str {
    // Format: "500ms", "1.5s", "2m 30.0s"
    if duration_str.ends_with("ms") {
        return ""; // Sub-second: no color
    }

    if let Some(m_pos) = duration_str.find('m') {
        // Contains minutes (e.g., "2m 30.0s")
        if let Ok(minutes) = duration_str[..m_pos].trim().parse::<u32>() {
            return match minutes {
                5.. => "\x1b[31m",      // Red: >= 5 minutes
                1.. => "\x1b[38;5;208m", // Orange: 1-4 minutes
                _ => "",
            };
        }
    } else if let Some(s_value) = duration_str.strip_suffix('s') {
        // Seconds only (e.g., "1.5s")
        if let Ok(seconds) = s_value.trim().parse::<f64>() {
            if seconds >= 1.0 {
                return "\x1b[33m"; // Yellow: >= 1 second
            }
        }
    }

    "" // Default: no color
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
            current_output_line: std::sync::Mutex::new(None),
            output_line_printed: std::sync::Mutex::new(false),
            is_agent_mode: std::sync::Mutex::new(false),
            is_shell_compact: std::sync::Mutex::new(false),
            markdown_formatter: Mutex::new(None),
            last_output_was_text: std::sync::Mutex::new(false),
            last_output_was_tool: std::sync::Mutex::new(false),
            last_read_file_path: std::sync::Mutex::new(None),
        }
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

    fn print_context_thinning(&self, message: &str) {
        // Animated highlight for context thinning
        // Use bright cyan/green with a quick flash animation

        // Flash animation: print with bright background, then normal
        let frames = vec![
            "\x1b[1;97;46m", // Frame 1: Bold white on cyan background
            "\x1b[1;97;42m", // Frame 2: Bold white on green background
            "\x1b[1;96;40m", // Frame 3: Bold cyan on black background
        ];

        println!();

        // Quick flash animation
        for frame in &frames {
            print!("\r{} ✨ {} ✨\x1b[0m\x1b[K", frame, message);
            let _ = io::stdout().flush();
            std::thread::sleep(std::time::Duration::from_millis(80));
        }

        // Final display with bright cyan and sparkle emojis
        print!("\r\x1b[1;96m✨ {} ✨\x1b[0m\x1b[K", message);
        println!();

        // Add a subtle "success" indicator line
        println!("\x1b[2;36m   └─ Context optimized successfully\x1b[0m");
        println!();

        let _ = io::stdout().flush();
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
        // Add blank line if last output was text (for visual separation)
        let mut last_was_text = self.last_output_was_text.lock().unwrap();
        if *last_was_text {
            println!();
        }
        *last_was_text = false; // We're now outputting a tool call
        *self.last_output_was_tool.lock().unwrap() = true;
        drop(last_was_text); // Release lock early

        // Reset output_line_printed at the start of a new tool output
        // This ensures the header isn't cleared by update_tool_output_line
        *self.output_line_printed.lock().unwrap() = false;
        // Reset shell compact mode
        *self.is_shell_compact.lock().unwrap() = false;
        // Now print the tool header with the most important arg
        // Use light gray/silver in agent mode, bold green otherwise
        let is_agent_mode = *self.is_agent_mode.lock().unwrap();
        // Light gray/silver: \x1b[38;5;250m, Bold green: \x1b[1;32m
        let tool_color = if is_agent_mode { "\x1b[1;38;5;250m" } else { "\x1b[1;32m" };
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

                // Truncate long values for display
                let display_value = if first_line.len() > 80 {
                    // Use char_indices to safely truncate at character boundary
                    let truncate_at = first_line
                        .char_indices()
                        .nth(77)
                        .map(|(i, _)| i)
                        .unwrap_or(first_line.len());
                    format!("{}...", &first_line[..truncate_at])
                } else {
                    first_line.to_string()
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
                    // Print compact shell header: "● shell | command"
                    println!(
                        " \x1b[2m●\x1b[0m {}{} \x1b[2m|\x1b[0m \x1b[35m{}\x1b[0m",
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
        // Handle file operation tools and other compact tools
        let is_compact_tool = matches!(tool_name, "read_file" | "write_file" | "str_replace" | "remember" | "take_screenshot" | "code_coverage" | "rehydrate");
        if !is_compact_tool {
            // Reset continuation tracking for non-compact tools
            *self.last_read_file_path.lock().unwrap() = None;
            return false;
        }

        // Add blank line if last output was text (for visual separation)
        let mut last_was_text = self.last_output_was_text.lock().unwrap();
        if *last_was_text {
            println!();
        }
        *last_was_text = false; // We're now outputting a tool call
        *self.last_output_was_tool.lock().unwrap() = true;

        let args = self.current_tool_args.lock().unwrap();
        let is_agent_mode = *self.is_agent_mode.lock().unwrap();

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
            // For remember, take_screenshot, etc. - no path to show
            String::new()
        } else {
            // Truncate long paths
            if file_path.len() > 60 {
                let truncate_at = file_path
                    .char_indices()
                    .nth(57)
                    .map(|(i, _)| i)
                    .unwrap_or(file_path.len());
                format!("{}", &file_path[..truncate_at])
            } else {
                file_path.to_string()
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
        let tool_color = if is_agent_mode { "\x1b[38;5;250m" } else { "\x1b[32m" };

        // Print compact single line
        if is_continuation {
            // Continuation line for consecutive read_file on same file:
            // "   └─ reading further [range] | summary | tokens ◉ time"
            println!(
                "   \x1b[2m└─ reading further\x1b[0m\x1b[35m{}\x1b[0m \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
                range_suffix,
                summary,
                tokens_delta,
                duration_str
            );
        } else if display_arg.is_empty() {
            // Tools without file path: " ● tool_name | summary | tokens ◉ time"
            println!(
                " \x1b[2m●\x1b[0m {}{} \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
                tool_color, tool_name, summary, tokens_delta, duration_str
            );
        } else {
            // Tools with file path: " ● tool_name | path [range] | summary | tokens ◉ time"
            println!(
                " \x1b[2m●\x1b[0m {}{} \x1b[2m|\x1b[0m \x1b[35m{}{}\x1b[0m \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
                tool_color, tool_name, display_arg, range_suffix, summary, tokens_delta, duration_str
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
            let mut last_was_tool = self.last_output_was_tool.lock().unwrap();
            if *last_was_tool && !content.trim().is_empty() {
                println!();
                *last_was_tool = false;
            }
            drop(last_was_tool);

            let formatted = formatter.process(content);
            print!("{}", formatted);
            // Track that we just output text (only if non-empty)
            if !content.trim().is_empty() {
                *self.last_output_was_text.lock().unwrap() = true;
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
        // Apply JSON tool call filtering for display
        filter_json_tool_calls(content)
    }

    fn reset_json_filter(&self) {
        // Reset the filter state for a new response
        reset_json_tool_state();
    }

    fn set_agent_mode(&self, is_agent_mode: bool) {
        *self.is_agent_mode.lock().unwrap() = is_agent_mode;
    }
}
