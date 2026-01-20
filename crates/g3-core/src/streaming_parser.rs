//! Streaming tool parser for processing LLM response chunks.
//!
//! This module handles parsing of tool calls from streaming LLM responses,
//! supporting both native tool calls and JSON-based fallback parsing.
//!
//! **Important**: JSON tool calls are only recognized when they appear on their
//! own line (preceded by a newline or at the start of the buffer). This prevents
//! inline JSON examples in prose from being incorrectly parsed as tool calls.

use tracing::debug;

use crate::ToolCall;

/// Patterns used to detect JSON tool calls in text.
/// These cover common whitespace variations in JSON formatting.
const TOOL_CALL_PATTERNS: [&str; 4] = [
    r#"{"tool":"#,
    r#"{ "tool":"#,
    r#"{"tool" :"#,
    r#"{ "tool" :"#,
];

// ============================================================================
// Code Fence Tracking
// ============================================================================

/// Tracks whether we're inside a markdown code fence (``` block).
///
/// Used during streaming to avoid parsing JSON examples inside code blocks
/// as tool calls.
#[derive(Debug, Default)]
struct CodeFenceTracker {
    /// Whether we're currently inside a code fence
    in_fence: bool,
    /// Buffer for the current incomplete line (text since last newline)
    current_line: String,
}

impl CodeFenceTracker {
    fn new() -> Self {
        Self::default()
    }

    /// Update fence state based on new streaming content.
    fn process(&mut self, content: &str) {
        for ch in content.chars() {
            if ch == '\n' {
                self.check_and_toggle_fence();
                self.current_line.clear();
            } else {
                self.current_line.push(ch);
            }
        }
    }

    /// Check if current_line is a code fence marker and toggle state if so.
    fn check_and_toggle_fence(&mut self) {
        let trimmed = self.current_line.trim_start();
        if trimmed.starts_with("```") && trimmed.chars().take_while(|&c| c == '`').count() >= 3 {
            self.in_fence = !self.in_fence;
            debug!(
                "Code fence toggled: in_fence={} (line: {:?})",
                self.in_fence, self.current_line
            );
        }
    }

    fn is_in_fence(&self) -> bool {
        self.in_fence
    }

    fn reset(&mut self) {
        self.in_fence = false;
        self.current_line.clear();
    }
}

/// Find all code fence ranges in text (for batch processing).
///
/// Returns a vector of (start, end) byte positions where code fences are.
/// Each range represents content INSIDE a fence (between ``` markers).
fn find_code_fence_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut in_fence = false;
    let mut fence_start = 0;
    let mut line_start = 0;

    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            let line = &text[line_start..i];
            let trimmed = line.trim_start();

            if trimmed.starts_with("```")
                && trimmed.chars().take_while(|&c| c == '`').count() >= 3
            {
                if in_fence {
                    ranges.push((fence_start, line_start));
                    in_fence = false;
                } else {
                    fence_start = i + 1; // +1 to skip the newline
                    in_fence = true;
                }
            }
            line_start = i + 1;
        }
    }

    // If we ended while still in a fence, include everything to the end
    if in_fence {
        ranges.push((fence_start, text.len()));
    }

    ranges
}

fn is_position_in_fence_ranges(pos: usize, ranges: &[(usize, usize)]) -> bool {
    ranges.iter().any(|(start, end)| pos >= *start && pos < *end)
}

// ============================================================================
// JSON Parsing Utilities
// ============================================================================

/// Find the end position (byte index) of a complete JSON object in the text.
/// Returns None if no complete JSON object is found.
fn find_json_object_end(text: &str) -> Option<usize> {
    let mut brace_count = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut found_start = false;

    for (i, ch) in text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => {
                brace_count += 1;
                found_start = true;
            }
            '}' if !in_string => {
                brace_count -= 1;
                if brace_count == 0 && found_start {
                    return Some(i);
                }
            }
            _ => {}
        }
    }

    None
}

