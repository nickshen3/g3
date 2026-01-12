//! Streaming completion logic for the Agent.
//!
//! This module handles the streaming response from LLM providers,
//! including tool call detection, execution, and auto-continue logic.

use crate::context_window::ContextWindow;
use crate::streaming_parser::StreamingToolParser;
use crate::ToolCall;
use g3_providers::{CompletionRequest, MessageRole};
use std::time::{Duration, Instant};
use tracing::{debug, error};

/// Constants for streaming behavior
pub const MAX_ITERATIONS: usize = 400;

/// State tracked across streaming iterations
pub struct StreamingState {
    pub full_response: String,
    pub first_token_time: Option<Duration>,
    pub stream_start: Instant,
    pub iteration_count: usize,
    pub response_started: bool,
    pub any_tool_executed: bool,
    pub auto_summary_attempts: usize,
    pub turn_accumulated_usage: Option<g3_providers::Usage>,
}

impl StreamingState {
    pub fn new() -> Self {
        Self {
            full_response: String::new(),
            first_token_time: None,
            stream_start: Instant::now(),
            iteration_count: 0,
            response_started: false,
            any_tool_executed: false,
            auto_summary_attempts: 0,
            turn_accumulated_usage: None,
        }
    }

    pub fn record_first_token(&mut self) {
        if self.first_token_time.is_none() {
            self.first_token_time = Some(self.stream_start.elapsed());
        }
    }

    pub fn get_ttft(&self) -> Duration {
        self.first_token_time.unwrap_or_else(|| self.stream_start.elapsed())
    }
}

impl Default for StreamingState {
    fn default() -> Self {
        Self::new()
    }
}

/// State tracked within a single streaming iteration
pub struct IterationState {
    pub parser: StreamingToolParser,
    pub current_response: String,
    pub tool_executed: bool,
    pub chunks_received: usize,
    pub raw_chunks: Vec<String>,
    pub accumulated_usage: Option<g3_providers::Usage>,
}

impl IterationState {
    pub fn new() -> Self {
        Self {
            parser: StreamingToolParser::new(),
            current_response: String::new(),
            tool_executed: false,
            chunks_received: 0,
            raw_chunks: Vec::new(),
            accumulated_usage: None,
        }
    }

    /// Store a raw chunk for debugging (limited to first 20 + last few)
    pub fn record_chunk(&mut self, chunk: &g3_providers::CompletionChunk) {
        if self.chunks_received < 20 || chunk.finished {
            self.raw_chunks.push(format!(
                "Chunk #{}: content={:?}, finished={}, tool_calls={:?}",
                self.chunks_received + 1,
                chunk.content,
                chunk.finished,
                chunk.tool_calls
            ));
        } else if self.raw_chunks.len() == 20 {
            self.raw_chunks.push("... (chunks 21+ omitted for brevity) ...".to_string());
        }
        self.chunks_received += 1;
    }
}

impl Default for IterationState {
    fn default() -> Self {
        Self::new()
    }
}

/// Clean LLM-specific tokens from content
pub fn clean_llm_tokens(content: &str) -> String {
    content
        .replace("<|im_end|>", "")
        .replace("</s>", "")
        .replace("[/INST]", "")
        .replace("<</SYS>>", "")
}

/// Format a duration for display
pub fn format_duration(duration: Duration) -> String {
    let total_ms = duration.as_millis();

    if total_ms < 1000 {
        format!("{}ms", total_ms)
    } else if total_ms < 60_000 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        let minutes = total_ms / 60_000;
        let remaining_seconds = (total_ms % 60_000) as f64 / 1000.0;
        format!("{}m {:.1}s", minutes, remaining_seconds)
    }
}

/// Format the timing footer with optional token usage info
pub fn format_timing_footer(
    elapsed: Duration,
    ttft: Duration,
    turn_tokens: Option<u32>,
    context_percentage: f32,
) -> String {
    let timing = format!(
        "⏱️ {} | 💭 {}",
        format_duration(elapsed),
        format_duration(ttft)
    );

    // Add token usage info if available (dimmed)
    if let Some(tokens) = turn_tokens {
        format!(
            "{}  \x1b[2m{} ◉ | {:.0}%\x1b[0m",
            timing, tokens, context_percentage
        )
    } else {
        format!("{}  \x1b[2m{:.0}%\x1b[0m", timing, context_percentage)
    }
}

