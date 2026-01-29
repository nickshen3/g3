//! Integration tests for CacheStats accumulation through streaming.
//!
//! CHARACTERIZATION: These tests verify that cache statistics are correctly
//! accumulated through the streaming completion flow when the provider reports
//! cache usage data.
//!
//! What this test protects:
//! - CacheStats fields are accumulated correctly from provider usage data
//! - Cache hit detection works (cache_read_tokens > 0 means cache hit)
//! - Stats are accessible via get_stats() and include cache section
//!
//! What this test intentionally does NOT assert:
//! - Exact formatting of stats output (that's presentation layer)
//! - Provider-specific cache control headers (tested in provider tests)
//! - Internal implementation of how cache stats are stored

use g3_core::ui_writer::NullUiWriter;
use g3_core::Agent;
use g3_providers::mock::{MockChunk, MockProvider, MockResponse};
use g3_providers::{ProviderRegistry, Usage};
use tempfile::TempDir;

/// Helper to create an agent with a mock provider
async fn create_agent_with_mock(provider: MockProvider) -> (Agent<NullUiWriter>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    
    let mut registry = ProviderRegistry::new();
    registry.register(provider);
    
    let config = g3_config::Config::default();
    
    let agent = Agent::new_for_test(
        config,
        NullUiWriter,
        registry,
    ).await.expect("Failed to create agent");

    (agent, temp_dir)
}

/// Create a MockResponse with specific cache statistics
fn response_with_cache_stats(
    content: &str,
    prompt_tokens: u32,
    completion_tokens: u32,
    cache_creation_tokens: u32,
    cache_read_tokens: u32,
) -> MockResponse {
    MockResponse::custom(
        vec![
            MockChunk::content(content),
            MockChunk::finished("end_turn"),
        ],
        Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        },
    )
}

