//! Agent mode for G3 CLI - runs specialized agents with custom prompts.

use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use g3_core::ui_writer::UiWriter;
use g3_core::Agent;

use crate::project_files::{combine_project_content, read_agents_config, read_project_memory, read_project_readme};
use crate::simple_output::SimpleOutput;
use crate::ui_writer_impl::ConsoleUiWriter;

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
) -> Result<()> {
    use g3_core::find_incomplete_agent_session;
    use g3_core::get_agent_system_prompt;

    // Initialize logging
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
    let filter = EnvFilter::from_default_env()
        .add_directive("g3_core=info".parse().unwrap())
        .add_directive("g3_cli=info".parse().unwrap())
        .add_directive("llama_cpp=off".parse().unwrap())
        .add_directive("llama=off".parse().unwrap());
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();

    let output = SimpleOutput::new();

    // Determine workspace directory (current dir if not specified)
    let workspace_dir = workspace.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Change to the workspace directory first so session scanning works correctly
    std::env::set_current_dir(&workspace_dir)?;

    // Check for incomplete agent sessions before starting a new one (unless --new-session is set)
    let resuming_session = if new_session {
        output.print("\n🆕 Starting new session (--new-session flag set)");
        output.print("");
        None
    } else {
        find_incomplete_agent_session(agent_name).ok().flatten()
    };

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

    // Load agent prompt from agents/<name>.md
    let agent_prompt_path = workspace_dir
        .join("agents")
        .join(format!("{}.md", agent_name));

    // Also check in the g3 installation directory
    let agent_prompt = if agent_prompt_path.exists() {
        std::fs::read_to_string(&agent_prompt_path).map_err(|e| {
            anyhow::anyhow!(
                "Failed to read agent prompt from {:?}: {}",
                agent_prompt_path,
                e
            )
        })?
    } else {
        // Try to find agents/ relative to the executable or in common locations
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));

        let possible_paths = [
            exe_dir
                .as_ref()
                .map(|d| d.join("agents").join(format!("{}.md", agent_name))),
            Some(PathBuf::from(format!("agents/{}.md", agent_name))),
        ];

        let mut found_prompt = None;
        for path_opt in possible_paths.iter().flatten() {
            if path_opt.exists() {
                found_prompt = Some(std::fs::read_to_string(path_opt).map_err(|e| {
                    anyhow::anyhow!("Failed to read agent prompt from {:?}: {}", path_opt, e)
                })?);
                break;
            }
        }

        found_prompt.ok_or_else(|| {
            anyhow::anyhow!(
                "Agent prompt not found: agents/{}.md\nSearched in: {:?} and current directory",
                agent_name,
                agent_prompt_path
            )
        })?
    };

    output.print(&format!(">> agent mode | {}", agent_name));
    // Format workspace path, replacing home dir with ~
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
    output.print(&format!("-> {}", workspace_display));

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
    output.print(&format!(
        "   {} README | {} AGENTS.md | {} Memory",
        readme_status, agents_status, memory_status
    ));

    // Combine all content for the agent's context
    let combined_content = combine_project_content(
        agents_content_opt,
        readme_content_opt,
        memory_content_opt,
    );

    // Create agent with custom system prompt
    let ui_writer = ConsoleUiWriter::new();
    // Set agent mode on UI writer for visual differentiation (light gray tool names)
    ui_writer.set_agent_mode(true);
    let mut agent =
        Agent::new_with_custom_prompt(config, ui_writer, system_prompt, combined_content).await?;

    // Set agent mode for session tracking
    agent.set_agent_mode(agent_name);

    // Auto-memory is always enabled in agent mode
    // This prompts the LLM to save discoveries to project memory after each turn
    agent.set_auto_memory(true);

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
