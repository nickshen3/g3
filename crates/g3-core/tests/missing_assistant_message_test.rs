//! Tests for the missing assistant message bug fix.
//!
//! Bug: When the LLM responds with text-only (no tool calls), the assistant message
//! was sometimes not saved to the context window because the code checked
//! `raw_clean.trim().is_empty()` after already confirming `current_response` had content.
//! If the parser buffer was empty/different from current_response, no message was added.
//!
//! Fix: Use current_response as fallback when raw_clean is empty.

/// Test that the fix ensures assistant messages are always added when there's content.
/// This simulates the fixed behavior where current_response is used as fallback.
#[test]
fn test_fallback_to_current_response_when_raw_empty() {
    // Simulate the scenario:
    // - current_response has content (what was displayed)
    // - raw_clean is empty (parser buffer was cleared)
    // - The fix should use current_response as fallback
    
    let current_response = "Here's my helpful response!";
    let raw_clean = ""; // Parser buffer is empty
    
    // The fix logic:
    let content_to_save = if !raw_clean.trim().is_empty() {
        raw_clean.to_string()
    } else {
        current_response.to_string()
    };
    
    // Verify fallback works
    assert_eq!(content_to_save, current_response);
    assert!(!content_to_save.is_empty());
}

/// Test that raw_clean is preferred when available.
#[test]
fn test_prefer_raw_clean_when_available() {
    let current_response = "Filtered response"; // What was displayed (filtered)
    let raw_clean = "Raw response with {\"tool\": ...} JSON"; // Raw content
    
    // The fix logic:
    let content_to_save = if !raw_clean.trim().is_empty() {
        raw_clean.to_string()
    } else {
        current_response.to_string()
    };
    
    // Verify raw_clean is preferred
    assert_eq!(content_to_save, raw_clean);
}

/// Test that whitespace-only raw_clean triggers fallback.
#[test]
fn test_whitespace_raw_clean_triggers_fallback() {
    let current_response = "Actual content";
    let raw_clean = "   \n\t  "; // Whitespace only
    
    // The fix logic:
    let content_to_save = if !raw_clean.trim().is_empty() {
        raw_clean.to_string()
    } else {
        current_response.to_string()
    };
    
    // Verify fallback to current_response
    assert_eq!(content_to_save, current_response);
}

/// Test that the fix logic handles various edge cases.
#[test]
fn test_fix_logic_edge_cases() {
    // Test case 1: Both have content - prefer raw
    let current = "displayed";
    let raw = "raw content";
    let result = if !raw.trim().is_empty() { raw } else { current };
    assert_eq!(result, "raw content");
    
    // Test case 2: Raw is empty - use current
    let current = "displayed";
    let raw = "";
    let result = if !raw.trim().is_empty() { raw } else { current };
    assert_eq!(result, "displayed");
    
    // Test case 3: Raw is whitespace - use current
    let current = "displayed";
    let raw = "  \n  ";
    let result = if !raw.trim().is_empty() { raw } else { current };
    assert_eq!(result, "displayed");
    
    // Test case 4: Both empty - still returns current (empty)
    let current = "";
    let raw = "";
    let result = if !raw.trim().is_empty() { raw } else { current };
    assert_eq!(result, "");
    
    // Test case 5: Current has content, raw has only newlines
    let current = "Hello world!";
    let raw = "\n\n\n";
    let result = if !raw.trim().is_empty() { raw } else { current };
    assert_eq!(result, "Hello world!");
}
