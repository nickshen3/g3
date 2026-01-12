//! Utility functions for G3 CLI.

use anyhow::Result;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use g3_config::Config;
use g3_core::ui_writer::UiWriter;
use g3_core::Agent;
use std::path::PathBuf;

use crate::cli_args::Cli;
use crate::simple_output::SimpleOutput;

/// Display context window progress bar.
pub fn display_context_progress<W: UiWriter>(agent: &Agent<W>, _output: &SimpleOutput) {
    let context = agent.get_context_window();
    let percentage = context.percentage_used();

    // Create 10 dots representing context fullness
    let total_dots: usize = 10;
    let filled_dots = ((percentage / 100.0) * total_dots as f32).round() as usize;
    let empty_dots = total_dots.saturating_sub(filled_dots);

    let filled_str = "●".repeat(filled_dots);
    let empty_str = "○".repeat(empty_dots);

    // Determine color based on percentage
    let color = if percentage < 40.0 {
        Color::Green
    } else if percentage < 60.0 {
        Color::Yellow
    } else if percentage < 80.0 {
        Color::Rgb {
            r: 255,
            g: 165,
            b: 0,
        } // Orange
    } else {
        Color::Red
    };

    // Format tokens as compact strings (e.g., "38.5k" instead of "38531")
    let format_tokens = |tokens: u32| -> String {
        if tokens >= 1_000_000 {
            format!("{:.1}m", tokens as f64 / 1_000_000.0)
        } else if tokens >= 1_000 {
            let k = tokens as f64 / 1000.0;
            if k >= 100.0 {
                format!("{:.0}k", k)
            } else {
                format!("{:.1}k", k)
            }
        } else {
            format!("{}", tokens)
        }
    };

    // Print with colored dots (using print! directly to handle color codes)
    print!(
        "{}{}{}{} {}/{} ◉ | {:.0}%\n",
        SetForegroundColor(color),
        filled_str,
        empty_str,
        ResetColor,
        format_tokens(context.used_tokens),
        format_tokens(context.total_tokens),
        percentage
    );
}

/// Set up the workspace directory for autonomous mode.
/// Uses G3_WORKSPACE environment variable or defaults to ~/tmp/workspace.
pub fn setup_workspace_directory() -> Result<PathBuf> {
    let workspace_dir = if let Ok(env_workspace) = std::env::var("G3_WORKSPACE") {
        PathBuf::from(env_workspace)
    } else {
        // Default to ~/tmp/workspace
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        home_dir.join("tmp").join("workspace")
    };

    // Create the directory if it doesn't exist
    if !workspace_dir.exists() {
        std::fs::create_dir_all(&workspace_dir)?;
        let output = SimpleOutput::new();
        output.print(&format!(
            "📁 Created workspace directory: {}",
            workspace_dir.display()
        ));
    }

    Ok(workspace_dir)
}

/// Load configuration with CLI argument overrides applied.
///
/// This is the canonical function for loading config with CLI overrides.
/// All CLI entry points should use this to ensure consistent behavior.
pub fn load_config_with_cli_overrides(cli: &Cli) -> Result<Config> {
    let mut config = Config::load_with_overrides(
        cli.config.as_deref(),
        cli.provider.clone(),
        cli.model.clone(),
    )?;

    // Apply webdriver flag override
    if cli.webdriver {
        config.webdriver.enabled = true;
    }

    // Apply chrome-headless flag override
    if cli.chrome_headless {
        config.webdriver.enabled = true;
        config.webdriver.browser = g3_config::WebDriverBrowser::ChromeHeadless;

        // Run Chrome diagnostics on first use
        let report =
            g3_computer_control::run_chrome_diagnostics(config.webdriver.chrome_binary.as_deref());
        println!("{}", report.format_report());
    }

    // Apply safari flag override
    if cli.safari {
        config.webdriver.enabled = true;
        config.webdriver.browser = g3_config::WebDriverBrowser::Safari;
    }

    // Apply no-auto-compact flag override
    if cli.manual_compact {
        config.agent.auto_compact = false;
    }

    // Validate provider if specified
    if let Some(ref provider) = cli.provider {
        let valid_providers = ["anthropic", "databricks", "embedded", "openai"];
        if !valid_providers.contains(&provider.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid provider '{}'. Valid options: {:?}",
                provider,
                valid_providers
            ));
        }
    }

    Ok(config)
}
