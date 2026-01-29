//! Context window management for conversation history and token tracking.
//!
//! This module handles:
//! - Token counting and usage tracking
//! - Conversation history management
//! - Context thinning (reducing context size by saving large tool results to disk)
//! - Summarization triggers

use g3_providers::{Message, MessageRole, Usage};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::paths::get_thinned_dir;
use crate::ToolCall;

// ============================================================================
// Types
// ============================================================================

/// Result of a context thinning operation.
/// Contains semantic data for the UI layer to format.
#[derive(Debug, Clone)]
pub struct ThinResult {
    pub scope: ThinScope,
    pub before_percentage: u32,
    pub after_percentage: u32,
    /// Number of tool result messages that were thinned
    pub leaned_count: usize,
    /// Number of tool calls in assistant messages that were thinned
    pub tool_call_leaned_count: usize,
    pub chars_saved: usize,
    pub had_changes: bool,
}

/// Scope for context thinning operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinScope {
    /// Process only the first third of messages (incremental thinning)
    FirstThird,
    /// Process all messages (aggressive thinning, aka "skinnify")
    All,
}

impl ThinScope {
    fn file_prefix(&self) -> &'static str {
        match self {
            ThinScope::FirstThird => "leaned",
            ThinScope::All => "skinny",
        }
    }

    fn error_action(&self) -> &'static str {
        match self {
            ThinScope::FirstThird => "thinning",
            ThinScope::All => "skinnifying",
        }
    }
}

/// Represents a modification to be applied to a message during thinning
#[derive(Debug)]
enum ThinModification {
    ReplaceContent {
        index: usize,
        new_content: String,
        chars_saved: usize,
    },
}

// ============================================================================
// ContextWindow
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextWindow {
    pub used_tokens: u32,
    pub total_tokens: u32,
    /// Track cumulative tokens across all interactions
    pub cumulative_tokens: u32,
    pub conversation_history: Vec<Message>,
    /// Track the last percentage at which we thinned
    pub last_thinning_percentage: u32,
}

impl ContextWindow {
    pub fn new(total_tokens: u32) -> Self {
        Self {
            used_tokens: 0,
            total_tokens,
            cumulative_tokens: 0,
            conversation_history: Vec::new(),
            last_thinning_percentage: 0,
        }
    }

    // ========================================================================
    // Message Management
    // ========================================================================

    pub fn add_message(&mut self, message: Message) {
        self.add_message_with_tokens(message, None);
    }

    /// Add a message with optional token count from the provider
    pub fn add_message_with_tokens(&mut self, message: Message, tokens: Option<u32>) {
        if message.content.trim().is_empty() {
            warn!("Skipping empty message to avoid API error");
            return;
        }

        let token_count = tokens.unwrap_or_else(|| Self::estimate_tokens(&message.content));
        self.used_tokens += token_count;
        self.cumulative_tokens += token_count;
        self.conversation_history.push(message);

        debug!(
            "Added message with {} tokens (used: {}/{}, cumulative: {})",
            token_count, self.used_tokens, self.total_tokens, self.cumulative_tokens
        );
    }

    /// Clear the conversation history while preserving system messages.
    /// Used by /clear command to start fresh.
    pub fn clear_conversation(&mut self) {
        let system_messages: Vec<Message> = self
            .conversation_history
            .iter()
            .filter(|m| matches!(m.role, MessageRole::System))
            .cloned()
            .collect();

        self.conversation_history = system_messages;
        self.used_tokens = self
            .conversation_history
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
        self.last_thinning_percentage = 0;
    }

    // ========================================================================
    // Token Tracking
    // ========================================================================

    /// Update token usage from provider response.
    ///
    /// NOTE: This only updates cumulative_tokens (total API usage tracking).
    /// It does NOT update used_tokens because:
    /// 1. prompt_tokens represents the ENTIRE context sent to API (already tracked via add_message)
    /// 2. completion_tokens will be tracked when the assistant message is added via add_message
    /// Adding total_tokens here would cause double/triple counting and break the 80% threshold check.
    pub fn update_usage_from_response(&mut self, usage: &Usage) {
        self.cumulative_tokens += usage.total_tokens;
        debug!(
            "Updated cumulative tokens: {} (used: {}/{}, cumulative: {})",
            usage.total_tokens, self.used_tokens, self.total_tokens, self.cumulative_tokens
        );
    }

