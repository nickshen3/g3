//! Interactive mode for G3 CLI.

use anyhow::Result;
use crossterm::style::{Color, ResetColor, SetForegroundColor};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::path::Path;
use tracing::{debug, error};

use g3_core::ui_writer::UiWriter;
use g3_core::Agent;

use crate::machine_ui_writer::MachineUiWriter;
use crate::project_files::extract_readme_heading;
use crate::simple_output::SimpleOutput;
use crate::task_execution::{execute_task_with_retry, OutputMode};
use crate::utils::display_context_progress;

/// Run interactive mode with console output.
pub async fn run_interactive<W: UiWriter>(
    mut agent: Agent<W>,
    show_prompt: bool,
    show_code: bool,
    combined_content: Option<String>,
    workspace_path: &Path,
) -> Result<()> {
    let output = SimpleOutput::new();

    // Check for session continuation
    if let Ok(Some(continuation)) = g3_core::load_continuation() {
        output.print("");
        output.print("🔄 Previous session detected!");
        output.print(&format!(
            "   Session: {}",
            &continuation.session_id[..continuation.session_id.len().min(20)]
        ));
        output.print(&format!(
            "   Context: {:.1}% used",
            continuation.context_percentage
        ));
        if let Some(ref summary) = continuation.summary {
            let preview: String = summary.chars().take(80).collect();
            output.print(&format!("   Last output: {}...", preview));
        }
        output.print("");
        output.print("Resume this session? [Y/n] ");

        // Read user input
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input.is_empty() || input == "y" || input == "yes" {
            // Resume the session
            match agent.restore_from_continuation(&continuation) {
                Ok(true) => {
                    output.print("✅ Full context restored from previous session");
                }
                Ok(false) => {
                    output.print("✅ Session resumed with summary (context was > 80%)");
                }
                Err(e) => {
                    output.print(&format!("⚠️ Could not restore session: {}", e));
                    output.print("Starting fresh session instead.");
                    // Clear the invalid continuation
                    let _ = g3_core::clear_continuation();
                }
            }
        } else {
            // User declined, clear the continuation
            output.print("🧹 Starting fresh session...");
            let _ = g3_core::clear_continuation();
        }
        output.print("");
    }

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
        // Check what was loaded
        let has_agents = content.contains("Agent Configuration");
        let has_readme = content.contains("Project README");
        let has_memory = content.contains("Project Memory");

        if has_agents {
            print!(
                "{}🤖 AGENTS.md configuration loaded{}\n",
                SetForegroundColor(Color::DarkGrey),
                ResetColor
            );
        }

        if has_readme {
            // Extract the first heading or title from the README
            let readme_snippet = extract_readme_heading(content)
                .unwrap_or_else(|| "Project documentation loaded".to_string());

            print!(
                "{}📚 detected: {}{}\n",
                SetForegroundColor(Color::DarkGrey),
                readme_snippet,
                ResetColor
            );
        }

        if has_memory {
            print!(
                "{}🧠 Project memory loaded{}\n",
                SetForegroundColor(Color::DarkGrey),
                ResetColor
            );
        }
    }

    // Display workspace path
    print!(
        "{}workspace: {}{}\n",
        SetForegroundColor(Color::DarkGrey),
        workspace_path.display(),
        ResetColor
    );
    output.print("");

    // Initialize rustyline editor with history
    let mut rl = DefaultEditor::new()?;

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
        let prompt = if in_multiline { "... > " } else { "g3> " };

        let readline = rl.readline(prompt);
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
                        OutputMode::Console(&output),
                    )
                    .await;

                    // Send auto-memory reminder if enabled and tools were called
                    if let Err(e) = agent.send_auto_memory_reminder().await {
                        debug!("Auto-memory reminder failed: {}", e);
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
                        if handle_command(&input, &mut agent, &output, &mut rl).await? {
                            continue;
                        }
                    }

                    // Process the single line input
                    execute_task_with_retry(
                        &mut agent,
                        &input,
                        show_prompt,
                        show_code,
                        OutputMode::Console(&output),
                    )
                    .await;

                    // Send auto-memory reminder if enabled and tools were called
                    if let Err(e) = agent.send_auto_memory_reminder().await {
                        debug!("Auto-memory reminder failed: {}", e);
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

    output.print("👋 Goodbye!");
    Ok(())
}

