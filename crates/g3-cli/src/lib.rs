//! G3 CLI - Command-line interface for the G3 AI coding agent.

pub mod filter_json;
pub mod metrics;
pub mod project_files;
pub mod streaming_markdown;

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
use utils::{load_config_with_cli_overrides, setup_workspace_directory};

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Check if flock mode is enabled
    if let (Some(project_dir), Some(flock_workspace), Some(num_segments)) =
        (&cli.project, &cli.flock_workspace, cli.segments)
    {
        return run_flock_mode(
            project_dir.clone(),
            flock_workspace.clone(),
            num_segments,
            cli.flock_max_turns,
        )
        .await;
    }

    if cli.codebase_fast_start.is_some() {
        print!("codebase_fast_start is temporarily disabled.");
        std::process::exit(1);
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
        )
        .await;
    }

    // Initialize logging
    initialize_logging(&cli);

    // Set up workspace directory
    let workspace_dir = determine_workspace_dir(&cli)?;

    // Load project context files
    let agents_content = read_agents_config(&workspace_dir);
    let readme_content = read_project_readme(&workspace_dir);
    let memory_content = read_project_memory(&workspace_dir);

    // Create project model
    let project = create_project(&cli, &workspace_dir)?;

    // Ensure workspace exists and enter it
    project.ensure_workspace_exists()?;
    project.enter_workspace()?;

    // Load configuration with CLI overrides
    let config = load_config_with_cli_overrides(&cli)?;

    // Combine AGENTS.md, README, and memory content
    let combined_content = combine_project_content(agents_content, readme_content, memory_content);

    run_console_mode(cli, config, project, combined_content, workspace_dir).await
}

// --- Helper functions ---

fn initialize_logging(cli: &Cli) {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let filter = if cli.verbose {
        EnvFilter::from_default_env()
            .add_directive(format!("{}=debug", env!("CARGO_PKG_NAME")).parse().unwrap())
            .add_directive("g3_core=debug".parse().unwrap())
            .add_directive("g3_cli=debug".parse().unwrap())
            .add_directive("g3_execution=debug".parse().unwrap())
            .add_directive("g3_providers=debug".parse().unwrap())
    } else {
        EnvFilter::from_default_env()
            .add_directive(format!("{}=info", env!("CARGO_PKG_NAME")).parse().unwrap())
            .add_directive("g3_core=info".parse().unwrap())
            .add_directive("g3_cli=info".parse().unwrap())
            .add_directive("g3_execution=info".parse().unwrap())
            .add_directive("g3_providers=info".parse().unwrap())
            .add_directive("llama_cpp=off".parse().unwrap())
            .add_directive("llama=off".parse().unwrap())
    };

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(filter)
        .init();
}

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
        output.print_smart(&result.response);

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
        )
        .await
    }
}

/// Run flock mode - parallel multi-agent development
async fn run_flock_mode(
    project_dir: PathBuf,
    flock_workspace: PathBuf,
    num_segments: usize,
    max_turns: usize,
) -> Result<()> {
    let output = SimpleOutput::new();

    output.print("");
    output.print("🦅 G3 FLOCK MODE - Parallel Multi-Agent Development");
    output.print("");
    output.print(&format!("📁 Project: {}", project_dir.display()));
    output.print(&format!("🗂️  Workspace: {}", flock_workspace.display()));
    output.print(&format!("🔢 Segments: {}", num_segments));
    output.print(&format!("🔄 Max Turns per Segment: {}", max_turns));
    output.print("");

    let config = g3_ensembles::FlockConfig::new(project_dir, flock_workspace, num_segments)?
        .with_max_turns(max_turns);

    let mut flock = g3_ensembles::FlockMode::new(config)?;

    match flock.run().await {
        Ok(_) => output.print("\n✅ Flock mode completed successfully"),
        Err(e) => output.print(&format!("\n❌ Flock mode failed: {}", e)),
    }

    Ok(())
}
