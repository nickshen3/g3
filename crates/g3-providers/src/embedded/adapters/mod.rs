//! Tool format adapters for embedded models
//!
//! Different model families use different formats for tool calling.
//! Adapters transform model-specific formats to g3's standard JSON format:
//! `{"tool": "name", "args": {...}}`
//!
//! This module provides:
//! - `ToolFormatAdapter` trait for implementing format transformations
//! - `GlmToolAdapter` for GLM/Z-AI models that use `<|assistant|>tool_name` format

mod glm;

pub use glm::GlmToolAdapter;

/// Output from processing a chunk through an adapter
#[derive(Debug, Clone, Default)]
pub struct AdapterOutput {
    /// Text safe to emit downstream (prose and/or complete tool calls)
    pub emit: String,
    /// True if a complete tool call was detected and transformed
    pub has_tool_call: bool,
}

impl AdapterOutput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_emit(emit: String) -> Self {
        Self {
            emit,
            has_tool_call: false,
        }
    }

    pub fn with_tool_call(emit: String) -> Self {
        Self {
            emit,
            has_tool_call: true,
        }
    }
}

/// Trait for adapting model-specific tool call formats to g3's standard format
///
/// Adapters are stateful to handle streaming - they buffer incomplete patterns
/// and emit complete chunks as soon as they're ready.
pub trait ToolFormatAdapter: Send + Sync {
    /// Check if this adapter handles the given model type
    fn handles(&self, model_type: &str) -> bool;

    /// Process a chunk of model output
    ///
    /// The adapter may buffer content if it's in the middle of a potential pattern.
    /// Returns content that's safe to emit downstream.
    fn process_chunk(&mut self, chunk: &str) -> AdapterOutput;

    /// Flush any remaining buffered content (call at end of stream)
    ///
    /// This should emit any buffered content, even if incomplete.
    fn flush(&mut self) -> AdapterOutput;

    /// Reset the adapter state (call between conversations)
    fn reset(&mut self);
}

/// Create an adapter for the given model type, if one exists
pub fn create_adapter_for_model(model_type: &str) -> Option<Box<dyn ToolFormatAdapter>> {
    let glm_adapter = GlmToolAdapter::new();
    if glm_adapter.handles(model_type) {
        return Some(Box::new(glm_adapter));
    }

    // Add other adapters here as needed:
    // let mistral_adapter = MistralToolAdapter::new();
    // if mistral_adapter.handles(model_type) { ... }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_adapter_for_glm() {
        assert!(create_adapter_for_model("glm4").is_some());
        assert!(create_adapter_for_model("glm").is_some());
        assert!(create_adapter_for_model("some-glm-variant").is_some());
    }

    #[test]
    fn test_no_adapter_for_unknown() {
        assert!(create_adapter_for_model("qwen").is_none());
        assert!(create_adapter_for_model("llama").is_none());
        assert!(create_adapter_for_model("mistral").is_none());
    }
}
