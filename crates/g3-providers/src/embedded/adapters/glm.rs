//! GLM/Z-AI tool format adapter
//!
//! GLM models can use two tool calling formats:
//!
//! 1. Native format:
//! ```text
//! <|assistant|>tool_name
//! {"arg": "value"}
//! ```
//!
//! 2. Code-fenced JSON (when following system prompt instructions):
//! ````text
//! ```json
//! {"tool": "shell", "args": {"command": "ls"}}
//! ```
//! ````
//!
//! This adapter handles both formats and strips code fences when present.

use super::{AdapterOutput, ToolFormatAdapter};

/// Safety limits to prevent unbounded buffering
const MAX_PATTERN_BUFFER: usize = 20; // `<|assistant|>` is 13 chars
const MAX_TOOL_NAME: usize = 64;
const MAX_JSON_BUFFER: usize = 65536; // 64KB
const MAX_NEWLINES_BEFORE_JSON: usize = 2;

/// The pattern that indicates a tool call in GLM format
const ASSISTANT_PATTERN: &str = "<|assistant|>";

/// Parser state for the main state machine
#[derive(Debug, Clone, PartialEq)]
enum ParseState {
    /// Normal prose, watching for `<|assistant|>`
    Prose,
    /// Saw start of potential pattern (e.g., "<|"), buffering to confirm
    MaybePattern,
    /// Confirmed `<|assistant|>`, now reading tool name until newline
    InToolName,
    /// Got tool name, waiting for `{` to start JSON (allowing whitespace/newlines)
    AwaitingJson { tool_name: String, newline_count: usize },
    /// Inside JSON body, tracking depth to find end
    InToolJson { tool_name: String },
}

/// State for JSON parsing (to handle strings correctly)
#[derive(Debug, Clone, Copy, PartialEq)]
enum JsonState {
    /// Normal JSON, counting braces
    Normal,
    /// Inside a string literal, ignore braces
    InString,
    /// Just saw backslash in string, next char is escaped
    InStringEscape,
}

/// Adapter for GLM/Z-AI model tool calling format
#[derive(Debug)]
pub struct GlmToolAdapter {
    /// Buffer for accumulating content
    buffer: String,
    /// Buffer for current line (to detect code fences)
    line_buffer: String,
    /// Current parse state
    state: ParseState,
    /// JSON parsing state (when in InToolJson)
    json_state: JsonState,
    /// Brace depth for JSON parsing
    json_depth: i32,
    /// Content to emit that's been confirmed as prose
    pending_emit: String,
}

