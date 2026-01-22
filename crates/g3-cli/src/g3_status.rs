//! Centralized formatting for g3 system status messages.
//!
//! Provides consistent "g3:" prefixed status messages with progress indicators
//! and completion statuses. Use `progress()` + `done()`/`failed()` for two-step
//! output, or `complete()` for one-shot messages.

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
    pub fn parse(s: &str) -> Self {
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
    /// Print "g3: <message> ..." (no newline). Complete with `done()` or `failed()`.
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

    /// Print "g3: <message> ..." with newline (standalone progress).
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

    pub fn done() {
        println!(
            " {}{}[done]{}",
            SetForegroundColor(Color::Green),
            SetAttribute(Attribute::Bold),
            ResetColor
        );
    }

    pub fn failed() {
        println!(
            " {}[failed]{}",
            SetForegroundColor(Color::Red),
            ResetColor
        );
    }

    pub fn error(msg: &str) {
        println!(
            " {}[error: {}]{}",
            SetForegroundColor(Color::Red),
            msg,
            ResetColor
        );
    }

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

    /// Print "g3: <message> ... [status]" (one-shot).
    pub fn complete(message: &str, status: Status) {
        Self::progress(message);
        Self::status(&status);
    }

    #[allow(dead_code)]
    pub fn info(message: &str) {
        println!(
            "{}... {}{}",
            SetForegroundColor(Color::DarkGrey),
            message,
            ResetColor
        );
    }

    /// Print info inline (moves cursor up, appends to previous line).
    pub fn info_inline(message: &str) {
        print!(
            "\x1b[1A\x1b[999C {}... {}{}\n",
            SetForegroundColor(Color::DarkGrey),
            message,
            ResetColor
        );
        let _ = io::stdout().flush();
    }

    /// Format a status for inline use (returns formatted string).
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
                "{}{}{}",
                SetForegroundColor(Color::Red),
                if msg.is_empty() {
                    "[error]".to_string()
                } else {
                    format!("[error: {}]", msg)
                },
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

    pub fn format_prefix() -> String {
        format!(
            "{}{}g3:{}{}",
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            ResetColor,
            SetAttribute(Attribute::Reset),
        )
    }

    /// Print "... resuming <session_id> [status]" with cyan session ID.
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

    /// Print thinning result: "g3: thinning context ... 70% -> 40% ... [done]"
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

    /// Print "g3: <message> <path> [status]" with cyan path.
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

    /// Print project loading status: "g3: loading <project-name> .. ✓ file1  ✓ file2 .. [done]"
    ///
    /// Used by the /project command to show what project files were loaded.
    pub fn loading_project(project_name: &str, loaded_files_status: &str) {
        print!(
            "{} loading {}{}{} .. {} ..",
            Self::format_prefix(),
            SetForegroundColor(Color::Cyan),
            project_name,
            ResetColor,
            loaded_files_status
        );
        Self::done();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_from_str() {
        assert_eq!(Status::parse("done"), Status::Done);
        assert_eq!(Status::parse("failed"), Status::Failed);
        assert_eq!(Status::parse("resolved"), Status::Resolved);
        assert_eq!(Status::parse("insufficient"), Status::Insufficient);
        assert_eq!(Status::parse("error: timeout"), Status::Error("timeout".to_string()));
        assert_eq!(Status::parse("error timeout"), Status::Error("timeout".to_string()));
        assert_eq!(Status::parse("custom"), Status::Custom("custom".to_string()));
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
