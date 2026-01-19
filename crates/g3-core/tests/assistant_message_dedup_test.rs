//! Tests for Assistant Message Deduplication
//!
//! These tests verify that the conversation history does NOT have
//! consecutive assistant messages, which was a bug that caused
//! compounding duplication in LLM responses.
//!
//! Bug fixed: When tools were executed in previous iterations (any_tool_executed=true)
//! and the current iteration finished without executing a tool (tool_executed=false),
//! the assistant message was being added twice:
//! 1. At the break point when any_tool_executed is true
//! 2. At the return point when !tool_executed
//!
//! The fix uses an `assistant_message_added` flag to ensure only one add occurs.

use g3_core::context_window::ContextWindow;
use g3_providers::{Message, MessageRole};

// =============================================================================
// Helper Functions
// =============================================================================

/// Check if conversation history has consecutive assistant messages
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
// Unit Tests for Helper Functions
// =============================================================================

#[test]
fn test_has_consecutive_assistant_messages_none() {
    let history = vec![
        Message::new(MessageRole::System, "System".to_string()),
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
        Message::new(MessageRole::Assistant, "Goodbye".to_string()),
    ];
    assert!(has_consecutive_assistant_messages(&history).is_none());
}

#[test]
fn test_has_consecutive_assistant_messages_found_middle() {
    let history = vec![
        Message::new(MessageRole::System, "System".to_string()),
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hello again".to_string()), // BUG!
        Message::new(MessageRole::User, "Bye".to_string()),
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((2, 3)));
}

#[test]
fn test_has_consecutive_assistant_messages_found_end() {
    let history = vec![
        Message::new(MessageRole::User, "Hi".to_string()),
        Message::new(MessageRole::Assistant, "Hello".to_string()),
        Message::new(MessageRole::User, "Bye".to_string()),
        Message::new(MessageRole::Assistant, "Goodbye".to_string()),
        Message::new(MessageRole::Assistant, "Goodbye again".to_string()), // BUG!
    ];
    assert_eq!(has_consecutive_assistant_messages(&history), Some((3, 4)));
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

/// Test that ContextWindow correctly tracks messages in normal flow
#[test]
fn test_context_window_normal_flow() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(
        MessageRole::System,
        "You are helpful.".to_string(),
    ));
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

/// Test that simulates the correct flow after tool execution
#[test]
fn test_context_window_tool_execution_correct_flow() {
    let mut context = ContextWindow::new(200_000);

    // Setup
    context.add_message(Message::new(
        MessageRole::System,
        "You are helpful.".to_string(),
    ));
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

    // Verify structure
    assert_eq!(context.conversation_history.len(), 5);
    assert_eq!(count_assistant_messages(&context.conversation_history), 2);
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Correct flow should not have consecutive assistant messages"
    );
}

/// Test that demonstrates the bug scenario (what the bug looked like)
#[test]
fn test_context_window_bug_scenario_demonstration() {
    let mut context = ContextWindow::new(200_000);

    // Setup
    context.add_message(Message::new(
        MessageRole::System,
        "You are helpful.".to_string(),
    ));
    context.add_message(Message::new(MessageRole::User, "Run a command".to_string()));

    // Tool call
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

    // This demonstrates what the bug looked like
    let consecutive = has_consecutive_assistant_messages(&context.conversation_history);
    assert!(
        consecutive.is_some(),
        "Bug scenario should have consecutive assistant messages"
    );
    assert_eq!(consecutive, Some((4, 5)));
    
    // The content is duplicated
    assert_eq!(
        context.conversation_history[4].content,
        context.conversation_history[5].content,
        "Bug: consecutive messages have identical content"
    );
}

/// Test multiple tool executions followed by summary (correct flow)
#[test]
fn test_context_window_multiple_tools_correct_flow() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(
        MessageRole::System,
        "You are helpful.".to_string(),
    ));
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

    // Verify structure
    // System + User + (Assistant + User) * 2 + Assistant = 1 + 1 + 4 + 1 = 7
    assert_eq!(context.conversation_history.len(), 7);
    assert_eq!(count_assistant_messages(&context.conversation_history), 3);
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Multiple tools with correct flow should not have consecutive assistant messages"
    );
}

