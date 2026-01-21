// Suppress warnings from objc crate macros
#![allow(unexpected_cfgs)]

pub mod platform;
pub mod types;
pub mod webdriver;

// Re-export webdriver types for convenience
pub use webdriver::{
    chrome::ChromeDriver, safari::SafariDriver, WebDriverController, WebElement,
    diagnostics::{run_diagnostics as run_chrome_diagnostics, ChromeDiagnosticReport, DiagnosticStatus},
};

use anyhow::Result;
use async_trait::async_trait;
use types::*;

#[async_trait]
pub trait ComputerController: Send + Sync {
    // Screen capture
    async fn take_screenshot(
        &self,
        path: &str,
        region: Option<Rect>,
        window_id: Option<&str>,
    ) -> Result<()>;
}

// Platform-specific constructor
pub fn create_controller() -> Result<Box<dyn ComputerController>> {
    #[cfg(target_os = "macos")]
    return Ok(Box::new(platform::macos::MacOSController::new()?));

    #[cfg(target_os = "linux")]
    return Ok(Box::new(platform::linux::LinuxController::new()?));

    #[cfg(target_os = "windows")]
    return Ok(Box::new(platform::windows::WindowsController::new()?));

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    anyhow::bail!("Unsupported platform")
}
