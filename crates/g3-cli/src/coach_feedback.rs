//! Coach feedback extraction from session logs.
//!
//! Extracts feedback from the coach agent's session logs for the coach-player loop.

use anyhow::Result;
use std::path::Path;

use g3_core::Agent;

use crate::simple_output::SimpleOutput;
use crate::ui_writer_impl::ConsoleUiWriter;

/// Extract coach feedback by reading from the coach agent's specific log file.
///
/// Uses the coach agent's session ID to find the exact log file.
pub fn extract_from_logs(
    coach_result: &g3_core::TaskResult,
    coach_agent: &Agent<ConsoleUiWriter>,
    output: &SimpleOutput,
) -> Result<String> {
    let session_id = coach_agent
        .get_session_id()
        .ok_or_else(|| anyhow::anyhow!("Coach agent has no session ID"))?;

    let log_file_path = resolve_log_path(&session_id);

    // Try to extract from session log
    if let Some(feedback) = try_extract_from_log(&log_file_path) {
        output.print(&format!("✅ Extracted coach feedback from session: {}", session_id));
        return Ok(feedback);
    }

    // Fallback: use the TaskResult's extract_summary method
    let fallback = coach_result.extract_summary();
    if !fallback.is_empty() {
        output.print(&format!(
            "✅ Extracted coach feedback from response: {} chars",
            fallback.len()
        ));
        return Ok(fallback);
    }

    Err(anyhow::anyhow!(
        "Could not extract coach feedback from session: {}\n\
         Log file path: {:?}\n\
         Log file exists: {}\n\
         Coach result response length: {} chars",
        session_id,
        log_file_path,
        log_file_path.exists(),
        coach_result.response.len()
    ))
}

/// Resolve the log file path, trying new path first then falling back to old.
fn resolve_log_path(session_id: &str) -> std::path::PathBuf {
    g3_core::get_session_file(session_id)
}

/// Extract feedback from a session log file.
///
/// Searches backwards for the last assistant message with substantial text content.
fn try_extract_from_log(log_file_path: &Path) -> Option<String> {
    if !log_file_path.exists() {
        return None;
    }

    let log_content = std::fs::read_to_string(log_file_path).ok()?;
    let log_json: serde_json::Value = serde_json::from_str(&log_content).ok()?;

    let messages = log_json
        .get("context_window")?
        .get("conversation_history")?
        .as_array()?;

    // Search backwards for the last assistant message with text content
    for msg in messages.iter().rev() {
        if let Some(feedback) = extract_assistant_text(msg) {
            return Some(feedback);
        }
    }

    None
}

/// Extract text content from an assistant message.
fn extract_assistant_text(msg: &serde_json::Value) -> Option<String> {
    let role = msg.get("role").and_then(|v| v.as_str())?;
    if !role.eq_ignore_ascii_case("assistant") {
        return None;
    }

    let content = msg.get("content")?;

    // Handle string content
    if let Some(content_str) = content.as_str() {
        return filter_substantial_text(content_str);
    }

    // Handle array content (native tool calling format)
    if let Some(content_array) = content.as_array() {
        for block in content_array {
            if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if let Some(result) = filter_substantial_text(text) {
                        return Some(result);
                    }
                }
            }
        }
    }

    None
}

/// Filter out empty or very short responses (likely just tool calls).
fn filter_substantial_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !trimmed.is_empty() && trimmed.len() > 10 {
        Some(trimmed.to_string())
    } else {
        None
    }
}
