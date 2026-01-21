use crate::{
    types::Rect, ComputerController,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use core_foundation::array::CFArray;
use core_foundation::base::{TCFType, ToVoid};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::window::{
    kCGNullWindowID, kCGWindowListOptionOnScreenOnly, CGWindowListCopyWindowInfo,
};
use std::path::Path;

pub struct MacOSController;

impl MacOSController {
    pub fn new() -> Result<Self> {
        tracing::debug!("Initialized macOS controller");
        Ok(Self)
    }
}

#[async_trait]
impl ComputerController for MacOSController {
    async fn take_screenshot(
        &self,
        path: &str,
        region: Option<Rect>,
        window_id: Option<&str>,
    ) -> Result<()> {
        // Enforce that window_id must be provided
        if window_id.is_none() {
            return Err(anyhow::anyhow!("window_id is required. You must specify which window to capture (e.g., 'Safari', 'Terminal', 'Google Chrome'). Use list_windows to see available windows."));
        }

        // Determine the temporary directory for screenshots
        let temp_dir = std::env::var("TMPDIR")
            .or_else(|_| std::env::var("HOME").map(|h| format!("{}/tmp", h)))
            .unwrap_or_else(|_| "/tmp".to_string());

        // Ensure temp directory exists
        std::fs::create_dir_all(&temp_dir)?;

        // If path is relative or doesn't specify a directory, use temp_dir
        let final_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("{}/{}", temp_dir.trim_end_matches('/'), path)
        };

        let path_obj = Path::new(&final_path);
        if let Some(parent) = path_obj.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let app_name = window_id.unwrap(); // Safe because we checked is_none() above

        // Get the window ID for the specified application
        let cg_window_id = unsafe {
            let window_list =
                CGWindowListCopyWindowInfo(kCGWindowListOptionOnScreenOnly, kCGNullWindowID);

            let array = CFArray::<CFDictionary>::wrap_under_create_rule(window_list);
            let count = array.len();

            let mut found_window_id: Option<(u32, String)> = None; // (id, owner)
            let app_name_lower = app_name.to_lowercase();

            for i in 0..count {
                let dict = array.get(i).unwrap();

                // Get owner name
                let owner_key = CFString::from_static_string("kCGWindowOwnerName");
                let owner: String = if let Some(value) = dict.find(owner_key.to_void()) {
                    let s: CFString = TCFType::wrap_under_get_rule(*value as *const _);
                    s.to_string()
                } else {
                    continue;
                };

                tracing::debug!(
                    "Checking window: owner='{}', looking for '{}'",
                    owner,
                    app_name
                );
                let owner_lower = owner.to_lowercase();

                // Normalize by removing spaces for exact matching
                let app_name_normalized = app_name_lower.replace(" ", "");
                let owner_normalized = owner_lower.replace(" ", "");

                // ONLY accept exact matches (case-insensitive, with or without spaces)
                // This prevents "Goose" from matching "GooseStudio"
                let is_match =
                    owner_lower == app_name_lower || owner_normalized == app_name_normalized;

                if is_match {
                    // Get window ID
                    let window_id_key = CFString::from_static_string("kCGWindowNumber");
                    if let Some(value) = dict.find(window_id_key.to_void()) {
                        let num: core_foundation::number::CFNumber =
                            TCFType::wrap_under_get_rule(*value as *const _);
                        if let Some(id) = num.to_i64() {
                            // Get window layer to filter out menu bar windows
                            let layer_key = CFString::from_static_string("kCGWindowLayer");
                            let layer: i32 = if let Some(value) = dict.find(layer_key.to_void()) {
                                let num: core_foundation::number::CFNumber =
                                    TCFType::wrap_under_get_rule(*value as *const _);
                                num.to_i32().unwrap_or(0)
                            } else {
                                0
                            };

                            // Get window bounds to verify it's a real window
                            let bounds_key = CFString::from_static_string("kCGWindowBounds");
                            let has_real_bounds =
                                if let Some(value) = dict.find(bounds_key.to_void()) {
                                    let bounds_dict: CFDictionary =
                                        TCFType::wrap_under_get_rule(*value as *const _);
                                    let width_key = CFString::from_static_string("Width");
                                    let height_key = CFString::from_static_string("Height");

                                    if let (Some(w_val), Some(h_val)) = (
                                        bounds_dict.find(width_key.to_void()),
                                        bounds_dict.find(height_key.to_void()),
                                    ) {
                                        let w_num: core_foundation::number::CFNumber =
                                            TCFType::wrap_under_get_rule(*w_val as *const _);
                                        let h_num: core_foundation::number::CFNumber =
                                            TCFType::wrap_under_get_rule(*h_val as *const _);
                                        let width = w_num.to_f64().unwrap_or(0.0);
                                        let height = h_num.to_f64().unwrap_or(0.0);
                                        // Real windows should be at least 100x100 pixels
                                        width >= 100.0 && height >= 100.0
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                };

                            // Only accept windows that are:
                            // 1. At layer 0 (normal windows, not menu bar)
                            // 2. Have real bounds (width and height >= 100)
                            if layer == 0 && has_real_bounds {
                                tracing::debug!("Found valid window: ID {} for app '{}' (layer={}, bounds valid)", id, owner, layer);
                                found_window_id = Some((id as u32, owner.clone()));
                                break;
                            } else {
                                tracing::debug!(
                                    "Skipping window ID {} for '{}': layer={}, has_real_bounds={}",
                                    id,
                                    owner,
                                    layer,
                                    has_real_bounds
                                );
                            }
                        }
                    }
                }
            }

            found_window_id
        };

        let (cg_window_id, matched_owner) = cg_window_id.ok_or_else(|| {
            anyhow::anyhow!("Could not find window for application '{}'. Use list_windows to see available windows.", app_name)
        })?;
        tracing::debug!(
            "Taking screenshot of window ID {} for app '{}'",
            cg_window_id,
            matched_owner
        );

        // Use screencapture with the window ID for now
        // TODO: Implement direct CGWindowListCreateImage approach with proper image saving
        let mut cmd = std::process::Command::new("screencapture");
        cmd.arg("-x"); // No sound
        cmd.arg("-l");
        cmd.arg(cg_window_id.to_string());

        if let Some(region) = region {
            cmd.arg("-R");
            cmd.arg(format!(
                "{},{},{},{}",
                region.x, region.y, region.width, region.height
            ));
        }

        cmd.arg(&final_path);

        let screenshot_result = cmd.output()?;

        if !screenshot_result.status.success() {
            let stderr = String::from_utf8_lossy(&screenshot_result.stderr);
            return Err(anyhow::anyhow!(
                "screencapture failed for window {}: {}",
                cg_window_id,
                stderr
            ));
        }

        Ok(())
    }

    fn move_mouse(&self, x: i32, y: i32) -> Result<()> {
        use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok()
            .context("Failed to create event source")?;

        let event = CGEvent::new_mouse_event(
            source,
            CGEventType::MouseMoved,
            CGPoint::new(x as f64, y as f64),
            CGMouseButton::Left,
        )
        .ok()
        .context("Failed to create mouse event")?;

        event.post(CGEventTapLocation::HID);

        Ok(())
    }

    fn click_at(&self, x: i32, y: i32, _app_name: Option<&str>) -> Result<()> {
        use core_graphics::display::CGDisplay;
        use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        // IMPORTANT: Coordinates passed here are in NSScreen/CGWindowListCopyWindowInfo space
        // (Y=0 at BOTTOM, increases UPWARD)
        // But CGEvent uses a different coordinate system (Y=0 at TOP, increases DOWNWARD)
        // We need to convert: CGEvent.y = screenHeight - NSScreen.y

        let screen_height = CGDisplay::main().pixels_high() as i32;
        let cgevent_x = x;
        let cgevent_y = screen_height - y;

        tracing::debug!(
            "click_at: NSScreen coords ({}, {}) -> CGEvent coords ({}, {}) [screen_height={}]",
            x,
            y,
            cgevent_x,
            cgevent_y,
            screen_height
        );

        let (global_x, global_y) = (cgevent_x, cgevent_y);

        let point = CGPoint::new(global_x as f64, global_y as f64);

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok()
            .context("Failed to create event source")?;

        // Move mouse to position first
        let move_event = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::MouseMoved,
            point,
            CGMouseButton::Left,
        )
        .ok()
        .context("Failed to create mouse move event")?;
        move_event.post(CGEventTapLocation::HID);

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Mouse down
        let mouse_down = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::LeftMouseDown,
            point,
            CGMouseButton::Left,
        )
        .ok()
        .context("Failed to create mouse down event")?;
        mouse_down.post(CGEventTapLocation::HID);

        std::thread::sleep(std::time::Duration::from_millis(50));

        // Mouse up
        let mouse_up =
            CGEvent::new_mouse_event(source, CGEventType::LeftMouseUp, point, CGMouseButton::Left)
                .ok()
                .context("Failed to create mouse up event")?;
        mouse_up.post(CGEventTapLocation::HID);

        Ok(())
    }
}


#[path = "macos_window_matching_test.rs"]
#[cfg(test)]
mod tests;