impl GlmToolAdapter {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            line_buffer: String::new(),
            state: ParseState::Prose,
            json_state: JsonState::Normal,
            json_depth: 0,
            pending_emit: String::new(),
        }
    }

    /// Process a character for code fence detection (streaming-safe)
    /// Returns the string to emit (empty if content should be suppressed)
    fn process_for_code_fence(&mut self, c: char) -> String {
        if c == '\n' {
            // End of line - check if it's a code fence
            let trimmed = self.line_buffer.trim();
            if trimmed.starts_with("```") {
                let after_fence = trimmed.trim_start_matches('`').trim();
                if after_fence.is_empty() || after_fence.chars().all(|c| c.is_ascii_alphanumeric()) {
                    // This is a code fence marker line - suppress it
                    self.line_buffer.clear();
                    return String::new(); // Don't emit anything for fence lines
                }
            }
            // Not a fence line - just emit the newline
            // (buffered content was already emitted char-by-char)
            self.line_buffer.clear();
            c.to_string()
        } else {
            self.line_buffer.push(c);
            // Only suppress output if the line looks like it could be a code fence
            // A code fence line starts with optional whitespace then ```
            let trimmed = self.line_buffer.trim_start();
            if trimmed.starts_with('`') && trimmed.len() <= 10 {
                // Potentially a fence marker - buffer until we see newline
                String::new()
            } else {
                // Not a fence - emit the entire buffer (which includes current char)
                // and clear it since we've emitted everything
                let result = std::mem::take(&mut self.line_buffer);
                result
            }
        }
    }

    /// Strip markdown code fence markers from output
    /// 
    /// GLM models sometimes wrap tool calls in code fences like:
    /// ```json
    /// {"tool": "shell", ...}
    /// ```
    /// 
    /// This strips those markers so the JSON can be parsed as a tool call.
    fn strip_code_fences(text: &str) -> String {
        text.lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                // Filter out lines that are just code fence markers (with optional language)
                if trimmed.starts_with("```") {
                    // Check if there's content after the fence marker on the same line
                    let after_fence = trimmed.trim_start_matches('`').trim();
                    if after_fence.is_empty() || after_fence.chars().all(|c| c.is_ascii_alphanumeric()) {
                        // Just a fence marker (possibly with language like "json"), skip it
                        return None;
                    }
                }
                Some(line)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Strip inline code backticks from text
    /// 
    /// GLM models sometimes wrap tool calls in inline backticks like:
    /// `{"tool": "shell", ...}`
    fn strip_inline_backticks(text: &str) -> String {
        let trimmed = text.trim();
        if trimmed.starts_with('`') && trimmed.ends_with('`') && !trimmed.starts_with("```") {
            trimmed[1..trimmed.len()-1].to_string()
        } else {
            text.to_string()
        }
    }

    /// Check if a string is a valid tool name
    /// Pattern: starts with letter or underscore, followed by alphanumeric or underscore
    fn is_valid_tool_name(name: &str) -> bool {
        if name.is_empty() || name.len() > MAX_TOOL_NAME {
            return false;
        }
        let mut chars = name.chars();
        match chars.next() {
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
            _ => return false,
        }
        chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    }

    /// Process a single character in Prose state
    fn process_prose_char(&mut self, c: char) {
        // First, filter through code fence detection
        let filtered = self.process_for_code_fence(c);
        for filtered_c in filtered.chars() {
            if filtered_c == '<' {
                // Potential start of pattern
                self.buffer.push(filtered_c);
                self.state = ParseState::MaybePattern;
            } else {
                self.pending_emit.push(filtered_c);
            }
        }
        // If empty string, the character is being buffered for code fence detection
    }

    /// Process a single character in MaybePattern state
    fn process_maybe_pattern_char(&mut self, c: char) {
        self.buffer.push(c);

        // Check if buffer matches start of pattern
        if ASSISTANT_PATTERN.starts_with(&self.buffer) {
            // Still could be the pattern
            if self.buffer == ASSISTANT_PATTERN {
                // Complete pattern match!
                self.buffer.clear();
                self.state = ParseState::InToolName;
            }
            // else: keep buffering
        } else {
            // Not the pattern, emit buffer as prose
            self.pending_emit.push_str(&self.buffer);
            self.buffer.clear();
            self.state = ParseState::Prose;
        }

        // Safety: if buffer gets too long, it's not our pattern
        if self.buffer.len() > MAX_PATTERN_BUFFER {
            self.pending_emit.push_str(&self.buffer);
            self.buffer.clear();
            self.state = ParseState::Prose;
        }
    }

    /// Process a single character in InToolName state
    fn process_tool_name_char(&mut self, c: char) {
        if c == '\n' {
            // End of tool name
            let tool_name = self.buffer.trim().to_string();
            self.buffer.clear();

            if Self::is_valid_tool_name(&tool_name) {
                self.state = ParseState::AwaitingJson {
                    tool_name,
                    newline_count: 1,
                };
            } else {
                // Invalid tool name, emit as prose
                self.pending_emit.push_str(ASSISTANT_PATTERN);
                self.pending_emit.push_str(&tool_name);
                self.pending_emit.push(c);
                self.state = ParseState::Prose;
            }
        } else if c.is_whitespace() && self.buffer.is_empty() {
            // Skip leading whitespace after <|assistant|>
        } else {
            self.buffer.push(c);

            // Safety: tool name too long
            if self.buffer.len() > MAX_TOOL_NAME {
                self.pending_emit.push_str(ASSISTANT_PATTERN);
                self.pending_emit.push_str(&self.buffer);
                self.buffer.clear();
                self.state = ParseState::Prose;
            }
        }
    }

    /// Process a single character in AwaitingJson state
    fn process_awaiting_json_char(&mut self, c: char, tool_name: String, newline_count: usize) {
        if c == '{' {
            // Start of JSON!
            self.buffer.push(c);
            self.json_depth = 1;
            self.json_state = JsonState::Normal;
            self.state = ParseState::InToolJson { tool_name };
        } else if c == '\n' {
            let new_count = newline_count + 1;
            if new_count > MAX_NEWLINES_BEFORE_JSON {
                // Too many newlines, not a tool call
                self.pending_emit.push_str(ASSISTANT_PATTERN);
                self.pending_emit.push_str(&tool_name);
                for _ in 0..new_count {
                    self.pending_emit.push('\n');
                }
                self.state = ParseState::Prose;
            } else {
                self.state = ParseState::AwaitingJson {
                    tool_name,
                    newline_count: new_count,
                };
            }
        } else if c.is_whitespace() {
            // Skip whitespace while waiting for JSON
            self.state = ParseState::AwaitingJson {
                tool_name,
                newline_count,
            };
        } else {
            // Non-JSON character, not a tool call
            self.pending_emit.push_str(ASSISTANT_PATTERN);
            self.pending_emit.push_str(&tool_name);
            self.pending_emit.push('\n');
            self.pending_emit.push(c);
            self.state = ParseState::Prose;
        }
    }

    /// Process a single character in InToolJson state
    fn process_json_char(&mut self, c: char, tool_name: String) -> Option<String> {
        self.buffer.push(c);

        // Update JSON state machine
        match self.json_state {
            JsonState::Normal => {
                match c {
                    '{' => self.json_depth += 1,
                    '}' => {
                        self.json_depth -= 1;
                        if self.json_depth == 0 {
                            // JSON complete!
                            let json_args = self.buffer.clone();
                            self.buffer.clear();
                            self.state = ParseState::Prose;

                            // Transform to g3 format
                            let transformed = format!(
                                "{{\"tool\": \"{}\", \"args\": {}}}",
                                tool_name, json_args
                            );
                            return Some(transformed);
                        }
                    }
                    '"' => self.json_state = JsonState::InString,
                    _ => {}
                }
            }
            JsonState::InString => {
                match c {
                    '\\' => self.json_state = JsonState::InStringEscape,
                    '"' => self.json_state = JsonState::Normal,
                    _ => {}
                }
            }
            JsonState::InStringEscape => {
                // Any character after backslash, return to InString
                self.json_state = JsonState::InString;
            }
        }

        // Safety: JSON buffer too large
        if self.buffer.len() > MAX_JSON_BUFFER {
            // Emit as malformed - let downstream handle it
            self.pending_emit.push_str(ASSISTANT_PATTERN);
            self.pending_emit.push_str(&tool_name);
            self.pending_emit.push('\n');
            self.pending_emit.push_str(&self.buffer);
            self.buffer.clear();
            self.state = ParseState::Prose;
            self.json_state = JsonState::Normal;
            self.json_depth = 0;
        }

        // Keep state for next iteration
        self.state = ParseState::InToolJson { tool_name };
        None
    }
}

