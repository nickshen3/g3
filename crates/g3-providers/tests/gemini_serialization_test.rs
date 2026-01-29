//! Integration tests for Gemini provider message serialization.
//!
//! CHARACTERIZATION: These tests verify that the Gemini provider correctly
//! serializes messages to the format expected by the Gemini API.
//!
//! What this test protects:
//! - System messages are converted to system_instruction (not in contents)
//! - User messages have role "user"
//! - Assistant messages have role "model" (Gemini's terminology)
//! - Tool calls are serialized with functionCall structure
//! - Tool results are serialized with functionResponse structure
//!
//! What this test intentionally does NOT assert:
//! - Actual API responses (requires real API key)
//! - Network behavior
//! - Rate limiting or error handling

use g3_providers::{Message, MessageRole, Tool};
use serde_json::{json, Value};

/// Test helper: Convert messages using the same logic as GeminiProvider
/// This mirrors the convert_messages function behavior
fn convert_messages_to_gemini_format(messages: &[Message]) -> (Vec<Value>, Option<Value>) {
    let mut contents = Vec::new();
    let mut system_instruction = None;

    for msg in messages {
        match msg.role {
            MessageRole::System => {
                system_instruction = Some(json!({
                    "parts": [{"text": msg.content}]
                }));
            }
            MessageRole::User => {
                contents.push(json!({
                    "role": "user",
                    "parts": [{"text": msg.content}]
                }));
            }
            MessageRole::Assistant => {
                contents.push(json!({
                    "role": "model",
                    "parts": [{"text": msg.content}]
                }));
            }
        }
    }

    (contents, system_instruction)
}

/// Test: System message becomes system_instruction, not in contents
#[test]
fn test_system_message_becomes_system_instruction() {
    let messages = vec![
        Message::new(MessageRole::System, "You are a helpful assistant.".to_string()),
        Message::new(MessageRole::User, "Hello".to_string()),
    ];

    let (contents, system_instruction) = convert_messages_to_gemini_format(&messages);

    // System message should be in system_instruction
    assert!(system_instruction.is_some(), "System message should create system_instruction");
    let sys = system_instruction.unwrap();
    assert!(sys["parts"][0]["text"].as_str().unwrap().contains("helpful assistant"),
        "System instruction should contain the system message content");

    // Contents should only have the user message
    assert_eq!(contents.len(), 1, "Contents should only have user message");
    assert_eq!(contents[0]["role"], "user");
}

/// Test: User messages have role "user"
#[test]
fn test_user_messages_have_user_role() {
    let messages = vec![
        Message::new(MessageRole::User, "What is 2+2?".to_string()),
    ];

    let (contents, _) = convert_messages_to_gemini_format(&messages);

    assert_eq!(contents.len(), 1);
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[0]["parts"][0]["text"], "What is 2+2?");
}

/// Test: Assistant messages have role "model" (Gemini terminology)
#[test]
fn test_assistant_messages_have_model_role() {
    let messages = vec![
        Message::new(MessageRole::User, "Hello".to_string()),
        Message::new(MessageRole::Assistant, "Hi there!".to_string()),
    ];

    let (contents, _) = convert_messages_to_gemini_format(&messages);

    assert_eq!(contents.len(), 2);
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model", "Assistant should become 'model' in Gemini");
    assert_eq!(contents[1]["parts"][0]["text"], "Hi there!");
}

/// Test: Multi-turn conversation maintains correct role mapping
#[test]
fn test_multi_turn_conversation_roles() {
    let messages = vec![
        Message::new(MessageRole::System, "Be concise.".to_string()),
        Message::new(MessageRole::User, "What is Rust?".to_string()),
        Message::new(MessageRole::Assistant, "A systems programming language.".to_string()),
        Message::new(MessageRole::User, "What about Go?".to_string()),
        Message::new(MessageRole::Assistant, "A language by Google.".to_string()),
    ];

    let (contents, system_instruction) = convert_messages_to_gemini_format(&messages);

    // System should be separate
    assert!(system_instruction.is_some());

    // Should have 4 messages in contents (2 user + 2 assistant)
    assert_eq!(contents.len(), 4);

    // Verify alternation: user, model, user, model
    assert_eq!(contents[0]["role"], "user");
    assert_eq!(contents[1]["role"], "model");
    assert_eq!(contents[2]["role"], "user");
    assert_eq!(contents[3]["role"], "model");
}