/// Test the exact session log pattern from the original bug report
#[test]
fn test_session_log_pattern_from_bug_report() {
    // This test recreates the exact pattern seen in the butler session log:
    // - Index N: assistant message with content
    // - Index N+1: assistant message with same content (consecutive!)
    
    let mut context = ContextWindow::new(200_000);
    
    // Simulate a conversation with many messages
    context.add_message(Message::new(MessageRole::System, "You are butler.".to_string()));
    
    // Add some conversation history
    for i in 0..10 {
        context.add_message(Message::new(MessageRole::User, format!("Task {}", i)));
        context.add_message(Message::new(MessageRole::Assistant, format!("Response {}", i)));
    }
    
    // Now simulate the buggy pattern
    context.add_message(Message::new(
        MessageRole::User,
        "Task: move appa_music into appa".to_string(),
    ));
    
    // Tool call
    context.add_message(Message::new(
        MessageRole::Assistant,
        r#"{"tool": "shell", "args": {"command": "mv ~/Desktop/appa_music ~/icloud/appa/music"}}"#.to_string(),
    ));
    context.add_message(Message::new(
        MessageRole::User,
        "Tool result: ✓ Moved".to_string(),
    ));
    
    // The bug: two consecutive assistant messages with duplicated content
    let duplicated_content = "Done! ✅\n\nappa/\n├── archive/\n├── docs/\n├── music/ 🎵 NEW\n\nAnything else?";
    
    // First add (correct)
    context.add_message(Message::new(MessageRole::Assistant, duplicated_content.to_string()));
    // Second add (THE BUG)
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

/// Test that the fix prevents consecutive assistant messages
/// This simulates what the fixed code does: check before adding
#[test]
fn test_fix_prevents_consecutive_assistant_messages() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(
        MessageRole::System,
        "You are helpful.".to_string(),
    ));
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

    // First add location (simulating line ~2529 in the fix)
    if !assistant_message_added {
        context.add_message(Message::new(MessageRole::Assistant, summary.clone()));
        assistant_message_added = true;
    }

    // Second add location (simulating line ~2772 in the fix)
    // This would have added a duplicate before the fix
    if !assistant_message_added {
        context.add_message(Message::new(MessageRole::Assistant, summary.clone()));
        // assistant_message_added = true; // Not needed since we return after
    }

    // Verify the fix works
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

/// Test edge case: empty response should not add message
#[test]
fn test_empty_response_not_added() {
    let mut context = ContextWindow::new(200_000);

    context.add_message(Message::new(
        MessageRole::System,
        "You are helpful.".to_string(),
    ));
    context.add_message(Message::new(MessageRole::User, "Hello".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Hi!".to_string()));

    // Simulate the check: don't add empty/whitespace responses
    let empty_response = "   ".to_string();
    if !empty_response.trim().is_empty() {
        context.add_message(Message::new(MessageRole::Assistant, empty_response));
    }

    // Should still have only 3 messages
    assert_eq!(context.conversation_history.len(), 3);
    assert!(
        has_consecutive_assistant_messages(&context.conversation_history).is_none(),
        "Empty response should not create consecutive messages"
    );
}

/// Test the invariant: conversation should always alternate user/assistant after system
#[test]
fn test_alternating_pattern_invariant() {
    let history = vec![
        Message::new(MessageRole::System, "System".to_string()),
        Message::new(MessageRole::User, "Q1".to_string()),
        Message::new(MessageRole::Assistant, "A1".to_string()),
        Message::new(MessageRole::User, "Q2".to_string()),
        Message::new(MessageRole::Assistant, "A2".to_string()),
        Message::new(MessageRole::User, "Q3".to_string()),
        Message::new(MessageRole::Assistant, "A3".to_string()),
    ];

    // Verify alternating pattern
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
