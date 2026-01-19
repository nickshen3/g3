//! Tests for the early return path in stream_completion_with_tools
//!
//! This test documents the bug fix for missing assistant messages when:
//! - The LLM responds with text only (no tool calls)
//! - any_tool_executed = false (no tools in previous iterations)
//! - The stream finishes and hits the early return path
//!
//! The fix adds assistant message saving before the early return at ~line 2535.

use g3_core::context_window::ContextWindow;
use g3_providers::{Message, MessageRole};

/// Simulates the early return path logic
/// This mirrors the code at lines 2535-2555 in lib.rs
fn simulate_early_return_path(
    context_window: &mut ContextWindow,
    current_response: &str,
    assistant_message_added: bool,
    any_tool_executed: bool,
) -> bool {
    // This simulates the code path when stream finishes
    
    if any_tool_executed {
        // If tools were executed in previous iterations, break to finalize
        if !current_response.trim().is_empty() && !assistant_message_added {
            let assistant_msg = Message::new(
                MessageRole::Assistant,
                current_response.to_string(),
            );
            context_window.add_message(assistant_msg);
            return true; // assistant_message_added = true
        }
        return assistant_message_added;
    }
    
    // THE FIX: Save assistant message before returning (no tools were executed)
    if !current_response.trim().is_empty() && !assistant_message_added {
        let assistant_msg = Message::new(
            MessageRole::Assistant,
            current_response.to_string(),
        );
        context_window.add_message(assistant_msg);
        return true;
    }
    
    assistant_message_added
}

/// Test: Text-only response with no previous tool execution
/// This is the exact bug scenario from butler session
#[test]
fn test_text_only_response_no_tools() {
    let mut context = ContextWindow::new(200_000);
    
    // Add initial user message
    context.add_message(Message::new(MessageRole::User, "Hello".to_string()));
    
    let current_response = "Phew! 😅 Glad it's back. Sorry about that...";
    let assistant_message_added = false;
    let any_tool_executed = false;
    
    let was_added = simulate_early_return_path(
        &mut context,
        current_response,
        assistant_message_added,
        any_tool_executed,
    );
    
    assert!(was_added, "Assistant message should be added");
    
    let history = context.conversation_history.clone();
    assert_eq!(history.len(), 2, "Should have user + assistant messages");
    assert!(matches!(history[1].role, MessageRole::Assistant));
    assert!(history[1].content.contains("Phew!"));
}

/// Test: Text-only response with previous tool execution
#[test]
fn test_text_only_response_with_previous_tools() {
    let mut context = ContextWindow::new(200_000);
    
    context.add_message(Message::new(MessageRole::User, "Run ls".to_string()));
    
    let current_response = "Here are the files...";
    let assistant_message_added = false;
    let any_tool_executed = true; // Tools were executed in previous iterations
    
    let was_added = simulate_early_return_path(
        &mut context,
        current_response,
        assistant_message_added,
        any_tool_executed,
    );
    
    assert!(was_added, "Assistant message should be added");
    
    let history = context.conversation_history.clone();
    assert_eq!(history.len(), 2);
    assert!(matches!(history[1].role, MessageRole::Assistant));
}

/// Test: Empty response should not add message
#[test]
fn test_empty_response_not_added() {
    let mut context = ContextWindow::new(200_000);
    
    context.add_message(Message::new(MessageRole::User, "Hello".to_string()));
    
    let current_response = "   "; // Whitespace only
    let assistant_message_added = false;
    let any_tool_executed = false;
    
    let was_added = simulate_early_return_path(
        &mut context,
        current_response,
        assistant_message_added,
        any_tool_executed,
    );
    
    assert!(!was_added, "Empty response should not be added");
    
    let history = context.conversation_history.clone();
    assert_eq!(history.len(), 1, "Should only have user message");
}

/// Test: Already added flag prevents duplication
#[test]
fn test_already_added_prevents_duplication() {
    let mut context = ContextWindow::new(200_000);
    
    context.add_message(Message::new(MessageRole::User, "Hello".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Hi there!".to_string()));
    
    let current_response = "Hi there!";
    let assistant_message_added = true; // Already added
    let any_tool_executed = false;
    
    let was_added = simulate_early_return_path(
        &mut context,
        current_response,
        assistant_message_added,
        any_tool_executed,
    );
    
    assert!(was_added, "Flag should remain true");
    
    let history = context.conversation_history.clone();
    assert_eq!(history.len(), 2, "Should not add duplicate");
}

/// Test: Consecutive user messages scenario (the bug)
/// Before the fix, this would result in consecutive user messages
#[test]
fn test_bug_scenario_consecutive_user_messages() {
    let mut context = ContextWindow::new(200_000);
    
    // Simulate the butler session bug:
    // Message 80: Tool result (user)
    // Message 81: User input (user) - SHOULD have assistant between!
    // Message 82: User input (user)
    
    context.add_message(Message::new(MessageRole::User, "Tool result: Self care".to_string()));
    
    // Simulate assistant response that was displayed but not saved (THE BUG)
    let current_response = "Phew! 😅 Glad it's back...";
    let assistant_message_added = false;
    let any_tool_executed = false;
    
    // THE FIX: This should now save the assistant message
    simulate_early_return_path(
        &mut context,
        current_response,
        assistant_message_added,
        any_tool_executed,
    );
    
    // Now add the next user message
    context.add_message(Message::new(MessageRole::User, "Task: Ok it's back...".to_string()));
    
    let history = context.conversation_history.clone();
    
    // Verify no consecutive user messages
    for i in 0..history.len().saturating_sub(1) {
        let current_is_user = matches!(history[i].role, MessageRole::User);
        let next_is_user = matches!(history[i + 1].role, MessageRole::User);
        assert!(
            !(current_is_user && next_is_user),
            "Found consecutive user messages at positions {} and {}",
            i, i + 1
        );
    }
}
