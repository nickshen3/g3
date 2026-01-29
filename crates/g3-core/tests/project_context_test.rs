//! Integration tests for project context loading and ordering.
//!
//! Tests that the context window has the correct structure when project context is loaded.
//! Also tests that project content survives compaction.

use g3_core::{
    ui_writer::NullUiWriter,
    Agent,
};
use g3_config::Config;
use g3_providers::{mock::MockProvider, ProviderRegistry, MockResponse, MessageRole};

/// Helper to create a test agent with mock provider
async fn create_test_agent(project_context: Option<String>) -> Agent<NullUiWriter> {
    let config = Config::default();
    let provider = MockProvider::new()
        .with_response(MockResponse::text("Test response"));
    
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    
    Agent::new_for_test_with_project_context(config, NullUiWriter, registry, project_context)
        .await
        .expect("Failed to create test agent")
}

#[tokio::test]
async fn test_context_window_initial_structure() {
    // Create agent with README content
    let readme = "📂 Working Directory: /test/workspace\n\n\
        🤖 Agent Configuration (from AGENTS.md):\nTest agent config\n\n\
        📚 Project README (from README.md):\n# Test Project\nA test project.".to_string();
    
    let agent = create_test_agent(Some(readme)).await;
    let context = agent.get_context_window();
    
    // Should have exactly 2 messages: system prompt + README
    assert_eq!(context.conversation_history.len(), 2, 
        "Expected 2 messages (system + README), got {}", context.conversation_history.len());
    
    // First message should be system prompt
    let system_msg = &context.conversation_history[0];
    assert!(system_msg.content.contains("You have access to tools"),
        "First message should be system prompt with tool instructions");
    
    // Second message should be README content
    let readme_msg = &context.conversation_history[1];
    assert!(readme_msg.content.contains("📂 Working Directory:"),
        "Second message should start with working directory");
    assert!(readme_msg.content.contains("🤖 Agent Configuration"),
        "Second message should contain AGENTS.md");
    assert!(readme_msg.content.contains("📚 Project README"),
        "Second message should contain README");
}

#[tokio::test]
async fn test_context_window_order_agents_before_readme() {
    let readme = "📂 Working Directory: /test\n\n\
        🤖 Agent Configuration (from AGENTS.md):\nAgent stuff\n\n\
        📚 Project README (from README.md):\nReadme stuff".to_string();
    
    let agent = create_test_agent(Some(readme)).await;
    let context = agent.get_context_window();
    let content = &context.conversation_history[1].content;
    
    let cwd_pos = content.find("📂 Working Directory").expect("CWD not found");
    let agents_pos = content.find("🤖 Agent Configuration").expect("AGENTS not found");
    let readme_pos = content.find("📚 Project README").expect("README not found");
    
    assert!(cwd_pos < agents_pos, "Working directory should come before AGENTS.md");
    assert!(agents_pos < readme_pos, "AGENTS.md should come before README");
}

#[tokio::test]
async fn test_set_project_content_appends_to_readme() {
    let readme = "📂 Working Directory: /test\n\n\
        📚 Project README (from README.md):\n# Test".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Set project content
    let project_content = "=== PROJECT INSTRUCTIONS ===\nGlobal instructions\n=== END PROJECT INSTRUCTIONS ===\n\n\
        === ACTIVE PROJECT: /projects/myproject ===\n\
        ## Brief\nProject brief here\n\n\
        ## Status\nIn progress".to_string();
    
    let success = agent.set_project_content(Some(project_content));
    assert!(success, "set_project_content should succeed");
    
    let context = agent.get_context_window();
    let content = &context.conversation_history[1].content;
    
    // Verify project content was appended
    assert!(content.contains("=== PROJECT INSTRUCTIONS ==="),
        "Should contain PROJECT INSTRUCTIONS marker");
    assert!(content.contains("=== END PROJECT INSTRUCTIONS ==="),
        "Should contain END PROJECT INSTRUCTIONS marker");
    assert!(content.contains("=== ACTIVE PROJECT: /projects/myproject ==="),
        "Should contain ACTIVE PROJECT marker");
    assert!(content.contains("## Brief"),
        "Should contain Brief section");
    assert!(content.contains("## Status"),
        "Should contain Status section");
    
    // Verify order: README content comes before project content
    let readme_pos = content.find("📚 Project README").expect("README not found");
    let project_instructions_pos = content.find("=== PROJECT INSTRUCTIONS ===").expect("PROJECT INSTRUCTIONS not found");
    let active_project_pos = content.find("=== ACTIVE PROJECT:").expect("ACTIVE PROJECT not found");
    
    assert!(readme_pos < project_instructions_pos, 
        "README should come before PROJECT INSTRUCTIONS");
    assert!(project_instructions_pos < active_project_pos,
        "PROJECT INSTRUCTIONS should come before ACTIVE PROJECT");
}