    /// Deprecated: Use update_usage_from_response instead
    pub fn update_usage(&mut self, usage: &Usage) {
        self.update_usage_from_response(usage);
    }

    /// Update cumulative token usage (for streaming) when no provider usage data is available.
    /// NOTE: This only updates cumulative_tokens, not used_tokens.
    pub fn add_streaming_tokens(&mut self, new_tokens: u32) {
        self.cumulative_tokens += new_tokens;
        debug!(
            "Updated cumulative streaming tokens: {} (used: {}/{}, cumulative: {})",
            new_tokens, self.used_tokens, self.total_tokens, self.cumulative_tokens
        );
    }

    /// Recalculate token usage based on current conversation history.
    pub fn recalculate_tokens(&mut self) {
        self.used_tokens = self
            .conversation_history
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
        debug!("Recalculated tokens after thinning: {} tokens", self.used_tokens);
    }

    /// More accurate token estimation.
    pub fn estimate_tokens(text: &str) -> u32 {
        // Heuristic:
        // - Average English text: ~4 characters per token
        // - Code/JSON: ~3 characters per token (more symbols)
        // - Add 10% buffer for safety
        let base_estimate = if text.contains('{') || text.contains("```") || text.contains("fn ") {
            (text.len() as f32 / 3.0).ceil() as u32
        } else {
            (text.len() as f32 / 4.0).ceil() as u32
        };
        (base_estimate as f32 * 1.1).ceil() as u32
    }

    // ========================================================================
    // Capacity Queries
    // ========================================================================

    pub fn percentage_used(&self) -> f32 {
        if self.total_tokens == 0 {
            0.0
        } else {
            (self.used_tokens as f32 / self.total_tokens as f32) * 100.0
        }
    }

    pub fn remaining_tokens(&self) -> u32 {
        self.total_tokens.saturating_sub(self.used_tokens)
    }

    /// Check if we should trigger compaction (at 80% capacity or 150k tokens).
    pub fn should_compact(&self) -> bool {
        self.percentage_used() >= 80.0 || self.used_tokens > 150_000
    }

    /// Check if we should trigger context thinning.
    /// Triggers at 50%, 60%, 70%, and 80% thresholds.
    pub fn should_thin(&self) -> bool {
        let current_percentage = self.percentage_used() as u32;
        if current_percentage < 50 {
            return false;
        }

        let current_threshold = (current_percentage / 10) * 10;
        current_threshold > self.last_thinning_percentage && current_threshold <= 80
    }

    // ========================================================================
    // Compaction / Summary
    // ========================================================================

    /// Create a summary request prompt for the current conversation.
    pub fn create_summary_prompt(&self) -> String {
        "Please provide a comprehensive summary of our conversation so far. Include:

1. **Main Topic/Goal**: What is the primary task or objective being worked on?
2. **Key Decisions**: What important decisions have been made?
3. **Actions Taken**: What specific actions, commands, or code changes have been completed?
4. **Current State**: What is the current status of the work?
5. **Important Context**: Any critical information, file paths, configurations, or constraints that should be remembered?
6. **Pending Items**: What remains to be done or what was the user's last request?

Format this as a detailed but concise summary that can be used to resume the conversation from scratch while maintaining full context.".to_string()
    }

    /// Reset the context window with a summary.
    /// Preserves the original system prompt as the first message.
    pub fn reset_with_summary(
        &mut self,
        summary: String,
        latest_user_message: Option<String>,
    ) -> usize {
        self.reset_with_summary_and_stub(summary, latest_user_message, None)
    }

