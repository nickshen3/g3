//! Integration tests for context compaction
//!
//! These tests verify that compaction correctly preserves important messages
//! when summarizing conversation history.

use g3_core::ui_writer::NullUiWriter;
use g3_core::Agent;
use g3_providers::mock::{MockProvider, MockResponse};
use g3_providers::{Message, MessageRole, ProviderRegistry};
use tempfile::TempDir;

/// Helper to create an agent with a mock provider
async fn create_agent_with_mock(provider: MockProvider) -> (Agent<NullUiWriter>, TempDir) {
    let temp_dir = TempDir::new().unwrap();

    // Create a provider registry with the mock provider
    let mut registry = ProviderRegistry::new();
    registry.register(provider);

    // Create a minimal config
    let config = g3_config::Config::default();

    let agent = Agent::new_for_test(config, NullUiWriter, registry)
        .await
        .expect("Failed to create agent");

    (agent, temp_dir)
}

/// Helper to find the last assistant message in history
fn find_last_assistant_message(history: &[Message]) -> Option<&Message> {
    history
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::Assistant))
}

/// Helper to check if a message contains a substring
fn message_contains(history: &[Message], role: MessageRole, substring: &str) -> bool {
    history.iter().any(|m| {
        std::mem::discriminant(&m.role) == std::mem::discriminant(&role)
            && m.content.contains(substring)
    })
}

// =============================================================================
// Compaction Tests
// =============================================================================

