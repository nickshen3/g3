//! Tab completion support for g3 interactive mode.
//!
//! Provides:
//! - Command completion for `/` commands
//! - File path completion for `/run <path>`
//! - Extensible for future semantic completions (sessions, fragments, etc.)

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

/// Available `/` commands for completion
const COMMANDS: &[&str] = &[
    "/clear",
    "/compact",
    "/dump",
    "/fragments",
    "/help",
    "/readme",
    "/rehydrate",
    "/resume",
    "/run",
    "/skinnify",
    "/stats",
    "/thinnify",
];

/// Helper struct for rustyline that provides tab completion.
pub struct G3Helper {
    /// File path completer for `/run` command
    file_completer: FilenameCompleter,
}

impl G3Helper {
    pub fn new() -> Self {
        Self {
            file_completer: FilenameCompleter::new(),
        }
    }
}

impl Default for G3Helper {
    fn default() -> Self {
        Self::new()
    }
}

impl Completer for G3Helper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        // Only complete up to cursor position
        let line_to_cursor = &line[..pos];

        // Case 1: `/run <path>` - complete file paths
        if line_to_cursor.starts_with("/run ") {
            // Delegate to file completer
            return self.file_completer.complete(line, pos, ctx);
        }

        // Case 2: `/rehydrate <fragment_id>` - future: complete fragment IDs
        // Case 3: `/resume <session>` - future: complete session IDs

        // Case 4: `/` commands - complete command names
        if line_to_cursor.starts_with('/') {
            let prefix = line_to_cursor;
            let matches: Vec<Pair> = COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| Pair {
                    display: cmd.to_string(),
                    replacement: cmd.to_string(),
                })
                .collect();
            return Ok((0, matches));
        }

        // No completion for regular prompts
        Ok((pos, vec![]))
    }
}

// Required trait implementations for Helper
impl Hinter for G3Helper {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        None
    }
}

impl Highlighter for G3Helper {}

impl Validator for G3Helper {}

impl Helper for G3Helper {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_completion() {
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);

        // Complete "/com" -> "/compact"
        let (start, matches) = helper.complete("/com", 4, &ctx).unwrap();
        assert_eq!(start, 0);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].replacement, "/compact");
    }

    #[test]
    fn test_command_completion_multiple() {
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);

        // Complete "/s" -> "/skinnify", "/stats"
        let (start, matches) = helper.complete("/s", 2, &ctx).unwrap();
        assert_eq!(start, 0);
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().any(|m| m.replacement == "/skinnify"));
        assert!(matches.iter().any(|m| m.replacement == "/stats"));
    }

    #[test]
    fn test_no_completion_for_regular_input() {
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);

        // Regular text should not complete
        let (start, matches) = helper.complete("hello world", 11, &ctx).unwrap();
        assert_eq!(start, 11);
        assert!(matches.is_empty());
    }
}
