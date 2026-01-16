use crossterm::style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor};

/// Simple output helper for printing messages
#[derive(Clone)]
pub struct SimpleOutput;

impl SimpleOutput {
    pub fn new() -> Self {
        SimpleOutput
    }

    pub fn print(&self, message: &str) {
        println!("{}", message);
    }

    pub fn print_smart(&self, message: &str) {
        println!("{}", message);
    }

    /// Print a g3 status message with colored tag and status
    /// Format: "g3: <message> ... [status]"
    /// - "g3:" is bold green
    /// - "done" status is normal
    /// - "failed" and "error" statuses are red
    pub fn print_g3_status(&self, message: &str, status: &str) {
        let status_colored = match status {
            s if s.starts_with("error") || s == "failed" => {
                format!(
                    "{}[{}]{}",
                    SetForegroundColor(Color::Red),
                    status,
                    ResetColor
                )
            }
            _ => format!("[{}]", status),
        };

        println!(
            "{}{}g3:{}{} {} ... {}",
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            ResetColor,
            SetAttribute(Attribute::Reset),
            message,
            status_colored
        );
    }

    /// Print a g3 status message in progress (no status yet)
    /// Format: "g3: <message> ..."
    /// - "g3:" is bold green
    pub fn print_g3_progress(&self, message: &str) {
        println!(
            "{}{}g3:{}{} {} ...",
            SetAttribute(Attribute::Bold),
            SetForegroundColor(Color::Green),
            ResetColor,
            SetAttribute(Attribute::Reset),
            message
        );
    }
}

impl Default for SimpleOutput {
    fn default() -> Self {
        Self::new()
    }
}
