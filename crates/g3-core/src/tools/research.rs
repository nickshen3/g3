//! Research tool: spawns a scout agent to perform web-based research.

use anyhow::Result;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tracing::debug;

use crate::ui_writer::UiWriter;
use crate::ToolCall;

use super::executor::ToolContext;

/// Execute the research tool by spawning a scout agent.
///
/// This tool:
/// 1. Spawns `g3 --agent scout` with the query
/// 2. Captures stdout and extracts the last line (file path to report)
/// 3. Reads the report file and returns its contents
pub async fn execute_research<W: UiWriter>(
    tool_call: &ToolCall,
    ctx: &mut ToolContext<'_, W>,
) -> Result<String> {
    let query = tool_call
        .args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing required 'query' parameter"))?;

    debug!("Research tool called with query: {}", query);
    ctx.ui_writer.print_tool_header("research", None);
    ctx.ui_writer.print_tool_arg("query", query);
    
    // Find the g3 executable path
    let g3_path = std::env::current_exe()
        .unwrap_or_else(|_| std::path::PathBuf::from("g3"));

    // Spawn the scout agent
    let mut child = Command::new(&g3_path)
        .arg("--agent")
        .arg("scout")
        .arg("--webdriver")  // Scout needs webdriver for web research
        .arg("--new-session")  // Always start fresh for research
        .arg("--quiet")  // Suppress log file creation
        .arg(query)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to spawn scout agent: {}", e))?;

    // Capture stdout to find the report file path
    let stdout = child.stdout.take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture scout agent stdout"))?;
    
    let mut reader = BufReader::new(stdout).lines();
    let mut last_line = String::new();
    
    // Read all lines, keeping track of the last one
    while let Some(line) = reader.next_line().await? {
        debug!("Scout output: {}", line);
        last_line = line;
    }

    // Wait for the process to complete
    let status = child.wait().await
        .map_err(|e| anyhow::anyhow!("Failed to wait for scout agent: {}", e))?;

    if !status.success() {
        return Ok(format!("❌ Scout agent failed with exit code: {:?}", status.code()));
    }

    // The last line should be the path to the report file
    let report_path = last_line.trim();
    
    if report_path.is_empty() {
        return Ok("❌ Scout agent did not output a report file path".to_string());
    }

    debug!("Report file path: {}", report_path);

    // Expand tilde if present
    let expanded_path = if report_path.starts_with('~') {
        if let Ok(home) = std::env::var("HOME") {
            std::path::PathBuf::from(home).join(&report_path[2..])  // Skip "~/"
        } else {
            std::path::PathBuf::from(report_path)
        }
    } else {
        std::path::PathBuf::from(report_path)
    };

    // Read the report file
    match std::fs::read_to_string(&expanded_path) {
        Ok(content) => {
            debug!("Report loaded: {} chars", content.len());
            Ok(format!("📋 Research Report:\n\n{}", content))
        }
        Err(e) => {
            Ok(format!("❌ Failed to read report file '{}': {}", report_path, e))
        }
    }
}
