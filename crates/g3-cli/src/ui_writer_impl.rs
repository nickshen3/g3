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
        println!();
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
        // Only handle file operation tools in compact format
        let is_compact_tool = matches!(tool_name, "read_file" | "write_file" | "str_replace");
        if !is_compact_tool {
            return false;
        }

        let args = self.current_tool_args.lock().unwrap();
        let is_agent_mode = *self.is_agent_mode.lock().unwrap();

        // Get file path
        let file_path = args
            .iter()
            .find(|(k, _)| k == "file_path")
            .map(|(_, v)| v.as_str())
            .unwrap_or("?");

        // Truncate long paths
        let display_path = if file_path.len() > 60 {
            let truncate_at = file_path
                .char_indices()
                .nth(57)
                .map(|(i, _)| i)
                .unwrap_or(file_path.len());
            format!("{}...", &file_path[..truncate_at])
        } else {
            file_path.to_string()
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

        // Print compact single line:
        // " ● read_file | path [range] | summary | tokens ◉ time"
        println!(
            " \x1b[2m●\x1b[0m {}{} \x1b[2m|\x1b[0m \x1b[35m{}{}\x1b[0m \x1b[2m| {}\x1b[0m \x1b[2m| {} ◉ {}\x1b[0m",
            tool_color,
            tool_name,
            display_path,
            range_suffix,
            summary,
            tokens_delta,
            duration_str
        );

        // Clear the stored tool info
        drop(args); // Release the lock before clearing
        *self.current_tool_name.lock().unwrap() = None;
        self.current_tool_args.lock().unwrap().clear();
        *self.current_output_line.lock().unwrap() = None;
        *self.output_line_printed.lock().unwrap() = false;

        true
    }

    fn print_tool_timing(&self, duration_str: &str, tokens_delta: u32, context_percentage: f32) {
        // Parse the duration string to determine color
        // Format is like "1.5s", "500ms", "2m 30.0s"
        let color_code = if duration_str.ends_with("ms") {
            // Milliseconds - use default color (< 1s)
            ""
        } else if duration_str.contains('m') {
            // Contains minutes
            // Extract minutes value
            if let Some(m_pos) = duration_str.find('m') {
                if let Ok(minutes) = duration_str[..m_pos].trim().parse::<u32>() {
                    if minutes >= 5 {
                        "\x1b[31m" // Red for >= 5 minutes
                    } else {
                        "\x1b[38;5;208m" // Orange for >= 1 minute but < 5 minutes
                    }
                } else {
                    "" // Default color if parsing fails
                }
            } else {
                "" // Default color if 'm' not found (shouldn't happen)
            }
        } else if duration_str.ends_with('s') {
            // Seconds only
            if let Some(s_value) = duration_str.strip_suffix('s') {
                if let Ok(seconds) = s_value.trim().parse::<f64>() {
                    if seconds >= 1.0 {
                        "\x1b[33m" // Yellow for >= 1 second
                    } else {
                        "" // Default color for < 1 second
                    }
                } else {
                    "" // Default color if parsing fails
                }
            } else {
                "" // Default color
            }
        } else {
            // Milliseconds or other format - use default color
            ""
        };

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
        *self.current_tool_name.lock().unwrap() = None;
        self.current_tool_args.lock().unwrap().clear();
        *self.current_output_line.lock().unwrap() = None;
        *self.output_line_printed.lock().unwrap() = false;
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
            let formatted = formatter.process(content);
            print!("{}", formatted);
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