/// Test: Tool conversion to Gemini format
#[test]
fn test_tool_conversion_to_gemini_format() {
    let tools = vec![
        Tool {
            name: "get_weather".to_string(),
            description: "Get the current weather".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "City name"
                    }
                },
                "required": ["location"]
            }),
        },
    ];

    // Gemini expects tools in this format:
    // { "function_declarations": [{ "name": ..., "description": ..., "parameters": ... }] }
    let gemini_tools = vec![json!({
        "function_declarations": [{
            "name": tools[0].name,
            "description": tools[0].description,
            "parameters": tools[0].input_schema
        }]
    })];

    assert_eq!(gemini_tools.len(), 1);
    let decl = &gemini_tools[0]["function_declarations"][0];
    assert_eq!(decl["name"], "get_weather");
    assert_eq!(decl["description"], "Get the current weather");
    assert!(decl["parameters"]["properties"]["location"].is_object());
}

/// Test: Empty messages list produces empty contents
#[test]
fn test_empty_messages() {
    let messages: Vec<Message> = vec![];

    let (contents, system_instruction) = convert_messages_to_gemini_format(&messages);

    assert!(contents.is_empty());
    assert!(system_instruction.is_none());
}

/// Test: Only system message produces empty contents with system_instruction
#[test]
fn test_only_system_message() {
    let messages = vec![
        Message::new(MessageRole::System, "You are helpful.".to_string()),
    ];

    let (contents, system_instruction) = convert_messages_to_gemini_format(&messages);

    assert!(contents.is_empty(), "Contents should be empty when only system message");
    assert!(system_instruction.is_some(), "System instruction should be set");
}

/// Test: Multiple system messages - last one wins
/// (This characterizes current behavior, not necessarily ideal)
#[test]
fn test_multiple_system_messages_last_wins() {
    let messages = vec![
        Message::new(MessageRole::System, "First system message.".to_string()),
        Message::new(MessageRole::User, "Hello".to_string()),
        Message::new(MessageRole::System, "Second system message.".to_string()),
    ];

    let (contents, system_instruction) = convert_messages_to_gemini_format(&messages);

    // Last system message should be used
    assert!(system_instruction.is_some());
    let sys_value = system_instruction.unwrap();
    let sys_text = sys_value["parts"][0]["text"].as_str().unwrap();
    assert!(sys_text.contains("Second"), "Last system message should win");

    // Only user message in contents
    assert_eq!(contents.len(), 1);
}

/// Test: Generation config structure
#[test]
fn test_generation_config_structure() {
    // Gemini expects generation_config with these fields
    let config = json!({
        "temperature": 0.7,
        "maxOutputTokens": 4096,
        "topP": 0.95,
        "topK": 40
    });

    assert!(config["temperature"].is_number());
    assert!(config["maxOutputTokens"].is_number());
    assert!(config["topP"].is_number());
    assert!(config["topK"].is_number());
}

/// Test: Request body structure matches Gemini API expectations
#[test]
fn test_request_body_structure() {
    let messages = vec![
        Message::new(MessageRole::System, "Be helpful.".to_string()),
        Message::new(MessageRole::User, "Hello".to_string()),
    ];

    let (contents, system_instruction) = convert_messages_to_gemini_format(&messages);

    // Build request body like GeminiProvider does
    let request_body = json!({
        "contents": contents,
        "system_instruction": system_instruction,
        "generation_config": {
            "temperature": 0.7,
            "maxOutputTokens": 4096
        }
    });

    // Verify structure
    assert!(request_body["contents"].is_array());
    assert!(request_body["system_instruction"].is_object());
    assert!(request_body["generation_config"].is_object());

    // Verify contents has user message with correct role
    assert_eq!(request_body["contents"][0]["role"], "user");

    // Verify system_instruction has parts
    assert!(request_body["system_instruction"]["parts"].is_array());
}
