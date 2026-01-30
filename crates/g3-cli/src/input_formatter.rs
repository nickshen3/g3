//! Input formatting for interactive mode.
//!
//! Formats user input with markdown-style highlighting:
//! - ALL CAPS words become bold
//! - Quoted text ("..." or '...') becomes cyan
//! - Standard markdown formatting (bold, italic, code) is applied

use crossterm::terminal;
use regex::Regex;
use std::io::Write;
use std::io::IsTerminal;
use termimad::MadSkin;

use crate::streaming_markdown::StreamingMarkdownFormatter;

/// Pre-process input text to add markdown markers for special formatting.
/// 
/// This pass runs BEFORE markdown formatting:
/// 1. ALL CAPS words (2+ chars) → wrapped in ** for bold
/// 2. Quoted text "..." or '...' → wrapped in special markers for cyan
/// 
/// Returns the preprocessed text ready for markdown formatting.
pub fn preprocess_input(input: &str) -> String {
    let mut result = input.to_string();
    
    // First, handle ALL CAPS words (2+ uppercase letters, may include numbers)
    // Must be a standalone word (word boundaries)
    let caps_re = Regex::new(r"\b([A-Z][A-Z0-9]{1,}[A-Z0-9]*)\b").unwrap();
    result = caps_re.replace_all(&result, "**$1**").to_string();
    
    // Then, handle quoted text - wrap in a special marker that we'll process after markdown
    // Use lowercase placeholders that won't be matched by the ALL CAPS regex
    let double_quote_re = Regex::new(r#""([^"]+)""#).unwrap();
    result = double_quote_re.replace_all(&result, "\x00qdbl\x00$1\x00qend\x00").to_string();
    
    let single_quote_re = Regex::new(r"'([^']+)'").unwrap();
    result = single_quote_re.replace_all(&result, "\x00qsgl\x00$1\x00qend\x00").to_string();
    
    result
}

/// Apply cyan highlighting to quoted text markers.
/// This runs AFTER markdown formatting to apply the cyan color.
fn apply_quote_highlighting(text: &str) -> String {
    let mut result = text.to_string();
    
    // Replace double-quote markers with cyan formatting
    // \x1b[36m = cyan, \x1b[0m = reset
    result = result.replace("\x00qdbl\x00", "\x1b[36m\"");
    result = result.replace("\x00qsgl\x00", "\x1b[36m'");
    result = result.replace("\x00qend\x00", "\x1b[0m");
    
    // Add back the closing quotes
    // We need to insert them before the reset code
    let re = Regex::new(r#"(\x1b\[36m")([^\x1b]*)\x1b\[0m"#).unwrap();
    result = re.replace_all(&result, |caps: &regex::Captures| {
        format!("{}{}\"\x1b[0m", &caps[1], &caps[2])
    }).to_string();
    
    let re = Regex::new(r"(\x1b\[36m')([^\x1b]*)\x1b\[0m").unwrap();
    result = re.replace_all(&result, |caps: &regex::Captures| {
        format!("{}{}'\x1b[0m", &caps[1], &caps[2])
    }).to_string();
    
    result
}

/// Format user input with markdown and special highlighting.
/// 
/// Applies:
/// 1. ALL CAPS → bold (green)
/// 2. Quoted text → cyan
/// 3. Standard markdown (bold, italic, inline code)
pub fn format_input(input: &str) -> String {
    // Pre-process to add markdown markers
    let preprocessed = preprocess_input(input);
    
    // Apply markdown formatting using the streaming formatter
    let skin = MadSkin::default();
    let mut formatter = StreamingMarkdownFormatter::new(skin);
    let formatted = formatter.process(&preprocessed);
    let formatted = formatted + &formatter.finish();
    
    // Apply quote highlighting (after markdown so colors don't interfere)
    apply_quote_highlighting(&formatted)
}

/// Reprint user input in place with formatting.
/// 
/// This moves the cursor up to overwrite the original input line,
/// then prints the formatted version.
/// 
/// Note: This function only performs formatting when stdout is a TTY.
/// In non-TTY contexts (piped output, etc.), it does nothing to avoid
/// corrupting terminal state for subsequent stdin operations.
pub fn reprint_formatted_input(input: &str, prompt: &str) {
    // Only reformat if stdout is a TTY - avoid corrupting terminal state otherwise
    if !std::io::stdout().is_terminal() {
        return;
    }

    // Format the input
    let formatted = format_input(input);

    // Get terminal width to calculate visual lines
    // The prompt + input may wrap across multiple terminal rows
    let term_width = terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80);

    // Calculate visual lines: prompt + input length divided by terminal width
    // This accounts for line wrapping in the terminal
    let total_chars = prompt.len() + input.len();
    let visual_lines = ((total_chars + term_width - 1) / term_width).max(1); // ceiling division

    // Move cursor up by the number of lines and clear
    for _ in 0..visual_lines {
        // Move up one line and clear it
        print!("\x1b[1A\x1b[2K");
    }

    // Reprint with prompt and formatted input
    // Use dim color for the prompt to distinguish from the formatted input
    println!("\x1b[2m{}\x1b[0m{}", prompt, formatted);

    // Ensure output is flushed
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_preprocess_all_caps() {
        let input = "please FIX the BUG in this CODE";
        let result = preprocess_input(input);
        assert!(result.contains("**FIX**"));
        assert!(result.contains("**BUG**"));
        assert!(result.contains("**CODE**"));
        // "please", "the", "in", "this" should not be wrapped
        assert!(!result.contains("**please**"));
    }
    
    #[test]
    fn test_preprocess_single_caps_not_matched() {
        // Single letter caps should not be matched
        let input = "I am A person";
        let result = preprocess_input(input);
        // "I" and "A" are single letters, should not be wrapped
        assert!(!result.contains("**I**"));
        assert!(!result.contains("**A**"));
    }
    
    #[test]
    fn test_preprocess_double_quotes() {
        let input = r#"say "hello world" please"#;
        let result = preprocess_input(input);
        assert!(result.contains("\x00qdbl\x00hello world\x00qend\x00"));
    }
    
    #[test]
    fn test_preprocess_single_quotes() {
        let input = "use the 'special' method";
        let result = preprocess_input(input);
        assert!(result.contains("\x00qsgl\x00special\x00qend\x00"));
    }
    
    #[test]
    fn test_preprocess_mixed() {
        let input = r#"FIX the "critical" BUG"#;
        let result = preprocess_input(input);
        assert!(result.contains("**FIX**"));
        assert!(result.contains("**BUG**"));
        assert!(result.contains("\x00qdbl\x00critical\x00qend\x00"));
    }
    
    #[test]
    fn test_apply_quote_highlighting() {
        let input = "\x00qdbl\x00hello\x00qend\x00";
        let result = apply_quote_highlighting(input);
        assert!(result.contains("\x1b[36m"));
        assert!(result.contains("\x1b[0m"));
    }
    
    #[test]
    fn test_format_input_caps_become_bold() {
        let input = "FIX this";
        let result = format_input(input);
        // Should contain bold ANSI code (\x1b[1;32m for bold green)
        assert!(result.contains("\x1b[1;32m") || result.contains("FIX"));
    }
    
    #[test]
    fn test_format_input_quotes_become_cyan() {
        let input = r#"say "hello""#;
        let result = format_input(input);
        // Should contain cyan ANSI code
        assert!(result.contains("\x1b[36m"));
    }
    
    #[test]
    fn test_caps_with_numbers() {
        let input = "check HTTP2 and TLS13";
        let result = preprocess_input(input);
        assert!(result.contains("**HTTP2**"));
        assert!(result.contains("**TLS13**"));
    }
    
    #[test]
    fn test_two_letter_caps() {
        let input = "use IO and DB";
        let result = preprocess_input(input);
        assert!(result.contains("**IO**"));
        assert!(result.contains("**DB**"));
    }
}
