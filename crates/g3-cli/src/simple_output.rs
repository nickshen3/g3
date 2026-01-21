use crate::g3_status::{G3Status, Status};

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

    pub fn print_inline(&self, message: &str) {
        use std::io::{Write, stdout};
        print!("{}", message);
        let _ = stdout().flush();
    }

    pub fn print_smart(&self, message: &str) {
        println!("{}", message);
    }

    /// Print a g3 status message with colored tag and status
    /// Format: "g3: <message> ... [status]"
    /// Uses centralized G3Status formatting.
    pub fn print_g3_status(&self, message: &str, status: &str) {
        G3Status::complete(message, Status::parse(status));
    }

    /// Print a g3 status message in progress (no status yet)
    /// Format: "g3: <message> ..."
    /// Uses centralized G3Status formatting.
    pub fn print_g3_progress(&self, message: &str) {
        G3Status::progress_ln(message);
    }
}

impl Default for SimpleOutput {
    fn default() -> Self {
        Self::new()
    }
}
