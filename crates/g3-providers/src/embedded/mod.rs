//! Embedded LLM provider using llama.cpp
//!
//! This module provides local model inference via llama.cpp with Metal acceleration.

pub mod adapters;
mod provider;

// Re-export adapter types
pub use adapters::{create_adapter_for_model, AdapterOutput, ToolFormatAdapter};

// Re-export the main provider
pub use provider::EmbeddedProvider;
