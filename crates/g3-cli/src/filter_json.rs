//! JSON tool call filtering for streaming LLM responses.
//!
//! This module filters out JSON tool calls from LLM output streams while preserving
//! regular text content. It uses a simple state machine optimized for streaming.
//!
//! # Design
//!
//! The filter uses three states:
//! - **Streaming**: Normal pass-through mode. Watches for newline + whitespace + `{`
//! - **Buffering**: Saw potential tool call start, buffering to confirm/deny
//! - **Suppressing**: Confirmed tool call, counting braces (string-aware) to find end
//!
//! The key insight is that we only need to buffer a small amount (around 12 chars)
//! to confirm whether `{` starts a tool call pattern like `{"tool":`.

use std::cell::RefCell;
use tracing::debug;

/// Maximum chars needed to confirm/deny a tool call pattern.
/// Pattern is: { + optional whitespace + "tool" + optional whitespace + : + optional whitespace + "
/// Realistically: `{"tool":"` = 9 chars, with whitespace maybe 15 max
const MAX_BUFFER_FOR_DETECTION: usize = 20;

// Thread-local state for tracking JSON tool call suppression
thread_local! {
    static JSON_TOOL_STATE: RefCell<FilterState> = RefCell::new(FilterState::new());
}

/// The three possible states of the filter
#[derive(Debug, Clone, PartialEq)]
enum State {
    /// Normal streaming - pass through content, watch for newline + whitespace + {
    Streaming,
    /// Saw potential start, buffering to confirm/deny tool pattern
    Buffering,
    /// Confirmed tool call, suppressing until braces balance
    Suppressing,
}

/// Internal state for the filter
#[derive(Debug, Clone)]
struct FilterState {
    state: State,
    /// Buffer for potential tool call detection (Buffering state)
    buffer: String,
    /// Brace depth for JSON tracking (Suppressing state) - string-aware
    brace_depth: i32,
    /// Are we inside a JSON string? (for proper brace counting)
    in_string: bool,
    /// Was the previous char a backslash? (for escape handling)
    escape_next: bool,
    /// Track if we just saw a newline (to detect line-start patterns)
    at_line_start: bool,
    /// Whitespace seen after newline (before potential {)
    pending_whitespace: String,
}

impl FilterState {
    fn new() -> Self {
        Self {
            state: State::Streaming,
            buffer: String::new(),
            brace_depth: 0,
            in_string: false,
            escape_next: false,
            at_line_start: true, // Start of input counts as line start
            pending_whitespace: String::new(),
        }
    }

    fn reset(&mut self) {
        self.state = State::Streaming;
        self.buffer.clear();
        self.brace_depth = 0;
        self.in_string = false;
        self.escape_next = false;
        self.at_line_start = true;
        self.pending_whitespace.clear();
    }
}

/// Check if buffer matches the tool call pattern.
/// Pattern: `{` followed by optional whitespace, `"tool"`, optional whitespace, `:`, optional whitespace, `"`
/// 
/// Returns:
/// - Some(true) if confirmed as tool call
/// - Some(false) if confirmed NOT a tool call  
/// - None if need more data
fn check_tool_pattern(buffer: &str) -> Option<bool> {
    // Must start with {
    if !buffer.starts_with('{') {
        return Some(false);
    }
    
    let after_brace = &buffer[1..];
    
    // Skip leading whitespace after {
    let trimmed = after_brace.trim_start();
    
    // Need at least `"tool":"` = 8 chars after whitespace
    if trimmed.len() < 8 {
        // Not enough data yet - but check for early rejection
        if trimmed.starts_with('"') {
            let after_quote = &trimmed[1..];
            // If we have chars after the quote, check if it starts with 't'
            if !after_quote.is_empty() && !after_quote.starts_with('t') {
                return Some(false); // Definitely not "tool
            }
            if after_quote.len() >= 2 && !after_quote.starts_with("to") {
                return Some(false);
            }
            if after_quote.len() >= 3 && !after_quote.starts_with("too") {
                return Some(false);
            }
            if after_quote.len() >= 4 && !after_quote.starts_with("tool") {
                return Some(false);
            }
        } else if !trimmed.is_empty() && !trimmed.starts_with('"') {
            // First non-whitespace char after { is not " - not a tool call
            return Some(false);
        }
        return None; // Need more data
    }
    
    // We have enough data - check the full pattern
    // Must be: "tool" followed by optional whitespace, :, optional whitespace, "
    if !trimmed.starts_with("\"tool\"") {
        return Some(false);
    }
    
    let after_tool = trimmed[6..].trim_start(); // 6 = len of "tool"
    
    if after_tool.is_empty() {
        return None; // Need more data
    }
    
    if !after_tool.starts_with(':') {
        return Some(false);
    }
    
    let after_colon = after_tool[1..].trim_start();
    
    if after_colon.is_empty() {
        return None; // Need more data
    }
    
    if after_colon.starts_with('"') {
        return Some(true); // Confirmed tool call!
    }
    
    Some(false) // Has : but not followed by "
}