#[tokio::test]
async fn test_set_project_content_without_instructions() {
    let readme = "📂 Working Directory: /test\n\n📚 Project README:\n# Test".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Set project content without PROJECT INSTRUCTIONS (no projects.md in workspace)
    let project_content = "=== ACTIVE PROJECT: /projects/myproject ===\n\
        ## Brief\nProject brief\n\n\
        ## Contacts\ncontacts: []\n\n\
        ## Status\nDone".to_string();
    
    agent.set_project_content(Some(project_content));
    
    let context = agent.get_context_window();
    let content = &context.conversation_history[1].content;
    
    // Should NOT contain PROJECT INSTRUCTIONS
    assert!(!content.contains("=== PROJECT INSTRUCTIONS ==="),
        "Should NOT contain PROJECT INSTRUCTIONS when not provided");
    
    // Should contain ACTIVE PROJECT
    assert!(content.contains("=== ACTIVE PROJECT: /projects/myproject ==="),
        "Should contain ACTIVE PROJECT marker");
    assert!(content.contains("## Brief"),
        "Should contain Brief section");
    assert!(content.contains("## Contacts"),
        "Should contain Contacts section");
    assert!(content.contains("## Status"),
        "Should contain Status section");
}

#[tokio::test]
async fn test_clear_project_content() {
    let readme = "📂 Working Directory: /test\n\n📚 Project README:\n# Test".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Set project content
    let project_content = "=== ACTIVE PROJECT: /projects/test ===\n## Brief\nTest".to_string();
    agent.set_project_content(Some(project_content));
    
    // Verify it was set
    assert!(agent.has_project_content(), "Project should be loaded");
    
    // Clear project content
    let success = agent.clear_project_content();
    assert!(success, "clear_project_content should succeed");
    
    // Verify it was cleared
    assert!(!agent.has_project_content(), "Project should be unloaded");
    
    let context = agent.get_context_window();
    let content = &context.conversation_history[1].content;
    
    assert!(!content.contains("=== ACTIVE PROJECT:"),
        "Should NOT contain ACTIVE PROJECT after clearing");
    assert!(content.contains("📚 Project README"),
        "Should still contain README after clearing project");
}

#[tokio::test]
async fn test_set_project_content_replaces_existing() {
    let readme = "📂 Working Directory: /test\n\n📚 Project README:\n# Test".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Set first project
    let project1 = "=== ACTIVE PROJECT: /projects/first ===\n## Brief\nFirst project".to_string();
    agent.set_project_content(Some(project1));
    
    // Set second project (should replace first)
    let project2 = "=== ACTIVE PROJECT: /projects/second ===\n## Brief\nSecond project".to_string();
    agent.set_project_content(Some(project2));
    
    let context = agent.get_context_window();
    let content = &context.conversation_history[1].content;
    
    // Should only contain second project
    assert!(!content.contains("/projects/first"),
        "Should NOT contain first project after replacement");
    assert!(content.contains("/projects/second"),
        "Should contain second project");
    assert!(content.contains("Second project"),
        "Should contain second project content");
    
    // Should only have one ACTIVE PROJECT marker
    let count = content.matches("=== ACTIVE PROJECT:").count();
    assert_eq!(count, 1, "Should have exactly one ACTIVE PROJECT marker, got {}", count);
}