/// Test: Last assistant message is preserved after compaction
///
/// This is the main feature test. After compaction:
/// 1. System prompt is preserved
/// 2. Summary is added
/// 3. Last assistant message should be preserved (NEW)
/// 4. Latest user message is preserved
///
/// The order should be:
/// [System] -> [Summary] -> [Last Assistant] -> [Latest User]
#[tokio::test]
async fn test_compaction_preserves_last_assistant_message() {
    // Create a provider that will:
    // 1. Respond to initial conversation
    // 2. Provide a summary when compaction is triggered
    let provider = MockProvider::new()
        // Response 1: Initial assistant response
        .with_response(MockResponse::text(
            "I understand you want to build a web server. Let me help you with that.",
        ))
        // Response 2: Second assistant response (this should be preserved after compaction)
        .with_response(MockResponse::text(
            "Here's the implementation plan:\n1. Create main.rs\n2. Add dependencies\n3. Implement routes",
        ))
        // Response 3: This will be the summary response during compaction
        .with_response(MockResponse::text(
            "Summary: User wants to build a web server. We discussed implementation plan with 3 steps.",
        ))
        // Response 4: Post-compaction response
        .with_response(MockResponse::text(
            "Continuing from where we left off...",
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Build up conversation history
    agent
        .execute_task("I want to build a web server in Rust", None, false)
        .await
        .unwrap();

    agent
        .execute_task("What's the implementation plan?", None, false)
        .await
        .unwrap();

    // Verify the last assistant message before compaction
    let history_before = agent.get_context_window().conversation_history.clone();
    let last_assistant_before = find_last_assistant_message(&history_before)
        .expect("Should have assistant message before compaction");
    assert!(
        last_assistant_before.content.contains("implementation plan"),
        "Last assistant message should contain 'implementation plan', got: {}",
        last_assistant_before.content
    );

    // Trigger manual compaction
    let compaction_result = agent.force_compact().await;
    assert!(
        compaction_result.is_ok(),
        "Compaction should succeed: {:?}",
        compaction_result.err()
    );
    assert!(
        compaction_result.unwrap(),
        "Compaction should return true on success"
    );

    // Verify the context after compaction
    let history_after = &agent.get_context_window().conversation_history;

    // Debug: Print the history after compaction
    eprintln!("\n=== History after compaction ===");
    for (i, msg) in history_after.iter().enumerate() {
        eprintln!(
            "  {}: {:?} - {}...",
            i,
            msg.role,
            msg.content.chars().take(80).collect::<String>()
        );
    }

    // 1. Should have a summary message
    assert!(
        message_contains(history_after, MessageRole::System, "Summary:"),
        "Should have summary message after compaction"
    );

    // 2. Should preserve the last assistant message content
    // This is the key assertion - the last assistant message should be preserved
    assert!(
        message_contains(history_after, MessageRole::Assistant, "implementation plan"),
        "Should preserve last assistant message with 'implementation plan' after compaction.\n\
         History: {:?}",
        history_after
            .iter()
            .map(|m| format!("{:?}: {}...", m.role, m.content.chars().take(50).collect::<String>()))
            .collect::<Vec<_>>()
    );

    // 3. Should preserve the latest user message
    assert!(
        message_contains(history_after, MessageRole::User, "implementation plan"),
        "Should preserve latest user message after compaction"
    );
}

/// Test: Compaction with no assistant messages doesn't crash
///
/// Edge case: If there are no assistant messages (e.g., fresh session),
/// compaction should still work without errors.
#[tokio::test]
async fn test_compaction_no_assistant_message() {
    // Provider that returns a summary
    let provider = MockProvider::new()
        .with_response(MockResponse::text("Summary: Empty conversation."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Add a user message directly to context without getting a response.
    // This simulates a state where we have user input but no assistant response yet.
    use g3_providers::Message;
    agent.add_message_to_context(Message::new(MessageRole::User, "Hello".to_string()));

    // Trigger compaction - should not crash
    let result = agent.force_compact().await;
    assert!(result.is_ok(), "Compaction should succeed even with no assistant messages");
}

/// Test: Compaction preserves tool-call-only assistant message
///
/// Even if the last assistant message is just a tool call (no prose),
/// it should still be preserved as it contains important context.
#[tokio::test]
async fn test_compaction_preserves_tool_call_only_message() {
    let provider = MockProvider::new()
        // Response 1: Text response
        .with_response(MockResponse::text("Let me check that file."))
        // Response 2: Tool call response (this is the last assistant message)
        .with_response(MockResponse::text_with_json_tool(
            "",  // No prose, just tool call
            "read_file",
            serde_json::json!({"file_path": "important.rs"}),
        ))
        // Response 3: Summary
        .with_response(MockResponse::text("Summary: User asked to check a file."))
        // Response 4: Post-compaction
        .with_response(MockResponse::text("Continuing..."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Build conversation
    agent.execute_task("Check the file", None, false).await.unwrap();
    agent.execute_task("Read important.rs", None, false).await.unwrap();

    // Get the last assistant message before compaction
    let history_before = agent.get_context_window().conversation_history.clone();
    let last_assistant_before = find_last_assistant_message(&history_before);
    
    // Verify we have an assistant message (might contain tool call JSON)
    assert!(
        last_assistant_before.is_some(),
        "Should have an assistant message before compaction"
    );

    // Trigger compaction
    let result = agent.force_compact().await;
    assert!(result.is_ok(), "Compaction should succeed");

    // The assistant message should be preserved (even if it's tool-call-only)
    let history_after = &agent.get_context_window().conversation_history;
    let has_assistant = history_after
        .iter()
        .any(|m| matches!(m.role, MessageRole::Assistant));
    
    assert!(
        has_assistant,
        "Should have at least one assistant message after compaction (the preserved last one)"
    );
}

/// Test: Compaction with multiple assistant messages preserves only the last one
///
/// When there are multiple assistant messages, only the most recent one
/// should be preserved (in addition to the summary).
#[tokio::test]
async fn test_compaction_preserves_only_last_assistant() {
    let provider = MockProvider::new()
        // Response 1: First assistant response
        .with_response(MockResponse::text("FIRST_RESPONSE: Hello!"))
        // Response 2: Second assistant response
        .with_response(MockResponse::text("SECOND_RESPONSE: How can I help?"))
        // Response 3: Third assistant response (this should be preserved)
        .with_response(MockResponse::text("THIRD_RESPONSE: Let me assist you."))
        // Response 4: Summary
        .with_response(MockResponse::text("Summary: Greeted user three times."))
        // Response 5: Post-compaction
        .with_response(MockResponse::text("Continuing..."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Build conversation with multiple exchanges
    agent.execute_task("Hi", None, false).await.unwrap();
    agent.execute_task("What can you do?", None, false).await.unwrap();
    agent.execute_task("Help me", None, false).await.unwrap();

    // Trigger compaction
    let result = agent.force_compact().await;
    assert!(result.is_ok(), "Compaction should succeed");

    let history_after = &agent.get_context_window().conversation_history;

    // Debug output
    eprintln!("\n=== History after compaction ===");
    for (i, msg) in history_after.iter().enumerate() {
        eprintln!(
            "  {}: {:?} - {}",
            i,
            msg.role,
            msg.content.chars().take(100).collect::<String>()
        );
    }

    // Should have THIRD_RESPONSE (the last one)
    assert!(
        message_contains(history_after, MessageRole::Assistant, "THIRD_RESPONSE"),
        "Should preserve the LAST assistant message (THIRD_RESPONSE)"
    );

    // Should NOT have FIRST_RESPONSE or SECOND_RESPONSE as separate messages
    // (they might be mentioned in the summary, but not as standalone assistant messages)
    let assistant_messages: Vec<_> = history_after
        .iter()
        .filter(|m| matches!(m.role, MessageRole::Assistant))
        .collect();
    
    // Should have exactly one assistant message (the preserved last one)
    assert_eq!(
        assistant_messages.len(),
        1,
        "Should have exactly one assistant message after compaction (the last one), got {}: {:?}",
        assistant_messages.len(),
        assistant_messages.iter().map(|m| &m.content).collect::<Vec<_>>()
    );
}

/// Test: Compaction without a trailing user message
///
/// Edge case: The last message in history is from the assistant (no user follow-up).
/// The assistant message should still be preserved.
#[tokio::test]
async fn test_compaction_no_trailing_user_message() {
    let provider = MockProvider::new()
        // Response 1: Assistant response (will be the last message)
        .with_response(MockResponse::text("LAST_ASSISTANT_MESSAGE: Here's your answer."))
        // Response 2: Summary
        .with_response(MockResponse::text("Summary: Provided an answer."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Single exchange - assistant response is the last message
    agent.execute_task("What's the answer?", None, false).await.unwrap();

    // Verify last message is from assistant
    let history_before = agent.get_context_window().conversation_history.clone();
    let last_msg = history_before.last().expect("Should have messages");
    assert!(
        matches!(last_msg.role, MessageRole::Assistant),
        "Last message should be from assistant before compaction"
    );

    // Trigger compaction
    let result = agent.force_compact().await;
    assert!(result.is_ok(), "Compaction should succeed");

    let history_after = &agent.get_context_window().conversation_history;

    // Should preserve the last assistant message
    assert!(
        message_contains(history_after, MessageRole::Assistant, "LAST_ASSISTANT_MESSAGE"),
        "Should preserve last assistant message even without trailing user message"
    );
}

/// Test: Message order after compaction is correct
///
/// The order should be:
/// [System Prompt] -> [README if present] -> [Summary] -> [Last Assistant] -> [Latest User if present]
#[tokio::test]
async fn test_compaction_message_order() {
    let provider = MockProvider::new()
        .with_response(MockResponse::text("ASSISTANT_TO_PRESERVE: I'll help you."))
        .with_response(MockResponse::text("SUMMARY_CONTENT: User asked for help."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    agent.execute_task("USER_MESSAGE_TO_PRESERVE: Help me", None, false).await.unwrap();

    // Trigger compaction
    agent.force_compact().await.unwrap();

    let history = &agent.get_context_window().conversation_history;

    // Debug output
    eprintln!("\n=== Message order after compaction ===");
    for (i, msg) in history.iter().enumerate() {
        eprintln!(
            "  {}: {:?} - {}",
            i,
            msg.role,
            msg.content.chars().take(60).collect::<String>()
        );
    }

    // Find indices of key messages
    let summary_idx = history
        .iter()
        .position(|m| m.content.contains("SUMMARY_CONTENT"))
        .expect("Should have summary");
    
    let assistant_idx = history
        .iter()
        .position(|m| matches!(m.role, MessageRole::Assistant) && m.content.contains("ASSISTANT_TO_PRESERVE"))
        .expect("Should have preserved assistant message");
    
    let user_idx = history
        .iter()
        .position(|m| matches!(m.role, MessageRole::User) && m.content.contains("USER_MESSAGE_TO_PRESERVE"));

    // Summary should come before assistant message
    assert!(
        summary_idx < assistant_idx,
        "Summary (idx {}) should come before assistant message (idx {})",
        summary_idx,
        assistant_idx
    );

    // If there's a user message, it should come after assistant message
    if let Some(user_idx) = user_idx {
        assert!(
            assistant_idx < user_idx,
            "Assistant message (idx {}) should come before user message (idx {})",
            assistant_idx,
            user_idx
        );
    }
}
