//! Tool dispatch module - routes tool calls to their implementations.
//!
//! This module provides a clean dispatch mechanism that routes tool calls
//! to the appropriate handler in the `tools/` module.

use anyhow::Result;
use tracing::{debug, warn};

use crate::tools::executor::ToolContext;
use crate::tools::{file_ops, macax, misc, shell, todo, vision, webdriver};
use crate::ui_writer::UiWriter;
use crate::ToolCall;

/// Dispatch a tool call to the appropriate handler.
///
/// This function routes tool calls to their implementations in the `tools/` module,
/// providing a single point of dispatch for all tool execution.
pub async fn dispatch_tool<W: UiWriter>(
    tool_call: &ToolCall,
    ctx: &mut ToolContext<'_, W>,
) -> Result<String> {
    debug!("Dispatching tool: {}", tool_call.tool);

    match tool_call.tool.as_str() {
        // Shell tools
        "shell" => shell::execute_shell(tool_call, ctx).await,
        "background_process" => shell::execute_background_process(tool_call, ctx).await,

        // File operations
        "read_file" => file_ops::execute_read_file(tool_call, ctx).await,
        "read_image" => file_ops::execute_read_image(tool_call, ctx).await,
        "write_file" => file_ops::execute_write_file(tool_call, ctx).await,
        "str_replace" => file_ops::execute_str_replace(tool_call, ctx).await,

        // TODO management
        "todo_read" => todo::execute_todo_read(tool_call, ctx).await,
        "todo_write" => todo::execute_todo_write(tool_call, ctx).await,

        // Miscellaneous tools
        "final_output" => {
            let result = misc::execute_final_output(tool_call, ctx).await?;
            // Note: Session continuation saving is handled by the caller
            Ok(result)
        }
        "take_screenshot" => misc::execute_take_screenshot(tool_call, ctx).await,
        "extract_text" => misc::execute_extract_text(tool_call, ctx).await,
        "code_coverage" => misc::execute_code_coverage(tool_call, ctx).await,
        "code_search" => misc::execute_code_search(tool_call, ctx).await,

        // WebDriver tools
        "webdriver_start" => webdriver::execute_webdriver_start(tool_call, ctx).await,
        "webdriver_navigate" => webdriver::execute_webdriver_navigate(tool_call, ctx).await,
        "webdriver_get_url" => webdriver::execute_webdriver_get_url(tool_call, ctx).await,
        "webdriver_get_title" => webdriver::execute_webdriver_get_title(tool_call, ctx).await,
        "webdriver_find_element" => webdriver::execute_webdriver_find_element(tool_call, ctx).await,
        "webdriver_find_elements" => webdriver::execute_webdriver_find_elements(tool_call, ctx).await,
        "webdriver_click" => webdriver::execute_webdriver_click(tool_call, ctx).await,
        "webdriver_send_keys" => webdriver::execute_webdriver_send_keys(tool_call, ctx).await,
        "webdriver_execute_script" => webdriver::execute_webdriver_execute_script(tool_call, ctx).await,
        "webdriver_get_page_source" => webdriver::execute_webdriver_get_page_source(tool_call, ctx).await,
        "webdriver_screenshot" => webdriver::execute_webdriver_screenshot(tool_call, ctx).await,
        "webdriver_back" => webdriver::execute_webdriver_back(tool_call, ctx).await,
        "webdriver_forward" => webdriver::execute_webdriver_forward(tool_call, ctx).await,
        "webdriver_refresh" => webdriver::execute_webdriver_refresh(tool_call, ctx).await,
        "webdriver_quit" => webdriver::execute_webdriver_quit(tool_call, ctx).await,

        // macOS Accessibility tools
        "macax_list_apps" => macax::execute_macax_list_apps(tool_call, ctx).await,
        "macax_get_frontmost_app" => macax::execute_macax_get_frontmost_app(tool_call, ctx).await,
        "macax_activate_app" => macax::execute_macax_activate_app(tool_call, ctx).await,
        "macax_press_key" => macax::execute_macax_press_key(tool_call, ctx).await,
        "macax_type_text" => macax::execute_macax_type_text(tool_call, ctx).await,

        // Vision tools
        "vision_find_text" => vision::execute_vision_find_text(tool_call, ctx).await,
        "vision_click_text" => vision::execute_vision_click_text(tool_call, ctx).await,
        "vision_click_near_text" => vision::execute_vision_click_near_text(tool_call, ctx).await,
        "extract_text_with_boxes" => vision::execute_extract_text_with_boxes(tool_call, ctx).await,

        // Unknown tool
        _ => {
            warn!("Unknown tool: {}", tool_call.tool);
            Ok(format!("❓ Unknown tool: {}", tool_call.tool))
        }
    }
}
