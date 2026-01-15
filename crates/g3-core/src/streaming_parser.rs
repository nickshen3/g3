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

/// Modern streaming tool parser that properly handles native tool calls and SSE chunks.
#[derive(Debug)]
pub struct StreamingToolParser {
    /// Buffer for accumulating text content
    text_buffer: String,
    /// Position in text_buffer up to which tool calls have been consumed/executed.
    /// This prevents has_unexecuted_tool_call() from returning true for already-executed tools.
    last_consumed_position: usize,
    /// Whether we've received a message_stop event
    message_stopped: bool,
    /// Whether we're currently in a JSON tool call (for fallback parsing)
    in_json_tool_call: bool,
    /// Start position of JSON tool call (for fallback parsing)
    json_tool_start: Option<usize>,
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
        }
    }

    /// Find the starting position of the last tool call pattern in the given text,
    /// but ONLY if it appears on its own line (preceded by newline or at start of text,
    /// with only whitespace before the pattern on that line).
    /// Returns None if no valid tool call pattern is found.
    pub fn find_last_tool_call_start(text: &str) -> Option<usize> {
        let mut best_start: Option<usize> = None;
        for pattern in &TOOL_CALL_PATTERNS {
            let mut search_end = text.len();
            while search_end > 0 {
                if let Some(pos) = text[..search_end].rfind(pattern) {
                    // Check if this pattern is on its own line
                    if Self::is_on_own_line(text, pos) {
                        if best_start.map_or(true, |best| pos > best) {
                            best_start = Some(pos);
                        }
                        break; // Found a valid one for this pattern
                    }
                    // Not on its own line, keep searching backwards
                    search_end = pos;
                } else {
                    break;
                }
            }
        }
        best_start
    }

    /// Find the starting position of the FIRST tool call pattern in the given text,
    /// but ONLY if it appears on its own line (preceded by newline or at start of text,
    /// with only whitespace before the pattern on that line).
    /// Returns None if no valid tool call pattern is found.
    pub fn find_first_tool_call_start(text: &str) -> Option<usize> {
        let mut best_start: Option<usize> = None;
        for pattern in &TOOL_CALL_PATTERNS {
            let mut search_start = 0;
            while search_start < text.len() {
                if let Some(relative_pos) = text[search_start..].find(pattern) {
                    let pos = search_start + relative_pos;
                    // Check if this pattern is on its own line
                    if Self::is_on_own_line(text, pos) {
                        if best_start.map_or(true, |best| pos < best) {
                            best_start = Some(pos);
                        }
                        break; // Found a valid one for this pattern
                    }
                    // Not on its own line, keep searching forward
                    search_start = pos + 1;
                } else {
                    break;
                }
            }
        }
        best_start
    }

    /// Check if a position in text is "on its own line" - meaning it's either
    /// at the start of the text, or preceded by a newline with only whitespace
    /// between the newline and the position.
    pub fn is_on_own_line(text: &str, pos: usize) -> bool {
        if pos == 0 {
            return true;
        }
        // Find the start of the current line (position after the last newline before pos)
        let line_start = text[..pos].rfind('\n').map(|p| p + 1).unwrap_or(0);
        // Check if everything between line_start and pos is whitespace
        text[line_start..pos].chars().all(|c| c.is_whitespace())
    }

    /// Detect malformed tool calls where LLM prose leaked into JSON keys.
    ///
    /// When the LLM "stutters" or mixes formats, it sometimes emits JSON where
    /// the keys are actually fragments of conversational text rather than valid
    /// parameter names. This heuristic catches such cases by looking for:
    /// - Unusually long keys (>100 chars)
    /// - Newlines in keys (never valid in JSON keys)
    /// - Common LLM response phrases that indicate prose, not parameters
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

    /// Process a streaming chunk and return completed tool calls if any.
    pub fn process_chunk(&mut self, chunk: &g3_providers::CompletionChunk) -> Vec<ToolCall> {
        let mut completed_tools = Vec::new();

        // Add text content to buffer
        if !chunk.content.is_empty() {
            self.text_buffer.push_str(&chunk.content);
        }

        // Handle native tool calls - return them immediately when received.
        // This allows tools to be executed as soon as they're fully parsed,
        // preventing duplicate tool calls from being accumulated.
        if let Some(ref tool_calls) = chunk.tool_calls {
            debug!("Received native tool calls: {:?}", tool_calls);

            // Convert and return tool calls immediately
            for tool_call in tool_calls {
                let converted_tool = ToolCall {
                    tool: tool_call.tool.clone(),
                    args: tool_call.args.clone(),
                };
                completed_tools.push(converted_tool);
            }
        }

        // Check if message is finished/stopped
        if chunk.finished {
            self.message_stopped = true;
            debug!("Message finished, processing accumulated tool calls");

            // When stream finishes, find ALL JSON tool calls in the accumulated buffer
            if completed_tools.is_empty() && !self.text_buffer.is_empty() {
                let all_tools = self.try_parse_all_json_tool_calls_from_buffer();
                if !all_tools.is_empty() {
                    debug!(
                        "Found {} JSON tool calls in buffer at stream end",
                        all_tools.len()
                    );
                    completed_tools.extend(all_tools);
                }
            }
        }

        // Fallback: Try to parse JSON tool calls from current chunk content if no native tool calls
        if completed_tools.is_empty() && !chunk.content.is_empty() && !chunk.finished {
            if let Some(json_tool) = self.try_parse_json_tool_call(&chunk.content) {
                completed_tools.push(json_tool);
            }
        }

        completed_tools
    }

    /// Fallback method to parse JSON tool calls from text content.
    /// 
    /// This method maintains state (`in_json_tool_call`, `json_tool_start`) to track
    /// partial JSON tool calls across streaming chunks. When a pattern like `{"tool":`
    /// is found on its own line, we enter "in JSON tool call" mode and wait for the
    /// JSON to complete.
    /// 
    /// IMPORTANT: We must also detect when the JSON has been **invalidated** - i.e.,
    /// when subsequent content makes it clear this isn't a real tool call. For example:
    /// - `{"tool": "read_file` followed by `\nsome regular text` is NOT a tool call
    /// - The newline followed by non-JSON text invalidates the partial JSON
    fn try_parse_json_tool_call(&mut self, _content: &str) -> Option<ToolCall> {
        // If we're not currently in a JSON tool call, look for the start
        if !self.in_json_tool_call {
            // Only search in the unconsumed portion of the buffer to avoid
            // re-parsing already-executed tool calls
            let unchecked_buffer = &self.text_buffer[self.last_consumed_position..];
            // Use find_first_tool_call_start to find the FIRST tool call, not the last.
            // This ensures we process tool calls in order when multiple arrive together.
            if let Some(relative_pos) = Self::find_first_tool_call_start(unchecked_buffer) {
                let pos = self.last_consumed_position + relative_pos;
                debug!("Found JSON tool call pattern at position {} (relative: {})", pos, relative_pos);
                self.in_json_tool_call = true;
                self.json_tool_start = Some(pos);
            }
        }

        // If we're in a JSON tool call, try to find the end and parse it
        if self.in_json_tool_call {
            if let Some(start_pos) = self.json_tool_start {
                let json_text = &self.text_buffer[start_pos..];

                // Try to find a complete JSON object
                if let Some(end_pos) = Self::find_complete_json_object_end(json_text) {
                    let json_str = &json_text[..=end_pos];
                    debug!("Attempting to parse JSON tool call: {}", json_str);

                    // Try to parse as a ToolCall
                    if let Ok(tool_call) = serde_json::from_str::<ToolCall>(json_str) {
                        // Validate that args is an object with reasonable keys
                        if let Some(args_obj) = tool_call.args.as_object() {
                            if Self::args_contain_prose_fragments(args_obj) {
                                debug!(
                                    "Detected malformed tool call with message-like keys, skipping"
                                );
                                self.in_json_tool_call = false;
                                self.json_tool_start = None;
                                return None;
                            }

                            debug!("Successfully parsed valid JSON tool call: {:?}", tool_call);
                            self.in_json_tool_call = false;
                            self.json_tool_start = None;
                            return Some(tool_call);
                        }
                        debug!("Tool call args is not an object, skipping");
                    } else {
                        debug!("Failed to parse JSON tool call: {}", json_str);
                    }
                    // Reset and continue looking
                    self.in_json_tool_call = false;
                    self.json_tool_start = None;
                }
                
                // If we didn't find a complete JSON object, check if the partial JSON
                // has been invalidated by subsequent content (e.g., a newline followed
                // by regular text when not inside a string).
                if self.in_json_tool_call && Self::is_json_invalidated(json_text) {
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
    /// This finds all complete tool calls, not just the last one.
    fn try_parse_all_json_tool_calls_from_buffer(&self) -> Vec<ToolCall> {
        let mut tool_calls = Vec::new();
        let mut search_start = 0;

        while search_start < self.text_buffer.len() {
            let search_text = &self.text_buffer[search_start..];

            // Find the next tool call pattern
            if let Some(relative_pos) = Self::find_first_tool_call_start(search_text) {
                let abs_start = search_start + relative_pos;
                let json_text = &self.text_buffer[abs_start..];

                // Try to find a complete JSON object
                if let Some(end_pos) = Self::find_complete_json_object_end(json_text) {
                    let json_str = &json_text[..=end_pos];

                    if let Ok(tool_call) = serde_json::from_str::<ToolCall>(json_str) {
                        if let Some(args_obj) = tool_call.args.as_object() {
                            if !Self::args_contain_prose_fragments(args_obj) {
                                debug!(
                                    "Found tool call at position {}: {:?}",
                                    abs_start, tool_call.tool
                                );
                                tool_calls.push(tool_call);
                            }
                        }
                    }
                    // Move past this tool call
                    search_start = abs_start + end_pos + 1;
                } else {
                    // Incomplete JSON, stop searching
                    break;
                }
            } else {
                // No more tool call patterns found
                break;
            }
        }

        tool_calls
    }

    /// Get the accumulated text content (excluding tool calls).
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
    /// This detects cases where the LLM started emitting a tool call but the stream ended
    /// before the JSON was complete (truncated output).
    pub fn has_incomplete_tool_call(&self) -> bool {
        // Only check the unconsumed portion of the buffer
        let unchecked_buffer = &self.text_buffer[self.last_consumed_position..];
        if let Some(start_pos) = Self::find_last_tool_call_start(unchecked_buffer) {
            let json_text = &unchecked_buffer[start_pos..];
            // If JSON is complete, it's not incomplete
            if Self::find_complete_json_object_end(json_text).is_some() {
                return false;
            }
            // If JSON has been invalidated by subsequent content, it's not a real tool call
            if Self::is_json_invalidated(json_text) {
                return false;
            }
            // Otherwise, it's a genuinely incomplete tool call
            true
        } else {
            false
        }
    }

    /// Check if the text buffer contains an unexecuted tool call.
    /// This detects cases where the LLM emitted a complete tool call JSON
    /// but it wasn't parsed/executed (e.g., due to parsing issues).
    pub fn has_unexecuted_tool_call(&self) -> bool {
        // Only check the unconsumed portion of the buffer
        let unchecked_buffer = &self.text_buffer[self.last_consumed_position..];
        if let Some(start_pos) = Self::find_last_tool_call_start(unchecked_buffer) {
            let json_text = &unchecked_buffer[start_pos..];
            // If the JSON IS complete, it means there's an unexecuted tool call
            if let Some(json_end) = Self::find_complete_json_object_end(json_text) {
                let json_only = &json_text[..=json_end];
                return serde_json::from_str::<serde_json::Value>(json_only).is_ok();
            }
        }
        false
    }

    /// Mark all tool calls up to the current buffer position as consumed/executed.
    /// This prevents has_unexecuted_tool_call() from returning true for already-executed tools.
    pub fn mark_tool_calls_consumed(&mut self) {
        self.last_consumed_position = self.text_buffer.len();
    }

    /// Check if a partial JSON tool call has been invalidated by subsequent content.
    /// 
    /// This detects cases where we started parsing what looked like a tool call
    /// (e.g., `{"tool": "read_file`) but subsequent content makes it clear this
    /// isn't valid JSON (e.g., a newline followed by regular prose).
    /// 
    /// The key insight: in valid JSON, after an open quote for a string value,
    /// we must see the string content and closing quote before any unescaped newline.
    /// If we see a newline followed by text that looks like prose (starts with a letter),
    /// this can't be a valid JSON tool call.
    /// 
    /// Additionally, an unescaped newline INSIDE a string is invalid JSON. So if we're
    /// in a string and see a newline (not escaped), the JSON is invalid.
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
                '\n' if in_string => {
                    // Unescaped newline inside a string is invalid JSON!
                    // Valid JSON strings cannot contain literal newlines (must be \n).
                    return true;
                }
                '\n' if !in_string => {
                    // We hit a newline outside of a string.
                    // Check what comes after - if it's regular prose, this isn't valid JSON.
                    
                    // Skip any whitespace after the newline
                    while let Some(&(_, next_ch)) = chars.peek() {
                        if next_ch == ' ' || next_ch == '\t' {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    
                    // Check the first non-whitespace character after the newline
                    if let Some(&(_, next_ch)) = chars.peek() {
                        // Valid JSON continuation characters after newline:
                        // - '"' (string)
                        // - '{' or '}' (object)
                        // - '[' or ']' (array)  
                        // - digits (number)
                        // - 't', 'f', 'n' could be true/false/null but also prose
                        // - ':' or ',' (separators)
                        // 
                        // If we see a letter that's clearly prose (not t/f/n at start of token),
                        // or other non-JSON characters, this is invalidated.
                        let is_valid_json_continuation = matches!(next_ch, 
                            '"' | '{' | '}' | '[' | ']' | ':' | ',' | '-' | 
                            '0'..='9' | 't' | 'f' | 'n' | '\n'
                        );
                        
                        if !is_valid_json_continuation {
                            return true; // Invalidated!
                        }
                    }
                }
                _ => {}
            }
        }
        
        false // Not invalidated (yet)
    }

    /// Find the end position (byte index) of a complete JSON object in the text.
    /// Returns None if no complete JSON object is found.
    pub fn find_complete_json_object_end(text: &str) -> Option<usize> {
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
                '"' if !escape_next => in_string = !in_string,
                '{' if !in_string => {
                    brace_count += 1;
                    found_start = true;
                }
                '}' if !in_string => {
                    brace_count -= 1;
                    if brace_count == 0 && found_start {
                        return Some(i); // Return the byte index of the closing brace
                    }
                }
                _ => {}
            }
        }

        None // No complete JSON object found
    }

    /// Reset the parser state for a new message.
    pub fn reset(&mut self) {
        self.text_buffer.clear();
        self.last_consumed_position = 0;
        self.message_stopped = false;
        self.in_json_tool_call = false;
        self.json_tool_start = None;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_complete_json_object_end_simple() {
        let text = r#"{"tool":"shell","args":{"command":"ls"}}"#;
        assert_eq!(
            StreamingToolParser::find_complete_json_object_end(text),
            Some(text.len() - 1)
        );
    }

    #[test]
    fn test_find_complete_json_object_end_nested() {
        let text = r#"{"tool":"write","args":{"content":"{nested}"}}"#;
        assert_eq!(
            StreamingToolParser::find_complete_json_object_end(text),
            Some(text.len() - 1)
        );
    }

    #[test]
    fn test_find_complete_json_object_end_incomplete() {
        let text = r#"{"tool":"shell","args":{"command":"ls""#;
        assert_eq!(StreamingToolParser::find_complete_json_object_end(text), None);
    }

    #[test]
    fn test_tool_call_patterns() {
        // Test that all patterns are detected
        assert!(StreamingToolParser::find_first_tool_call_start(r#"{"tool":"test"}"#).is_some());
        assert!(StreamingToolParser::find_first_tool_call_start(r#"{ "tool":"test"}"#).is_some());
        assert!(StreamingToolParser::find_first_tool_call_start(r#"{"tool" :"test"}"#).is_some());
        assert!(StreamingToolParser::find_first_tool_call_start(r#"{ "tool" :"test"}"#).is_some());
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
        // Test that when multiple tool calls arrive together, they are processed
        // in order (first one first, not last one first)
        let mut parser = StreamingToolParser::new();
        
        // Simulate two tool calls arriving in the same chunk
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
        };
        
        let tools = parser.process_chunk(&chunk);
        
        // Should find both tool calls
        assert_eq!(tools.len(), 2, "Expected 2 tool calls, got {}", tools.len());
        
        // First tool call should be "first", not "second"
        assert_eq!(tools[0].tool, "shell");
        assert_eq!(tools[0].args["command"], "first", 
            "First tool call should have command 'first', got {:?}", tools[0].args);
        
        // Second tool call should be "second"
        assert_eq!(tools[1].tool, "shell");
        assert_eq!(tools[1].args["command"], "second",
            "Second tool call should have command 'second', got {:?}", tools[1].args);
    }


    #[test]
    fn test_find_first_vs_last_tool_call() {
        // Both tool calls are on their own lines
        let text = "{\"tool\": \"first\"}\n{\"tool\": \"second\"}";
        
        let first_pos = StreamingToolParser::find_first_tool_call_start(text);
        let last_pos = StreamingToolParser::find_last_tool_call_start(text);
        
        assert!(first_pos.is_some(), "Should find first tool call");
        assert!(last_pos.is_some(), "Should find last tool call");
        assert!(first_pos.unwrap() < last_pos.unwrap(), 
            "First position ({:?}) should be less than last position ({:?})", first_pos, last_pos);
    }

    #[test]
    fn test_inline_tool_call_ignored() {
        // Tool call pattern inline with other text should NOT be detected
        let text = "Here is an example: {\"tool\": \"shell\"} in text";
        assert!(StreamingToolParser::find_first_tool_call_start(text).is_none(),
            "Inline tool call pattern should be ignored");
        assert!(StreamingToolParser::find_last_tool_call_start(text).is_none(),
            "Inline tool call pattern should be ignored");
    }

    #[test]
    fn test_standalone_tool_call_detected() {
        // Tool call on its own line (at start of text) should be detected
        let text = r#"{"tool": "shell", "args": {"command": "ls"}}"#;
        assert!(StreamingToolParser::find_first_tool_call_start(text).is_some(),
            "Standalone tool call should be detected");
    }

    #[test]
    fn test_indented_tool_call_detected() {
        // Tool call with leading whitespace should be detected
        let text = r#"  {"tool": "shell", "args": {"command": "ls"}}"#;
        assert!(StreamingToolParser::find_first_tool_call_start(text).is_some(),
            "Indented tool call should be detected");
    }

    #[test]
    fn test_tool_call_after_newline_detected() {
        // Tool call after a newline should be detected
        let text = "Some prose here\n{\"tool\": \"shell\", \"args\": {}}";
        let pos = StreamingToolParser::find_first_tool_call_start(text);
        assert!(pos.is_some(), "Tool call after newline should be detected");
        assert_eq!(pos.unwrap(), 16, "Should find tool call at position after newline");
    }

    #[test]
    fn test_inline_ignored_but_standalone_detected() {
        // Mixed: inline on first line (ignored), standalone on second line (detected)
        let text = "Some text with {\"tool\": \"inline\"} here\n{\"tool\": \"standalone\", \"args\": {}}";
        let pos = StreamingToolParser::find_first_tool_call_start(text);
        assert!(pos.is_some(), "Should find the standalone tool call");
        // The standalone one starts after the newline
        assert!(pos.unwrap() > 30, "Should skip the inline pattern and find the standalone one");
    }

    #[test]
    fn test_multiple_inline_patterns_all_ignored() {
        // Multiple inline patterns on same line - all should be ignored
        let text = "Compare {\"tool\": \"a\"} with {\"tool\": \"b\"}";
        assert!(StreamingToolParser::find_first_tool_call_start(text).is_none(),
            "All inline patterns should be ignored");
    }

    #[test]
    fn test_is_on_own_line() {
        // Test the is_on_own_line helper directly
        let text = "prefix {\"tool\":\n          {\"tool\":";
        
        // Position 0 is always on its own line
        assert!(StreamingToolParser::is_on_own_line(text, 0));
        
        // Position 7 (after "prefix ") is NOT on its own line
        assert!(!StreamingToolParser::is_on_own_line(text, 7));
        
        // Position after newline with only whitespace before pattern IS on its own line
        let newline_pos = text.find('\n').unwrap();
        assert!(StreamingToolParser::is_on_own_line(text, newline_pos + 11)); // 10 spaces before {
    }

    #[test]
    fn test_all_pattern_variants_require_own_line() {
        // All whitespace variants should require their own line
        let patterns = [
            "text { \"tool\":\"x\"}",
            "text {\"tool\" :\"x\"}",
            "text { \"tool\" :\"x\"}",
        ];
        for pattern in patterns {
            assert!(StreamingToolParser::find_first_tool_call_start(pattern).is_none(),
                "Inline pattern '{}' should be ignored", pattern);
        }
    }
}