/// Check if a partial JSON tool call has been invalidated by subsequent content.
///
/// Detects two invalidation cases:
/// 1. Unescaped newline inside a JSON string (invalid JSON)
/// 2. Newline followed by non-JSON prose (e.g., regular text, not `"`, `{`, `}`, etc.)
fn is_json_invalidated(json_text: &str) -> bool {
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = json_text.char_indices().peekable();

    while let Some((_, ch)) = chars.next() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => escape_next = true,
            '"' => in_string = !in_string,
            '\n' if in_string => return true, // Unescaped newline in string = invalid
            '\n' if !in_string => {
                // Skip whitespace after newline
                while let Some(&(_, next_ch)) = chars.peek() {
                    if next_ch != ' ' && next_ch != '\t' {
                        break;
                    }
                    chars.next();
                }

                // Check if next char is valid JSON continuation
                if let Some(&(_, next_ch)) = chars.peek() {
                    let valid_json_char = matches!(
                        next_ch,
                        '"' | '{' | '}' | '[' | ']' | ':' | ',' | '-' | '0'..='9' | 't' | 'f' | 'n' | '\n'
                    );
                    if !valid_json_char {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }

    false
}

/// Detect malformed tool calls where LLM prose leaked into JSON keys.
///
/// When the LLM "stutters" or mixes formats, it sometimes emits JSON where
/// the keys are actually fragments of conversational text rather than valid
/// parameter names.
fn args_contain_prose_fragments(args: &serde_json::Map<String, serde_json::Value>) -> bool {
    const PROSE_MARKERS: &[&str] = &[
        "I'll", "Let me", "Here's", "I can", "I need", "First", "Now", "The ",
    ];

    args.keys().any(|key| {
        key.len() > 100
            || key.contains('\n')
            || PROSE_MARKERS.iter().any(|marker| key.contains(marker))
    })
}

// ============================================================================
// Tool Call Pattern Matching
// ============================================================================

/// Check if a position in text is "on its own line" - meaning it's either
/// at the start of the text, or preceded by a newline with only whitespace
/// between the newline and the position.
fn is_on_own_line(text: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let line_start = text[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
    text[line_start..pos].chars().all(|c| c.is_whitespace())
}

/// Find the first tool call pattern that appears on its own line.
fn find_first_tool_call_start(text: &str) -> Option<usize> {
    find_tool_call_start(text, false)
}

/// Find the last tool call pattern that appears on its own line.
fn find_last_tool_call_start(text: &str) -> Option<usize> {
    find_tool_call_start(text, true)
}

/// Find a tool call pattern in text, optionally searching backwards.
/// Only matches patterns on their own line (at start or after newline + whitespace).
fn find_tool_call_start(text: &str, find_last: bool) -> Option<usize> {
    let mut best_pos: Option<usize> = None;

    for pattern in &TOOL_CALL_PATTERNS {
        if find_last {
            // Search backwards
            let mut search_end = text.len();
            while search_end > 0 {
                if let Some(pos) = text[..search_end].rfind(pattern) {
                    if is_on_own_line(text, pos) {
                        if best_pos.map_or(true, |best| pos > best) {
                            best_pos = Some(pos);
                        }
                        break;
                    }
                    search_end = pos;
                } else {
                    break;
                }
            }
        } else {
            // Search forwards
            let mut search_start = 0;
            while search_start < text.len() {
                if let Some(rel) = text[search_start..].find(pattern) {
                    let pos = search_start + rel;
                    if is_on_own_line(text, pos) {
                        if best_pos.map_or(true, |best| pos < best) {
                            best_pos = Some(pos);
                        }
                        break;
                    }
                    search_start = pos + 1;
                } else {
                    break;
                }
            }
        }
    }

    best_pos
}

// ============================================================================
// StreamingToolParser
// ============================================================================

/// Modern streaming tool parser that properly handles native tool calls and SSE chunks.
#[derive(Debug)]
pub struct StreamingToolParser {
    /// Buffer for accumulating text content
    text_buffer: String,
    /// Position in text_buffer up to which tool calls have been consumed/executed.
    last_consumed_position: usize,
    /// Whether we've received a message_stop event
    message_stopped: bool,
    /// Whether we're currently in a JSON tool call (for fallback parsing)
    in_json_tool_call: bool,
    /// Start position of JSON tool call (for fallback parsing)
    json_tool_start: Option<usize>,
    /// Tracks code fence state during streaming
    fence_tracker: CodeFenceTracker,
}

impl Default for StreamingToolParser {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingToolParser {
    pub fn new() -> Self {
        Self {
            text_buffer: String::new(),
            last_consumed_position: 0,
            message_stopped: false,
            in_json_tool_call: false,
            json_tool_start: None,
            fence_tracker: CodeFenceTracker::new(),
        }
    }

    /// Process a streaming chunk and return completed tool calls if any.
    pub fn process_chunk(&mut self, chunk: &g3_providers::CompletionChunk) -> Vec<ToolCall> {
        let mut completed_tools = Vec::new();

        // Add text content to buffer and track code fence state
        if !chunk.content.is_empty() {
            self.fence_tracker.process(&chunk.content);
            self.text_buffer.push_str(&chunk.content);
        }

        // Handle native tool calls - return them immediately when received
        if let Some(ref tool_calls) = chunk.tool_calls {
            debug!("Received native tool calls: {:?}", tool_calls);
            for tool_call in tool_calls {
                completed_tools.push(ToolCall {
                    tool: tool_call.tool.clone(),
                    args: tool_call.args.clone(),
                });
            }
        }

        // Check if message is finished/stopped
        if chunk.finished {
            self.message_stopped = true;
            debug!("Message finished, processing accumulated tool calls");

            // When stream finishes, find ALL JSON tool calls in the accumulated buffer
            if completed_tools.is_empty() && !self.text_buffer.is_empty() {
                let all_tools = self.parse_all_json_tool_calls();
                if !all_tools.is_empty() {
                    debug!(
                        "Found {} JSON tool calls in buffer at stream end",
                        all_tools.len()
                    );
                    completed_tools.extend(all_tools);
                }
            }
        }

        // Fallback: Try to parse JSON tool calls from current chunk content if no native tool calls.
        // Skip when inside a code fence to prevent false positives from JSON examples.
        if completed_tools.is_empty()
            && !chunk.content.is_empty()
            && !chunk.finished
            && !self.fence_tracker.is_in_fence()
        {
            if let Some(json_tool) = self.try_parse_streaming_json_tool_call() {
                completed_tools.push(json_tool);
            }
        }

        completed_tools
    }

    /// Try to parse a JSON tool call from the streaming buffer.
    ///
    /// Maintains state (`in_json_tool_call`, `json_tool_start`) to track
    /// partial JSON tool calls across streaming chunks.
    fn try_parse_streaming_json_tool_call(&mut self) -> Option<ToolCall> {
        let fence_ranges = find_code_fence_ranges(&self.text_buffer);

        // If not currently in a JSON tool call, look for the start
        if !self.in_json_tool_call {
            let unchecked_buffer = &self.text_buffer[self.last_consumed_position..];
            if let Some(relative_pos) = find_first_tool_call_start(unchecked_buffer) {
                let pos = self.last_consumed_position + relative_pos;

                // Skip if inside a code fence
                if is_position_in_fence_ranges(pos, &fence_ranges) {
                    debug!("Skipping tool call at position {} - inside code fence", pos);
                    return None;
                }

                debug!(
                    "Found JSON tool call pattern at position {} (relative: {})",
                    pos, relative_pos
                );
                self.in_json_tool_call = true;
                self.json_tool_start = Some(pos);
            }
        }

        // If in a JSON tool call, try to find the end and parse it
        if self.in_json_tool_call {
            if let Some(start_pos) = self.json_tool_start {
                let json_text = &self.text_buffer[start_pos..];

                // Try to find a complete JSON object
                if let Some(end_pos) = find_json_object_end(json_text) {
                    let json_str = &json_text[..=end_pos];
                    debug!("Attempting to parse JSON tool call: {}", json_str);

                    if let Some(tool_call) = self.try_parse_tool_call_json(json_str) {
                        self.in_json_tool_call = false;
                        self.json_tool_start = None;
                        return Some(tool_call);
                    }

                    // Parse failed, reset and continue looking
                    self.in_json_tool_call = false;
                    self.json_tool_start = None;
                }

                // Check if the partial JSON has been invalidated
                if self.in_json_tool_call && is_json_invalidated(json_text) {
                    debug!("JSON tool call invalidated by subsequent content, clearing state");
                    self.in_json_tool_call = false;
                    self.json_tool_start = None;
                    self.last_consumed_position = start_pos + json_text.len();
                    return None;
                }
            }
        }

        None
    }

    /// Parse ALL JSON tool calls from the accumulated text buffer.
    fn parse_all_json_tool_calls(&self) -> Vec<ToolCall> {
        let mut tool_calls = Vec::new();
        let mut search_start = 0;
        let fence_ranges = find_code_fence_ranges(&self.text_buffer);

        while search_start < self.text_buffer.len() {
            let search_text = &self.text_buffer[search_start..];

            let Some(relative_pos) = find_first_tool_call_start(search_text) else {
                break;
            };

            let abs_start = search_start + relative_pos;
            let json_text = &self.text_buffer[abs_start..];

            // Skip if inside a code fence
            if is_position_in_fence_ranges(abs_start, &fence_ranges) {
                search_start = abs_start + 1;
                continue;
            }

            // Try to find a complete JSON object
            let Some(end_pos) = find_json_object_end(json_text) else {
                break; // Incomplete JSON, stop searching
            };

            let json_str = &json_text[..=end_pos];
            if let Some(tool_call) = self.try_parse_tool_call_json(json_str) {
                debug!("Found tool call at position {}: {:?}", abs_start, tool_call.tool);
                tool_calls.push(tool_call);
            }

            search_start = abs_start + end_pos + 1;
        }

        tool_calls
    }

    /// Try to parse a JSON string as a ToolCall, validating the args.
    fn try_parse_tool_call_json(&self, json_str: &str) -> Option<ToolCall> {
        let tool_call: ToolCall = serde_json::from_str(json_str).ok()?;

        // Validate that args is an object with reasonable keys
        let args_obj = tool_call.args.as_object()?;
        if args_contain_prose_fragments(args_obj) {
            debug!("Detected malformed tool call with message-like keys, skipping");
            return None;
        }

        debug!("Successfully parsed valid JSON tool call: {:?}", tool_call);
        Some(tool_call)
    }

    // ========================================================================
    // Public Accessors
    // ========================================================================

    /// Get the accumulated text content.
    pub fn get_text_content(&self) -> &str {
        &self.text_buffer
    }

    /// Get content before a specific position (for display purposes).
    pub fn get_content_before_position(&self, pos: usize) -> String {
        if pos <= self.text_buffer.len() {
            self.text_buffer[..pos].to_string()
        } else {
            self.text_buffer.clone()
        }
    }

    /// Check if the message has been stopped/finished.
    pub fn is_message_stopped(&self) -> bool {
        self.message_stopped
    }

    /// Check if the text buffer contains an incomplete JSON tool call.
    pub fn has_incomplete_tool_call(&self) -> bool {
        let unchecked_buffer = &self.text_buffer[self.last_consumed_position..];
        let Some(start_pos) = find_last_tool_call_start(unchecked_buffer) else {
            return false;
        };

        let json_text = &unchecked_buffer[start_pos..];

        // Complete or invalidated = not incomplete
        if find_json_object_end(json_text).is_some() || is_json_invalidated(json_text) {
            return false;
        }

        true
    }

    /// Check if the text buffer contains an unexecuted tool call.
    pub fn has_unexecuted_tool_call(&self) -> bool {
        let unchecked_buffer = &self.text_buffer[self.last_consumed_position..];
        let Some(start_pos) = find_last_tool_call_start(unchecked_buffer) else {
            return false;
        };

        let json_text = &unchecked_buffer[start_pos..];
        let Some(json_end) = find_json_object_end(json_text) else {
            return false;
        };

        let json_only = &json_text[..=json_end];
        serde_json::from_str::<serde_json::Value>(json_only).is_ok()
    }

    /// Mark all tool calls up to the current buffer position as consumed/executed.
    pub fn mark_tool_calls_consumed(&mut self) {
        self.last_consumed_position = self.text_buffer.len();
    }

    /// Get the current text buffer length (for position tracking).
    pub fn text_buffer_len(&self) -> usize {
        self.text_buffer.len()
    }

    /// Check if currently parsing a JSON tool call (for debugging).
    pub fn is_in_json_tool_call(&self) -> bool {
        self.in_json_tool_call
    }

    /// Get the JSON tool start position (for debugging).
    pub fn json_tool_start_position(&self) -> Option<usize> {
        self.json_tool_start
    }

    /// Reset the parser state for a new message.
    pub fn reset(&mut self) {
        self.text_buffer.clear();
        self.last_consumed_position = 0;
        self.message_stopped = false;
        self.in_json_tool_call = false;
        self.json_tool_start = None;
        self.fence_tracker.reset();
    }

    // ========================================================================
    // Static Methods (for external use)
    // ========================================================================

    /// Find the starting position of the FIRST tool call pattern on its own line.
    pub fn find_first_tool_call_start(text: &str) -> Option<usize> {
        find_first_tool_call_start(text)
    }

    /// Find the starting position of the LAST tool call pattern on its own line.
    pub fn find_last_tool_call_start(text: &str) -> Option<usize> {
        find_last_tool_call_start(text)
    }

    /// Check if a position in text is "on its own line".
    pub fn is_on_own_line(text: &str, pos: usize) -> bool {
        is_on_own_line(text, pos)
    }

    /// Find the end position of a complete JSON object.
    pub fn find_complete_json_object_end(text: &str) -> Option<usize> {
        find_json_object_end(text)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_complete_json_object_end_simple() {
        let text = r#"{"tool":"shell","args":{"command":"ls"}}"#;
        assert_eq!(find_json_object_end(text), Some(text.len() - 1));
    }

    #[test]
    fn test_find_complete_json_object_end_nested() {
        let text = r#"{"tool":"write","args":{"content":"{nested}"}}"#;
        assert_eq!(find_json_object_end(text), Some(text.len() - 1));
    }

    #[test]
    fn test_find_complete_json_object_end_incomplete() {
        let text = r#"{"tool":"shell","args":{"command":"ls""#;
        assert_eq!(find_json_object_end(text), None);
    }

    #[test]
    fn test_tool_call_patterns() {
        assert!(find_first_tool_call_start(r#"{"tool":"test"}"#).is_some());
        assert!(find_first_tool_call_start(r#"{ "tool":"test"}"#).is_some());
        assert!(find_first_tool_call_start(r#"{"tool" :"test"}"#).is_some());
        assert!(find_first_tool_call_start(r#"{ "tool" :"test"}"#).is_some());
    }

    #[test]
    fn test_parser_reset() {
        let mut parser = StreamingToolParser::new();
        parser.text_buffer = "some content".to_string();
        parser.message_stopped = true;
        parser.last_consumed_position = 5;

        parser.reset();

        assert!(parser.text_buffer.is_empty());
        assert!(!parser.message_stopped);
        assert_eq!(parser.last_consumed_position, 0);
    }

    #[test]
    fn test_multiple_tool_calls_processed_in_order() {
        let mut parser = StreamingToolParser::new();

        let content = r#"Some text before

{"tool": "shell", "args": {"command": "first"}}

{"tool": "shell", "args": {"command": "second"}}

Some text after"#;

        let chunk = g3_providers::CompletionChunk {
            content: content.to_string(),
            finished: true,
            tool_calls: None,
            usage: None,
            stop_reason: None,
            tool_call_streaming: None,
        };

        let tools = parser.process_chunk(&chunk);

        assert_eq!(tools.len(), 2, "Expected 2 tool calls, got {}", tools.len());
        assert_eq!(tools[0].args["command"], "first");
        assert_eq!(tools[1].args["command"], "second");
    }

    #[test]
    fn test_find_first_vs_last_tool_call() {
        let text = "{\"tool\": \"first\"}\n{\"tool\": \"second\"}";

        let first_pos = find_first_tool_call_start(text);
        let last_pos = find_last_tool_call_start(text);

        assert!(first_pos.is_some());
        assert!(last_pos.is_some());
        assert!(first_pos.unwrap() < last_pos.unwrap());
    }

    #[test]
    fn test_inline_tool_call_ignored() {
        let text = "Here is an example: {\"tool\": \"shell\"} in text";
        assert!(find_first_tool_call_start(text).is_none());
        assert!(find_last_tool_call_start(text).is_none());
    }

    #[test]
    fn test_standalone_tool_call_detected() {
        let text = r#"{"tool": "shell", "args": {"command": "ls"}}"#;
        assert!(find_first_tool_call_start(text).is_some());
    }

    #[test]
    fn test_indented_tool_call_detected() {
        let text = r#"  {"tool": "shell", "args": {"command": "ls"}}"#;
        assert!(find_first_tool_call_start(text).is_some());
    }

    #[test]
    fn test_tool_call_after_newline_detected() {
        let text = "Some prose here\n{\"tool\": \"shell\", \"args\": {}}";
        let pos = find_first_tool_call_start(text);
        assert!(pos.is_some());
        assert_eq!(pos.unwrap(), 16);
    }

    #[test]
    fn test_inline_ignored_but_standalone_detected() {
        let text = "Some text with {\"tool\": \"inline\"} here\n{\"tool\": \"standalone\", \"args\": {}}";
        let pos = find_first_tool_call_start(text);
        assert!(pos.is_some());
        assert!(pos.unwrap() > 30);
    }

    #[test]
    fn test_multiple_inline_patterns_all_ignored() {
        let text = "Compare {\"tool\": \"a\"} with {\"tool\": \"b\"}";
        assert!(find_first_tool_call_start(text).is_none());
    }

    #[test]
    fn test_is_on_own_line() {
        let text = "prefix {\"tool\":\n          {\"tool\":";

        assert!(is_on_own_line(text, 0));
        assert!(!is_on_own_line(text, 7));

        let newline_pos = text.find('\n').unwrap();
        assert!(is_on_own_line(text, newline_pos + 11));
    }

    #[test]
    fn test_all_pattern_variants_require_own_line() {
        let patterns = [
            "text { \"tool\":\"x\"}",
            "text {\"tool\" :\"x\"}",
            "text { \"tool\" :\"x\"}",
        ];
        for pattern in patterns {
            assert!(
                find_first_tool_call_start(pattern).is_none(),
                "Inline pattern '{}' should be ignored",
                pattern
            );
        }
    }

    #[test]
    fn test_find_code_fence_ranges_simple() {
        let text = "Before\n```\ncode\n```\nAfter";
        let ranges = find_code_fence_ranges(text);

        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        let inside = &text[start..end];
        assert!(inside.contains("code"));
    }

    #[test]
    fn test_find_code_fence_ranges_multiple() {
        let text = "First:\n```json\ncode1\n```\n\nSecond:\n```\ncode2\n```\nEnd";
        let ranges = find_code_fence_ranges(text);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn test_find_code_fence_ranges_with_tool_json() {
        let text = "Example:\n```json\n{\"tool\": \"shell\", \"args\": {}}\n```\nDone.";
        let ranges = find_code_fence_ranges(text);

        assert_eq!(ranges.len(), 1);

        let tool_pos = text.find("{\"tool\"").unwrap();
        assert!(is_position_in_fence_ranges(tool_pos, &ranges));
    }

    #[test]
    fn test_is_position_in_fence_ranges() {
        let ranges = vec![(10, 20), (30, 40)];
        assert!(!is_position_in_fence_ranges(5, &ranges));
        assert!(is_position_in_fence_ranges(15, &ranges));
        assert!(!is_position_in_fence_ranges(25, &ranges));
        assert!(is_position_in_fence_ranges(35, &ranges));
    }
}