/// Log detailed error information when stream produces no content
pub fn log_stream_error(
    iteration_count: usize,
    provider_name: &str,
    provider_model: &str,
    chunks_received: usize,
    parser: &StreamingToolParser,
    request: &CompletionRequest,
    context_window: &ContextWindow,
    session_id: Option<&str>,
    raw_chunks: &[String],
) {
    error!("=== STREAM ERROR: No content or tool calls received ===");
    error!("Iteration: {}/{}", iteration_count, MAX_ITERATIONS);
    error!("Provider: {} (model: {})", provider_name, provider_model);
    error!("Chunks received: {}", chunks_received);
    
    error!("Parser state:");
    error!("  - Text buffer length: {}", parser.text_buffer_len());
    error!("  - Text buffer content: {:?}", parser.get_text_content());
    error!("  - Has incomplete tool call: {}", parser.has_incomplete_tool_call());
    error!("  - Message stopped: {}", parser.is_message_stopped());
    error!("  - In JSON tool call: {}", parser.is_in_json_tool_call());
    error!("  - JSON tool start: {:?}", parser.json_tool_start_position());
    
    error!("Request details:");
    error!("  - Messages count: {}", request.messages.len());
    error!("  - Has tools: {}", request.tools.is_some());
    error!("  - Max tokens: {:?}", request.max_tokens);
    error!("  - Temperature: {:?}", request.temperature);
    error!("  - Stream: {}", request.stream);

    error!("Raw chunks received ({} total):", chunks_received);
    for (i, chunk_str) in raw_chunks.iter().take(25).enumerate() {
        error!("  [{}] {}", i, chunk_str);
    }

    match serde_json::to_string_pretty(request) {
        Ok(json) => {
            error!("(turn on DEBUG logging for the raw JSON request)");
            debug!("Full request JSON:\n{}", json);
        }
        Err(e) => error!("Failed to serialize request: {}", e),
    }

    if let Some(last_user_msg) = request
        .messages
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::User))
    {
        let truncated = if last_user_msg.content.len() > 500 {
            format!("{}... (truncated)", &last_user_msg.content[..500])
        } else {
            last_user_msg.content.clone()
        };
        error!("Last user message: {}", truncated);
    }

    error!("Context window state:");
    error!(
        "  - Used tokens: {}/{}",
        context_window.used_tokens, context_window.total_tokens
    );
    error!("  - Percentage used: {:.1}%", context_window.percentage_used());
    error!(
        "  - Conversation history length: {}",
        context_window.conversation_history.len()
    );

    error!("Session ID: {:?}", session_id);
    error!("=== END STREAM ERROR ===");
}

/// Truncate a string value for display, respecting UTF-8 boundaries
pub fn truncate_for_display(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.char_indices().take(max_len).map(|(_, c)| c).collect();
        format!("{}...", truncated)
    }
}

/// Truncate a line for tool output display
pub fn truncate_line(line: &str, max_width: usize, should_truncate: bool) -> String {
    if !should_truncate || line.chars().count() <= max_width {
        line.to_string()
    } else {
        let truncated: String = line.chars().take(max_width.saturating_sub(3)).collect();
        format!("{truncated}...")
    }
}

/// Check if two tool calls are duplicates (same tool and args)
pub fn are_tool_calls_duplicate(tc1: &ToolCall, tc2: &ToolCall) -> bool {
    tc1.tool == tc2.tool && tc1.args == tc2.args
}

/// Format a tool argument value for display (truncated for readability).
/// Special handling for shell commands (show first line only if multiline).
pub fn format_tool_arg_value(tool_name: &str, key: &str, value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => {
            if tool_name == "shell" && key == "command" {
                // For shell commands, show first line only if multiline
                match s.lines().next() {
                    Some(first_line) if s.lines().count() > 1 => format!("{}...", first_line),
                    Some(first_line) => first_line.to_string(),
                    None => s.clone(),
                }
            } else if s.chars().count() > 100 {
                truncate_for_display(s, 100)
            } else {
                s.clone()
            }
        }
        _ => value.to_string(),
    }
}

/// Format tool output lines for display, respecting machine vs human mode.
pub fn format_tool_output_summary(output: &str, max_lines: usize, max_width: usize, wants_full: bool) -> Vec<String> {
    let lines: Vec<&str> = output.lines().collect();
    let limit = if wants_full { lines.len() } else { max_lines };
    
    lines
        .iter()
        .take(limit)
        .map(|line| truncate_line(line, max_width, !wants_full))
        .collect()
}

/// Format a read_file result summary (e.g., "✅ read 42 lines | 1.2k chars").
pub fn format_read_file_summary(line_count: usize, char_count: usize) -> String {
    let char_display = if char_count >= 1000 {
        format!("{:.1}k", char_count as f64 / 1000.0)
    } else {
        format!("{}", char_count)
    };
    format!("{} lines ({} chars)", line_count, char_display)
}

/// Format a write_file result summary.
pub fn format_write_file_summary(line_count: usize, char_count: usize) -> String {
    let char_display = if char_count >= 1000 {
        format!("{:.1}k", char_count as f64 / 1000.0)
    } else {
        format!("{}", char_count)
    };
    format!("✏️  {} lines ({} chars)", line_count, char_display)
}

/// Format a write_file result for compact display.
/// Parses the tool result which is in format: "✅ wrote N lines | M chars"
/// Returns a compact summary like "✏️  N lines (M chars)"
pub fn format_write_file_result(tool_result: &str) -> String {
    // Parse "✅ wrote N lines | M chars" or "✅ wrote N lines | M.Mk chars"
    if let Some(rest) = tool_result.strip_prefix("✅ wrote ") {
        // rest is "N lines | M chars" or "N lines | M.Mk chars"
        if let Some((lines_part, chars_part)) = rest.split_once(" | ") {
            let lines = lines_part.trim_end_matches(" lines");
            let chars = chars_part.trim_end_matches(" chars");
            return format!("✏️  {} lines ({} chars)", lines, chars);
        }
    }
    // Fallback: return the original result if parsing fails
    tool_result.to_string()
}

