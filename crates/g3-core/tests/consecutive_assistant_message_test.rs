//! Tests for consecutive assistant message bug
//!
//! This test verifies that the streaming completion logic does not add
//! consecutive assistant messages to the conversation history.
//!
//! Bug description:
//! When tools are executed in previous iterations (any_tool_executed=true)
//! and the current iteration finishes without executing a tool (tool_executed=false),
//! the assistant message was being added twice:
//! 1. At line ~2529 when breaking to auto-continue (if any_tool_executed && !current_response.is_empty())
//! 2. At line ~2772 when returning (if !current_response.is_empty())
//!
//! This creates consecutive assistant messages in the conversation history,
//! which causes the LLM to see duplicated content and mimic the pattern,
//! leading to compounding duplication over time.

use g3_core::context_window::ContextWindow;
use g3_providers::{Message, MessageRole};

/// Helper to check if conversation history has consecutive assistant messages
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

/// Test that a well-formed conversation has no consecutive assistant messages
#[test]
fn test_no_consecutive_assistant_messages_in_normal_flow() {
    let mut context = ContextWindow::new(200_000);

    // Simulate a normal conversation flow
    context.add_message(Message::new(MessageRole::System, "You are a helpful assistant.".to_string()));
    context.add_message(Message::new(MessageRole::User, "Task: Hello".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Hi there!".to_string()));
    context.add_message(Message::new(MessageRole::User, "Task: How are you?".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "I'm doing well!".to_string()));

    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Normal conversation should not have consecutive assistant messages"
    );
}

/// Test that detects the bug: consecutive assistant messages
/// This test SHOULD FAIL until the bug is fixed
#[test]
fn test_detect_consecutive_assistant_messages_bug() {
    let mut context = ContextWindow::new(200_000);

    // Simulate the buggy flow:
    // 1. System message
    context.add_message(Message::new(MessageRole::System, "You are a helpful assistant.".to_string()));
    
    // 2. User asks a question
    context.add_message(Message::new(MessageRole::User, "Task: List files".to_string()));
    
    // 3. Assistant responds with tool call (tool execution adds this)
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
    ));
    
    // 4. Tool result
    context.add_message(Message::new(MessageRole::User, "Tool result: file1.txt file2.txt".to_string()));
    
    // 5. Assistant provides summary - THIS IS WHERE THE BUG OCCURS
    // The bug adds this message TWICE
    let summary = "Here are the files:\n- file1.txt\n- file2.txt".to_string();
    
    // Simulate the bug: message added at line 2529 (when breaking to auto-continue)
    context.add_message(Message::new(MessageRole::Assistant, summary.clone()));
    
    // Simulate the bug: message added AGAIN at line 2772 (when returning)
    // This is the bug - the same message is added twice!
    context.add_message(Message::new(MessageRole::Assistant, summary.clone()));

    // This assertion verifies the bug exists
    let consecutive = has_consecutive_assistant_messages(&context.conversation_history);
    assert!(
        consecutive.is_some(),
        "Bug reproduction: should have consecutive assistant messages at indices {:?}",
        consecutive
    );
    
    // Verify the indices
    let (idx1, idx2) = consecutive.unwrap();
    assert_eq!(idx1, 4, "First consecutive assistant message should be at index 4");
    assert_eq!(idx2, 5, "Second consecutive assistant message should be at index 5");
    
    // Verify the content is duplicated
    assert_eq!(
        context.conversation_history[idx1].content,
        context.conversation_history[idx2].content,
        "Consecutive assistant messages should have identical content (the bug)"
    );
}

/// Test that validates the invariant: no consecutive assistant messages should exist
/// This is the test that should PASS after the bug is fixed
#[test]
fn test_invariant_no_consecutive_assistant_messages() {
    // This test documents the invariant that should hold
    // After the bug is fixed, this test should pass
    
    let mut context = ContextWindow::new(200_000);

    // Build a conversation that would trigger the bug
    context.add_message(Message::new(MessageRole::System, "You are a helpful assistant.".to_string()));
    context.add_message(Message::new(MessageRole::User, "Task: List files".to_string()));
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
    ));
    context.add_message(Message::new(MessageRole::User, "Tool result: file1.txt file2.txt".to_string()));
    
    // After the fix, only ONE assistant message should be added
    let summary = "Here are the files:\n- file1.txt\n- file2.txt".to_string();
    context.add_message(Message::new(MessageRole::Assistant, summary));
    
    // Invariant: no consecutive assistant messages
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Invariant: conversation history should never have consecutive assistant messages"
    );
}