impl Default for GlmToolAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolFormatAdapter for GlmToolAdapter {
    fn handles(&self, model_type: &str) -> bool {
        model_type.contains("glm")
    }

    fn process_chunk(&mut self, chunk: &str) -> AdapterOutput {
        let mut has_tool_call = false;

        for c in chunk.chars() {
            match self.state.clone() {
                ParseState::Prose => {
                    self.process_prose_char(c);
                }
                ParseState::MaybePattern => {
                    self.process_maybe_pattern_char(c);
                }
                ParseState::InToolName => {
                    self.process_tool_name_char(c);
                }
                ParseState::AwaitingJson { tool_name, newline_count } => {
                    self.process_awaiting_json_char(c, tool_name, newline_count);
                }
                ParseState::InToolJson { tool_name } => {
                    if let Some(transformed) = self.process_json_char(c, tool_name) {
                        self.pending_emit.push('\n');
                        self.pending_emit.push_str(&transformed);
                        has_tool_call = true;
                    }
                }
            }
        }

        // Return accumulated emit content, stripping any code fence markers
        let raw_emit = std::mem::take(&mut self.pending_emit);
        let stripped_fences = Self::strip_code_fences(&raw_emit);
        let emit = Self::strip_inline_backticks(&stripped_fences);
        AdapterOutput {
            emit: emit.to_string(),
            has_tool_call,
        }
    }

