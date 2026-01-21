use crate::{types::Rect, ComputerController};
use anyhow::Result;
use async_trait::async_trait;

pub struct LinuxController;

impl LinuxController {
    pub fn new() -> Result<Self> {
        tracing::warn!("Linux computer control not fully implemented");
        Ok(Self)
    }
}

#[async_trait]
impl ComputerController for LinuxController {
    async fn take_screenshot(
        &self,
        _path: &str,
        _region: Option<Rect>,
        _window_id: Option<&str>,
    ) -> Result<()> {
        anyhow::bail!("Linux screenshot implementation not yet available")
    }
}
