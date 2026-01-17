//! Agent mode for G3 CLI - runs specialized agents with custom prompts.

use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use g3_core::ui_writer::UiWriter;
use g3_core::Agent;

use crate::project_files::{combine_project_content, read_agents_config, read_include_prompt, read_project_memory, read_project_readme};
use crate::language_prompts::{get_language_prompts_for_workspace, get_agent_language_prompts_for_workspace_with_langs};
use crate::simple_output::SimpleOutput;
use crate::embedded_agents::load_agent_prompt;
use crate::ui_writer_impl::ConsoleUiWriter;
use crate::interactive::run_interactive;

/// Run agent mode - loads a specialized agent prompt and executes a single task.
pub async fn run_agent_mode(
    agent_name: &str,
    workspace: Option<PathBuf>,
    config_path: Option<&str>,
    _quiet: bool,
    new_session: bool,
    task: Option<String>,
    chrome_headless: bool,
    safari: bool,
    chat: bool,
    include_prompt_path: Option<PathBuf>,
    no_auto_memory: bool,
) -> Result<()> {
    use g3_core::find_incomplete_agent_session;
    use g3_core::get_agent_system_prompt;

    // Set process title to agent name (shows in ps, Activity Monitor, etc.)
    proctitle::set_title(format!("g3 [{}]", agent_name));

    // Initialize logging
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let filter = EnvFilter::from_default_env()
        .add_directive("g3_core=info".parse().unwrap())
        .add_directive("g3_cli=info".parse().unwrap())
        .add_directive("llama_cpp=off".parse().unwrap())
        .add_directive("llama=off".parse().unwrap());
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .try_init();

    let output = SimpleOutput::new();

    // Determine workspace directory (current dir if not specified)
    let workspace_dir = workspace.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Change to the workspace directory first so session scanning works correctly
    std::env::set_current_dir(&workspace_dir)?;

    // Check for incomplete agent sessions before starting a new one
    // Skip session resume entirely when in chat mode (--agent --chat)
    let resuming_session = if chat {
        None // Chat mode always starts fresh
    } else if new_session {
        if !chat {
            output.print("\n🆕 Starting new session (--new-session flag set)");
            output.print("");
        }
        None
    } else {
        find_incomplete_agent_session(agent_name).ok().flatten()
    };

    // Only show session resume info when not in chat mode
    if !chat {
      if let Some(ref incomplete_session) = resuming_session {
        output.print(&format!(
            "\n🔄 Found incomplete session for agent '{}'",
            agent_name
        ));
        output.print(&format!("   Session: {}", incomplete_session.session_id));
        output.print(&format!("   Created: {}", incomplete_session.created_at));
        if let Some(ref todo) = incomplete_session.todo_snapshot {
            // Show first few lines of TODO
            let preview: String = todo.lines().take(5).collect::<Vec<_>>().join("\n");
            output.print(&format!("   TODO preview:\n{}", preview));
        }
        output.print("");
        output.print("   Resuming incomplete session...");
        output.print("");
      }
    }

    // Load agent prompt: workspace agents/<name>.md first, then embedded fallback
    let (agent_prompt, from_disk) = load_agent_prompt(agent_name, &workspace_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "Agent '{}' not found.\nAvailable embedded agents: breaker, carmack, euler, fowler, hopper, lamport, scout\nOr create agents/{}.md in your workspace.",
            agent_name,
            agent_name
        )
    })?;

    let source = if from_disk { "workspace" } else { "embedded" };
    // Only print verbose header when not in chat mode
    if !chat {
        output.print(&format!(">> agent mode | {} ({})", agent_name, source));
    }
    let workspace_display = {
        let path_str = workspace_dir.display().to_string();
        dirs::home_dir()
            .and_then(|home| {
                path_str
                    .strip_prefix(&home.display().to_string())
                    .map(|s| format!("~{}", s))
            })
            .unwrap_or(path_str)
    };
    // Always print workspace path (it's part of minimal output)
    print!("{}-> {}{}\n", crossterm::style::SetForegroundColor(crossterm::style::Color::DarkGrey), workspace_display, crossterm::style::ResetColor);

    // Load config
    let mut config = g3_config::Config::load(config_path)?;

    // Apply chrome-headless flag override
    if chrome_headless {
        config.webdriver.enabled = true;
        config.webdriver.browser = g3_config::WebDriverBrowser::ChromeHeadless;
    }

    // Apply safari flag override
    if safari {
        config.webdriver.enabled = true;
        config.webdriver.browser = g3_config::WebDriverBrowser::Safari;
    }

    // Generate the combined system prompt (agent prompt + tool instructions)
    // Note: allow_multiple_tool_calls parameter is deprecated but kept for API compatibility
    let system_prompt = get_agent_system_prompt(&agent_prompt, true);

    // Load AGENTS.md, README, and memory - same as normal mode
    let agents_content_opt = read_agents_config(&workspace_dir);
    let readme_content_opt = read_project_readme(&workspace_dir);
    let memory_content_opt = read_project_memory(&workspace_dir);

    // Show what was loaded
    let readme_status = if readme_content_opt.is_some() {
        "✓"
    } else {
        "·"
    };
    let agents_status = if agents_content_opt.is_some() {
        "✓"
    } else {
        "·"
    };
    let memory_status = if memory_content_opt.is_some() {
        "✓"
    } else {
        "·"
    };
    // Always print status line (part of minimal output)
    print!(
        "{}   {} README  {} AGENTS.md  {} Memory{}\n",
        crossterm::style::SetForegroundColor(crossterm::style::Color::DarkGrey),
        readme_status, agents_status, memory_status,
        crossterm::style::ResetColor
    );

    // Get language-specific prompts (same mechanism as normal mode)
    let language_content = get_language_prompts_for_workspace(&workspace_dir);

    // Get agent+language-specific prompts (e.g., carmack.racket.md) and show which languages
    let detected_langs = crate::language_prompts::detect_languages(&workspace_dir);
    let agent_lang_content = if detected_langs.is_empty() {
        None
    } else {
        let (content, matched_langs) = get_agent_language_prompts_for_workspace_with_langs(&workspace_dir, agent_name);
        // Only print language guidance info when not in chat mode
        if !chat {
            for lang in matched_langs {
                output.print(&format!("   ✓ {}: {} language guidance", agent_name, lang));
            }
        }
        content
    };

    // Append agent+language-specific content to system prompt if available
    let system_prompt = if let Some(agent_lang) = agent_lang_content {
        format!("{}\n\n{}", system_prompt, agent_lang)
    } else {
        system_prompt
    };

    // Read include prompt if specified
    let include_prompt = read_include_prompt(include_prompt_path.as_deref());

    // Combine all content for the agent's context
    let combined_content = combine_project_content(
        agents_content_opt,
        readme_content_opt,
        memory_content_opt,
        language_content,
        include_prompt,
        &workspace_dir,
    );

    // Create agent with custom system prompt
    let ui_writer = ConsoleUiWriter::new();
    // Set agent mode on UI writer for visual differentiation (light gray tool names)
    ui_writer.set_agent_mode(true);
    let mut agent =
        Agent::new_with_custom_prompt(config, ui_writer, system_prompt, combined_content.clone()).await?;

    // Set agent mode for session tracking
    agent.set_agent_mode(agent_name);

    // Auto-memory is enabled by default in agent mode (unless --no-auto-memory is set)
    // This prompts the LLM to save discoveries to project memory after each turn
    agent.set_auto_memory(!no_auto_memory);

    // If resuming a session, restore context and TODO
    let initial_task = if let Some(ref incomplete_session) = resuming_session {
        // Restore the session context
        match agent.restore_from_continuation(incomplete_session) {
            Ok(full_restore) => {
                if full_restore {
                    output.print("   ✅ Full context restored from previous session");
                } else {
                    output.print("   ⚠️ Restored from summary (context was > 80%)");
                }
            }
            Err(e) => {
                output.print(&format!("   ⚠️ Could not restore context: {}", e));
            }
        }

        // Copy TODO from old session to new session directory
        let todo_content = if let Some(ref content) = incomplete_session.todo_snapshot {
            Some(content.clone())
        } else {
            // Fallback: read from the actual todo.g3.md file in the old session directory
            let old_session_dir =
                std::path::Path::new(".g3/sessions").join(&incomplete_session.session_id);
            let old_todo_path = old_session_dir.join("todo.g3.md");
            if old_todo_path.exists() {
                std::fs::read_to_string(&old_todo_path).ok()
            } else {
                None
            }
        };

        if let Some(ref content) = todo_content {
            if let Some(session_id) = agent.get_session_id() {
                let new_todo_path = g3_core::paths::get_session_todo_path(session_id);
                let _ = g3_core::paths::ensure_session_dir(session_id);
                if let Err(e) = std::fs::write(&new_todo_path, content) {
                    output.print(&format!("   ⚠️ Could not restore TODO: {}", e));
                } else {
                    output.print("   ✅ TODO list restored");
                }
            }
        }
        output.print("");

        // Resume message instead of fresh start
        "Continue working on the incomplete tasks. Use todo_read to see the current TODO list and resume from where you left off."
    } else {
        // Fresh start - the agent prompt should contain instructions to start working immediately
        "Begin your analysis and work on the current project. Follow your mission and workflow as specified in your instructions."
    };
    // Use provided task if available, otherwise use the default initial_task
    let final_task = task.as_deref().unwrap_or(initial_task);

    // If chat mode is enabled, run interactive loop instead of single task
    if chat {
        return run_interactive(
            agent,
            false, // show_prompt
            false, // show_code
            combined_content,
            &workspace_dir,
            new_session,
            Some(agent_name),  // agent name for prompt (e.g., "butler>")
        )
        .await;
    }

    // Single-shot mode: execute the task and exit
    let _result = agent.execute_task(final_task, None, true).await?;

    // Send auto-memory reminder if enabled and tools were called
    if let Err(e) = agent.send_auto_memory_reminder().await {
        debug!("Auto-memory reminder failed: {}", e);
    }

    // Save session continuation for resume capability
    agent.save_session_continuation(None);

    // Don't print completion message for scout agent - it needs the last line
    // to be the report file path for the research tool to read
    if agent_name != "scout" {
        output.print("\n✅ Agent mode completed");
    }
    Ok(())
}
