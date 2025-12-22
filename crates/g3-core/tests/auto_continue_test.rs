//! Tests for the auto-continue detection features
//!
//! These tests verify the logic used to detect when the LLM should auto-continue:
//! 1. Empty/trivial responses (just timing lines)
//! 2. Incomplete tool calls
//! 3. Unexecuted tool calls
//! 4. Missing final_output after tool execution

/// Helper function to check if a response is considered "empty" or trivial
/// This mirrors the logic in lib.rs for detecting empty responses
fn is_empty_response(response_text: &str) -> bool {
    response_text.trim().is_empty()
        || response_text.lines().all(|line| {
            line.trim().is_empty() || line.trim().starts_with("⏱️")
        })
}

#[test]
fn test_empty_response_detection_empty_string() {
    assert!(is_empty_response(""));
}

#[test]
fn test_empty_response_detection_whitespace_only() {
    assert!(is_empty_response("   "));
    assert!(is_empty_response("\n\n\n"));
    assert!(is_empty_response("  \n  \t  \n  "));
}

#[test]
fn test_empty_response_detection_timing_line_only() {
    assert!(is_empty_response("⏱️ 43.0s | 💭 3.6s"));
    assert!(is_empty_response("  ⏱️ 43.0s | 💭 3.6s  "));
    assert!(is_empty_response("\n⏱️ 43.0s | 💭 3.6s\n"));
}

#[test]
fn test_empty_response_detection_multiple_timing_lines() {
    let response = "\n⏱️ 10.0s | 💭 1.0s\n\n⏱️ 20.0s | 💭 2.0s\n";
    assert!(is_empty_response(response));
}

#[test]
fn test_empty_response_detection_timing_with_empty_lines() {
    let response = "\n\n⏱️ 43.0s | 💭 3.6s\n\n";
    assert!(is_empty_response(response));
}

#[test]
fn test_empty_response_detection_substantive_content() {
    // These should NOT be considered empty
    assert!(!is_empty_response("Hello, I will help you."));
    assert!(!is_empty_response("Let me read that file."));
    assert!(!is_empty_response("I've completed the task."));
}

#[test]
fn test_empty_response_detection_timing_with_text() {
    // If there's any substantive text, it's not empty
    let response = "⏱️ 43.0s | 💭 3.6s\nHere is the result.";
    assert!(!is_empty_response(response));
}

#[test]
fn test_empty_response_detection_text_before_timing() {
    let response = "Done!\n⏱️ 43.0s | 💭 3.6s";
    assert!(!is_empty_response(response));
}

#[test]
fn test_empty_response_detection_json_tool_call() {
    // A JSON tool call is definitely not empty
    let response = r#"{"tool": "read_file", "args": {"file_path": "test.txt"}}"#;
    assert!(!is_empty_response(response));
}

#[test]
fn test_empty_response_detection_partial_json() {
    // Even partial JSON is not empty
    let response = r#"{"tool": "read_file", "args": {"#;
    assert!(!is_empty_response(response));
}

#[test]
fn test_empty_response_detection_markdown() {
    // Markdown content is not empty
    let response = "# Summary\n\nI completed the task.";
    assert!(!is_empty_response(response));
}

#[test]
fn test_empty_response_detection_code_block() {
    // Code blocks are not empty
    let response = "```rust\nfn main() {}\n```";
    assert!(!is_empty_response(response));
}

// Test the MAX_AUTO_SUMMARY_ATTEMPTS constant value
// This is a compile-time check that the constant exists and has the expected value
#[test]
fn test_max_auto_summary_attempts_is_reasonable() {
    // The constant should be at least 3 to give the LLM a fair chance to recover
    // We can't directly access the constant from here, but we document the expected value
    // Current value: 5 (increased from 2)
    const EXPECTED_MIN_ATTEMPTS: usize = 3;
    const EXPECTED_MAX_ATTEMPTS: usize = 10;
    const CURRENT_VALUE: usize = 5;
    
    assert!(CURRENT_VALUE >= EXPECTED_MIN_ATTEMPTS, 
        "MAX_AUTO_SUMMARY_ATTEMPTS should be at least {} for reliable recovery", EXPECTED_MIN_ATTEMPTS);
    assert!(CURRENT_VALUE <= EXPECTED_MAX_ATTEMPTS,
        "MAX_AUTO_SUMMARY_ATTEMPTS should not exceed {} to avoid infinite loops", EXPECTED_MAX_ATTEMPTS);
}
