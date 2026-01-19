//! Integration tests using MockProvider
//!
//! These tests use the mock provider to exercise real code paths in
//! stream_completion_with_tools without needing a real LLM.

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
    
    let agent = Agent::new_for_test(
        config,
        NullUiWriter,
        registry,
    ).await.expect("Failed to create agent");

    (agent, temp_dir)
}

/// Helper to count messages by role
fn count_by_role(history: &[Message], role: MessageRole) -> usize {
    history.iter().filter(|m| std::mem::discriminant(&m.role) == std::mem::discriminant(&role)).count()
}

/// Helper to check for consecutive user messages
fn has_consecutive_user_messages(history: &[Message]) -> Option<(usize, usize)> {
    for i in 0..history.len().saturating_sub(1) {
        if matches!(history[i].role, MessageRole::User) 
            && matches!(history[i + 1].role, MessageRole::User) 
        {
            return Some((i, i + 1));
        }
    }
    None
}

/// Test: Text-only response saves assistant message to context
///
/// This is the exact bug scenario from the butler session:
/// - User sends a message
/// - LLM responds with text only (no tool calls)
/// - Assistant message should be saved to context window
#[tokio::test]
async fn test_text_only_response_saves_to_context() {
    let provider = MockProvider::new()
        .with_response(MockResponse::text("Hello! I'm here to help."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Get initial message count
    let initial_count = agent.get_context_window().conversation_history.len();

    // Execute a task (this adds user message and gets response)
    let result = agent.execute_task("Hello", None, false).await;
    assert!(result.is_ok(), "Task should succeed: {:?}", result.err());

    // Check that messages were added
    let final_count = agent.get_context_window().conversation_history.len();
    assert!(
        final_count > initial_count,
        "Should have more messages after task, got {} -> {}",
        initial_count,
        final_count
    );

    // Verify the last message is from assistant
    let history = &agent.get_context_window().conversation_history;
    let last_msg = history.last().unwrap();
    assert!(
        matches!(last_msg.role, MessageRole::Assistant),
        "Last message should be assistant, got {:?}",
        last_msg.role
    );
}

/// Test: Multiple text-only responses maintain proper alternation
#[tokio::test]
async fn test_multi_turn_text_only_maintains_alternation() {
    let provider = MockProvider::new().with_responses(vec![
        MockResponse::text("First response"),
        MockResponse::text("Second response"),
        MockResponse::text("Third response"),
    ]);

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Execute three tasks
    agent.execute_task("First question", None, false).await.unwrap();
    agent.execute_task("Second question", None, false).await.unwrap();
    agent.execute_task("Third question", None, false).await.unwrap();

    // Verify no consecutive user messages
    let history = &agent.get_context_window().conversation_history;
    
    if let Some((i, j)) = has_consecutive_user_messages(history) {
        // Print debug info
        eprintln!("\n=== BUG: Consecutive user messages ===");
        for (idx, msg) in history.iter().enumerate() {
            let marker = if idx == i || idx == j { ">>>" } else { "   " };
            eprintln!("{} {}: {:?} - {}...", 
                marker, idx, msg.role, 
                msg.content.chars().take(50).collect::<String>()
            );
        }
        panic!("Found consecutive user messages at positions {} and {}", i, j);
    }
}

/// Test: Streaming response with multiple chunks saves correctly
#[tokio::test]
async fn test_streaming_chunks_save_complete_response() {
    let provider = MockProvider::new()
        .with_response(MockResponse::streaming(vec!["Hello ", "world ", "from ", "streaming!"]));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    agent.execute_task("Test streaming", None, false).await.unwrap();

    // Find the assistant message
    let history = &agent.get_context_window().conversation_history;
    let assistant_msg = history
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::Assistant))
        .expect("Should have an assistant message");
    
    // The complete streamed content should be saved
    assert!(
        assistant_msg.content.contains("Hello")
            && assistant_msg.content.contains("streaming"),
        "Should contain full streamed content: {}",
        assistant_msg.content
    );
}

/// Test: Truncated response (max_tokens) still saves
#[tokio::test]
async fn test_truncated_response_saves() {
    let provider = MockProvider::new()
        .with_response(MockResponse::truncated("This response was cut off mid-sent"));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    agent.execute_task("Generate a long response", None, false).await.unwrap();

    // Find the assistant message
    let history = &agent.get_context_window().conversation_history;
    let assistant_msg = history
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::Assistant))
        .expect("Should have an assistant message");
    
    assert!(
        assistant_msg.content.contains("cut off"),
        "Should save truncated content: {}",
        assistant_msg.content
    );
}

/// Test: The exact butler bug scenario
/// 
/// Scenario:
/// 1. User sends message
/// 2. LLM responds with text (no tools) - this was NOT being saved
/// 3. User sends another message
/// 4. Result: consecutive user messages in context (BUG)
#[tokio::test]
async fn test_butler_bug_scenario() {
    let provider = MockProvider::new().with_responses(vec![
        MockResponse::text("Phew! 😅 Glad it's back. Sorry about that - direct SQLite manipulation was too risky."),
        MockResponse::text("Yes, tasks with subtasks is a much safer approach!"),
    ]);

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Simulate the butler session:
    agent.execute_task(
        "Ok it's back. I have a different solution, instead of headings, what about tasks with inner subtasks?",
        None,
        false
    ).await.unwrap();

    agent.execute_task(
        "yep that's good enough for now",
        None,
        false
    ).await.unwrap();

    // Verify: no consecutive user messages
    let history = &agent.get_context_window().conversation_history;
    
    if let Some((i, j)) = has_consecutive_user_messages(history) {
        // Print debug info
        eprintln!("\n=== BUG DETECTED: Consecutive user messages ===");
        for (idx, msg) in history.iter().enumerate() {
            let marker = if idx == i || idx == j { ">>>" } else { "   " };
            eprintln!("{} {}: {:?} - {}...", 
                marker, idx, msg.role, 
                msg.content.chars().take(50).collect::<String>()
            );
        }
        panic!(
            "Found consecutive user messages at positions {} and {}",
            i, j
        );
    }
    
    // Also verify we have the expected assistant responses
    let assistant_count = count_by_role(history, MessageRole::Assistant);
    assert!(
        assistant_count >= 2,
        "Should have at least 2 assistant messages, got {}",
        assistant_count
    );
}