/// Filters JSON tool calls from streaming LLM content.
///
/// Processes content character-by-character and removes JSON tool calls 
/// while preserving regular text. Maintains state across calls.
///
/// # Arguments
/// * `content` - A chunk of streaming content from the LLM
///
/// # Returns
/// The filtered content with JSON tool calls removed
pub fn filter_json_tool_calls(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }

    JSON_TOOL_STATE.with(|state| {
        let mut state = state.borrow_mut();
        let mut output = String::new();
        
        for ch in content.chars() {
            match state.state {
                State::Streaming => {
                    handle_streaming_char(&mut state, ch, &mut output);
                }
                State::Buffering => {
                    handle_buffering_char(&mut state, ch, &mut output);
                }
                State::Suppressing => {
                    handle_suppressing_char(&mut state, ch, &mut output);
                }
            }
        }
        
        output
    })
}

/// Handle a character in Streaming state
fn handle_streaming_char(state: &mut FilterState, ch: char, output: &mut String) {
    match ch {
        '\n' => {
            // Output the newline and any pending whitespace
            output.push_str(&state.pending_whitespace);
            output.push(ch);
            state.pending_whitespace.clear();
            state.at_line_start = true;
        }
        ' ' | '\t' if state.at_line_start => {
            // Accumulate whitespace at line start
            state.pending_whitespace.push(ch);
        }
        '{' if state.at_line_start => {
            // Potential tool call! Enter buffering mode
            debug!("Potential tool call detected - entering Buffering state");
            state.state = State::Buffering;
            state.buffer.clear();
            state.buffer.push(ch);
            // Don't output pending_whitespace yet - we might need to suppress it
        }
        _ => {
            // Regular character - output any pending whitespace first
            output.push_str(&state.pending_whitespace);
            state.pending_whitespace.clear();
            output.push(ch);
            state.at_line_start = false;
        }
    }
}

/// Handle a character in Buffering state
fn handle_buffering_char(state: &mut FilterState, ch: char, output: &mut String) {
    state.buffer.push(ch);
    
    // Check if we can determine tool call status
    match check_tool_pattern(&state.buffer) {
        Some(true) => {
            // Confirmed tool call! Enter suppression mode
            debug!("Confirmed tool call - entering Suppressing state");
            state.state = State::Suppressing;
            state.brace_depth = 1; // We already have the opening {
            state.in_string = true; // We're inside the "tool" value string
            state.escape_next = false;
            // Discard pending_whitespace (it's part of the tool call line)
            state.pending_whitespace.clear();
            state.buffer.clear();
        }
        Some(false) => {
            // Not a tool call - release buffered content
            debug!("Not a tool call - releasing buffer");
            output.push_str(&state.pending_whitespace);
            output.push_str(&state.buffer);
            state.pending_whitespace.clear();
            state.buffer.clear();
            state.state = State::Streaming;
            state.at_line_start = ch == '\n';
        }
        None => {
            // Need more data - check if buffer is getting too long
            if state.buffer.len() > MAX_BUFFER_FOR_DETECTION {
                // Too long without confirmation - not a tool call
                debug!("Buffer exceeded max length - not a tool call");
                output.push_str(&state.pending_whitespace);
                output.push_str(&state.buffer);
                state.pending_whitespace.clear();
                state.buffer.clear();
                state.state = State::Streaming;
                state.at_line_start = false;
            }
            // Otherwise keep buffering
        }
    }
}

