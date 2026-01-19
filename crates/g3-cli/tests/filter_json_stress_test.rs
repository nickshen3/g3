//! Stress tests for JSON tool call filtering.
//!
//! These tests hammer the filter with malformed JSON, partial tool calls,
//! edge cases, and adversarial inputs to ensure robustness.

use g3_cli::filter_json::{filter_json_tool_calls, flush_json_tool_filter, reset_json_tool_state};

// ============================================================================
// Malformed JSON Tests
// ============================================================================

#[test]
fn test_unclosed_brace_at_end() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shell\", \"args\": {\"cmd\": \"ls\"";
    let result = filter_json_tool_calls(input);
    // Should suppress the incomplete tool call
    assert_eq!(result, "Text\n");
}

#[test]
fn test_missing_closing_quote() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shell\", \"args\": {\"cmd\": \"ls}}\nMore";
    let result = filter_json_tool_calls(input);
    // The unbalanced quote makes brace counting tricky
    // Should still filter the tool call attempt
    assert_eq!(result, "Text\n");
}

#[test]
fn test_extra_closing_braces() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shell\", \"args\": {}}}}}\nMore";
    let result = filter_json_tool_calls(input);
    // Extra braces after valid JSON should pass through
    assert_eq!(result, "Text\n}}}\nMore");
}

