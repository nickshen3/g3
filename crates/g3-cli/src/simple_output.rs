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
}

impl Default for SimpleOutput {
    fn default() -> Self {
        Self::new()
    }
}
