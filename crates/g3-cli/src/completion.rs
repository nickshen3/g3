//! Tab completion support for g3 interactive mode.
//!
//! Provides:
//! - Command completion for `/` commands (at start of line)
//! - File path completion for paths anywhere in the line:
//!   - `./` - current directory
//!   - `../` - parent directory  
//!   - `~/` - home directory
//!   - `/` (not at start) - root directory
//! - Session ID completion for `/resume` command

use rustyline::completion::{Completer, FilenameCompleter, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};
use std::path::PathBuf;

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
        // A word starts after a space (unless quoted or escaped)
        let mut word_start = 0;
        let mut in_quotes = false;
        let mut quote_char = ' ';
        let mut prev_was_backslash = false;
        
        let chars: Vec<(usize, char)> = line_to_cursor.char_indices().collect();
        for (idx, &(i, c)) in chars.iter().enumerate() {
            if in_quotes {
                if c == quote_char && !prev_was_backslash {
                    in_quotes = false;
                }
            } else if prev_was_backslash {
                // This char is escaped, don't treat it as special
                // (e.g., backslash-space is part of the word)
            } else {
                match c {
                    '"' | '\'' => {
                        in_quotes = true;
                        quote_char = c;
                        word_start = i;
                    }
                    ' ' | '\t' => {
                        // Space starts a new word (unless escaped)
                        if idx + 1 < chars.len() {
                            word_start = chars[idx + 1].0;
                        } else {
                            word_start = pos; // At end, empty word
                        }
                    }
                    _ => {}
                }
            }
            prev_was_backslash = c == '\\' && !prev_was_backslash;
        }
        
        (word_start, &line_to_cursor[word_start..])
    }

    /// Check if a word looks like a path prefix
    fn is_path_prefix(&self, word: &str) -> bool {
        // Strip leading quote if present (for paths like "~/...)
        let word = word.trim_start_matches('"').trim_start_matches('\'');
        
        word.starts_with("./")
            || word.starts_with("../")
            || word.starts_with("~/")
            || word.starts_with('/')
            || word == "."
            || word == ".."
            || word == "~"
    }
    
    /// Strip quotes from a word for path completion
    fn strip_quotes<'a>(&self, word: &'a str) -> &'a str {
        word.trim_start_matches('"').trim_start_matches('\'')
            .trim_end_matches('"').trim_end_matches('\'')
    }
    
    /// Unescape backslash-escaped characters in a path
    /// e.g., "~/My\ Files" -> "~/My Files"
    fn unescape_path(&self, path: &str) -> String {
        let mut result = String::with_capacity(path.len());
        let mut chars = path.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\\' && chars.peek().is_some() {
                // Skip the backslash, take the next char literally
                if let Some(next) = chars.next() {
                    result.push(next);
                }
            } else {
                result.push(c);
            }
        }
        result
    }
    
    /// List available session IDs from .g3/sessions/, sorted by newest first.
    /// If `limit` is Some(n), returns at most n sessions.
    fn list_sessions(&self, limit: Option<usize>) -> Vec<String> {
        let sessions_dir = PathBuf::from(".g3/sessions");
        if !sessions_dir.is_dir() {
            return Vec::new();
        }
        
        let mut sessions: Vec<_> = std::fs::read_dir(&sessions_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_dir())
                    .filter_map(|entry| {
                        let modified = entry.metadata().ok()?.modified().ok()?;
                        Some((entry.file_name().to_string_lossy().to_string(), modified))
                    })
                    .collect()
            })
            .unwrap_or_default();
        
        // Sort by modification time, newest first
        sessions.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Apply limit if specified
        let sessions: Vec<String> = sessions
            .into_iter()
            .map(|(name, _)| name)
            .take(limit.unwrap_or(usize::MAX))
            .collect();
        
        sessions
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
        // We handle quotes ourselves since FilenameCompleter doesn't understand our extraction
        if self.is_path_prefix(word) || (word_start > 0 && line_to_cursor[word_start..].starts_with('/')) {
            // Check if word starts with a quote
            let has_leading_quote = word.starts_with('"') || word.starts_with('\'');
            let quote_char = if has_leading_quote { &word[..1] } else { "" };
            // Check if word has backslash escapes
            let has_escapes = word.contains('\\');
            
            // Strip quotes and unescape backslashes to get the actual path
            let path_str = self.strip_quotes(word);
            let path_unescaped = self.unescape_path(path_str);
            let path: &str = &path_unescaped;
            
            // Complete just the path portion
            let (rel_start, completions) = self.file_completer.complete(path, path.len(), ctx)?;
            
            if completions.is_empty() {
                return Ok((pos, vec![]));
            }
            
            // Adjust completions to account for quotes and word position
            let adjusted: Vec<Pair> = completions
                .into_iter()
                .map(|pair| {
                    // If we had a leading quote, add it back
                    // Also check if the path has spaces - if so, wrap in quotes
                    let has_spaces = pair.replacement.contains(' ');
                    let replacement = if has_leading_quote {
                        // Preserve the original quote style
                        format!("{}{}{}", quote_char, pair.replacement, quote_char)
                    } else if has_escapes && has_spaces {
                        // User was using backslash escapes, continue with that style
                        pair.replacement.replace(' ', "\\ ")
                    } else if has_spaces {
                        // Add quotes around paths with spaces
                        format!("\"{}\"" , pair.replacement)
                    } else {
                        pair.replacement
                    };
                    
                    let needs_quotes = has_spaces || has_leading_quote;
                    let display = if needs_quotes && !pair.display.starts_with('"') {
                        format!("\"{}\"" , pair.display)
                    } else {
                        pair.display
                    };
                    
                    Pair { display, replacement }
                })
                .collect();
            
            // Return with word_start so the whole word gets replaced
            return Ok((word_start, adjusted));
        }
        
        // Case 3: Check if we're after a command that takes a path argument
        if line_to_cursor.starts_with("/run ") 
        {
            // For commands, just use the file completer on the path portion
            let path = self.strip_quotes(word);
            let (_, completions) = self.file_completer.complete(path, path.len(), ctx)?;
            return Ok((word_start, completions));
        }
        
        // Case 4: Session ID completion for /resume command
        if line_to_cursor.starts_with("/resume ") {
            let partial = word;
            // Get all sessions sorted by newest first, then filter and limit
            let sessions = self.list_sessions(None);
            let matches: Vec<Pair> = sessions
                .into_iter()
                .filter(|s| s.starts_with(partial))
                .map(|s| Pair {
                    display: s.clone(),
                    replacement: s,
                })
                .take(8)  // Show at most 8 most recent matches
                .collect();
            return Ok((word_start, matches));
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

    #[test]
    fn test_actual_completion_with_quotes() {
        use rustyline::completion::Completer;
        use rustyline::Context;
        
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);
        
        // Test 1: "~/ - unclosed quote at start of path
        println!("\n=== Test 1: Unclosed quote \"~/ ===");
        let line = "edit \"~/";
        let pos = line.len();
        match helper.complete(line, pos, &ctx) {
            Ok((start, completions)) => {
                println!("Line: '{}', pos: {}", line, pos);
                println!("Start: {}, num_completions: {}", start, completions.len());
                if !completions.is_empty() {
                    println!("First few: {:?}", completions.iter().take(3).map(|p| &p.replacement).collect::<Vec<_>>());
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
        
        // Test 2: ~/My\ - backslash before cursor
        println!("\n=== Test 2: Backslash escape ~/My\\ ===");
        let line = "edit ~/My\\ ";
        let pos = line.len();
        match helper.complete(line, pos, &ctx) {
            Ok((start, completions)) => {
                println!("Line: '{}', pos: {}", line, pos);
                println!("Start: {}, num_completions: {}", start, completions.len());
                if !completions.is_empty() {
                    println!("First few: {:?}", completions.iter().take(3).map(|p| &p.replacement).collect::<Vec<_>>());
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
        
        // Test 3: "~/" - closed quote
        println!("\n=== Test 3: Closed quote \"/~/\" ===");
        let line = "edit \"~/\"";
        let pos = line.len();
        match helper.complete(line, pos, &ctx) {
            Ok((start, completions)) => {
                println!("Line: '{}', pos: {}", line, pos);
                println!("Start: {}, num_completions: {}", start, completions.len());
                if !completions.is_empty() {
                    println!("First few: {:?}", completions.iter().take(3).map(|p| &p.replacement).collect::<Vec<_>>());
                }
            }
            Err(e) => println!("Error: {:?}", e),
        }
    }

    #[test]
    fn test_no_completion_for_bare_quote() {
        use rustyline::completion::Completer;
        use rustyline::Context;
        
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);
        
        // Just a quote with no path prefix - should NOT trigger completion
        let line = "edit \"";
        let pos = line.len();
        let (start, completions) = helper.complete(line, pos, &ctx).unwrap();
        println!("Line: '{}', start: {}, completions: {}", line, start, completions.len());
        
        // Should return no completions since "" is not a path prefix
        assert_eq!(completions.len(), 0, "Bare quote should not trigger path completion");
    }

    #[test]
    fn test_no_completion_for_random_text_in_quotes() {
        use rustyline::completion::Completer;
        use rustyline::Context;
        
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);
        
        // Random text in quotes - should NOT trigger completion
        let line = "edit \"hello world";
        let pos = line.len();
        let (start, completions) = helper.complete(line, pos, &ctx).unwrap();
        println!("Line: '{}', start: {}, completions: {}", line, start, completions.len());
        assert_eq!(completions.len(), 0, "Random quoted text should not trigger path completion");
        
        // Just "foo - no path prefix
        let line = "edit \"foo";
        let pos = line.len();
        let (start, completions) = helper.complete(line, pos, &ctx).unwrap();
        println!("Line: '{}', start: {}, completions: {}", line, start, completions.len());
        assert_eq!(completions.len(), 0, "Quoted non-path should not trigger completion");
    }

    #[test]
    fn test_resume_completion_lists_sessions() {
        use rustyline::completion::Completer;
        use rustyline::Context;
        
        let helper = G3Helper::new();
        let history = rustyline::history::DefaultHistory::new();
        let ctx = Context::new(&history);
        
        // Test against real .g3/sessions in current project
        // This test runs from the project root where .g3/sessions exists
        let line = "/resume ";
        let pos = line.len();
        let (start, completions) = helper.complete(line, pos, &ctx).unwrap();
        println!("Sessions found: {}", completions.len());
        
        // If .g3/sessions exists, we should get some completions
        if std::path::Path::new(".g3/sessions").is_dir() {
            assert!(completions.len() > 0, "Should list sessions when .g3/sessions exists");
            
            // Test filtering - use first few chars of first session
            if let Some(first) = completions.first() {
                let prefix = &first.replacement[..first.replacement.len().min(5)];
                let line = format!("/resume {}", prefix);
                let pos = line.len();
                let (_, filtered) = helper.complete(&line, pos, &ctx).unwrap();
                assert!(filtered.len() >= 1, "Should find at least one match");
                assert!(filtered.iter().all(|p| p.replacement.starts_with(prefix)));
            }
        }
        
        // Test with non-matching prefix - should return empty
        let line = "/resume zzz_nonexistent_prefix_";
        let pos = line.len();
        let (_, completions) = helper.complete(line, pos, &ctx).unwrap();
        assert_eq!(completions.len(), 0, "Non-matching prefix should return empty");
    }
    
    #[test]
    fn test_resume_completion_graceful_no_panic() {
        let helper = G3Helper::new();
        
        // Test list_sessions directly - should not panic regardless of whether
        // .g3/sessions exists or not
        let sessions = helper.list_sessions(None);
        
        // This will either return sessions (if .g3/sessions exists) or empty
        // The important thing is it doesn't panic
        println!("list_sessions returned {} sessions", sessions.len());
    }
}
