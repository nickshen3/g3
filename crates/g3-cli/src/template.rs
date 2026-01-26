//! Template variable injection for included prompt files.
//!
//! Supports `{{var}}` syntax for variable substitution.
//! Currently supported variables:
//! - `today`: Current date in ISO format (YYYY-MM-DD)

use chrono::Local;
use regex::Regex;
use std::collections::HashSet;

/// Process template variables in the given content.
/// 
/// Replaces `{{var}}` patterns with their values.
/// Warns about unknown variables and leaves them unchanged.
pub fn process_template(content: &str) -> String {
    // Regex to match {{variable_name}}
    let re = Regex::new(r"\{\{([a-zA-Z_][a-zA-Z0-9_]*)\}\}").unwrap();
    
    // Track unknown variables to warn only once per variable
    let mut unknown_vars: HashSet<String> = HashSet::new();
    
    let result = re.replace_all(content, |caps: &regex::Captures| {
        let var_name = &caps[1];
        match resolve_variable(var_name) {
            Some(value) => value,
            None => {
                if unknown_vars.insert(var_name.to_string()) {
                    tracing::warn!("Unknown template variable: {{{{{}}}}}", var_name);
                }
                // Leave unknown variables unchanged
                caps[0].to_string()
            }
        }
    });
    
    result.into_owned()
}

/// Resolve a template variable to its value.
fn resolve_variable(name: &str) -> Option<String> {
    match name {
        "today" => Some(Local::now().format("%Y-%m-%d (%A)").to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_today_variable() {
        let input = "Today is {{today}}";
        let result = process_template(input);
        
        // Should contain a date in YYYY-MM-DD format
        assert!(!result.contains("{{today}}"));
        assert!(result.starts_with("Today is "));
        
        // Verify date format (YYYY-MM-DD (DayName))
        let date_part = &result["Today is ".len()..];
        // Should be at least "YYYY-MM-DD (X)" = 13+ chars
        assert!(date_part.len() >= 13, "Date should be at least 13 chars, got: {}", date_part);
        assert_eq!(&date_part[4..5], "-", "Should have dash at position 4");
        assert_eq!(&date_part[7..8], "-", "Should have dash at position 7");
        assert!(date_part.contains("(") && date_part.contains(")"), "Should contain day name in parens");
    }

    #[test]
    fn test_multiple_today_variables() {
        let input = "Start: {{today}}, End: {{today}}";
        let result = process_template(input);
        
        // Both should be replaced
        assert!(!result.contains("{{today}}"));
        assert!(result.contains("Start: "));
        assert!(result.contains(", End: "));
    }

    #[test]
    fn test_unknown_variable_unchanged() {
        let input = "Hello {{unknown_var}}!";
        let result = process_template(input);
        
        // Unknown variable should remain unchanged
        assert_eq!(result, "Hello {{unknown_var}}!");
    }

    #[test]
    fn test_mixed_known_and_unknown() {
        let input = "Date: {{today}}, Name: {{name}}";
        let result = process_template(input);
        
        // today should be replaced, name should remain
        assert!(!result.contains("{{today}}"));
        assert!(result.contains("{{name}}"));
    }

    #[test]
    fn test_no_variables() {
        let input = "No variables here";
        let result = process_template(input);
        
        assert_eq!(result, "No variables here");
    }

    #[test]
    fn test_empty_braces() {
        let input = "Empty {{}} braces";
        let result = process_template(input);
        
        // Empty braces don't match the pattern, should remain unchanged
        assert_eq!(result, "Empty {{}} braces");
    }

    #[test]
    fn test_single_braces_ignored() {
        let input = "Single {today} braces";
        let result = process_template(input);
        
        // Single braces should not be processed
        assert_eq!(result, "Single {today} braces");
    }

    #[test]
    fn test_variable_with_underscores() {
        let input = "{{my_custom_var}}";
        let result = process_template(input);
        
        // Unknown but valid variable name, should remain unchanged
        assert_eq!(result, "{{my_custom_var}}");
    }

    #[test]
    fn test_variable_with_numbers() {
        let input = "{{var123}}";
        let result = process_template(input);
        
        // Unknown but valid variable name, should remain unchanged
        assert_eq!(result, "{{var123}}");
    }
}
