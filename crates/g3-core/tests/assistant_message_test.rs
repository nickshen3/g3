//! Tests for Assistant Message Handling
//!
//! These tests verify correct handling of assistant messages in conversation history:
//! 1. No consecutive assistant messages (deduplication bug)
//! 2. Proper user/assistant alternation
//! 3. Missing assistant message fallback logic
//!
//! The original bugs:
//! - Consecutive assistant messages: When tools were executed in previous iterations
//!   and the current iteration finished without executing a tool, the assistant
//!   message was being added twice.
//! - Missing assistant messages: When the LLM responded with text-only (no tool calls),
//!   the assistant message was sometimes not saved because the code checked
//!   `raw_clean.trim().is_empty()` after already confirming `current_response` had content.
//!
//! These bugs are now tested through the public API in mock_provider_integration_test.rs.
//! This file contains unit tests for the helper functions and invariants.

use g3_core::context_window::ContextWindow;
use g3_providers::{Message, MessageRole};

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if conversation history has consecutive assistant messages.
/// Returns the indices of the first consecutive pair found, if any.
fn has_consecutive_assistant_messages(history: &[Message]) -> Option<(usize, usize)> {
    for i in 0..history.len().saturating_sub(1) {
        if matches!(history[i].role, MessageRole::Assistant)
            && matches!(history[i + 1].role, MessageRole::Assistant)
        {
            return Some((i, i + 1));
        }
    }
    None
}

/// Count assistant messages in history
fn count_assistant_messages(history: &[Message]) -> usize {
    history
        .iter()
        .filter(|m| matches!(m.role, MessageRole::Assistant))
        .count()
}

// =============================================================================
// Helper Function Tests
// =============================================================================

#[test]
fn test_consecutive_detection_no_consecutive() {
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
    ];
    assert!(has_consecutive_assistant_messages(&history).is_none());
}

#[test]
fn test_consecutive_detection_at_start() {
    let history = vec![
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()),
        Message::new(MessageRole::User, "Hi".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((0, 1)));
}

#[test]
fn test_consecutive_detection_in_middle() {
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((1, 2)));
}

#[test]
fn test_consecutive_detection_at_end() {
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((1, 2)));
}

#[test]
fn test_count_assistant_messages() {
    let history = vec![
        Message::new(MessageRole::System, "System".to_string()),
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
        Message::new(MessageRole::Assistant, "Goodbye".to_string()),
    ];
    assert_eq!(count_assistant_messages(&history), 2);
}

// =============================================================================
// ContextWindow Unit Tests
// =============================================================================

#[test]
fn test_normal_conversation_flow() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(MessageRole::System, "You are helpful.".to_string()));
    context.add_message(Message::new(MessageRole::User, "Hello".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Hi there!".to_string()));
    context.add_message(Message::new(MessageRole::User, "How are you?".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "I'm doing well!".to_string()));

    assert_eq!(context.conversation_history.len(), 5);
    assert_eq!(count_assistant_messages(&context.conversation_history), 2);
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Normal conversation should not have consecutive assistant messages"
    );
}

#[test]
fn test_tool_execution_correct_flow() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(MessageRole::System, "You are helpful.".to_string()));
    context.add_message(Message::new(MessageRole::User, "Run a command".to_string()));

    // Tool call (assistant message with tool JSON)
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
    ));

    // Tool result (user message)
    context.add_message(Message::new(
        MessageRole::User,
        "Tool result: file1.txt file2.txt".to_string(),
    ));

    // Summary - should only be added ONCE
    context.add_message(Message::new(
        MessageRole::Assistant,
        "Here are the files: file1.txt, file2.txt".to_string(),
    ));

    assert_eq!(context.conversation_history.len(), 5);
    assert_eq!(count_assistant_messages(&context.conversation_history), 2);
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Correct flow should not have consecutive assistant messages"
    );
}

#[test]
fn test_multiple_tools_correct_flow() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(MessageRole::System, "You are helpful.".to_string()));
    context.add_message(Message::new(
        MessageRole::User,
        "List files and show current directory".to_string(),
    ));

    // First tool call
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
    ));
    context.add_message(Message::new(
        MessageRole::User,
        "Tool result: file1.txt file2.txt".to_string(),
    ));

    // Second tool call
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "pwd"}}"#.to_string(),
    ));
    context.add_message(Message::new(
        MessageRole::User,
        "Tool result: /home/user".to_string(),
    ));

    // Final summary - only ONE
    context.add_message(Message::new(
        MessageRole::Assistant,
        "Files: file1.txt, file2.txt. Current directory: /home/user".to_string(),
    ));

    // System + User + (Assistant + User) * 2 + Assistant = 1 + 1 + 4 + 1 = 7
    assert_eq!(context.conversation_history.len(), 7);
    assert_eq!(count_assistant_messages(&context.conversation_history), 3);
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Multiple tools with correct flow should not have consecutive assistant messages"
    );
}