#[tokio::test]
async fn test_project_content_with_memory() {
    // Simulate full content with memory at the end
    let readme = "📂 Working Directory: /test\n\n\
        🤖 Agent Configuration (from AGENTS.md):\nAgent config\n\n\
        📚 Project README (from README.md):\n# Test\n\n\
        === Workspace Memory (read from analysis/memory.md, 1.2k chars) ===\n\
        ### Known Features\n- details\n\
        === End Workspace Memory ===".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Set project content
    let project_content = "=== ACTIVE PROJECT: /projects/test ===\n## Brief\nTest brief".to_string();
    agent.set_project_content(Some(project_content));
    
    let context = agent.get_context_window();
    let content = &context.conversation_history[1].content;
    
    // Verify all sections are present
    assert!(content.contains("📂 Working Directory"), "Should have CWD");
    assert!(content.contains("🤖 Agent Configuration"), "Should have AGENTS");
    assert!(content.contains("📚 Project README"), "Should have README");
    assert!(content.contains("=== Workspace Memory"), "Should have Memory");
    assert!(content.contains("=== ACTIVE PROJECT:"), "Should have Project");
    
    // Verify order: Memory should come before Project (since project is appended at the end)
    let memory_pos = content.find("=== Workspace Memory").expect("Memory not found");
    let project_pos = content.find("=== ACTIVE PROJECT:").expect("Project not found");
    
    assert!(memory_pos < project_pos,
        "Memory should come before Project (project is appended to existing content)");
}

#[tokio::test]
async fn test_has_project_content() {
    let readme = "📂 Working Directory: /test\n\n📚 Project README:\n# Test".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Initially no project
    assert!(!agent.has_project_content(), "Should not have project initially");
    
    // After setting project
    let project = "=== ACTIVE PROJECT: /test ===\n## Brief\nTest".to_string();
    agent.set_project_content(Some(project));
    assert!(agent.has_project_content(), "Should have project after setting");
    
    // After clearing
    agent.clear_project_content();
    assert!(!agent.has_project_content(), "Should not have project after clearing");
}

#[tokio::test]
async fn test_full_context_order() {
    // This test verifies the complete expected order of context window content
    let readme = "📂 Working Directory: /workspace\n\n\
        🤖 Agent Configuration (from AGENTS.md):\n## Agent Rules\nBe helpful\n\n\
        📚 Project README (from README.md):\n# My Project\nDescription here\n\n\
        🔧 Language-Specific Guidance:\n## Rust\nUse cargo\n\n\
        📎 Included Prompt (from prompt.md):\nCustom instructions\n\n\
        === Workspace Memory (read from analysis/memory.md, 500 chars) ===\n\
        ### Known Features\n- Feature A\n\
        === End Workspace Memory ===".to_string();
    
    let mut agent = create_test_agent(Some(readme)).await;
    
    // Add project content
    let project = "=== PROJECT INSTRUCTIONS ===\n\
        Global project rules\n\
        === END PROJECT INSTRUCTIONS ===\n\n\
        === ACTIVE PROJECT: /projects/current ===\n\
        ## Brief\nCurrent project brief\n\n\
        ## Contacts\nname: John\n\n\
        ## Status\nIn progress".to_string();
    agent.set_project_content(Some(project));
    
    let context = agent.get_context_window();
    
    // Message 0: System prompt
    let system = &context.conversation_history[0].content;
    assert!(system.contains("You have access to tools"),
        "Message 0 should be system prompt");
    
    // Message 1: Combined content with project appended
    let combined = &context.conversation_history[1].content;
    
    // Get positions of all sections
    let cwd_pos = combined.find("📂 Working Directory").expect("CWD missing");
    let agents_pos = combined.find("🤖 Agent Configuration").expect("AGENTS missing");
    let readme_pos = combined.find("📚 Project README").expect("README missing");
    let lang_pos = combined.find("🔧 Language-Specific").expect("Language missing");
    let include_pos = combined.find("📎 Included Prompt").expect("Include missing");
    let memory_pos = combined.find("=== Workspace Memory").expect("Memory missing");
    let proj_instr_pos = combined.find("=== PROJECT INSTRUCTIONS ===").expect("Project instructions missing");
    let active_proj_pos = combined.find("=== ACTIVE PROJECT:").expect("Active project missing");
    
    // Verify complete order
    assert!(cwd_pos < agents_pos, "CWD < AGENTS");
    assert!(agents_pos < readme_pos, "AGENTS < README");
    assert!(readme_pos < lang_pos, "README < Language");
    assert!(lang_pos < include_pos, "Language < Include");
    assert!(include_pos < memory_pos, "Include < Memory");
    assert!(memory_pos < proj_instr_pos, "Memory < Project Instructions");
    assert!(proj_instr_pos < active_proj_pos, "Project Instructions < Active Project");
    
    // Verify project sections order
    let brief_pos = combined.find("## Brief").expect("Brief missing");
    let contacts_pos = combined.find("## Contacts").expect("Contacts missing");
    let status_pos = combined.find("## Status").expect("Status missing");
    
    assert!(active_proj_pos < brief_pos, "Active Project < Brief");
    assert!(brief_pos < contacts_pos, "Brief < Contacts");
    assert!(contacts_pos < status_pos, "Contacts < Status");
    
    // Verify NO closing marker for ACTIVE PROJECT
    assert!(!combined.contains("=== END ACTIVE PROJECT ==="),
        "Should NOT have END ACTIVE PROJECT marker");
}