    /// Reset context window with a summary and optional ACD stub.
    /// Preserves the original system prompt as the first message.
    /// If stub is provided, it's added as a system message before the summary.
    pub fn reset_with_summary_and_stub(
        &mut self,
        summary: String,
        latest_user_message: Option<String>,
        stub: Option<String>,
    ) -> usize {
        let old_chars: usize = self
            .conversation_history
            .iter()
            .map(|m| m.content.len())
            .sum();

        // Extract preserved messages before clearing
        let preserved = self.extract_preserved_messages();

        // Clear and rebuild
        self.conversation_history.clear();
        self.used_tokens = 0;

        // Re-add preserved messages
        if let Some(system_prompt) = preserved.system_prompt {
            self.add_message(system_prompt);
        }
        if let Some(project_context) = preserved.project_context {
            self.add_message(project_context);
        }

        // Add ACD stub if provided (before summary so LLM knows about dehydrated context)
        if let Some(stub_content) = stub {
            self.add_message(Message::new(MessageRole::System, stub_content));
        }

        // Add the summary as a USER message (not System) to maintain proper alternation.
        // This allows: [Summary as User] -> [Last Assistant] -> [Latest User]
        // which is valid User/Assistant alternation.
        self.add_message(Message::new(
            MessageRole::User,
            format!("Previous conversation summary:\n\n{}", summary),
        ));

        // Add the last assistant message if present (preserves continuity)
        if let Some(assistant_msg) = preserved.last_assistant_message {
            self.add_message(assistant_msg);
        }

        // Add the latest user message if provided
        if let Some(user_msg) = latest_user_message {
            self.add_message(Message::new(MessageRole::User, user_msg));
        }

        let new_chars: usize = self
            .conversation_history
            .iter()
            .map(|m| m.content.len())
            .sum();
        old_chars.saturating_sub(new_chars)
    }

    /// Extract messages that should be preserved across compaction.
    fn extract_preserved_messages(&self) -> PreservedMessages {
        let system_prompt = self.conversation_history.first().cloned();

        // Look for project context (AGENTS.md, memory, etc.) in the second message
        let project_context = self.conversation_history.get(1).and_then(|msg| {
            if matches!(msg.role, MessageRole::System)
                && msg.content.contains("Agent Configuration")
            {
                Some(msg.clone())
            } else {
                None
            }
        });

        // Find the last assistant message in the conversation
        let last_assistant_message = self
            .conversation_history
            .iter()
            .rev()
            .find(|m| matches!(m.role, MessageRole::Assistant))
            .cloned();

        PreservedMessages {
            system_prompt,
            project_context,
            last_assistant_message,
        }
    }

    // ========================================================================
    // Context Thinning
    // ========================================================================

    /// Thin context (first third only).
    pub fn thin_context(&mut self, session_id: Option<&str>) -> ThinResult {
        self.thin_context_with_scope(session_id, ThinScope::FirstThird)
    }

    /// Thin entire context (all messages).
    pub fn thin_context_all(&mut self, session_id: Option<&str>) -> ThinResult {
        self.thin_context_with_scope(session_id, ThinScope::All)
    }

    /// Perform context thinning: scan messages and replace large tool results with file references.
    ///
    /// # Arguments
    /// * `session_id` - If provided, thinned content is saved to .g3/session/<session_id>/thinned/
    /// * `scope` - Controls which messages to process (first third or all)
    pub fn thin_context_with_scope(
        &mut self,
        session_id: Option<&str>,
        scope: ThinScope,
    ) -> ThinResult {
        let current_percentage = self.percentage_used() as u32;

        // Only update last_thinning_percentage for incremental thinning
        if scope == ThinScope::FirstThird {
            let current_threshold = (current_percentage / 10) * 10;
            self.last_thinning_percentage = current_threshold;
        }

        // Resolve output directory
        let tmp_dir = match Self::resolve_thinned_dir(session_id, scope) {
            Ok(dir) => dir,
            Err(_) => return ThinResult::no_changes(scope, current_percentage),
        };

        // Calculate message range based on scope
        let end_index = match scope {
            ThinScope::FirstThird => (self.conversation_history.len() / 3).max(1),
            ThinScope::All => self.conversation_history.len(),
        };

        // Collect and apply modifications
        let modifications =
            self.collect_thin_modifications(end_index, &tmp_dir, scope.file_prefix());
        let (leaned_count, tool_call_leaned_count, chars_saved) =
            self.apply_thin_modifications(&modifications);

        // Recalculate token usage after thinning
        self.recalculate_tokens();

        ThinResult {
            scope,
            before_percentage: current_percentage,
            after_percentage: self.percentage_used() as u32,
            leaned_count,
            tool_call_leaned_count,
            chars_saved,
            had_changes: leaned_count > 0 || tool_call_leaned_count > 0,
        }
    }