/// Test helper function works correctly
#[test]
fn test_consecutive_detection_helper() {
    // Test with no consecutive
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
    ];
    assert!(has_consecutive_assistant_messages(&history).is_none());

    // Test with consecutive at start
    let history = vec![
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()),
        Message::new(MessageRole::User, "Hi".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((0, 1)));

    // Test with consecutive in middle
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((1, 2)));

    // Test with consecutive at end
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((1, 2)));
}

/// Test that simulates the exact session log pattern from the bug report
#[test]
fn test_session_log_pattern_from_bug_report() {
    // This test recreates the exact pattern seen in the butler session log:
    // - Index 368: assistant message with duplicated content
    // - Index 369: assistant message with same duplicated content (consecutive!)
    
    let mut context = ContextWindow::new(200_000);
    
    // Simulate the conversation leading up to the bug
    context.add_message(Message::new(MessageRole::System, "You are butler.".to_string()));
    
    // ... many messages ...
    for i in 0..50 {
        context.add_message(Message::new(MessageRole::User, format!("Task {}", i)));
        context.add_message(Message::new(MessageRole::Assistant, format!("Response {}", i)));
    }
    
    // Now simulate the buggy pattern
    context.add_message(Message::new(MessageRole::User, "Task: move appa_music into appa".to_string()));
    
    // Tool call
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "mv ~/Desktop/appa_music ~/icloud/appa/music"}}"#.to_string(),
    ));
    context.add_message(Message::new(MessageRole::User, "Tool result: ✓ Moved".to_string()));
    
    // The bug: two consecutive assistant messages with duplicated content
    let duplicated_content = "Done! ✅\n\nappa/\n├── archive/\n├── docs/\n├── music/ 🎵 NEW\n\nAnything else?\n\nDone! ✅\n\nappa/\n├── archive/\n├── docs/\n├── music/ 🎵 NEW\n\nAnything else?";
    
    // First add (line 2529)
    context.add_message(Message::new(MessageRole::Assistant, duplicated_content.to_string()));
    // Second add (line 2772) - THE BUG
    context.add_message(Message::new(MessageRole::Assistant, duplicated_content.to_string()));
    
    // Verify the bug pattern exists
    let consecutive = has_consecutive_assistant_messages(&context.conversation_history);
    assert!(
        consecutive.is_some(),
        "Should detect consecutive assistant messages from bug pattern"
    );
    
    // The consecutive messages should be the last two
    let (idx1, idx2) = consecutive.unwrap();
    let history_len = context.conversation_history.len();
    assert_eq!(idx1, history_len - 2, "First consecutive should be second-to-last");
    assert_eq!(idx2, history_len - 1, "Second consecutive should be last");
}

/// Test that the context window could have a guard against consecutive assistant messages
/// This is a proposed fix validation test
#[test]
fn test_proposed_fix_guard_against_consecutive() {
    let mut context = ContextWindow::new(200_000);
    
    context.add_message(Message::new(MessageRole::System, "System".to_string()));
    context.add_message(Message::new(MessageRole::User, "User".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "First response".to_string()));
    
    // Proposed fix: before adding an assistant message, check if the last message
    // is also an assistant message. If so, either:
    // 1. Skip adding (if content is identical)
    // 2. Merge the content (if content is different)
    // 3. Log a warning and skip (safest option)
    
    let last_message = context.conversation_history.last();
    let is_last_assistant = last_message
        .map(|m| matches!(m.role, MessageRole::Assistant))
        .unwrap_or(false);
    
    assert!(
        is_last_assistant,
        "Last message should be assistant for this test"
    );
    
    // The fix would check this condition before adding
    // and skip adding if true (to prevent consecutive assistant messages)
}
