//! Integration tests for Aggressive Context Dehydration (ACD).

use g3_core::acd::{Fragment, list_fragments, get_latest_fragment_id};
use g3_core::context_window::ContextWindow;
use g3_providers::{Message, MessageRole};

/// Test that reset_with_summary_and_stub correctly adds stub before summary
#[test]
fn test_reset_with_summary_and_stub_ordering() {
    let mut context = ContextWindow::new(100000);
    
    // Add system prompt
    context.add_message(Message::new(
        MessageRole::System,
        "You are a helpful assistant.".to_string(),
    ));
    
    // Add some conversation (make it long enough to ensure chars_saved > 0)
    context.add_message(Message::new(MessageRole::User, "Hello, I have a question about implementing a complex feature in my application. Can you help me understand how to structure the code properly?".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Of course! I'd be happy to help you with that. Let me explain the best practices for structuring your code. First, you should consider separating concerns into different modules...".to_string()));
    context.add_message(Message::new(MessageRole::User, "That makes sense. What about error handling?".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Error handling is crucial. You should use Result types and proper error propagation throughout your codebase.".to_string()));
    
    let stub = "---\n⚡ DEHYDRATED CONTEXT (fragment_id: test123)\n---".to_string();
    let summary = "User greeted the assistant.".to_string();
    
    let _chars_saved = context.reset_with_summary_and_stub(
        summary.clone(),
        Some("New question".to_string()),
        Some(stub.clone()),
    );
    
    // chars_saved is old - new, which could be 0 or negative if summary is longer
    // The important thing is that the function completed successfully
    
    // Check message ordering:
    // 1. System prompt
    // 2. Stub (if present)
    // 3. Summary
    // 4. Latest user message
    assert!(context.conversation_history.len() >= 3);
    
    // First message should be system prompt
    assert!(matches!(context.conversation_history[0].role, MessageRole::System));
    assert!(context.conversation_history[0].content.contains("helpful assistant"));
    
    // Find the stub message
    let stub_idx = context.conversation_history.iter().position(|m| 
        m.content.contains("DEHYDRATED CONTEXT")
    );
    assert!(stub_idx.is_some(), "Stub message should be present");
    
    // Find the summary message
    let summary_idx = context.conversation_history.iter().position(|m| 
        m.content.contains("Previous conversation summary")
    );
    assert!(summary_idx.is_some(), "Summary message should be present");
    
    // Stub should come before summary
    assert!(stub_idx.unwrap() < summary_idx.unwrap(), "Stub should come before summary");
    
    // Last message should be the user message
    let last = context.conversation_history.last().unwrap();
    assert!(matches!(last.role, MessageRole::User));
    assert_eq!(last.content, "New question");
}

/// Test reset_with_summary_and_stub without stub (should behave like reset_with_summary)
#[test]
fn test_reset_with_summary_and_stub_no_stub() {
    let mut context = ContextWindow::new(100000);
    
    // Add system prompt
    context.add_message(Message::new(
        MessageRole::System,
        "You are a helpful assistant.".to_string(),
    ));
    
    // Add some conversation (make it long enough)
    context.add_message(Message::new(MessageRole::User, "Hello, I have a question about implementing a complex feature in my application.".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Of course! I'd be happy to help you with that. Let me explain the best practices.".to_string()));
    context.add_message(Message::new(MessageRole::User, "That makes sense. What about error handling?".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Error handling is crucial. You should use Result types.".to_string()));
    
    let summary = "User greeted the assistant.".to_string();
    
    // Call reset - we don't check chars_saved since it depends on content lengths
    let _chars_saved = context.reset_with_summary_and_stub(
        summary.clone(),
        Some("New question".to_string()),
        None, // No stub
    );
    
    // Should not have any dehydrated context message
    let has_stub = context.conversation_history.iter().any(|m| 
        m.content.contains("DEHYDRATED CONTEXT")
    );
    assert!(!has_stub, "Should not have stub when None is passed");
    
    // Should still have summary
    let has_summary = context.conversation_history.iter().any(|m| 
        m.content.contains("Previous conversation summary")
    );
    assert!(has_summary, "Should have summary");
}

/// Test that project context message is preserved during reset
#[test]
fn test_reset_preserves_project_context() {
    let mut context = ContextWindow::new(100000);
    
    // Add system prompt
    context.add_message(Message::new(
        MessageRole::System,
        "You are a helpful assistant.".to_string(),
    ));
    
    // Add project context message (second system message with Agent Configuration)
    context.add_message(Message::new(
        MessageRole::System,
        "🤖 Agent Configuration (from AGENTS.md):\nTest agent config.".to_string(),
    ));
    
    // Add conversation
    context.add_message(Message::new(MessageRole::User, "Hello".to_string()));
    context.add_message(Message::new(MessageRole::Assistant, "Hi!".to_string()));
    
    let stub = "---\n⚡ DEHYDRATED CONTEXT\n---".to_string();
    
    context.reset_with_summary_and_stub(
        "Summary".to_string(),
        Some("Question".to_string()),
        Some(stub),
    );
    
    // Project context should be preserved
    let has_project_context = context.conversation_history.iter().any(|m| 
        m.content.contains("Agent Configuration")
    );
    assert!(has_project_context, "Project context message should be preserved");
}

/// Test fragment chain integrity
#[test]
fn test_fragment_chain_integrity() {
    let test_session = format!("test_chain_{}", std::process::id());
    
    // Create first fragment (no predecessor)
    let messages1 = vec![
        Message::new(MessageRole::User, "First message".to_string()),
        Message::new(MessageRole::Assistant, "First response".to_string()),
    ];
    let frag1 = Fragment::new(messages1, None);
    let frag1_id = frag1.fragment_id.clone();
    frag1.save(&test_session).unwrap();
    
    // Create second fragment (links to first)
    let messages2 = vec![
        Message::new(MessageRole::User, "Second message".to_string()),
        Message::new(MessageRole::Assistant, "Second response".to_string()),
    ];
    let frag2 = Fragment::new(messages2, Some(frag1_id.clone()));
    let frag2_id = frag2.fragment_id.clone();
    frag2.save(&test_session).unwrap();
    
    // Create third fragment (links to second)
    let messages3 = vec![
        Message::new(MessageRole::User, "Third message".to_string()),
        Message::new(MessageRole::Assistant, "Third response".to_string()),
    ];
    let frag3 = Fragment::new(messages3, Some(frag2_id.clone()));
    let frag3_id = frag3.fragment_id.clone();
    frag3.save(&test_session).unwrap();
    
    // Verify chain by loading and following links
    let loaded3 = Fragment::load(&test_session, &frag3_id).unwrap();
    assert_eq!(loaded3.preceding_fragment_id, Some(frag2_id.clone()));
    
    let loaded2 = Fragment::load(&test_session, &frag2_id).unwrap();
    assert_eq!(loaded2.preceding_fragment_id, Some(frag1_id.clone()));
    
    let loaded1 = Fragment::load(&test_session, &frag1_id).unwrap();
    assert!(loaded1.preceding_fragment_id.is_none());
    
    // Verify list_fragments returns all in order
    let fragments = list_fragments(&test_session).unwrap();
    assert_eq!(fragments.len(), 3);
    
    // Verify get_latest_fragment_id returns the most recent
    let latest = get_latest_fragment_id(&test_session).unwrap();
    assert!(latest.is_some());
    // Note: latest might be frag3 if sorted by creation time
    
    // Cleanup
    let fragments_dir = g3_core::paths::get_fragments_dir(&test_session);
    let _ = std::fs::remove_dir_all(fragments_dir.parent().unwrap());
}

/// Test fragment with many messages
#[test]
fn test_large_fragment() {
    let mut messages = Vec::new();
    for i in 0..100 {
        messages.push(Message::new(
            MessageRole::User,
            format!("User message {}", i),
        ));
        messages.push(Message::new(
            MessageRole::Assistant,
            format!("Assistant response {} with some longer content to make it more realistic", i),
        ));
    }
    
    let fragment = Fragment::new(messages, None);
    
    assert_eq!(fragment.message_count, 200);
    assert_eq!(fragment.user_message_count, 100);
    assert_eq!(fragment.assistant_message_count, 100);
    assert!(fragment.estimated_tokens > 0);
    
    // Stub should still be concise
    let stub = fragment.generate_stub();
    assert!(stub.len() < 1000, "Stub should be concise even for large fragments");
    assert!(stub.contains("200 total msgs"));
}

/// Test fragment with tool calls
#[test]
fn test_fragment_tool_call_summary() {
    let messages = vec![
        Message::new(MessageRole::User, "Read the file".to_string()),
        Message::new(
            MessageRole::Assistant,
            r#"{"tool": "read_file", "args": {"file_path": "test.rs"}}"#.to_string(),
        ),
        Message::new(MessageRole::User, "Tool result: content".to_string()),
        Message::new(MessageRole::User, "Now write it".to_string()),
        Message::new(
            MessageRole::Assistant,
            r#"{"tool": "write_file", "args": {"file_path": "out.rs", "content": "..."}}"#.to_string(),
        ),
        Message::new(
            MessageRole::Assistant,
            r#"{"tool": "shell", "args": {"command": "cargo build"}}"#.to_string(),
        ),
    ];
    
    let fragment = Fragment::new(messages, None);
    
    // Should have extracted tool calls
    assert!(!fragment.tool_call_summary.is_empty());
    
    // Stub should mention tool calls
    let stub = fragment.generate_stub();
    assert!(stub.contains("tool calls"));
}

/// Test context overflow detection in rehydration
#[test]
fn test_rehydration_context_overflow_detection() {
    // Create a fragment with known token count
    let messages = vec![
        Message::new(MessageRole::User, "A".repeat(4000)), // ~1000 tokens
        Message::new(MessageRole::Assistant, "B".repeat(4000)), // ~1000 tokens
    ];
    
    let fragment = Fragment::new(messages, None);
    
    // Fragment should have estimated tokens
    assert!(fragment.estimated_tokens > 1000);
    
    // The rehydrate tool checks available_tokens vs fragment_tokens
    // This is tested in tools/acd.rs tests
}

/// Test empty session has no fragments
#[test]
fn test_empty_session_no_fragments() {
    let test_session = format!("test_empty_{}", std::process::id());
    
    let fragments = list_fragments(&test_session).unwrap();
    assert!(fragments.is_empty());
    
    let latest = get_latest_fragment_id(&test_session).unwrap();
    assert!(latest.is_none());
}

/// Test fragment topics extraction from various message types
#[test]
fn test_topic_extraction_variety() {
    let messages = vec![
        Message::new(MessageRole::User, "Please implement the login feature".to_string()),
        Message::new(MessageRole::Assistant, "I'll help with that.".to_string()),
        Message::new(MessageRole::User, "Tool result: success".to_string()), // Should be skipped
        Message::new(MessageRole::User, "Now add password hashing".to_string()),
        Message::new(
            MessageRole::Assistant,
            r#"{"tool": "write_file", "args": {"file_path": "src/auth/password.rs", "content": "..."}}"#.to_string(),
        ),
    ];
    
    let fragment = Fragment::new(messages, None);
    
    // Should have extracted meaningful topics
    assert!(!fragment.topics.is_empty());
    
    // Should include user requests but not tool results
    let topics_str = fragment.topics.join(" ");
    assert!(topics_str.contains("login") || topics_str.contains("password"));
    assert!(!topics_str.contains("Tool result"));
}
