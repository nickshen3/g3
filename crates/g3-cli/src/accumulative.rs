//! Accumulative autonomous mode for G3 CLI.

use anyhow::Result;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::PathBuf;
use tracing::error;

use g3_core::project::Project;
use g3_core::Agent;

use crate::autonomous::run_autonomous;
use crate::cli_args::Cli;
use crate::interactive::run_interactive;
use crate::simple_output::SimpleOutput;
use crate::ui_writer_impl::ConsoleUiWriter;
use crate::utils::load_config_with_cli_overrides;

/// Run accumulative autonomous mode - accumulates requirements from user input
/// and runs autonomous mode after each input.
pub async fn run_accumulative_mode(
    workspace_dir: PathBuf,
    cli: Cli,
    combined_content: Option<String>,
) -> Result<()> {
    let output = SimpleOutput::new();

    output.print("");
    output.print("g3 programming agent - autonomous mode");
    output.print("      >> describe what you want, I'll build it iteratively");
    output.print("");
    print!(
        "{}workspace: {}{}\n",
        SetForegroundColor(Color::DarkGrey),
        workspace_dir.display(),
        ResetColor
    );
    output.print("");
    output.print("💡 Each input you provide will be added to requirements");
    output.print("   and I'll automatically work on implementing them. You can");
    output.print("   interrupt at any time (Ctrl+C) to add clarifications or more requirements.");
    output.print("");
    output.print("   Type '/help' for commands, 'exit' or 'quit' to stop, Ctrl+D to finish");
    output.print("");

    // Initialize rustyline editor with history
    let mut rl = DefaultEditor::new()?;
    let history_file = dirs::home_dir().map(|mut path| {
        path.push(".g3_accumulative_history");
        path
    });

    if let Some(ref history_path) = history_file {
        let _ = rl.load_history(history_path);
    }

    // Accumulated requirements stored in memory
    let mut accumulated_requirements = Vec::new();
    let mut turn_number = 0;

    loop {
        output.print(&format!("\n{}", "=".repeat(60)));
        if accumulated_requirements.is_empty() {
            output.print("📝 What would you like me to build? (describe your requirements)");
        } else {
            output.print(&format!(
                "📝 Turn {} - What's next? (add more requirements or refinements)",
                turn_number + 1
            ));
        }
        output.print(&format!("{}", "=".repeat(60)));

        let readline = rl.readline("requirement> ");
        match readline {
            Ok(line) => {
                let input = line.trim().to_string();

                if input.is_empty() {
                    continue;
                }

                if input == "exit" || input == "quit" {
                    output.print("\n👋 Goodbye!");
                    break;
                }

                // Check for slash commands
                if input.starts_with('/') {
                    match handle_command(
                        &input,
                        &output,
                        &accumulated_requirements,
                        &cli,
                        &combined_content,
                        &workspace_dir,
                    )
                    .await?
                    {
                        CommandResult::Continue => continue,
                        CommandResult::Exit => break,
                        CommandResult::Unknown => {
                            output.print(&format!(
                                "❌ Unknown command: {}. Type /help for available commands.",
                                input
                            ));
                            continue;
                        }
                    }
                }

                // Add to history
                rl.add_history_entry(&input)?;

                // Add this requirement to accumulated list
                turn_number += 1;
                accumulated_requirements.push(format!("{}. {}", turn_number, input));

                // Build the complete requirements document
                let requirements_doc = format!(
                    "# Project Requirements\n\n\
                    ## Current Instructions and Requirements:\n\n\
                    {}\n\n\
                    ## Latest Requirement (Turn {}):\n\n\
                    {}",
                    accumulated_requirements.join("\n"),
                    turn_number,
                    input
                );

                output.print("");
                output.print(&format!(
                    "📋 Current instructions and requirements (Turn {}):",
                    turn_number
                ));
                output.print(&format!("   {}", input));
                output.print("");
                output.print("🚀 Starting autonomous implementation...");
                output.print("");

                // Create a project with the accumulated requirements
                let project = Project::new_autonomous_with_requirements(
                    workspace_dir.clone(),
                    requirements_doc.clone(),
                )?;

                // Ensure workspace exists and enter it
                project.ensure_workspace_exists()?;
                project.enter_workspace()?;

                // Load configuration with CLI overrides
                let config = load_config_with_cli_overrides(&cli)?;

                // Create agent for this autonomous run
                let ui_writer = ConsoleUiWriter::new();
                let agent = Agent::new_autonomous_with_readme_and_quiet(
                    config.clone(),
                    ui_writer,
                    combined_content.clone(),
                    cli.quiet,
                )
                .await?;

                // Run autonomous mode with the accumulated requirements
                let autonomous_result = tokio::select! {
                    result = run_autonomous(
                        agent,
                        project,
                        cli.show_prompt,
                        cli.show_code,
                        cli.max_turns,
                        cli.quiet,
                        cli.codebase_fast_start.clone(),
                    ) => result.map(Some),
                    _ = tokio::signal::ctrl_c() => {
                        output.print("\n⚠️  Autonomous run cancelled by user (Ctrl+C)");
                        Ok(None)
                    }
                };

                match autonomous_result {
                    Ok(Some(_returned_agent)) => {
                        output.print("");
                        output.print("✅ Autonomous run completed");
                    }
                    Ok(None) => {
                        output.print("   (session continuation not saved due to cancellation)");
                    }
                    Err(e) => {
                        output.print("");
                        output.print(&format!("❌ Autonomous run failed: {}", e));
                        output.print("   You can provide more requirements to continue.");
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                output.print("\n👋 Interrupted. Goodbye!");
                break;
            }
            Err(ReadlineError::Eof) => {
                output.print("\n👋 Goodbye!");
                break;
            }
            Err(err) => {
                error!("Error: {:?}", err);
                break;
            }
        }
    }

    // Save history before exiting
    if let Some(ref history_path) = history_file {
        let _ = rl.save_history(history_path);
    }

    Ok(())
}

enum CommandResult {
    Continue,
    Exit,
    Unknown,
}

async fn handle_command(
    input: &str,
    output: &SimpleOutput,
    accumulated_requirements: &[String],
    cli: &Cli,
    combined_content: &Option<String>,
    workspace_dir: &PathBuf,
) -> Result<CommandResult> {
    match input {
        "/help" => {
            output.print("");
            output.print("📖 Available Commands:");
            output.print("  /requirements - Show all accumulated requirements");
            output.print("  /chat         - Switch to interactive chat mode");
            output.print("  /help         - Show this help message");
            output.print("  exit/quit     - Exit the session");
            output.print("");
            Ok(CommandResult::Continue)
        }
        "/requirements" => {
            output.print("");
            if accumulated_requirements.is_empty() {
                output.print("📋 No requirements accumulated yet");
            } else {
                output.print("📋 Accumulated Requirements:");
                output.print("");
                for req in accumulated_requirements {
                    output.print(&format!("   {}", req));
                }
            }
            output.print("");
            Ok(CommandResult::Continue)
        }
        "/chat" => {
            output.print("");
            output.print("🔄 Switching to interactive chat mode...");
            output.print("");

            // Build context message with accumulated requirements
            let requirements_context = if accumulated_requirements.is_empty() {
                None
            } else {
                Some(format!(
                    "📋 Context from Accumulative Mode:\n\n\
                    We were working on these requirements. There may be unstaged or in-progress changes or recent changes to this branch. This is for your information.\n\n\
                    Requirements:\n{}\n",
                    accumulated_requirements.join("\n")
                ))
            };

            // Combine with existing content (README/AGENTS.md)
            let chat_combined_content = match (requirements_context, combined_content.clone()) {
                (Some(req_ctx), Some(existing)) => Some(format!("{}\n\n{}", req_ctx, existing)),
                (Some(req_ctx), None) => Some(req_ctx),
                (None, existing) => existing,
            };

            // Load configuration
            let config = load_config_with_cli_overrides(cli)?;

            // Create agent for interactive mode with requirements context
            let ui_writer = ConsoleUiWriter::new();
            let agent = Agent::new_with_readme_and_quiet(
                config,
                ui_writer,
                chat_combined_content.clone(),
                cli.quiet,
            )
            .await?;

            // Run interactive mode
            run_interactive(
                agent,
                cli.show_prompt,
                cli.show_code,
                chat_combined_content,
                workspace_dir,
            )
            .await?;

            // After returning from interactive mode, exit
            output.print("\n👋 Goodbye!");
            Ok(CommandResult::Exit)
        }
        _ => Ok(CommandResult::Unknown),
    }
}
