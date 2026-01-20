//! Centralized formatting for g3 system status messages.
//!
//! This module provides consistent formatting for all "g3:" prefixed status messages,
//! including progress indicators, completion statuses, and inline updates.
//!
//! # Usage
//!
//! ```ignore
//! use crate::g3_status::G3Status;
//!
//! // Start a progress message (stays on same line, no newline)
//! G3Status::progress("compacting session");
//!
//! // Complete with status (adds to same line, then newline)
//! G3Status::done();
//! // or
//! G3Status::failed();
//! // or
//! G3Status::error("timeout");
//!
//! // One-shot status message (progress + status on same line)
//! G3Status::complete("compacting session", Status::Done);
//! ```

use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};
use std::io::{self, Write};

/// Status types for g3 system messages
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Success - bold green "[done]"
    Done,
    /// Failure - red "[failed]"
    Failed,
    /// Error with message - red "[error: <msg>]"
    Error(String),
    /// Custom status - plain "[<status>]"
    Custom(String),
    /// Resolved status - for thinning operations
    Resolved,
    /// Insufficient - for thinning operations
    Insufficient,
    /// No changes - for thinning operations that didn't modify anything
    NoChanges,
}

impl Status {
    /// Parse a status string into a Status enum
    pub fn from_str(s: &str) -> Self {
        match s {
            "done" => Status::Done,
            "failed" => Status::Failed,
            "resolved" => Status::Resolved,
            "insufficient" => Status::Insufficient,
            s if s.starts_with("error:") => Status::Error(s[6..].trim().to_string()),
            s if s.starts_with("error") => Status::Error(s[5..].trim().to_string()),
            other => Status::Custom(other.to_string()),
        }
    }
}

/// Centralized g3 system status message formatting
pub struct G3Status;

impl G3Status {
    /// Print a progress message that stays on the same line.
    /// Format: "g3: <message> ..."
    /// - "g3:" is bold green
    /// - No trailing newline (use `done()`, `failed()`, etc. to complete)
    pub fn progress(message: &str) {
        print!(
            "{}{}g3:{}{} {} ...",
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            ResetColor,
            SetAttribute(Attribute::Reset),
            message
        );
        let _ = io::stdout().flush();
    }

    /// Print a progress message with a newline at the end.
    /// Use this when you don't plan to add a status on the same line.
    pub fn progress_ln(message: &str) {
        println!(
            "{}{}g3:{}{} {} ...",
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            ResetColor,
            SetAttribute(Attribute::Reset),
            message
        );
    }

    /// Complete a progress message with "[done]" in bold green.
    pub fn done() {
        println!(
            " {}{}[done]{}",
            SetForegroundColor(Color::Green),
            SetAttribute(Attribute::Bold),
            ResetColor
        );
    }

    /// Complete a progress message with "[failed]" in red.
    pub fn failed() {
        println!(
            " {}[failed]{}",
            SetForegroundColor(Color::Red),
            ResetColor
        );
    }

    /// Complete a progress message with "[error: <msg>]" in red.
    pub fn error(msg: &str) {
        println!(
            " {}[error: {}]{}",
            SetForegroundColor(Color::Red),
            msg,
            ResetColor
        );
    }

    /// Complete a progress message with a custom status.
    pub fn status(status: &Status) {
        match status {
            Status::Done => Self::done(),
            Status::Failed => Self::failed(),
            Status::Error(msg) => Self::error(msg),
            Status::Resolved => {
                println!(
                    " {}{}[resolved]{}",
                    SetForegroundColor(Color::Green),
                    SetAttribute(Attribute::Bold),
                    ResetColor
                );
            }
            Status::Insufficient => {
                println!(
                    " {}[insufficient]{}",
                    SetForegroundColor(Color::Yellow),
                    ResetColor
                );
            }
            Status::Custom(s) => {
                println!(" [{}]", s);
            }
            Status::NoChanges => {
                println!(
                    " {}[no changes]{}",
                    SetForegroundColor(Color::DarkGrey),
                    ResetColor
                );
            }
        }
    }

    /// Print a complete status message (progress + status) on one line.
    /// Format: "g3: <message> ... [status]"
    pub fn complete(message: &str, status: Status) {
        Self::progress(message);
        Self::status(&status);
    }

    /// Print an info message in dimmed/grey text.
    /// Format: "... <message>"
    pub fn info(message: &str) {
        println!(
            "{}... {}{}",
            SetForegroundColor(Color::DarkGrey),
            message,
            ResetColor
        );
    }

    /// Print an info message inline (no newline, for appending to user input).
    /// Uses ANSI escape to move cursor up and to end of previous line.
    pub fn info_inline(message: &str) {
        // Move cursor up one line, to end of line, then print
        print!(
            "\x1b[1A\x1b[999C {}... {}{}\n",
            SetForegroundColor(Color::DarkGrey),
            message,
            ResetColor
        );
        let _ = io::stdout().flush();
    }

