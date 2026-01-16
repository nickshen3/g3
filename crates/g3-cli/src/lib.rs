//! G3 CLI - Command-line interface for the G3 AI coding agent.

pub mod filter_json;
pub mod metrics;
pub mod project_files;
pub mod streaming_markdown;
pub mod embedded_agents;
pub mod language_prompts;

mod accumulative;
mod agent_mode;
mod autonomous;
mod cli_args;
mod coach_feedback;
mod interactive;
mod simple_output;
mod task_execution;
mod ui_writer_impl;
mod utils;

use anyhow::Result;
use std::path::PathBuf;
use tracing::debug;

use g3_config::Config;
use g3_core::project::Project;
use g3_core::Agent;

pub use cli_args::Cli;
use clap::Parser;

use accumulative::run_accumulative_mode;
use agent_mode::run_agent_mode;
use autonomous::run_autonomous;
use interactive::run_interactive;
use project_files::{combine_project_content, read_agents_config, read_project_memory, read_project_readme};
use simple_output::SimpleOutput;
use ui_writer_impl::ConsoleUiWriter;
use utils::{initialize_logging, load_config_with_cli_overrides, setup_workspace_directory};

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging FIRST (before any mode checks)
    initialize_logging(cli.verbose);

    if cli.codebase_fast_start.is_some() {
        print!("codebase_fast_start is temporarily disabled.");
        std::process::exit(1);
    }

    // Check if --list-agents was requested
    if cli.list_agents {
        let workspace_dir = cli.workspace.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        let agents = embedded_agents::get_available_agents(&workspace_dir);
        println!("Available agents:");
        let mut names: Vec<_> = agents.keys().collect();
        names.sort();
        for name in names {
            let source = if agents[name] { "workspace" } else { "embedded" };
            println!("  {} ({})", name, source);
        }
        println!("\nUse: g3 --agent <name> [task]");
        println!("Workspace agents override embedded agents with the same name.");
        return Ok(());
    }

    // Check if planning mode is enabled
    if cli.planning {
        let codepath = cli.codepath.clone();
        return g3_planner::run_planning_mode(
            codepath,
            cli.workspace.clone(),
            cli.no_git,
            cli.config.as_deref(),
        )
        .await;
    }

    // Check if agent mode is enabled
    if let Some(agent_name) = &cli.agent {
        return run_agent_mode(
            agent_name,
            cli.workspace.clone(),
            cli.config.as_deref(),
            cli.quiet,
            cli.new_session,
            cli.task.clone(),
            cli.chrome_headless,
            cli.safari,
            cli.chat,
        )
        .await;
    }

    // Set up workspace directory
    let workspace_dir = determine_workspace_dir(&cli)?;

    // Load project context files
    let agents_content = read_agents_config(&workspace_dir);
    let readme_content = read_project_readme(&workspace_dir);
    let memory_content = read_project_memory(&workspace_dir);
    let language_content = language_prompts::get_language_prompts_for_workspace(&workspace_dir);

    // Create project model
    let project = create_project(&cli, &workspace_dir)?;

    // Ensure workspace exists and enter it
    project.ensure_workspace_exists()?;
    project.enter_workspace()?;

    // Load configuration with CLI overrides
    let config = load_config_with_cli_overrides(&cli)?;

    // Combine AGENTS.md, README, and memory content
    let combined_content = combine_project_content(agents_content, readme_content, memory_content, language_content, &workspace_dir);

    run_console_mode(cli, config, project, combined_content, workspace_dir).await
}

// --- Helper functions ---

fn determine_workspace_dir(cli: &Cli) -> Result<PathBuf> {
    if let Some(ws) = &cli.workspace {
        Ok(ws.clone())
    } else if cli.autonomous {
        setup_workspace_directory()
    } else {
        Ok(std::env::current_dir()?)
    }
}

fn create_project(cli: &Cli, workspace_dir: &PathBuf) -> Result<Project> {
    if cli.autonomous {
        if let Some(requirements_text) = &cli.requirements {
            Project::new_autonomous_with_requirements(workspace_dir.clone(), requirements_text.clone())
        } else {
            Project::new_autonomous(workspace_dir.clone())
        }
    } else {
        Ok(Project::new(workspace_dir.clone()))
    }
}

async fn run_console_mode(
    cli: Cli,
    config: Config,
    project: Project,
    combined_content: Option<String>,
    workspace_dir: PathBuf,
) -> Result<()> {
    // Check for accumulative mode
    let use_accumulative = cli.task.is_none() && !cli.autonomous && cli.auto;
    if use_accumulative {
        return run_accumulative_mode(workspace_dir, cli, combined_content).await;
    }

    let ui_writer = ConsoleUiWriter::new();

    let mut agent = if cli.autonomous {
        Agent::new_autonomous_with_readme_and_quiet(
            config.clone(),
            ui_writer,
            combined_content.clone(),
            cli.quiet,
        )
        .await?
    } else {
        Agent::new_with_readme_and_quiet(
            config.clone(),
            ui_writer,
            combined_content.clone(),
            cli.quiet,
        )
        .await?
    };

    if cli.auto_memory {
        agent.set_auto_memory(true);
    }
    if cli.acd {
        agent.set_acd_enabled(true);
    }

    if cli.autonomous {
        let _agent = run_autonomous(
            agent,
            project,
            cli.show_prompt,
            cli.show_code,
            cli.max_turns,
            cli.quiet,
            cli.codebase_fast_start.clone(),
        )
        .await?;
        Ok(())
    } else if let Some(task) = cli.task {
        // Single-shot mode
        let output = SimpleOutput::new();
        let result = agent
            .execute_task_with_timing(&task, None, false, cli.show_prompt, cli.show_code, true, None)
            .await?;
        // Only print response if it's not empty (streaming already displayed it)
        if !result.response.trim().is_empty() {
            output.print_smart(&result.response);
        }

        if let Err(e) = agent.send_auto_memory_reminder().await {
            debug!("Auto-memory reminder failed: {}", e);
        }
        agent.save_session_continuation(Some(result.response.clone()));
        Ok(())
    } else {
        run_interactive(
            agent,
            cli.show_prompt,
            cli.show_code,
            combined_content,
            project.workspace(),
            cli.new_session,
            false, // from_agent_mode
        )
        .await
    }
}
