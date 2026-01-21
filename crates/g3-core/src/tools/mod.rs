//! Tool execution module for G3 agent.
//!
//! This module contains all tool implementations that the agent can execute.
//! Tools are organized by category:
//! - `shell` - Shell command execution and background processes
//! - `file_ops` - File reading, writing, and editing
//! - `todo` - TODO list management
//! - `webdriver` - Browser automation via WebDriver
//! - `misc` - Other tools (screenshots, code search, etc.)
//! - `research` - Web research via scout agent
//! - `memory` - Workspace memory (remember)
//! - `acd` - Aggressive Context Dehydration (rehydrate)

pub mod executor;
pub mod acd;
pub mod file_ops;
pub mod memory;
pub mod misc;
pub mod research;
pub mod shell;
pub mod todo;
pub mod webdriver;

pub use executor::ToolExecutor;