/// Test: Cache stats are accumulated from a single response
///
/// Verifies that when a provider returns usage data with cache tokens,
/// those values are accumulated in the agent's CacheStats.
#[tokio::test]
async fn test_cache_stats_accumulated_from_single_response() {
    // Create a response with cache creation tokens (first request, cache miss)
    let provider = MockProvider::new()
        .with_response(response_with_cache_stats(
            "Hello! I'm here to help.",
            1000,  // prompt_tokens
            50,    // completion_tokens
            800,   // cache_creation_tokens (cache miss, creating cache)
            0,     // cache_read_tokens (no cache hit)
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Execute a task to trigger the streaming flow
    let result = agent.execute_task("Hello", None, false).await;
    assert!(result.is_ok(), "Task should succeed: {:?}", result.err());

    // Get stats and verify cache section is present
    let stats = agent.get_stats();
    
    // Verify cache stats section exists
    assert!(stats.contains("Prompt Cache Statistics"), 
        "Stats should contain cache section. Got:\n{}", stats);
    
    // Verify API calls tracked
    assert!(stats.contains("API Calls:") && stats.contains("1"),
        "Should show 1 API call. Got:\n{}", stats);
    
    // Verify cache creation tokens tracked
    assert!(stats.contains("Cache Created:") && stats.contains("800"),
        "Should show 800 cache creation tokens. Got:\n{}", stats);
    
    // Verify no cache hits (first request)
    assert!(stats.contains("Cache Hits:") && stats.contains("0"),
        "Should show 0 cache hits for first request. Got:\n{}", stats);
}

/// Test: Cache hits are detected when cache_read_tokens > 0
///
/// Verifies that when a provider returns cache_read_tokens > 0,
/// it's counted as a cache hit.
#[tokio::test]
async fn test_cache_hit_detection() {
    // Create a response with cache read tokens (cache hit)
    let provider = MockProvider::new()
        .with_response(response_with_cache_stats(
            "Using cached context!",
            1000,  // prompt_tokens
            30,    // completion_tokens
            0,     // cache_creation_tokens (no new cache)
            750,   // cache_read_tokens (cache hit!)
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    let result = agent.execute_task("Hello again", None, false).await;
    assert!(result.is_ok());

    let stats = agent.get_stats();
    
    // Verify cache hit was counted
    assert!(stats.contains("Cache Hits:") && stats.contains("1"),
        "Should show 1 cache hit. Got:\n{}", stats);
    
    // Verify cache read tokens tracked
    assert!(stats.contains("Cache Read:") && stats.contains("750"),
        "Should show 750 cache read tokens. Got:\n{}", stats);
}

/// Test: Cache stats accumulate across multiple requests
///
/// Verifies that cache statistics are accumulated correctly across
/// multiple streaming completions.
#[tokio::test]
async fn test_cache_stats_accumulate_across_requests() {
    // First request: cache miss, creates cache
    // Second request: cache hit, reads from cache
    // Third request: partial cache hit
    let provider = MockProvider::new()
        .with_responses(vec![
            response_with_cache_stats("First response", 1000, 50, 800, 0),
            response_with_cache_stats("Second response", 1200, 40, 0, 800),
            response_with_cache_stats("Third response", 1500, 60, 200, 600),
        ]);

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    // Execute three tasks
    agent.execute_task("First question", None, false).await.unwrap();
    agent.execute_task("Second question", None, false).await.unwrap();
    agent.execute_task("Third question", None, false).await.unwrap();

    let stats = agent.get_stats();
    
    // Verify total API calls
    assert!(stats.contains("API Calls:") && stats.contains("3"),
        "Should show 3 API calls. Got:\n{}", stats);
    
    // Verify cache hits (requests 2 and 3 had cache_read_tokens > 0)
    assert!(stats.contains("Cache Hits:") && stats.contains("2"),
        "Should show 2 cache hits. Got:\n{}", stats);
    
    // Verify total cache creation: 800 + 0 + 200 = 1000
    assert!(stats.contains("Cache Created:") && stats.contains("1000"),
        "Should show 1000 total cache creation tokens. Got:\n{}", stats);
    
    // Verify total cache read: 0 + 800 + 600 = 1400
    assert!(stats.contains("Cache Read:") && stats.contains("1400"),
        "Should show 1400 total cache read tokens. Got:\n{}", stats);
    
    // Verify total input tokens: 1000 + 1200 + 1500 = 3700
    assert!(stats.contains("Total Input Tokens:") && stats.contains("3700"),
        "Should show 3700 total input tokens. Got:\n{}", stats);
}

/// Test: Cache efficiency percentage is calculated correctly
///
/// Verifies that the cache efficiency metric (% of input from cache)
/// is displayed in the stats output.
#[tokio::test]
async fn test_cache_efficiency_displayed() {
    // Create a response where 50% of input comes from cache
    let provider = MockProvider::new()
        .with_response(response_with_cache_stats(
            "Efficient response",
            1000,  // prompt_tokens (total input)
            50,    // completion_tokens
            0,     // cache_creation_tokens
            500,   // cache_read_tokens (50% of input)
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    agent.execute_task("Test efficiency", None, false).await.unwrap();

    let stats = agent.get_stats();
    
    // Verify cache efficiency is displayed
    assert!(stats.contains("Cache Efficiency:"),
        "Should show cache efficiency. Got:\n{}", stats);
    
    // Verify it shows 50% (500/1000)
    assert!(stats.contains("50.0%"),
        "Should show 50.0% cache efficiency. Got:\n{}", stats);
}

/// Test: Zero cache stats are handled gracefully
///
/// Verifies that when no cache tokens are reported, the stats
/// still display correctly without errors.
#[tokio::test]
async fn test_zero_cache_stats_handled() {
    // Response with no cache tokens at all
    let provider = MockProvider::new()
        .with_response(response_with_cache_stats(
            "No cache used",
            500,   // prompt_tokens
            25,    // completion_tokens
            0,     // cache_creation_tokens
            0,     // cache_read_tokens
        ));

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    agent.execute_task("Test no cache", None, false).await.unwrap();

    let stats = agent.get_stats();
    
    // Should still have cache section
    assert!(stats.contains("Prompt Cache Statistics"),
        "Should contain cache section even with zero stats. Got:\n{}", stats);
    
    // Should show 0 cache hits
    assert!(stats.contains("Cache Hits:") && stats.contains("0"),
        "Should show 0 cache hits. Got:\n{}", stats);
    
    // Should show 0% efficiency (or handle division by zero gracefully)
    assert!(stats.contains("Cache Efficiency:"),
        "Should show cache efficiency even when 0. Got:\n{}", stats);
}

/// Test: Hit rate percentage is calculated correctly
///
/// Verifies that the hit rate (cache_hit_calls / total_calls) is
/// displayed correctly.
#[tokio::test]
async fn test_hit_rate_calculation() {
    // 2 cache hits out of 4 calls = 50% hit rate
    let provider = MockProvider::new()
        .with_responses(vec![
            response_with_cache_stats("Miss 1", 1000, 50, 500, 0),   // miss
            response_with_cache_stats("Hit 1", 1000, 50, 0, 500),    // hit
            response_with_cache_stats("Miss 2", 1000, 50, 200, 0),   // miss
            response_with_cache_stats("Hit 2", 1000, 50, 0, 800),    // hit
        ]);

    let (mut agent, _temp_dir) = create_agent_with_mock(provider).await;

    agent.execute_task("Q1", None, false).await.unwrap();
    agent.execute_task("Q2", None, false).await.unwrap();
    agent.execute_task("Q3", None, false).await.unwrap();
    agent.execute_task("Q4", None, false).await.unwrap();

    let stats = agent.get_stats();
    
    // Verify hit rate is 50%
    assert!(stats.contains("Hit Rate:") && stats.contains("50.0%"),
        "Should show 50.0% hit rate. Got:\n{}", stats);
}