    /// Resolve the directory for storing thinned content.
    fn resolve_thinned_dir(session_id: Option<&str>, scope: ThinScope) -> Result<String, String> {
        let dir = if let Some(sid) = session_id {
            get_thinned_dir(sid).to_string_lossy().to_string()
        } else {
            shellexpand::tilde("~/tmp").to_string()
        };

        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!("Failed to create thinned directory: {}", e);
            return Err(format!(
                "⚠️  Context {} failed: could not create directory",
                scope.error_action()
            ));
        }

        Ok(dir)
    }

    /// Collect all modifications needed for thinning without mutating.
    fn collect_thin_modifications(
        &self,
        end_index: usize,
        tmp_dir: &str,
        file_prefix: &str,
    ) -> Vec<ThinModification> {
        let mut modifications = Vec::new();

        for i in 0..end_index {
            let Some(message) = self.conversation_history.get(i) else {
                continue;
            };

            // Process User messages that look like tool results
            if matches!(message.role, MessageRole::User)
                && message.content.starts_with("Tool result:")
                && !self.is_todo_tool_result(i)
                && message.content.len() > 500
            {
                if let Some(m) =
                    Self::create_tool_result_modification(&message.content, i, tmp_dir, file_prefix)
                {
                    modifications.push(m);
                }
            }

            // Process Assistant messages that contain tool calls with large arguments
            if matches!(message.role, MessageRole::Assistant) {
                if let Some(m) =
                    Self::create_tool_call_modification(&message.content, i, tmp_dir, file_prefix)
                {
                    modifications.push(m);
                }
            }
        }

        modifications
    }

    /// Apply collected modifications and return counts.
    fn apply_thin_modifications(
        &mut self,
        modifications: &[ThinModification],
    ) -> (usize, usize, usize) {
        let mut leaned_count = 0;
        let mut tool_call_leaned_count = 0;
        let mut chars_saved = 0;

        for modification in modifications {
            let ThinModification::ReplaceContent {
                index,
                new_content,
                chars_saved: saved,
            } = modification;

            if let Some(msg) = self.conversation_history.get_mut(*index) {
                if msg.content.starts_with("Tool result:") {
                    leaned_count += 1;
                } else {
                    tool_call_leaned_count += 1;
                }
                msg.content = new_content.clone();
                chars_saved += saved;
            }
        }

        (leaned_count, tool_call_leaned_count, chars_saved)
    }

    /// Check if message at index i is a result of a TODO tool call.
    fn is_todo_tool_result(&self, i: usize) -> bool {
        if i == 0 {
            return false;
        }

        self.conversation_history
            .get(i - 1)
            .map(|prev| {
                matches!(prev.role, MessageRole::Assistant)
                    && (prev.content.contains(r#""tool":"todo_read""#)
                        || prev.content.contains(r#""tool":"todo_write""#)
                        || prev.content.contains(r#""tool": "todo_read""#)
                        || prev.content.contains(r#""tool": "todo_write""#))
            })
            .unwrap_or(false)
    }

    /// Create a modification for thinning a tool result message.
    fn create_tool_result_modification(
        content: &str,
        index: usize,
        tmp_dir: &str,
        file_prefix: &str,
    ) -> Option<ThinModification> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("{}_tool_result_{}_{}.txt", file_prefix, timestamp, index);
        let file_path = format!("{}/{}", tmp_dir, filename);

        if let Err(e) = std::fs::write(&file_path, content) {
            warn!("Failed to write thinned content to {}: {}", file_path, e);
            return None;
        }

        let original_len = content.len();
        let new_content = format!("Tool result saved to {}", file_path);
        let chars_saved = original_len - new_content.len();

        debug!(
            "Thinned tool result {} ({} chars) to {}",
            index, original_len, file_path
        );

        Some(ThinModification::ReplaceContent {
            index,
            new_content,
            chars_saved,
        })
    }

    /// Create a modification for thinning tool calls in an assistant message.
    fn create_tool_call_modification(
        content: &str,
        index: usize,
        tmp_dir: &str,
        file_prefix: &str,
    ) -> Option<ThinModification> {
        // Look for JSON tool call patterns
        let tool_call_start = content
            .find(r#"{"tool":"#)
            .or_else(|| content.find(r#"{ "tool":"#))
            .or_else(|| content.find(r#"{"tool" :"#))
            .or_else(|| content.find(r#"{ "tool" :"#))?;

        let json_portion = &content[tool_call_start..];
        let json_end = Self::find_json_end(json_portion)?;
        let json_str = &json_portion[..=json_end];

        let mut tool_call: ToolCall = serde_json::from_str(json_str).ok()?;
        let mut modified = false;
        let mut chars_saved = 0;

        // Handle write_file tool calls
        if tool_call.tool == "write_file" {
            if let Some((saved, new_args)) =
                Self::thin_write_file_args(&tool_call.args, index, tmp_dir, file_prefix)
            {
                tool_call.args = new_args;
                modified = true;
                chars_saved += saved;
            }
        }

        // Handle str_replace tool calls
        if tool_call.tool == "str_replace" {
            if let Some((saved, new_args)) =
                Self::thin_str_replace_args(&tool_call.args, index, tmp_dir, file_prefix)
            {
                tool_call.args = new_args;
                modified = true;
                chars_saved += saved;
            }
        }

        if !modified {
            return None;
        }

        // Reconstruct the message
        let prefix = &content[..tool_call_start];
        let suffix = &content[tool_call_start + json_str.len()..];
        let new_json = serde_json::to_string(&tool_call).ok()?;
        let new_content = format!("{}{}{}", prefix, new_json, suffix);

        Some(ThinModification::ReplaceContent {
            index,
            new_content,
            chars_saved,
        })
    }

    /// Thin write_file args by saving content to file.
    /// Returns (chars_saved, new_args) if thinned.
    fn thin_write_file_args(
        args: &serde_json::Value,
        index: usize,
        tmp_dir: &str,
        file_prefix: &str,
    ) -> Option<(usize, serde_json::Value)> {
        let args_obj = args.as_object()?;
        let content_str = args_obj.get("content")?.as_str()?;

        if content_str.len() <= 500 {
            return None;
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!(
            "{}_write_file_content_{}_{}.txt",
            file_prefix, timestamp, index
        );
        let file_path = format!("{}/{}", tmp_dir, filename);

        std::fs::write(&file_path, content_str).ok()?;

        let content_len = content_str.len();
        let mut new_args = args_obj.clone();
        new_args.insert(
            "content".to_string(),
            serde_json::Value::String(format!("<content saved to {}>", file_path)),
        );

        debug!(
            "Thinned write_file content {} ({} chars) to {}",
            index, content_len, file_path
        );

        Some((content_len, serde_json::Value::Object(new_args)))
    }

    /// Thin str_replace args by saving diff to file.
    /// Returns (chars_saved, new_args) if thinned.
    fn thin_str_replace_args(
        args: &serde_json::Value,
        index: usize,
        tmp_dir: &str,
        file_prefix: &str,
    ) -> Option<(usize, serde_json::Value)> {
        let args_obj = args.as_object()?;
        let diff_str = args_obj.get("diff")?.as_str()?;

        if diff_str.len() <= 500 {
            return None;
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!(
            "{}_str_replace_diff_{}_{}.txt",
            file_prefix, timestamp, index
        );
        let file_path = format!("{}/{}", tmp_dir, filename);

        std::fs::write(&file_path, diff_str).ok()?;

        let diff_len = diff_str.len();
        let mut new_args = args_obj.clone();
        new_args.insert(
            "diff".to_string(),
            serde_json::Value::String(format!("<diff saved to {}>", file_path)),
        );

        debug!(
            "Thinned str_replace diff {} ({} chars) to {}",
            index, diff_len, file_path
        );

        Some((diff_len, serde_json::Value::Object(new_args)))
    }

    // ========================================================================
    // JSON Utilities
    // ========================================================================

    /// Find the end position of a JSON object.
    pub fn find_json_end(json_str: &str) -> Option<usize> {
        let mut brace_count = 0;
        let mut in_string = false;
        let mut escape_next = false;

        for (i, ch) in json_str.char_indices() {
            if escape_next {
                escape_next = false;
                continue;
            }

            match ch {
                '\\' => escape_next = true,
                '"' if !escape_next => in_string = !in_string,
                '{' if !in_string => brace_count += 1,
                '}' if !in_string => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }

        None
    }
}

// ============================================================================
// Helper Types
// ============================================================================

/// Messages preserved across compaction.
struct PreservedMessages {
    system_prompt: Option<Message>,
    project_context: Option<Message>,
    last_assistant_message: Option<Message>,
}

impl ThinResult {
    /// Create a ThinResult indicating no changes were made.
    fn no_changes(scope: ThinScope, percentage: u32) -> Self {
        Self {
            scope,
            before_percentage: percentage,
            after_percentage: percentage,
            leaned_count: 0,
            tool_call_leaned_count: 0,
            chars_saved: 0,
            had_changes: false,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context_window() {
        let cw = ContextWindow::new(100_000);
        assert_eq!(cw.used_tokens, 0);
        assert_eq!(cw.total_tokens, 100_000);
        assert_eq!(cw.cumulative_tokens, 0);
        assert!(cw.conversation_history.is_empty());
    }

    #[test]
    fn test_percentage_used() {
        let mut cw = ContextWindow::new(100);
        cw.used_tokens = 50;
        assert!((cw.percentage_used() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_remaining_tokens() {
        let mut cw = ContextWindow::new(100);
        cw.used_tokens = 30;
        assert_eq!(cw.remaining_tokens(), 70);
    }

    #[test]
    fn test_should_compact_at_80_percent() {
        let mut cw = ContextWindow::new(100);
        cw.used_tokens = 79;
        assert!(!cw.should_compact());
        cw.used_tokens = 80;
        assert!(cw.should_compact());
    }

    #[test]
    fn test_should_compact_at_absolute_limit() {
        let mut cw = ContextWindow::new(1_000_000);
        cw.used_tokens = 150_001;
        assert!(cw.should_compact());
    }

    #[test]
    fn test_should_thin_thresholds() {
        let mut cw = ContextWindow::new(100);

        // Below 50% - should not thin
        cw.used_tokens = 49;
        assert!(!cw.should_thin());

        // At 50% - should thin (first time)
        cw.used_tokens = 50;
        assert!(cw.should_thin());

        // After thinning at 50%, shouldn't thin again until 60%
        cw.last_thinning_percentage = 50;
        cw.used_tokens = 55;
        assert!(!cw.should_thin());

        // At 60% - should thin again
        cw.used_tokens = 60;
        assert!(cw.should_thin());
    }

    #[test]
    fn test_estimate_tokens_regular_text() {
        let text = "Hello world, this is a test.";
        let tokens = ContextWindow::estimate_tokens(text);
        assert!(tokens > 0 && tokens < 20);
    }

    #[test]
    fn test_estimate_tokens_code() {
        let code = "fn main() { println!(\"hello\"); }";
        let tokens = ContextWindow::estimate_tokens(code);
        assert!(tokens > 0);
    }

    #[test]
    fn test_find_json_end() {
        assert_eq!(ContextWindow::find_json_end("{}"), Some(1));
        assert_eq!(ContextWindow::find_json_end(r#"{"a": 1}"#), Some(7));
        assert_eq!(ContextWindow::find_json_end(r#"{"a": {"b": 2}}"#), Some(14));
        assert_eq!(ContextWindow::find_json_end("{incomplete"), None);
    }

    #[test]
    fn test_thin_scope_properties() {
        assert_eq!(ThinScope::FirstThird.file_prefix(), "leaned");
        assert_eq!(ThinScope::All.file_prefix(), "skinny");
        assert_eq!(ThinScope::FirstThird.error_action(), "thinning");
        assert_eq!(ThinScope::All.error_action(), "skinnifying");
    }
}
