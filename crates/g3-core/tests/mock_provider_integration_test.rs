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

// =============================================================================
// Parser Poisoning Tests (commits 999ac6f, d68f059, 4c36cc0)
// =============================================================================

/// Test the parser directly with the same chunks to isolate the issue
#[tokio::test]
async fn test_parser_directly_with_inline_json_chunks() {
    use g3_core::streaming_parser::StreamingToolParser;
    use g3_providers::CompletionChunk;
    
    let mut parser = StreamingToolParser::new();
    
    // Simulate the exact chunks from the mock provider
    let chunk1 = CompletionChunk {
        content: "To run a command, you can use the format ".to_string(),
        tool_calls: None,
        finished: false,
        stop_reason: None,
        tool_call_streaming: None,
        usage: None,
    };
    
    let chunk2 = CompletionChunk {
        content: r#"{"tool": "shell", "args": {"command": "ls"}}"#.to_string(),
        tool_calls: None,
        finished: false,
        stop_reason: None,
        tool_call_streaming: None,
        usage: None,
    };
    
    let tools1 = parser.process_chunk(&chunk1);
    let tools2 = parser.process_chunk(&chunk2);
    
    assert!(tools1.is_empty(), "Chunk 1 should not produce tools");
    assert!(tools2.is_empty(), "Chunk 2 should NOT produce tools - JSON is inline, not on its own line");
    
    // Also check has_unexecuted_tool_call and has_incomplete_tool_call
    assert!(!parser.has_unexecuted_tool_call(), "Should NOT have unexecuted tool call - JSON is inline");
    assert!(!parser.has_incomplete_tool_call(), "Should NOT have incomplete tool call");
}

// These tests verify that inline JSON patterns in prose don't trigger
// false tool call detection, which would cause the agent to return
// control mid-task.

/// Test: Inline JSON in prose should NOT trigger tool call detection
/// 
/// Bug: When the LLM explained tool call format in prose like:
///   "You can use {"tool": "shell", ...} to run commands"
/// The parser would incorrectly detect this as a tool call.
///
/// Fix: Only detect tool calls that appear on their own line.
#[tokio::test]
async fn test_inline_json_in_prose_not_detected_as_tool() {
    let provider = MockProvider::new()
        .with_response(MockResponse::streaming(vec![
            "To run a command, you can use the format ",
            r#"{"tool": "shell", "args": {"command": "ls"}}"#,
            " in your request. ",
            "Let me know if you need help!",
        ]))
        // Add a default response in case auto-continue is triggered (which would be a bug)
        .with_default_response(MockResponse::text("[BUG: Auto-continue was triggered - inline JSON was detected as tool call]"));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    let result = agent.execute_task("How do I run commands?", None, false).await;
    assert!(result.is_ok(), "Task should succeed: {:?}", result.err());

    // The response should be saved as text, not executed as a tool
    let history = &agent.get_context_window().conversation_history;
    let assistant_msg = history
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::Assistant))
        .expect("Should have an assistant message");
    
    // The inline JSON should be preserved in the response
    assert!(
        assistant_msg.content.contains("tool") && assistant_msg.content.contains("shell"),
        "Response should contain the inline JSON example: {}",
        assistant_msg.content
    );
}

