//! Interactive mode for G3 CLI.

use anyhow::Result;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use rustyline::error::ReadlineError;
use rustyline::{Config, Editor};
use crate::completion::G3Helper;
use std::path::Path;
use tracing::{debug, error};

use g3_core::ui_writer::UiWriter;
use g3_core::Agent;

use crate::display::{LoadedContent, print_loaded_status, print_project_heading, print_workspace_path};
use crate::g3_status::{G3Status, Status};
use crate::project_files::extract_readme_heading;
use crate::simple_output::SimpleOutput;
use crate::task_execution::execute_task_with_retry;
use crate::utils::display_context_progress;

/// Run interactive mode with console output.
/// If `agent_name` is Some, we're in agent+chat mode: skip session resume/verbose welcome,
/// and use the agent name as the prompt (e.g., "butler>").
pub async fn run_interactive<W: UiWriter>(
    mut agent: Agent<W>,
    show_prompt: bool,
    show_code: bool,
    combined_content: Option<String>,
    workspace_path: &Path,
    new_session: bool,
    agent_name: Option<&str>,
) -> Result<()> {
    let output = SimpleOutput::new();
    let from_agent_mode = agent_name.is_some();

    // Check for session continuation (skip if --new-session was passed or coming from agent mode)
    // Agent mode with --chat should start fresh without prompting
    if !new_session && !from_agent_mode {
      if let Ok(Some(continuation)) = g3_core::load_continuation() {
        // Print session info and prompt on same line (no newline)
        print!(
            "\n >> session in progress: {}{}{} | {:.1}% used | resume? [y/n] ",
            SetForegroundColor(Color::Cyan),
            &continuation.session_id[..continuation.session_id.len().min(20)],
            ResetColor,
            continuation.context_percentage
        );
        use std::io::Write;
        std::io::stdout().flush()?;

        // Read user input
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input.is_empty() || input == "y" || input == "yes" {
            // Resume the session
            match agent.restore_from_continuation(&continuation) {
                Ok(true) => {
                    G3Status::resuming(&continuation.session_id, Status::Done);
                }
                Ok(false) => {
                    G3Status::resuming_summary(&continuation.session_id);
                }
                Err(e) => {
                    G3Status::resuming(&continuation.session_id, Status::Error(e.to_string()));
                    // Clear the invalid continuation
                    let _ = g3_core::clear_continuation();
                }
            }
        } else {
            // User declined, clear the continuation
            G3Status::info_inline("starting fresh");
            let _ = g3_core::clear_continuation();
        }
      }
    }

    // Skip verbose welcome when coming from agent mode (it already printed context info)
    if !from_agent_mode {
        output.print("");
        output.print("g3 programming agent");
        output.print("      >> what shall we build today?");
        output.print("");

        // Display provider and model information
        match agent.get_provider_info() {
            Ok((provider, model)) => {
                print!(
                    "🔧 {}{}{} | {}{}{}\n",
                    SetForegroundColor(Color::Cyan),
                    provider,
                    ResetColor,
                    SetForegroundColor(Color::Yellow),
                    model,
                    ResetColor
                );
            }
            Err(e) => {
                error!("Failed to get provider info: {}", e);
            }
        }

           // Display message if AGENTS.md or README was loaded
        if let Some(ref content) = combined_content {
            let loaded = LoadedContent::from_combined_content(content);

            // Extract project name if README is loaded
            if loaded.has_readme {
                if let Some(name) = extract_readme_heading(content) {
                    print_project_heading(&name);
                }
            }

            print_loaded_status(&loaded);
        }

        // Display workspace path
        print_workspace_path(workspace_path);
        output.print("");
    }

    // Initialize rustyline editor with history
    let config = Config::builder()
        .completion_type(rustyline::CompletionType::List)
        .build();
    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(G3Helper::new()));

    // Try to load history from a file in the user's home directory
    let history_file = dirs::home_dir().map(|mut path| {
        path.push(".g3_history");
        path
    });

    if let Some(ref history_path) = history_file {
        let _ = rl.load_history(history_path);
    }

    // Track multiline input
    let mut multiline_buffer = String::new();
    let mut in_multiline = false;

    loop {
        // Display context window progress bar before each prompt
        display_context_progress(&agent, &output);

        // Adjust prompt based on whether we're in multi-line mode
        let prompt = if in_multiline {
            "... > ".to_string()
        } else if let Some(name) = agent_name {
            format!("{}> ", name)
        } else {
            "g3> ".to_string()
        };

        let readline = rl.readline(&prompt);
        match readline {
            Ok(line) => {
                let trimmed = line.trim_end();

                // Check if line ends with backslash for continuation
                if let Some(without_backslash) = trimmed.strip_suffix('\\') {
                    // Remove the backslash and add to buffer
                    multiline_buffer.push_str(without_backslash);
                    multiline_buffer.push('\n');
                    in_multiline = true;
                    continue;
                }

                // If we're in multiline mode and no backslash, this is the final line
                if in_multiline {
                    multiline_buffer.push_str(&line);
                    in_multiline = false;
                    // Process the complete multiline input
                    let input = multiline_buffer.trim().to_string();
                    multiline_buffer.clear();

                    if input.is_empty() {
                        continue;
                    }

                    // Add complete multiline to history
                    rl.add_history_entry(&input)?;

                    if input == "exit" || input == "quit" {
                        break;
                    }

                    // Process the multiline input
                    execute_task_with_retry(
                        &mut agent,
                        &input,
                        show_prompt,
                        show_code,
                        &output,
                    )
                    .await;

                    // Send auto-memory reminder if enabled and tools were called
                    // Skip per-turn reminders when from_agent_mode - we'll send once on exit
                    if !from_agent_mode {
                      if let Err(e) = agent.send_auto_memory_reminder().await {
                        debug!("Auto-memory reminder failed: {}", e);
                      }
                    }
                } else {
                    // Single line input
                    let input = line.trim().to_string();

                    if input.is_empty() {
                        continue;
                    }

                    if input == "exit" || input == "quit" {
                        break;
                    }

                    // Add to history
                    rl.add_history_entry(&input)?;

                    // Check for control commands
                    if input.starts_with('/') {
                        if handle_command(&input, &mut agent, &output, &mut rl, show_prompt, show_code).await? {
                            continue;
                        }
                    }

                    // Process the single line input
                    execute_task_with_retry(
                        &mut agent,
                        &input,
                        show_prompt,
                        show_code,
                        &output,
                    )
                    .await;

                    // Send auto-memory reminder if enabled and tools were called
                    // Skip per-turn reminders when from_agent_mode - we'll send once on exit
                    if !from_agent_mode {
                      if let Err(e) = agent.send_auto_memory_reminder().await {
                        debug!("Auto-memory reminder failed: {}", e);
                      }
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C pressed
                if in_multiline {
                    // Cancel multiline input
                    output.print("Multi-line input cancelled");
                    multiline_buffer.clear();
                    in_multiline = false;
                } else {
                    output.print("CTRL-C");
                }
                continue;
            }
            Err(ReadlineError::Eof) => {
                output.print("CTRL-D");
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

    // Save session continuation for resume capability
    agent.save_session_continuation(None);

    // Send auto-memory reminder once on exit when in agent+chat mode
    // (Per-turn reminders were skipped to avoid being too onerous)
    if from_agent_mode {
        if let Err(e) = agent.send_auto_memory_reminder().await {
            debug!("Auto-memory reminder on exit failed: {}", e);
        }
    }

    output.print("👋 Goodbye!");
    Ok(())
}

/// Handle a control command. Returns true if the command was handled and the loop should continue.
async fn handle_command<W: UiWriter>(
    input: &str,
    agent: &mut Agent<W>,
    output: &SimpleOutput,
    rl: &mut Editor<G3Helper, rustyline::history::DefaultHistory>,
    show_prompt: bool,
    show_code: bool,
) -> Result<bool> {
    match input {
        "/help" => {
            output.print("");
            output.print("📖 Control Commands:");
            output.print("  /compact   - Trigger compaction (compacts conversation history)");
            output.print("  /thinnify  - Trigger context thinning (replaces large tool results with file references)");
            output.print("  /skinnify  - Trigger full context thinning (like /thinnify but for entire context, not just first third)");
            output.print("  /clear     - Clear session and start fresh (discards continuation artifacts)");
            output.print("  /fragments - List dehydrated context fragments (ACD)");
            output.print("  /rehydrate - Restore a dehydrated fragment by ID");
            output.print("  /resume    - List and switch to a previous session");
            output.print("  /dump      - Dump entire context window to file for debugging");
            output.print("  /readme    - Reload README.md and AGENTS.md from disk");
            output.print("  /stats     - Show detailed context and performance statistics");
            output.print("  /run <file> - Read file and execute as prompt");
            output.print("  /help      - Show this help message");
            output.print("  exit/quit  - Exit the interactive session");
            output.print("");
            Ok(true)
        }
        "/compact" => {
            output.print_g3_progress("compacting session");
            match agent.force_compact().await {
                Ok(true) => {
                    output.print_g3_status("compacting session", "done");
                }
                Ok(false) => {
                    output.print_g3_status("compacting session", "failed");
                }
                Err(e) => {
                    output.print_g3_status("compacting session", &format!("error: {}", e));
                }
            }
            Ok(true)
        }
        "/thinnify" => {
            let result = agent.force_thin();
            G3Status::thin_result(&result);
            Ok(true)
        }
        "/skinnify" => {
            let result = agent.force_thin_all();
            G3Status::thin_result(&result);
            Ok(true)
        }
        "/fragments" => {
            if let Some(session_id) = agent.get_session_id() {
                match g3_core::acd::list_fragments(session_id) {
                    Ok(fragments) => {
                        if fragments.is_empty() {
                            output.print("No dehydrated fragments found for this session.");
                        } else {
                            output.print(&format!(
                                "📦 {} dehydrated fragment(s):\n",
                                fragments.len()
                            ));
                            for fragment in &fragments {
                                output.print(&fragment.generate_stub());
                                output.print("");
                            }
                        }
                    }
                    Err(e) => {
                        output.print(&format!("❌ Error listing fragments: {}", e));
                    }
                }
            } else {
                output.print("No active session - fragments are session-scoped.");
            }
            Ok(true)
        }
        cmd if cmd.starts_with("/rehydrate") => {
            let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
            if parts.len() < 2 || parts[1].trim().is_empty() {
                output.print("Usage: /rehydrate <fragment_id>");
                output.print("Use /fragments to list available fragment IDs.");
            } else {
                let fragment_id = parts[1].trim();
                if let Some(session_id) = agent.get_session_id() {
                    match g3_core::acd::Fragment::load(session_id, fragment_id) {
                        Ok(fragment) => {
                            output.print(&format!(
                                "✅ Fragment '{}' loaded ({} messages, ~{} tokens)",
                                fragment_id, fragment.message_count, fragment.estimated_tokens
                            ));
                            output.print("");
                            output.print(&fragment.generate_stub());
                        }
                        Err(e) => {
                            output.print(&format!(
                                "❌ Failed to load fragment '{}': {}",
                                fragment_id, e
                            ));
                        }
                    }
                } else {
                    output.print("No active session - fragments are session-scoped.");
                }
            }
            Ok(true)
        }
        cmd if cmd.starts_with("/run") => {
            let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
            if parts.len() < 2 || parts[1].trim().is_empty() {
                output.print("Usage: /run <file-path>");
                output.print("Reads the file and executes its content as a prompt.");
            } else {
                let file_path = parts[1].trim();
                // Expand tilde
                let expanded_path = if file_path.starts_with("~/") {
                    if let Some(home) = dirs::home_dir() {
                        home.join(&file_path[2..])
                    } else {
                        std::path::PathBuf::from(file_path)
                    }
                } else {
                    std::path::PathBuf::from(file_path)
                };
                match std::fs::read_to_string(&expanded_path) {
                    Ok(content) => {
                        let prompt = content.trim();
                        if prompt.is_empty() {
                            output.print("❌ File is empty.");
                        } else {
                            G3Status::progress(&format!("loading {}", file_path));
                            G3Status::done();
                            execute_task_with_retry(agent, prompt, show_prompt, show_code, output).await;
                        }
                    }
                    Err(e) => {
                        output.print(&format!("❌ Failed to read file '{}': {}", file_path, e));
                    }
                }
            }
            Ok(true)
        }
        "/dump" => {
            // Dump entire context window to a file for debugging
            let dump_dir = std::path::Path::new("tmp");
            if !dump_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(dump_dir) {
                    output.print(&format!("❌ Failed to create tmp directory: {}", e));
                    return Ok(true);
                }
            }

            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
            let dump_path = dump_dir.join(format!("context_dump_{}.txt", timestamp));

            let context = agent.get_context_window();
            let mut dump_content = String::new();
            dump_content.push_str("# Context Window Dump\n");
            dump_content.push_str(&format!("# Timestamp: {}\n", chrono::Utc::now()));
            dump_content.push_str(&format!(
                "# Messages: {}\n",
                context.conversation_history.len()
            ));
            dump_content.push_str(&format!(
                "# Used tokens: {} / {} ({:.1}%)\n\n",
                context.used_tokens,
                context.total_tokens,
                context.percentage_used()
            ));

            for (i, msg) in context.conversation_history.iter().enumerate() {
                dump_content.push_str(&format!("=== Message {} ===\n", i));
                dump_content.push_str(&format!("Role: {:?}\n", msg.role));
                dump_content.push_str(&format!("Kind: {:?}\n", msg.kind));
                dump_content.push_str(&format!("Content ({} chars):\n", msg.content.len()));
                dump_content.push_str(&msg.content);
                dump_content.push_str("\n\n");
            }

            match std::fs::write(&dump_path, &dump_content) {
                Ok(_) => output.print(&format!("📄 Context dumped to: {}", dump_path.display())),
                Err(e) => output.print(&format!("❌ Failed to write dump: {}", e)),
            }
            Ok(true)
        }
        "/clear" => {
            output.print("🧹 Clearing session...");
            agent.clear_session();
            output.print("✅ Session cleared. Starting fresh.");
            Ok(true)
        }
        "/readme" => {
            output.print("📚 Reloading README.md and AGENTS.md...");
            match agent.reload_readme() {
                Ok(true) => {
                    output.print("✅ README content reloaded successfully")
                }
                Ok(false) => {
                    output.print("⚠️ No README was loaded at startup, cannot reload")
                }
                Err(e) => output.print(&format!("❌ Error reloading README: {}", e)),
            }
            Ok(true)
        }
        "/stats" => {
            let stats = agent.get_stats();
            output.print(&stats);
            Ok(true)
        }
        "/resume" => {
            output.print("📋 Scanning for available sessions...");

            match g3_core::list_sessions_for_directory() {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        output.print("No sessions found for this directory.");
                        return Ok(true);
                    }

                    // Get current session ID to mark it
                    let current_session_id = agent.get_session_id().map(|s| s.to_string());

                    output.print("");
                    output.print("Available sessions:");
                    for (i, session) in sessions.iter().enumerate() {
                        let time_str = g3_core::format_session_time(&session.created_at);
                        let context_str = format!("{:.0}%", session.context_percentage);
                        let current_marker =
                            if current_session_id.as_deref() == Some(&session.session_id) {
                                " (current)"
                            } else {
                                ""
                            };
                        let todo_marker = if session.has_incomplete_todos() {
                            " 📝"
                        } else {
                            ""
                        };

                        // Use description if available, otherwise fall back to session ID
                        let display_name = match &session.description {
                            Some(desc) => format!("'{}'", desc),
                            None => {
                                if session.session_id.len() > 40 {
                                    format!("{}...", &session.session_id[..40])
                                } else {
                                    session.session_id.clone()
                                }
                            }
                        };
                        output.print(&format!(
                            "  {}. [{}] {} ({}){}{}\n",
                            i + 1,
                            time_str,
                            display_name,
                            context_str,
                            todo_marker,
                            current_marker
                        ));
                    }

                    output.print_inline("\nSession number to resume (Enter to cancel): ");
                    // Read user selection
                    if let Ok(selection) = rl.readline("") {
                        let selection = selection.trim();
                        if selection.is_empty() {
                            output.print("Cancelled.");
                        } else if let Ok(num) = selection.parse::<usize>() {
                            if num >= 1 && num <= sessions.len() {
                                let selected = &sessions[num - 1];
                                match agent.switch_to_session(selected) {
                                    Ok(true) => {
                                        G3Status::resuming(&selected.session_id, Status::Done);
                                    }
                                    Ok(false) => {
                                        G3Status::resuming_summary(&selected.session_id);
                                    }
                                    Err(e) => {
                                        G3Status::resuming(&selected.session_id, Status::Error(e.to_string()));
                                    }
                                }
                            } else {
                                output.print("Invalid selection.");
                            }
                        } else {
                            output.print("Invalid input. Please enter a number.");
                        }
                    }
                }
                Err(e) => output.print(&format!("❌ Error listing sessions: {}", e)),
            }
            Ok(true)
        }
        _ => {
            output.print(&format!(
                "❌ Unknown command: {}. Type /help for available commands.",
                input
            ));
            Ok(true)
        }
    }
}