/// Run interactive mode with machine-friendly output.
pub async fn run_interactive_machine(
    mut agent: Agent<MachineUiWriter>,
    show_prompt: bool,
    show_code: bool,
) -> Result<()> {
    println!("INTERACTIVE_MODE_STARTED");

    // Display provider and model information
    match agent.get_provider_info() {
        Ok((provider, model)) => {
            println!("PROVIDER: {}", provider);
            println!("MODEL: {}", model);
        }
        Err(e) => {
            println!("ERROR: Failed to get provider info: {}", e);
        }
    }

    // Initialize rustyline editor with history
    let mut rl = DefaultEditor::new()?;

    // Try to load history from a file in the user's home directory
    let history_file = dirs::home_dir().map(|mut path| {
        path.push(".g3_history");
        path
    });

    if let Some(ref history_path) = history_file {
        let _ = rl.load_history(history_path);
    }

    loop {
        let readline = rl.readline("");
        match readline {
            Ok(line) => {
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
                    if handle_machine_command(&input, &mut agent).await? {
                        continue;
                    }
                }

                // Execute task
                println!("TASK_START");
                execute_task_with_retry(&mut agent, &input, show_prompt, show_code, OutputMode::Machine)
                    .await;

                // Send auto-memory reminder if enabled and tools were called
                if let Err(e) = agent.send_auto_memory_reminder().await {
                    debug!("Auto-memory reminder failed: {}", e);
                }

                println!("TASK_END");
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                println!("ERROR: {:?}", err);
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

    println!("INTERACTIVE_MODE_ENDED");
    Ok(())
}

/// Handle a control command. Returns true if the command was handled and the loop should continue.
async fn handle_command<W: UiWriter>(
    input: &str,
    agent: &mut Agent<W>,
    output: &SimpleOutput,
    rl: &mut DefaultEditor,
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
            output.print("  /help      - Show this help message");
            output.print("  exit/quit  - Exit the interactive session");
            output.print("");
            Ok(true)
        }
        "/compact" => {
            output.print("🗜️ Triggering manual compaction...");
            match agent.force_compact().await {
                Ok(true) => {
                    output.print("✅ Compaction completed successfully");
                }
                Ok(false) => {
                    output.print("⚠️ Compaction failed");
                }
                Err(e) => {
                    output.print(&format!("❌ Error during compaction: {}", e));
                }
            }
            Ok(true)
        }
        "/thinnify" => {
            let summary = agent.force_thin();
            println!("{}", summary);
            Ok(true)
        }
        "/skinnify" => {
            let summary = agent.force_thin_all();
            println!("{}", summary);
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
                    output.print("");
                    output.print("Enter session number to resume (or press Enter to cancel):");

                    // Read user selection
                    if let Ok(selection) = rl.readline("> ") {
                        let selection = selection.trim();
                        if selection.is_empty() {
                            output.print("Resume cancelled.");
                        } else if let Ok(num) = selection.parse::<usize>() {
                            if num >= 1 && num <= sessions.len() {
                                let selected = &sessions[num - 1];
                                output.print(&format!(
                                    "🔄 Switching to session: {}",
                                    selected.session_id
                                ));
                                match agent.switch_to_session(selected) {
                                    Ok(true) => {
                                        output.print("✅ Full context restored from session.")
                                    }
                                    Ok(false) => {
                                        output.print("✅ Session restored from summary.")
                                    }
                                    Err(e) => {
                                        output.print(&format!("❌ Error restoring session: {}", e))
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

/// Handle a control command in machine mode. Returns true if the command was handled.
async fn handle_machine_command(
    input: &str,
    agent: &mut Agent<MachineUiWriter>,
) -> Result<bool> {
    match input {
        "/compact" => {
            println!("COMMAND: compact");
            match agent.force_compact().await {
                Ok(true) => println!("RESULT: Compaction completed"),
                Ok(false) => println!("RESULT: Compaction failed"),
                Err(e) => println!("ERROR: {}", e),
            }
            Ok(true)
        }
        "/thinnify" => {
            println!("COMMAND: thinnify");
            let summary = agent.force_thin();
            println!("{}", summary);
            Ok(true)
        }
        "/skinnify" => {
            println!("COMMAND: skinnify");
            let summary = agent.force_thin_all();
            println!("{}", summary);
            Ok(true)
        }
        "/fragments" => {
            println!("COMMAND: fragments");
            if let Some(session_id) = agent.get_session_id() {
                match g3_core::acd::list_fragments(session_id) {
                    Ok(fragments) => {
                        println!("FRAGMENT_COUNT: {}", fragments.len());
                        for fragment in &fragments {
                            println!("FRAGMENT_ID: {}", fragment.fragment_id);
                            println!("FRAGMENT_MESSAGES: {}", fragment.message_count);
                            println!("FRAGMENT_TOKENS: {}", fragment.estimated_tokens);
                        }
                    }
                    Err(e) => {
                        println!("ERROR: {}", e);
                    }
                }
            } else {
                println!("ERROR: No active session");
            }
            Ok(true)
        }
        cmd if cmd.starts_with("/rehydrate") => {
            println!("COMMAND: rehydrate");
            let parts: Vec<&str> = cmd.splitn(2, ' ').collect();
            if parts.len() < 2 || parts[1].trim().is_empty() {
                println!("ERROR: Usage: /rehydrate <fragment_id>");
            } else {
                let fragment_id = parts[1].trim();
                println!("FRAGMENT_ID: {}", fragment_id);
                println!("RESULT: Use the rehydrate tool to restore fragment content");
            }
            Ok(true)
        }
        "/dump" => {
            println!("COMMAND: dump");
            let dump_dir = std::path::Path::new("tmp");
            if !dump_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(dump_dir) {
                    println!("ERROR: Failed to create tmp directory: {}", e);
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
                dump_content.push_str(&format!(
                    "=== Message {} ===\nRole: {:?}\nKind: {:?}\nContent ({} chars):\n{}\n\n",
                    i,
                    msg.role,
                    msg.kind,
                    msg.content.len(),
                    msg.content
                ));
            }

            match std::fs::write(&dump_path, &dump_content) {
                Ok(_) => println!("RESULT: Context dumped to {}", dump_path.display()),
                Err(e) => println!("ERROR: Failed to write dump: {}", e),
            }
            Ok(true)
        }
        "/clear" => {
            println!("COMMAND: clear");
            agent.clear_session();
            println!("RESULT: Session cleared");
            Ok(true)
        }
        "/readme" => {
            println!("COMMAND: readme");
            match agent.reload_readme() {
                Ok(true) => println!("RESULT: README content reloaded successfully"),
                Ok(false) => println!("RESULT: No README was loaded at startup, cannot reload"),
                Err(e) => println!("ERROR: {}", e),
            }
            Ok(true)
        }
        "/stats" => {
            println!("COMMAND: stats");
            let stats = agent.get_stats();
            // Emit stats as structured data (name: value pairs)
            println!("{}", stats);
            Ok(true)
        }
        "/help" => {
            println!("COMMAND: help");
            println!("AVAILABLE_COMMANDS: /compact /thinnify /skinnify /clear /dump /fragments /rehydrate /resume /readme /stats /help");
            Ok(true)
        }
        "/resume" => {
            println!("COMMAND: resume");
            match g3_core::list_sessions_for_directory() {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        println!("RESULT: No sessions found");
                        return Ok(true);
                    }

                    println!("SESSIONS_START");
                    for (i, session) in sessions.iter().enumerate() {
                        let time_str = g3_core::format_session_time(&session.created_at);
                        let has_todos = if session.has_incomplete_todos() {
                            "true"
                        } else {
                            "false"
                        };
                        println!(
                            "SESSION: {} | {} | {} | {:.0}% | {}",
                            i + 1,
                            session.session_id,
                            time_str,
                            session.context_percentage,
                            has_todos
                        );
                    }
                    println!("SESSIONS_END");
                    println!("HINT: Use /resume <number> to switch to a session");
                }
                Err(e) => println!("ERROR: {}", e),
            }
            Ok(true)
        }
        _ => {
            // Check for /resume <number> pattern
            if input.starts_with("/resume ") {
                let num_str = input.strip_prefix("/resume ").unwrap().trim();
                if let Ok(num) = num_str.parse::<usize>() {
                    println!("COMMAND: resume {}", num);
                    match g3_core::list_sessions_for_directory() {
                        Ok(sessions) => {
                            if num >= 1 && num <= sessions.len() {
                                let selected = &sessions[num - 1];
                                match agent.switch_to_session(selected) {
                                    Ok(true) => println!(
                                        "RESULT: Full context restored from session {}",
                                        selected.session_id
                                    ),
                                    Ok(false) => println!(
                                        "RESULT: Session {} restored from summary",
                                        selected.session_id
                                    ),
                                    Err(e) => println!("ERROR: {}", e),
                                }
                            } else {
                                println!("ERROR: Invalid session number");
                            }
                        }
                        Err(e) => println!("ERROR: {}", e),
                    }
                    return Ok(true);
                }
            }
            println!("ERROR: Unknown command: {}", input);
            Ok(true)
        }
    }
}