#[test]
fn test_deeply_nested_malformed() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"x\", \"args\": {{{{{{}}}}}}}\nMore";
    let result = filter_json_tool_calls(input);
    // Should handle deep nesting - extra braces get consumed as part of the tool call
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_null_bytes_in_json() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shell\0\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // Should handle null bytes gracefully
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_unicode_in_tool_name() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shëll\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // Unicode in tool name - still a valid tool call pattern
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_emoji_in_args() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shell\", \"args\": {\"msg\": \"Hello 🎉\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_very_long_string_value() {
    reset_json_tool_state();
    let long_string = "x".repeat(10000);
    let input = format!("Text\n{{\"tool\": \"shell\", \"args\": {{\"data\": \"{}\"}}}}\nMore", long_string);
    let result = filter_json_tool_calls(&input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_many_escaped_quotes() {
    reset_json_tool_state();
    let input = r#"Text
{"tool": "shell", "args": {"cmd": "echo \"a\" \"b\" \"c\" \"d\" \"e\""}}
More"#;
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_escaped_backslash_before_quote() {
    reset_json_tool_state();
    // This is: {"tool": "shell", "args": {"path": "C:\\"}}
    let input = "Text\n{\"tool\": \"shell\", \"args\": {\"path\": \"C:\\\\\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_newlines_inside_string() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"shell\", \"args\": {\"cmd\": \"echo\\nworld\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

// ============================================================================
// Partial Tool Call Pattern Tests
// ============================================================================

#[test]
fn test_just_opening_brace() {
    reset_json_tool_state();
    let result = filter_json_tool_calls("Text\n{");
    // Should buffer, waiting for more
    assert_eq!(result, "Text\n");
    
    // Now send something that's not a tool call
    let result2 = filter_json_tool_calls("\"other\": 1}\nMore");
    assert_eq!(result2, "{\"other\": 1}\nMore");
}

#[test]
fn test_partial_tool_keyword() {
    reset_json_tool_state();
    let chunks = vec!["Text\n{", "\"to", "ol", "\": ", "\"sh", "ell\"", ", \"args\": {}", "}\nMore"];
    let mut result = String::new();
    for chunk in chunks {
        result.push_str(&filter_json_tool_calls(chunk));
    }
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_tool_then_not_colon() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\" \"shell\"}\nMore"; // Missing colon
    let result = filter_json_tool_calls(input);
    // Not a valid tool call pattern - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tool_colon_then_number() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": 123}\nMore"; // Number instead of string
    let result = filter_json_tool_calls(input);
    // Not a valid tool call pattern - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tool_colon_then_null() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": null}\nMore";
    let result = filter_json_tool_calls(input);
    // Not a valid tool call pattern - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tool_colon_then_array() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": []}\nMore";
    let result = filter_json_tool_calls(input);
    // Not a valid tool call pattern - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tool_colon_then_object() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // Not a valid tool call pattern - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tools_plural() {
    reset_json_tool_state();
    let input = "Text\n{\"tools\": \"shell\"}\nMore";
    let result = filter_json_tool_calls(input);
    // "tools" is not "tool" - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tool_with_prefix() {
    reset_json_tool_state();
    let input = "Text\n{\"mytool\": \"shell\"}\nMore";
    let result = filter_json_tool_calls(input);
    // "mytool" is not "tool" - should pass through
    assert_eq!(result, input);
}

#[test]
fn test_tool_uppercase() {
    reset_json_tool_state();
    let input = "Text\n{\"TOOL\": \"shell\"}\nMore";
    let result = filter_json_tool_calls(input);
    // "TOOL" is not "tool" - should pass through
    assert_eq!(result, input);
}

// ============================================================================
// Streaming Edge Cases
// ============================================================================

#[test]
fn test_single_char_streaming() {
    reset_json_tool_state();
    let input = "Hi\n{\"tool\": \"x\", \"args\": {}}\nBye";
    let mut result = String::new();
    for ch in input.chars() {
        result.push_str(&filter_json_tool_calls(&ch.to_string()));
    }
    assert_eq!(result, "Hi\n\nBye");
}

#[test]
fn test_two_char_streaming() {
    reset_json_tool_state();
    let input = "Hi\n{\"tool\": \"x\", \"args\": {}}\nBye";
    let mut result = String::new();
    let chars: Vec<char> = input.chars().collect();
    for chunk in chars.chunks(2) {
        let s: String = chunk.iter().collect();
        result.push_str(&filter_json_tool_calls(&s));
    }
    assert_eq!(result, "Hi\n\nBye");
}

#[test]
fn test_random_chunk_sizes() {
    reset_json_tool_state();
    let input = "Before\n{\"tool\": \"shell\", \"args\": {\"cmd\": \"ls -la\"}}\nAfter";
    
    // Chunk at various sizes
    let chunk_sizes = [1, 3, 7, 11, 13, 17];
    
    for &size in &chunk_sizes {
        reset_json_tool_state();
        let mut result = String::new();
        let mut pos = 0;
        while pos < input.len() {
            let end = (pos + size).min(input.len());
            let chunk = &input[pos..end];
            result.push_str(&filter_json_tool_calls(chunk));
            pos = end;
        }
        assert_eq!(result, "Before\n\nAfter", "Failed with chunk size {}", size);
    }
}

#[test]
fn test_chunk_boundary_at_brace() {
    reset_json_tool_state();
    let chunks = vec!["Text\n", "{", "\"tool\": \"x\", \"args\": {}", "}", "\nMore"];
    let mut result = String::new();
    for chunk in chunks {
        result.push_str(&filter_json_tool_calls(chunk));
    }
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_chunk_boundary_at_quote() {
    reset_json_tool_state();
    let chunks = vec!["Text\n{\"tool\": \"", "shell", "\", \"args\": {}}", "\nMore"];
    let mut result = String::new();
    for chunk in chunks {
        result.push_str(&filter_json_tool_calls(chunk));
    }
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_chunk_boundary_at_colon() {
    reset_json_tool_state();
    let chunks = vec!["Text\n{\"tool\"", ":", " \"shell\", \"args\": {}}\nMore"];
    let mut result = String::new();
    for chunk in chunks {
        result.push_str(&filter_json_tool_calls(chunk));
    }
    assert_eq!(result, "Text\n\nMore");
}

// ============================================================================
// Multiple Tool Calls
// ============================================================================

#[test]
fn test_two_tool_calls_same_line() {
    reset_json_tool_state();
    // Two tool calls on same line (no newline between)
    let input = "Text\n{\"tool\": \"a\", \"args\": {}}{\"tool\": \"b\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // First is filtered (starts at line beginning)
    // Second starts immediately after first's }, not at line start, so passes through
    // This is acceptable - LLMs typically put tool calls on separate lines
    assert_eq!(result, "Text\n{\"tool\": \"b\", \"args\": {}}\nMore");
}

#[test]
fn test_three_tool_calls_separate_lines() {
    reset_json_tool_state();
    let input = "A\n{\"tool\": \"x\", \"args\": {}}\nB\n{\"tool\": \"y\", \"args\": {}}\nC\n{\"tool\": \"z\", \"args\": {}}\nD";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "A\n\nB\n\nC\n\nD");
}

#[test]
fn test_tool_call_then_regular_json() {
    reset_json_tool_state();
    let input = "A\n{\"tool\": \"x\", \"args\": {}}\nB\n{\"data\": 123}\nC";
    let result = filter_json_tool_calls(input);
    // First is tool call (filtered), second is regular JSON (kept)
    assert_eq!(result, "A\n\nB\n{\"data\": 123}\nC");
}

#[test]
fn test_regular_json_then_tool_call() {
    reset_json_tool_state();
    let input = "A\n{\"data\": 123}\nB\n{\"tool\": \"x\", \"args\": {}}\nC";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "A\n{\"data\": 123}\nB\n\nC");
}

// ============================================================================
// Adversarial Inputs
// ============================================================================

#[test]
fn test_fake_tool_in_string() {
    reset_json_tool_state();
    // The tool pattern appears inside a string value
    let input = r#"Text
{"message": "{\"tool\": \"shell\"}"}
More"#;
    let result = filter_json_tool_calls(input);
    // Should pass through - the pattern is inside a string
    assert_eq!(result, input);
}

#[test]
fn test_nested_json_with_tool_key() {
    reset_json_tool_state();
    // Nested object has "tool" key but outer doesn't match pattern
    let input = "Text\n{\"outer\": {\"tool\": \"inner\"}}\nMore";
    let result = filter_json_tool_calls(input);
    // Should pass through - outer object doesn't start with "tool"
    assert_eq!(result, input);
}

#[test]
fn test_brace_bomb() {
    reset_json_tool_state();
    // Many braces to stress the counter
    let input = "Text\n{\"tool\": \"x\", \"args\": {\"a\": {\"b\": {\"c\": {\"d\": {\"e\": {}}}}}}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_string_with_many_braces() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"x\", \"args\": {\"code\": \"{{{{}}}}\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_alternating_braces_in_string() {
    reset_json_tool_state();
    let input = "Text\n{\"tool\": \"x\", \"args\": {\"pat\": \"}{}{}{\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_quote_after_backslash_in_string() {
    reset_json_tool_state();
    // Tricky: \" inside string should not end the string
    let input = r#"Text
{"tool": "x", "args": {"msg": "say \"hi\""}}
More"#;
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_double_backslash_then_quote() {
    reset_json_tool_state();
    // \\ followed by " - the quote DOES end the string
    let input = "Text\n{\"tool\": \"x\", \"args\": {\"path\": \"C:\\\\\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_triple_backslash_then_quote() {
    reset_json_tool_state();
    // \\\" - escaped backslash followed by escaped quote
    let input = "Text\n{\"tool\": \"x\", \"args\": {\"s\": \"a\\\\\\\"b\"}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

// ============================================================================
// Whitespace Variations
// ============================================================================

#[test]
fn test_tabs_before_brace() {
    reset_json_tool_state();
    let input = "Text\n\t\t{\"tool\": \"x\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // Indented JSON should NOT be filtered - real tool calls are never indented
    assert_eq!(result, input);
}

#[test]
fn test_spaces_before_brace() {
    reset_json_tool_state();
    let input = "Text\n    {\"tool\": \"x\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // Indented JSON should NOT be filtered - real tool calls are never indented
    assert_eq!(result, input);
}

#[test]
fn test_mixed_whitespace_before_brace() {
    reset_json_tool_state();
    let input = "Text\n \t \t {\"tool\": \"x\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    // Indented JSON should NOT be filtered - real tool calls are never indented
    assert_eq!(result, input);
}

#[test]
fn test_space_after_opening_brace() {
    reset_json_tool_state();
    let input = "Text\n{ \"tool\": \"x\", \"args\": {}}\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_lots_of_space_in_json() {
    reset_json_tool_state();
    let input = "Text\n{   \"tool\"   :   \"x\"   ,   \"args\"   :   {   }   }\nMore";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Text\n\nMore");
}

#[test]
fn test_crlf_line_endings() {
    reset_json_tool_state();
    let input = "Text\r\n{\"tool\": \"x\", \"args\": {}}\r\nMore";
    let result = filter_json_tool_calls(input);
    // \r is not treated as line start, so { after \r\n should work
    // Actually \n triggers line start, \r is just a regular char
    assert_eq!(result, "Text\r\n\r\nMore");
}

// ============================================================================
// Empty and Minimal Cases
// ============================================================================

#[test]
fn test_empty_input() {
    reset_json_tool_state();
    assert_eq!(filter_json_tool_calls(""), "");
}

#[test]
fn test_just_newline() {
    reset_json_tool_state();
    let result = filter_json_tool_calls("\n");
    let flushed = flush_json_tool_filter();
    assert_eq!(format!("{}{}", result, flushed), "\n");
}

#[test]
fn test_just_brace() {
    reset_json_tool_state();
    let r1 = filter_json_tool_calls("{");
    // At start of input (line start), { triggers buffering
    assert_eq!(r1, "");
    
    // Send non-tool content - the newline comes through
    let r2 = filter_json_tool_calls("}\n");
    assert_eq!(r2, "{}\n");
}

#[test]
fn test_minimal_tool_call() {
    reset_json_tool_state();
    let input = "{\"tool\":\"x\",\"args\":{}}";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "");
}

#[test]
fn test_tool_call_at_very_start() {
    reset_json_tool_state();
    let input = "{\"tool\": \"x\", \"args\": {}}\nAfter";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "\nAfter");
}

// ============================================================================
// State Reset Tests
// ============================================================================

#[test]
fn test_reset_clears_buffering_state() {
    reset_json_tool_state();
    
    // Start a potential tool call
    let _ = filter_json_tool_calls("Text\n{");
    
    // Reset
    reset_json_tool_state();
    
    // New input should work fresh
    let result = filter_json_tool_calls("Fresh start");
    assert_eq!(result, "Fresh start");
}

#[test]
fn test_reset_clears_suppressing_state() {
    reset_json_tool_state();
    
    // Start suppressing a tool call
    let _ = filter_json_tool_calls("Text\n{\"tool\": \"x\", \"args\": {");
    
    // Reset
    reset_json_tool_state();
    
    // New input should work fresh
    let result = filter_json_tool_calls("Fresh start");
    assert_eq!(result, "Fresh start");
}

// ============================================================================
// Real-World Patterns from Bug Reports
// ============================================================================

#[test]
fn test_str_replace_with_diff() {
    reset_json_tool_state();
    let input = r#"I'll update the file:
{"tool": "str_replace", "args": {"file_path": "src/main.rs", "diff": "@@ -1,3 +1,4 @@\n fn main() {\n+    println!(\"Hello\");\n }"}}
Done!"#;
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "I'll update the file:\n\nDone!");
}

#[test]
fn test_shell_with_complex_command() {
    reset_json_tool_state();
    let input = r#"Running command:
{"tool": "shell", "args": {"command": "find . -name '*.rs' -exec grep -l 'TODO' {} \;"}}
Results above."#;
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Running command:\n\nResults above.");
}

#[test]
fn test_write_file_with_json_content() {
    reset_json_tool_state();
    let input = r#"Creating config:
{"tool": "write_file", "args": {"file_path": "config.json", "content": "{\"key\": \"value\"}"}}
File created."#;
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Creating config:\n\nFile created.");
}

#[test]
fn test_read_file_simple() {
    reset_json_tool_state();
    let input = "Let me check:\n{\"tool\": \"read_file\", \"args\": {\"file_path\": \"README.md\"}}\nHere's what I found:";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Let me check:\n\nHere's what I found:");
}

#[test]
fn test_final_output() {
    reset_json_tool_state();
    let input = "Task complete.\n{\"tool\": \"final_output\", \"args\": {\"summary\": \"# Summary\\n\\nI completed the task.\\n\\n## Details\\n- Item 1\\n- Item 2\"}}\n";
    let result = filter_json_tool_calls(input);
    assert_eq!(result, "Task complete.\n\n");
}

// ============================================================================
// Truncated JSON followed by Complete JSON (the original bug)
// ============================================================================

#[test]
fn test_truncated_then_complete_streaming() {
    reset_json_tool_state();
    
    // Chunk 1: text
    let r1 = filter_json_tool_calls("Some text\n");
    assert_eq!(r1, "Some text\n");
    
    // Chunk 2: truncated tool call
    let r2 = filter_json_tool_calls(r#"{"tool": "str_replace", "args": {"diff":"partial"#);
    assert_eq!(r2, "");
    
    // Chunk 3: new complete tool call (LLM retry)
    let r3 = filter_json_tool_calls(r#"{"tool": "str_replace", "args": {"diff":"complete", "file_path":"x.rs"}}"#);
    assert_eq!(r3, "");
    
    // Chunk 4: text after
    let r4 = filter_json_tool_calls("\nMore text");
    assert_eq!(r4, "\nMore text");
}

#[test]
fn test_multiple_truncated_then_complete() {
    reset_json_tool_state();
    
    let chunks = vec![
        "Start\n",
        r#"{"tool": "a", "args": {"x": "trunc"#,  // truncated
        r#"{"tool": "b", "args": {"y": "also_trunc"#,  // another truncated
        r#"{"tool": "c", "args": {"z": "complete"}}"#,  // finally complete
        "\nEnd",
    ];
    
    let mut result = String::new();
    for chunk in chunks {
        result.push_str(&filter_json_tool_calls(chunk));
    }
    
    assert_eq!(result, "Start\n\nEnd");
}