    /// Format a status string for inline use (returns the formatted string).
    /// Useful when building custom messages.
    pub fn format_status(status: &Status) -> String {
        match status {
            Status::Done => format!(
                "{}{}[done]{}",
                SetForegroundColor(Color::Green),
                SetAttribute(Attribute::Bold),
                ResetColor
            ),
            Status::Failed => format!(
                "{}[failed]{}",
                SetForegroundColor(Color::Red),
                ResetColor
            ),
            Status::Error(msg) => format!(
                "{}[error: {}]{}",
                SetForegroundColor(Color::Red),
                msg,
                ResetColor
            ),
            Status::Resolved => format!(
                "{}{}[resolved]{}",
                SetForegroundColor(Color::Green),
                SetAttribute(Attribute::Bold),
                ResetColor
            ),
            Status::Insufficient => format!(
                "{}[insufficient]{}",
                SetForegroundColor(Color::Yellow),
                ResetColor
            ),
            Status::Custom(s) => format!("[{}]", s),
            Status::NoChanges => format!(
                "{}[no changes]{}",
                SetForegroundColor(Color::DarkGrey),
                ResetColor
            ),
        }
    }

    /// Format the "g3:" prefix for inline use.
    pub fn format_prefix() -> String {
        format!(
            "{}{}g3:{}{}",
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            ResetColor,
            SetAttribute(Attribute::Reset),
        )
    }

    /// Print a resuming session message with session ID highlighted.
    /// Format: "... resuming <session_id> [done/error]"
    pub fn resuming(session_id: &str, status: Status) {
        let status_str = Self::format_status(&status);
        println!(
            "... resuming {}{}{} {}",
            SetForegroundColor(Color::Cyan),
            session_id,
            ResetColor,
            status_str
        );
    }

    /// Print a resuming session message with "(summary)" note.
    pub fn resuming_summary(session_id: &str) {
        let status_str = Self::format_status(&Status::Done);
        println!(
            "... resuming {}{}{} (summary) {}",
            SetForegroundColor(Color::Cyan),
            session_id,
            ResetColor,
            status_str
        );
    }

    /// Print a context thinning result.
    /// Format: "g3: thinning context ... 70% -> 40% ... [done]"
    /// or: "g3: thinning context (full) ... 70% ... [no changes]"
    pub fn thin_result(result: &g3_core::ThinResult) {
        use g3_core::ThinScope;
        
        let scope_desc = match result.scope {
            ThinScope::FirstThird => "thinning context",
            ThinScope::All => "thinning context (full)",
        };
        
        if result.had_changes {
            // Format: "g3: thinning context ... 70% -> 40% ... [done]"
            print!(
                "{} {} ... {}% -> {}% ...",
                Self::format_prefix(),
                scope_desc,
                result.before_percentage,
                result.after_percentage
            );
            Self::done();
        } else {
            // Format: "g3: thinning context ... 70% ... [no changes]"
            Self::complete(&format!("{} ... {}%", scope_desc, result.before_percentage), Status::NoChanges);
        }
    }

    /// Print a complete status message with a path highlighted in cyan.
    /// Format: "g3: <message> <path> [status]"
    /// - "g3:" is bold green
    /// - path is cyan
    /// - status is formatted per Status type
    pub fn complete_with_path(message: &str, path: &str, status: Status) {
        print!(
            "{} {} {}{}{}",
            Self::format_prefix(),
            message,
            SetForegroundColor(Color::Cyan),
            path,
            ResetColor
        );
        Self::status(&status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_from_str() {
        assert_eq!(Status::from_str("done"), Status::Done);
        assert_eq!(Status::from_str("failed"), Status::Failed);
        assert_eq!(Status::from_str("resolved"), Status::Resolved);
        assert_eq!(Status::from_str("insufficient"), Status::Insufficient);
        assert_eq!(Status::from_str("error: timeout"), Status::Error("timeout".to_string()));
        assert_eq!(Status::from_str("error timeout"), Status::Error("timeout".to_string()));
        assert_eq!(Status::from_str("custom"), Status::Custom("custom".to_string()));
    }

    #[test]
    fn test_format_status_contains_ansi() {
        let done = G3Status::format_status(&Status::Done);
        assert!(done.contains("[done]"));
        assert!(done.contains("\x1b")); // Contains ANSI escape

        let failed = G3Status::format_status(&Status::Failed);
        assert!(failed.contains("[failed]"));

        let error = G3Status::format_status(&Status::Error("test".to_string()));
        assert!(error.contains("[error: test]"));
    }

    #[test]
    fn test_format_prefix() {
        let prefix = G3Status::format_prefix();
        assert!(prefix.contains("g3:"));
        assert!(prefix.contains("\x1b")); // Contains ANSI escape
    }
}
