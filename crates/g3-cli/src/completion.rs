//! Tab completion support for g3 interactive mode.
//!
//! Provides:
//! - Command completion for `/` commands (at start of line)
//! - File path completion for paths anywhere in the line:
//!   - `./` - current directory
//!   - `../` - parent directory  
//!   - `~/` - home directory
//!   - `/` (not at start) - root directory
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
    /// File path completer
    file_completer: FilenameCompleter,
}

impl G3Helper {
    pub fn new() -> Self {
        Self {
            file_completer: FilenameCompleter::new(),
        }
    }

    /// Find the start of the current "word" being typed, respecting quotes.
    /// Returns (word_start, word) where word_start is the byte index.
    fn extract_word<'a>(&self, line: &'a str, pos: usize) -> (usize, &'a str) {
        let line_to_cursor = &line[..pos];
        
        // Look backwards for the start of the word
        // A word starts after a space (unless quoted)
        let mut word_start = 0;
        let mut in_quotes = false;
        let mut quote_char = ' ';
        
        for (i, c) in line_to_cursor.char_indices() {
            if in_quotes {
                if c == quote_char {
                    in_quotes = false;
                }
            } else {
                match c {
                    '"' | '\'' => {
                        in_quotes = true;
                        quote_char = c;
                        word_start = i;
                    }
                    ' ' | '\t' => {
                        // Next char starts a new word
                        word_start = i + 1;
                    }
                    _ => {}
                }
            }
        }
        
        (word_start, &line_to_cursor[word_start..])
    }

    /// Check if a word looks like a path prefix
    fn is_path_prefix(&self, word: &str) -> bool {
        word.starts_with("./")
            || word.starts_with("../")
            || word.starts_with("~/")
            || word.starts_with('/')
            || word == "."
            || word == ".."
            || word == "~"
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
        let line_to_cursor = &line[..pos];
        
        // Extract the current word being typed
        let (word_start, word) = self.extract_word(line, pos);
        
        // Case 1: Command completion - `/` at the very start of the line
        // Only complete commands if we're typing the first word and it starts with `/`
        if word_start == 0 && word.starts_with('/') && !word.contains(' ') {
            // Check if this looks like a command (no path separators after the first /)
            let after_slash = &word[1..];
            if !after_slash.contains('/') {
                // This is a command like "/com" not a path like "/etc"
                let matches: Vec<Pair> = COMMANDS
                    .iter()
                    .filter(|cmd| cmd.starts_with(word))
                    .map(|cmd| Pair {
                        display: cmd.to_string(),
                        replacement: cmd.to_string(),
                    })
                    .collect();
                
                if !matches.is_empty() {
                    return Ok((0, matches));
                }
                // If no command matches, fall through to path completion
                // (e.g., "/etc" should complete as a path)
            }
        }
        
        // Case 2: Path completion for path-like prefixes
        // Delegate to FilenameCompleter which handles:
        // - Tilde expansion
        // - Quote handling for spaces
        // - Proper escaping
        if self.is_path_prefix(word) || word_start > 0 && line_to_cursor[word_start..].starts_with('/') {
            return self.file_completer.complete(line, pos, ctx);
        }
        
        // Case 3: Check if we're after a command that takes a path argument
        if line_to_cursor.starts_with("/run ") 
            || line_to_cursor.starts_with("/rehydrate ")
        {
            return self.file_completer.complete(line, pos, ctx);
        }

        // No completion for regular text
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
    fn test_path_prefix_detection() {
        let helper = G3Helper::new();
        
        assert!(helper.is_path_prefix("./"));
        assert!(helper.is_path_prefix("./src"));
        assert!(helper.is_path_prefix("../"));
        assert!(helper.is_path_prefix("~/"));
        assert!(helper.is_path_prefix("~/Documents"));
        assert!(helper.is_path_prefix("/etc"));
        assert!(helper.is_path_prefix("."));
        assert!(helper.is_path_prefix(".."));
        assert!(helper.is_path_prefix("~"));
        
        assert!(!helper.is_path_prefix("hello"));
        assert!(!helper.is_path_prefix("src"));
    }

    #[test]
    fn test_extract_word_simple() {
        let helper = G3Helper::new();
        
        let (start, word) = helper.extract_word("hello world", 11);
        assert_eq!(start, 6);
        assert_eq!(word, "world");
    }

    #[test]
    fn test_extract_word_with_path() {
        let helper = G3Helper::new();
        
        let (start, word) = helper.extract_word("edit ./src/main.rs", 18);
        assert_eq!(start, 5);
        assert_eq!(word, "./src/main.rs");
    }

    #[test]
    fn test_extract_word_quoted() {
        let helper = G3Helper::new();
        
        // Quoted path with spaces
        let (start, word) = helper.extract_word("edit \"./My Files/doc", 20);
        assert_eq!(start, 5);
        assert_eq!(word, "\"./My Files/doc");
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

    #[test]
    fn test_slash_at_start_is_command() {
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);

        // "/h" at start should complete to commands
        let (start, matches) = helper.complete("/h", 2, &ctx).unwrap();
        assert_eq!(start, 0);
        assert!(matches.iter().any(|m| m.replacement == "/help"));
    }
}