/// Format a str_replace result summary.
pub fn format_str_replace_summary(insertions: i32, deletions: i32) -> String {
    if insertions > 0 && deletions > 0 {
        format!("\x1b[32m+{}\x1b[0m \x1b[2m|\x1b[0m \x1b[31m-{}\x1b[0m", insertions, deletions)
    } else if insertions > 0 {
        format!("\x1b[32m+{}\x1b[0m", insertions)
    } else {
        format!("\x1b[31m-{}\x1b[0m", deletions)
    }
}

/// Format a remember tool result summary.
pub fn format_remember_summary(result: &str) -> String {
    // Result format: "Memory updated. Size: 1.2k" or similar
    if let Some(size_pos) = result.find("Size: ") {
        let size_str = &result[size_pos + 6..];
        let size = size_str.split_whitespace().next().unwrap_or("?");
        format!("📝 memory updated ({})", size)
    } else {
        "📝 memory updated".to_string()
    }
}

/// Format a take_screenshot result summary.
pub fn format_screenshot_summary(result: &str) -> String {
    // Result format: "✅ Screenshot of X saved to: /path/to/file.png"
    if let Some(path_pos) = result.find("saved to: ") {
        let path = &result[path_pos + 10..].trim();
        format!("📸 {}", path)
    } else if result.contains("❌") {
        "❌ failed".to_string()
    } else {
        "📸 saved".to_string()
    }
}

/// Format a code_coverage result summary.
pub fn format_coverage_summary(result: &str) -> String {
    // Try to extract coverage percentage from result
    if result.contains("❌") {
        "❌ failed".to_string()
    } else {
        "📊 report generated".to_string()
    }
}

/// Format a rehydrate result summary.
pub fn format_rehydrate_summary(result: &str) -> String {
    // Result format: "✅ Rehydrated fragment 'abc123' (47 messages, ~18500 tokens)"
    if let Some(start) = result.find("fragment '") {
        let after = &result[start + 10..];
        if let Some(end) = after.find("'") {
            let fragment_id = &after[..end];
            return format!("🔄 restored '{}'", fragment_id);
        }
    }
    if result.contains("❌") {
        "❌ failed".to_string()
    } else {
        "🔄 restored".to_string()
    }
}

/// Determine if a response is essentially empty (whitespace or timing only)
pub fn is_empty_response(response: &str) -> bool {
    response.trim().is_empty()
        || response
            .lines()
            .all(|line| line.trim().is_empty() || line.trim().starts_with("⏱️"))
}

/// Check if an error is a recoverable connection error
pub fn is_connection_error(error_msg: &str) -> bool {
    error_msg.contains("unexpected EOF")
        || error_msg.contains("connection")
        || error_msg.contains("chunk size line")
        || error_msg.contains("body error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_llm_tokens() {
        assert_eq!(clean_llm_tokens("hello<|im_end|>"), "hello");
        assert_eq!(clean_llm_tokens("test</s>more"), "testmore");
        assert_eq!(clean_llm_tokens("[/INST]response"), "response");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30.0s");
    }

    #[test]
    fn test_truncate_for_display() {
        assert_eq!(truncate_for_display("short", 10), "short");
        assert_eq!(truncate_for_display("this is long", 5), "this ...");
    }

    #[test]
    fn test_is_empty_response() {
        assert!(is_empty_response(""));
        assert!(is_empty_response("   \n  "));
        assert!(is_empty_response("⏱️ 1.5s"));
        assert!(!is_empty_response("actual content"));
    }

    #[test]
    fn test_is_connection_error() {
        assert!(is_connection_error("unexpected EOF during read"));
        assert!(is_connection_error("connection reset"));
        assert!(!is_connection_error("invalid JSON"));
    }

    #[test]
    fn test_format_tool_arg_value_shell_command() {
        let val = serde_json::json!("echo hello\necho world");
        assert_eq!(format_tool_arg_value("shell", "command", &val), "echo hello...");
        
        let single_line = serde_json::json!("ls -la");
        assert_eq!(format_tool_arg_value("shell", "command", &single_line), "ls -la");
    }

    #[test]
    fn test_format_tool_arg_value_truncation() {
        let long_str = "a".repeat(150);
        let val = serde_json::json!(long_str);
        let result = format_tool_arg_value("read_file", "path", &val);
        assert!(result.len() < 110); // 100 chars + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_format_read_file_summary() {
        assert_eq!(format_read_file_summary(42, 500), "42 lines (500 chars)");
        assert_eq!(format_read_file_summary(100, 1500), "100 lines (1.5k chars)");
    }

    #[test]
    fn test_format_tool_output_summary() {
        let output = "line1\nline2\nline3\nline4\nline5\nline6";
        let lines = format_tool_output_summary(output, 3, 80, false);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "line1");
    }
}