// =============================================================================
// Bug Demonstration Tests (document what the bugs looked like)
// =============================================================================

#[test]
fn test_bug_demonstration_consecutive_messages() {
    // This test demonstrates what the consecutive message bug looked like.
    // The bug is now fixed and tested through mock_provider_integration_test.rs.
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(MessageRole::System, "You are helpful.".to_string()));
    context.add_message(Message::new(MessageRole::User, "Run a command".to_string()));
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
    ));
    context.add_message(Message::new(
        MessageRole::User,
        "Tool result: file1.txt".to_string(),
    ));

    // THE BUG: Summary added TWICE
    let summary = "Here are the files: file1.txt".to_string();
    context.add_message(Message::new(MessageRole::Assistant, summary.clone()));
    context.add_message(Message::new(MessageRole::Assistant, summary.clone())); // BUG!

    // Verify the bug pattern
    let consecutive = has_consecutive_assistant_messages(&context.conversation_history);
    assert!(consecutive.is_some(), "Bug scenario should have consecutive assistant messages");
    assert_eq!(consecutive, Some((4, 5)));

    // The content is duplicated
    assert_eq!(
        context.conversation_history[4].content,
        context.conversation_history[5].content,
        "Bug: consecutive messages have identical content"
    );
}

// =============================================================================
// Invariant Tests
// =============================================================================

#[test]
fn test_alternating_pattern_invariant() {
    // After the system message, conversation should alternate user/assistant
    let history = vec![
        Message::new(MessageRole::System, "System".to_string()),
        Message::new(MessageRole::User, "Q1".to_string()),
        Message::new(MessageRole::Assistant, "A1".to_string()),
        Message::new(MessageRole::User, "Q2".to_string()),
        Message::new(MessageRole::Assistant, "A2".to_string()),
        Message::new(MessageRole::User, "Q3".to_string()),
        Message::new(MessageRole::Assistant, "A3".to_string()),
    ];

    for i in 1..history.len() - 1 {
        let current = &history[i].role;
        let next = &history[i + 1].role;

        if matches!(current, MessageRole::User) {
            assert!(
                matches!(next, MessageRole::Assistant),
                "User at {} should be followed by Assistant",
                i
            );
        }
        if matches!(current, MessageRole::Assistant) {
            assert!(
                matches!(next, MessageRole::User),
                "Assistant at {} should be followed by User",
                i
            );
        }
    }
}

#[test]
fn test_fix_prevents_consecutive_messages() {
    // This simulates what the fixed code does: use a flag to track if message was added
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(MessageRole::System, "You are helpful.".to_string()));
    context.add_message(Message::new(MessageRole::User, "Run a command".to_string()));
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
    ));
    context.add_message(Message::new(
        MessageRole::User,
        "Tool result: file1.txt".to_string(),
    ));

    // Simulate the fix: use a flag to track if message was added
    let mut assistant_message_added = false;
    let summary = "Here are the files: file1.txt".to_string();

    // First add location
    if !assistant_message_added {
        context.add_message(Message::new(MessageRole::Assistant, summary.clone()));
        assistant_message_added = true;
    }

    // Second add location (would have added duplicate before the fix)
    if !assistant_message_added {
        context.add_message(Message::new(MessageRole::Assistant, summary.clone()));
    }

    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Fix should prevent consecutive assistant messages"
    );
    assert_eq!(
        count_assistant_messages(&context.conversation_history),
        2,
        "Should have exactly 2 assistant messages (tool call + summary)"
    );
}

// =============================================================================
// Missing Assistant Message Fallback Tests
// =============================================================================

#[test]
fn test_fallback_to_current_response_when_raw_empty() {
    // Simulate the scenario where raw_clean is empty but current_response has content
    let current_response = "Here's my helpful response!";
    let raw_clean = "";

    let content_to_save = if !raw_clean.trim().is_empty() {
        raw_clean.to_string()
    } else {
        current_response.to_string()
    };

    assert_eq!(content_to_save, current_response);
    assert!(!content_to_save.is_empty());
}

#[test]
fn test_prefer_raw_clean_when_available() {
    let current_response = "Filtered response";
    let raw_clean = "Raw response with tool JSON";

    let content_to_save = if !raw_clean.trim().is_empty() {
        raw_clean.to_string()
    } else {
        current_response.to_string()
    };

    assert_eq!(content_to_save, raw_clean);
}

#[test]
fn test_whitespace_raw_clean_triggers_fallback() {
    let current_response = "Actual content";
    let raw_clean = "   \n\t  ";

    let content_to_save = if !raw_clean.trim().is_empty() {
        raw_clean.to_string()
    } else {
        current_response.to_string()
    };

    assert_eq!(content_to_save, current_response);
}