// =============================================================================
// Compaction Tests - Project Content Survival
// =============================================================================

/// Helper to create an agent with mock provider and custom README content
async fn create_agent_with_mock_and_readme(
    provider: MockProvider,
    readme_content: Option<String>,
) -> Agent<NullUiWriter> {
    let config = Config::default();
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    
    Agent::new_for_test_with_project_context(config, NullUiWriter, registry, readme_content)
        .await
        .expect("Failed to create test agent")
}

/// Test: Project content survives compaction
///
/// CHARACTERIZATION: This test verifies that project content (loaded via /project command)
/// is preserved through compaction because it's appended to the README message,
/// which is explicitly preserved during compaction.
///
/// What this test protects:
/// - Project content appended to README message survives compaction
/// - The README message (containing project content) is preserved as message[1]
///
/// What this test intentionally does NOT assert:
/// - The exact format of the summary (that's LLM-dependent)
/// - Internal compaction implementation details
#[tokio::test]
async fn test_project_content_survives_compaction() {
    // Create provider with responses for:
    // 1. Initial conversation
    // 2. Compaction summary
    let provider = MockProvider::new()
        .with_response(MockResponse::text("I understand. Let me help with the project."))
        .with_response(MockResponse::text("SUMMARY: Discussed project requirements."));
    
    // Create README with Agent Configuration marker (required for preservation)
    let readme = "📂 Working Directory: /test/workspace\n\n\
        🤖 Agent Configuration (from AGENTS.md):\nTest agent config\n\n\
        📚 Project README (from README.md):\n# Test Project\nA test project.".to_string();
    
    let mut agent = create_agent_with_mock_and_readme(provider, Some(readme)).await;
    
    // Set project content (simulates /project command)
    let project_content = "=== PROJECT INSTRUCTIONS ===\n\
        Global project rules\n\
        === END PROJECT INSTRUCTIONS ===\n\n\
        === ACTIVE PROJECT: /projects/myproject ===\n\
        ## Brief\nThis is the project brief.\n\n\
        ## Status\nIn progress".to_string();
    
    let success = agent.set_project_content(Some(project_content.clone()));
    assert!(success, "set_project_content should succeed");
    
    // Verify project content is present before compaction
    let context_before = agent.get_context_window();
    let readme_msg_before = &context_before.conversation_history[1].content;
    assert!(readme_msg_before.contains("=== ACTIVE PROJECT: /projects/myproject ==="),
        "Project content should be present before compaction");
    
    // Execute a task to build up conversation history
    agent.execute_task("Help me with this project", None, false).await.unwrap();
    
    // Trigger compaction
    let result = agent.force_compact().await;
    assert!(result.is_ok(), "Compaction should succeed: {:?}", result.err());
    
    // Verify project content survives compaction
    let context_after = agent.get_context_window();
    
    // The README message should still be at index 1
    assert!(context_after.conversation_history.len() >= 2,
        "Should have at least 2 messages after compaction");
    
    let readme_msg_after = &context_after.conversation_history[1].content;
    
    // Project content should be preserved
    assert!(readme_msg_after.contains("=== ACTIVE PROJECT: /projects/myproject ==="),
        "ACTIVE PROJECT marker should survive compaction. Got: {}...",
        readme_msg_after.chars().take(200).collect::<String>());
    
    assert!(readme_msg_after.contains("=== PROJECT INSTRUCTIONS ==="),
        "PROJECT INSTRUCTIONS should survive compaction");
    
    assert!(readme_msg_after.contains("## Brief"),
        "Brief section should survive compaction");
    
    assert!(readme_msg_after.contains("## Status"),
        "Status section should survive compaction");
    
    // Verify the README message is still a System message
    assert!(matches!(context_after.conversation_history[1].role, MessageRole::System),
        "README message should still be System role after compaction");
}
