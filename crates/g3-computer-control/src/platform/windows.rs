use crate::{types::Rect, ComputerController};
use anyhow::Result;
use async_trait::async_trait;

pub struct WindowsController;

impl WindowsController {
    pub fn new() -> Result<Self> {
        tracing::warn!("Windows computer control not fully implemented");
        Ok(Self)
    }
}

#[async_trait]
impl ComputerController for WindowsController {
    async fn take_screenshot(
        &self,
        _path: &str,
        _region: Option<Rect>,
        _window_id: Option<&str>,
    ) -> Result<()> {
        anyhow::bail!("Windows screenshot implementation not yet available")
    }
}