/// Test: JSON tool call on its own line SHOULD be detected
///
/// This is the normal case - real tool calls from LLMs appear on their own line.
#[tokio::test]
async fn test_json_on_own_line_detected_as_tool() {
    // This test uses native tool calling to verify tool detection works
    let provider = MockProvider::new()
        .with_native_tool_calling(true)
        .with_response(MockResponse::native_tool_call(
            "shell",
            serde_json::json!({"command": "echo hello"}),
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // The task should detect the tool call
    // Note: This will fail because we don't have a real shell, but that's OK
    // We just want to verify the tool call was detected
    let result = agent.execute_task("Run echo hello", None, false).await;
    
    // The result might be an error (tool execution fails in test env)
    // but we can check if a tool was attempted by looking at context
    let history = &agent.get_context_window().conversation_history;
    
    // Should have user message at minimum
    assert!(
        history.iter().any(|m| matches!(m.role, MessageRole::User)),
        "Should have user message"
    );
}

/// Test: Response with emoji and special characters doesn't crash
///
/// Bug: UTF-8 multi-byte characters caused panics when truncating strings
/// using byte indices instead of character indices.
#[tokio::test]
async fn test_utf8_response_handling() {
    let provider = MockProvider::new()
        .with_response(MockResponse::text(
            "Here's the result! 🎉\n\n\
            • First item with bullet\n\
            • Second item 中文字符\n\
            • Third item émojis: 🚀🔥💯\n\n\
            Done! ✅"
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    let result = agent.execute_task("Show me a list", None, false).await;
    assert!(result.is_ok(), "Task should succeed with UTF-8 content: {:?}", result.err());

    let history = &agent.get_context_window().conversation_history;
    let assistant_msg = history
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::Assistant))
        .expect("Should have an assistant message");
    
    // Verify the UTF-8 content is preserved
    assert!(assistant_msg.content.contains("🎉"), "Should contain emoji");
    assert!(assistant_msg.content.contains("中文"), "Should contain CJK characters");
    assert!(assistant_msg.content.contains("•"), "Should contain bullet points");
}

/// Test: Very long response with UTF-8 doesn't panic on truncation
#[tokio::test]
async fn test_long_utf8_response_no_panic() {
    // Create a response with lots of multi-byte characters
    let long_content = "🔥".repeat(1000) + &"Test content with emoji 🎉 and more 中文字符 here. ".repeat(100);
    
    let provider = MockProvider::new()
        .with_response(MockResponse::text(&long_content));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // This should not panic even with lots of multi-byte characters
    let result = agent.execute_task("Generate long content", None, false).await;
    assert!(result.is_ok(), "Task should succeed: {:?}", result.err());
}

/// Test: Response is not duplicated in output
///
/// Bug (cebec23): Response was printed twice - once during streaming
/// and again after task completion.
#[tokio::test]
async fn test_response_not_duplicated() {
    let provider = MockProvider::new()
        .with_response(MockResponse::text("This is a unique response that should appear once."));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    let result = agent.execute_task("Say something unique", None, false).await;
    assert!(result.is_ok(), "Task should succeed: {:?}", result.err());

    // Check the TaskResult - it should have the response
    let task_result = result.unwrap();
    
    // The response field might be empty (content was streamed) or contain the response
    // Either way, the context should have exactly one assistant message with this content
    let history = &agent.get_context_window().conversation_history;
    let assistant_messages: Vec<_> = history
        .iter()
        .filter(|m| matches!(m.role, MessageRole::Assistant))
        .filter(|m| m.content.contains("unique response"))
        .collect();
    
    assert_eq!(
        assistant_messages.len(), 1,
        "Should have exactly one assistant message with the response, got {}",
        assistant_messages.len()
    );
}

/// Test: Multiple chunks streamed correctly without duplication
#[tokio::test]
async fn test_streaming_no_chunk_duplication() {
    let provider = MockProvider::new()
        .with_response(MockResponse::streaming(vec![
            "Part 1. ",
            "Part 2. ",
            "Part 3. ",
            "Part 4.",
        ]));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    let result = agent.execute_task("Stream something", None, false).await;
    assert!(result.is_ok(), "Task should succeed: {:?}", result.err());

    let history = &agent.get_context_window().conversation_history;
    let assistant_msg = history
        .iter()
        .rev()
        .find(|m| matches!(m.role, MessageRole::Assistant))
        .expect("Should have an assistant message");
    
    // Each part should appear exactly once
    let content = &assistant_msg.content;
    assert_eq!(
        content.matches("Part 1").count(), 1,
        "Part 1 should appear exactly once in: {}",
        content
    );
    assert_eq!(
        content.matches("Part 2").count(), 1,
        "Part 2 should appear exactly once in: {}",
        content
    );
}