/// Handle a character in Suppressing state (string-aware brace counting)  
fn handle_suppressing_char(state: &mut FilterState, ch: char, _output: &mut String) {
    // Track chars to detect if we see a new tool call pattern while suppressing
    // This handles truncated JSON followed by complete JSON
    state.buffer.push(ch);
    
    // Handle escape sequences
    if state.escape_next {
        state.escape_next = false;
        return;
    }
    
    match ch {
        '\\' if state.in_string => {
            state.escape_next = true;
        }
        '"' => {
            state.in_string = !state.in_string;
        }
        '{' if !state.in_string => {
            state.brace_depth += 1;
        }
        '}' if !state.in_string => {
            state.brace_depth -= 1;
            if state.brace_depth <= 0 {
                // JSON complete! Return to streaming
                debug!("Tool call complete - returning to Streaming state");
                state.state = State::Streaming;
                state.at_line_start = false; // We're right after the }
                state.in_string = false;
                state.escape_next = false;
                state.buffer.clear();
            }
        }
        _ => {}
    }
    
    // Check if we're seeing a new tool call pattern (truncated JSON case)  
    // This can happen with or without a newline before the new {
    // Look for { followed by tool pattern in the buffer
    if state.buffer.len() >= 10 {
        // Find the last { that could start a new tool call
        for (i, c) in state.buffer.char_indices().rev() {
            if c == '{' && i > 0 {
                let potential_tool = &state.buffer[i..];
                if let Some(true) = check_tool_pattern(potential_tool) {
                    // New tool call detected! Restart suppression from here
                    debug!("New tool call detected while suppressing - restarting");
                    state.brace_depth = 1;
                    state.in_string = true;
                    // Keep only the part after the new { for continued tracking
                    state.buffer = potential_tool.to_string();
                    return;
                }
            }
        }
        
        // Limit buffer size to prevent unbounded growth
        if state.buffer.len() > 200 {
            // Find a valid character boundary near the 100-byte mark from the end
            // We can't just slice at byte offset - multi-byte chars (like emojis) would panic
            let target_keep = state.buffer.len() - 100;
            // Find the nearest char boundary at or after target_keep
            let keep_from = state.buffer.char_indices()
                .map(|(i, _)| i)
                .find(|&i| i >= target_keep)
                .unwrap_or(0);
            state.buffer = state.buffer[keep_from..].to_string();
        }
    }
}

/// Resets the global JSON filtering state.
///
/// Call this between independent filtering sessions to ensure clean state.
/// This is particularly important in tests and when starting new conversations.
pub fn reset_json_tool_state() {
    JSON_TOOL_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.reset();
    });
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_tool_pattern_confirmed() {
        assert_eq!(check_tool_pattern(r#"{"tool":""
"#), Some(true));
        assert_eq!(check_tool_pattern(r#"{"tool": "shell""#), Some(true));
        assert_eq!(check_tool_pattern(r#"{ "tool" : "test""#), Some(true));
    }

    #[test]
    fn test_check_tool_pattern_rejected() {
        assert_eq!(check_tool_pattern(r#"{"other": "value"}"#), Some(false));
        assert_eq!(check_tool_pattern(r#"{"tools": "value"}"#), Some(false));
        assert_eq!(check_tool_pattern(r#"{"tool": 123}"#), Some(false)); // number not string
    }

    #[test]
    fn test_check_tool_pattern_need_more() {
        assert_eq!(check_tool_pattern(r#"{"#), None);
        assert_eq!(check_tool_pattern(r#"{"tool"#), None);
        assert_eq!(check_tool_pattern(r#"{"tool":"#), None);
    }

    #[test]
    fn test_passthrough_no_tool() {
        reset_json_tool_state();
        let input = "Hello world";
        assert_eq!(filter_json_tool_calls(input), input);
    }

    #[test]
    fn test_simple_tool_filtered() {
        reset_json_tool_state();
        let input = "Before\n{\"tool\": \"shell\", \"args\": {}}\nAfter";
        let result = filter_json_tool_calls(input);
        assert_eq!(result, "Before\n\nAfter");
    }

    #[test]
    fn test_tool_with_braces_in_string() {
        reset_json_tool_state();
        let input = "Text\n{\"tool\": \"shell\", \"args\": {\"cmd\": \"echo }\"}}\nMore";
        let result = filter_json_tool_calls(input);
        assert_eq!(result, "Text\n\nMore");
    }

    #[test]
    fn test_non_tool_json_passes_through() {
        reset_json_tool_state();
        let input = "Text\n{\"other\": \"value\"}\nMore";
        let result = filter_json_tool_calls(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_streaming_chunks() {
        reset_json_tool_state();
        let chunks = vec![
            "Before\n",
            "{\"tool\": \"",
            "shell\", \"args\": {}",
            "}\nAfter",
        ];
        let mut result = String::new();
        for chunk in chunks {
            result.push_str(&filter_json_tool_calls(chunk));
        }
        assert_eq!(result, "Before\n\nAfter");
    }

    #[test]
    fn test_buffer_truncation_with_multibyte_chars() {
        // This test ensures that buffer truncation doesn't panic on multi-byte characters
        // The bug was: slicing at byte offset 100 from end could land mid-emoji
        reset_json_tool_state();
        
        // Create a string with emojis that's over 200 bytes to trigger truncation
        // Each emoji is 4 bytes, so we need ~50+ emojis to exceed 200 bytes
        let emoji_heavy = "🔄".repeat(60); // 240 bytes of emojis
        let input = format!("Text\n{{\"tool\": \"shell\", \"args\": {{\"data\": \"{}\"}}}}\nMore", emoji_heavy);
        
        // This should not panic - the fix ensures we find valid char boundaries
        let result = filter_json_tool_calls(&input);
        
        // The tool call should be filtered out
        assert_eq!(result, "Text\n\nMore");
    }
}