    fn flush(&mut self) -> AdapterOutput {
        let mut emit = std::mem::take(&mut self.pending_emit);

        // Emit any buffered content as-is
        match &self.state {
            ParseState::Prose => {
                // Nothing extra to emit
            }
            ParseState::MaybePattern => {
                emit.push_str(&self.buffer);
            }
            ParseState::InToolName => {
                emit.push_str(ASSISTANT_PATTERN);
                emit.push_str(&self.buffer);
            }
            ParseState::AwaitingJson { tool_name, newline_count } => {
                emit.push_str(ASSISTANT_PATTERN);
                emit.push_str(tool_name);
                for _ in 0..*newline_count {
                    emit.push('\n');
                }
            }
            ParseState::InToolJson { tool_name } => {
                emit.push_str(ASSISTANT_PATTERN);
                emit.push_str(tool_name);
                emit.push('\n');
                emit.push_str(&self.buffer);
            }
        }

        // Flush any remaining line buffer content (if not a code fence)
        if !self.line_buffer.is_empty() {
            let trimmed = self.line_buffer.trim();
            let is_fence = trimmed.starts_with("```") && 
                (trimmed.trim_start_matches('`').trim().is_empty() || 
                 trimmed.trim_start_matches('`').trim().chars().all(|c| c.is_ascii_alphanumeric()));
            if !is_fence {
                emit.push_str(&self.line_buffer);
            }
        }

        self.reset();

        // Strip code fences and inline backticks from final output
        let stripped_fences = Self::strip_code_fences(&emit);
        let stripped = Self::strip_inline_backticks(&stripped_fences);
        AdapterOutput {
            emit: stripped,
            has_tool_call: false,
        }
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.line_buffer.clear();
        self.state = ParseState::Prose;
        self.json_state = JsonState::Normal;
        self.json_depth = 0;
        self.pending_emit.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handles_glm_models() {
        let adapter = GlmToolAdapter::new();
        assert!(adapter.handles("glm4"));
        assert!(adapter.handles("glm"));
        assert!(adapter.handles("some-glm-variant"));
        assert!(!adapter.handles("qwen"));
        assert!(!adapter.handles("llama"));
    }

    #[test]
    fn test_valid_tool_names() {
        assert!(GlmToolAdapter::is_valid_tool_name("shell"));
        assert!(GlmToolAdapter::is_valid_tool_name("read_file"));
        assert!(GlmToolAdapter::is_valid_tool_name("_private"));
        assert!(GlmToolAdapter::is_valid_tool_name("tool123"));
        assert!(!GlmToolAdapter::is_valid_tool_name(""));
        assert!(!GlmToolAdapter::is_valid_tool_name("123tool"));
        assert!(!GlmToolAdapter::is_valid_tool_name("tool-name"));
        assert!(!GlmToolAdapter::is_valid_tool_name("tool name"));
    }

    #[test]
    fn test_basic_tool_call() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "Let me list files.<|assistant|>shell\n{\"command\": \"ls\"}";
        let output = adapter.process_chunk(input);
        
        assert!(output.has_tool_call);
        assert!(output.emit.contains("Let me list files."));
        assert!(output.emit.contains(r#"{"tool": "shell", "args": {"command": "ls"}}"#));
    }

    #[test]
    fn test_tool_call_chunked() {
        let mut adapter = GlmToolAdapter::new();
        
        // Simulate chunked input
        let chunks = vec![
            "Let me ",
            "list.<|assis",
            "tant|>shell\n{\"co",
            "mmand\": \"ls\"}",
        ];
        
        let mut full_output = String::new();
        let mut found_tool = false;
        
        for chunk in chunks {
            let output = adapter.process_chunk(chunk);
            full_output.push_str(&output.emit);
            if output.has_tool_call {
                found_tool = true;
            }
        }
        
        let final_output = adapter.flush();
        full_output.push_str(&final_output.emit);
        
        assert!(found_tool);
        assert!(full_output.contains("Let me list."));
        assert!(full_output.contains(r#"{"tool": "shell", "args": {"command": "ls"}}"#));
    }

    #[test]
    fn test_nested_json_in_string() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = r#"<|assistant|>shell
{"command": "echo '{\"nested\": true}'"}
Done."#;
        
        let output = adapter.process_chunk(input);
        let final_output = adapter.flush();
        
        assert!(output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        assert!(full.contains(r#""args": {"command": "echo '{\"nested\": true}'"}}"#));
    }

    #[test]
    fn test_escaped_quotes_in_string() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = r#"<|assistant|>shell
{"command": "echo \"hello\""}
Done."#;
        
        let output = adapter.process_chunk(input);
        
        assert!(output.has_tool_call);
        assert!(output.emit.contains(r#""args": {"command": "echo \"hello\""}"#));
    }

    #[test]
    fn test_false_pattern_in_prose() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "The format is <|assistant|>tool_name for GLM models.";
        let output = adapter.process_chunk(input);
        let final_output = adapter.flush();
        
        // Should not detect as tool call since no JSON follows
        assert!(!output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        assert!(full.contains("<|assistant|>"));
    }

    #[test]
    fn test_multiple_tool_calls() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = r#"First:<|assistant|>shell
{"command": "ls"}
Second:<|assistant|>read_file
{"path": "test.txt"}"#;
        
        let output = adapter.process_chunk(input);
        
        assert!(output.has_tool_call);
        assert!(output.emit.contains(r#"{"tool": "shell"#));
        assert!(output.emit.contains(r#"{"tool": "read_file"#));
    }

    #[test]
    fn test_whitespace_before_json() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "<|assistant|>shell\n  {\"command\": \"ls\"}";
        let output = adapter.process_chunk(input);
        
        assert!(output.has_tool_call);
    }

    #[test]
    fn test_extra_newline_before_json() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "<|assistant|>shell\n\n{\"command\": \"ls\"}";
        let output = adapter.process_chunk(input);
        
        assert!(output.has_tool_call);
    }

    #[test]
    fn test_too_many_newlines_before_json() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "<|assistant|>shell\n\n\n{\"command\": \"ls\"}";
        let output = adapter.process_chunk(input);
        let final_output = adapter.flush();
        
        // Should not detect as tool call - too many newlines
        assert!(!output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        assert!(full.contains("<|assistant|>shell"));
    }

    #[test]
    fn test_invalid_tool_name() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "<|assistant|>123invalid\n{\"command\": \"ls\"}";
        let output = adapter.process_chunk(input);
        let final_output = adapter.flush();
        
        // Should not detect as tool call - invalid name
        assert!(!output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        assert!(full.contains("<|assistant|>123invalid"));
    }

    #[test]
    fn test_stream_ends_mid_pattern() {
        let mut adapter = GlmToolAdapter::new();
        
        let output = adapter.process_chunk("text<|assis");
        let final_output = adapter.flush();
        
        assert!(!output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        assert_eq!(full, "text<|assis");
    }

    #[test]
    fn test_stream_ends_mid_json() {
        let mut adapter = GlmToolAdapter::new();
        
        let output = adapter.process_chunk("<|assistant|>shell\n{\"command\": \"ls");
        let final_output = adapter.flush();
        
        assert!(!output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        // Should emit the incomplete content
        assert!(full.contains("<|assistant|>shell"));
        assert!(full.contains("{\"command\": \"ls"));
    }

    #[test]
    fn test_prose_with_angle_brackets() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "Use <html> tags and <|other|> patterns.";
        let output = adapter.process_chunk(input);
        let final_output = adapter.flush();
        
        assert!(!output.has_tool_call);
        let full = format!("{}{}", output.emit, final_output.emit);
        assert_eq!(full, input);
    }

    #[test]
    fn test_reset() {
        let mut adapter = GlmToolAdapter::new();
        
        // Start processing but don't finish
        adapter.process_chunk("<|assistant|>shell\n{\"cmd");
        
        // Reset
        adapter.reset();
        
        // Should be back to clean state
        let output = adapter.process_chunk("Normal text");
        assert_eq!(output.emit, "Normal text");
        assert!(!output.has_tool_call);
    }

    #[test]
    fn test_strip_code_fences() {
        assert_eq!(
            GlmToolAdapter::strip_code_fences("```json\n{\"tool\": \"shell\"}\n```"),
            "{\"tool\": \"shell\"}"
        );
        assert_eq!(
            GlmToolAdapter::strip_code_fences("```\n{\"tool\": \"shell\"}\n```"),
            "{\"tool\": \"shell\"}"
        );
        assert_eq!(
            GlmToolAdapter::strip_code_fences("normal text"),
            "normal text"
        );
        assert_eq!(
            GlmToolAdapter::strip_code_fences("```json\ncode\n```\nmore text"),
            "code\nmore text"
        );
    }

    #[test]
    fn test_code_fenced_tool_call() {
        let mut adapter = GlmToolAdapter::new();
        
        let input = "```json\n{\"tool\": \"shell\", \"args\": {\"command\": \"ls\"}}\n```";
        let output = adapter.process_chunk(input);
        let final_output = adapter.flush();
        
        let full = format!("{}{}", output.emit, final_output.emit);
        // Should strip the code fences
        assert!(!full.contains("```"));
        assert!(full.contains("{\"tool\": \"shell\""));
    }
}
