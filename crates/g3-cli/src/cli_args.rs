//! CLI argument parsing for G3.

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Clone)]
#[command(name = "g3")]
#[command(about = "A modular, composable AI coding agent")]
#[command(version)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Enable manual control of context compaction (disables auto-compact at 90%)
    #[arg(long = "manual-compact")]
    pub manual_compact: bool,

    /// Show the system prompt being sent to the LLM
    #[arg(long)]
    pub show_prompt: bool,

    /// Show the generated code before execution
    #[arg(long)]
    pub show_code: bool,

    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<String>,

    /// Workspace directory (defaults to current directory)
    #[arg(short, long)]
    pub workspace: Option<PathBuf>,

    /// Task to execute (if provided, runs in single-shot mode instead of interactive)
    pub task: Option<String>,

    /// Enable autonomous mode with coach-player feedback loop
    #[arg(long)]
    pub autonomous: bool,

    /// Maximum number of turns in autonomous mode (default: 5)
    #[arg(long, default_value = "5")]
    pub max_turns: usize,

    /// Override requirements text for autonomous mode (instead of reading from requirements.md)
    #[arg(long, value_name = "TEXT")]
    pub requirements: Option<String>,

    /// Enable accumulative autonomous mode (default is chat mode)
    #[arg(long)]
    pub auto: bool,

    /// Enable interactive chat mode (no autonomous runs)
    #[arg(long)]
    pub chat: bool,

    /// Override the configured provider (anthropic, databricks, embedded, openai)
    #[arg(long, value_name = "PROVIDER")]
    pub provider: Option<String>,

    /// Override the model for the selected provider
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Disable session log file creation (no .g3/sessions/ or error logs)
    #[arg(long)]
    pub quiet: bool,

    /// Enable WebDriver browser automation tools
    #[arg(long)]
    pub webdriver: bool,

    /// Use Chrome in headless mode for WebDriver (instead of Safari)
    #[arg(long)]
    pub chrome_headless: bool,

    /// Use Safari for WebDriver (this is the default)
    #[arg(long)]
    pub safari: bool,

    /// Enable flock mode - parallel multi-agent development
    #[arg(long, requires = "flock_workspace", requires = "segments")]
    pub project: Option<PathBuf>,

    /// Flock workspace directory (where segment copies will be created)
    #[arg(long, requires = "project")]
    pub flock_workspace: Option<PathBuf>,

    /// Number of segments to partition work into (for flock mode)
    #[arg(long, requires = "project")]
    pub segments: Option<usize>,

    /// Maximum turns per segment in flock mode (default: 5)
    #[arg(long, default_value = "5")]
    pub flock_max_turns: usize,

    /// Enable planning mode for requirements-driven development
    #[arg(long, conflicts_with_all = ["autonomous", "auto", "chat"])]
    pub planning: bool,

    /// Path to the codebase to work on (for planning mode)
    #[arg(long, value_name = "PATH")]
    pub codepath: Option<String>,

    /// Disable git operations in planning mode
    #[arg(long)]
    pub no_git: bool,

    /// Enable fast codebase discovery before first LLM turn
    #[arg(long, value_name = "PATH")]
    pub codebase_fast_start: Option<PathBuf>,

    /// Run as a specialized agent (loads prompt from agents/<name>.md)
    #[arg(long, value_name = "NAME", conflicts_with_all = ["autonomous", "auto", "chat", "planning"])]
    pub agent: Option<String>,

    /// Skip session resumption and force a new session (for agent mode)
    #[arg(long)]
    pub new_session: bool,

    /// Automatically remind LLM to call remember tool after turns with tool calls
    #[arg(long)]
    pub auto_memory: bool,

    /// Enable aggressive context dehydration (save context to disk on compaction)
    #[arg(long)]
    pub acd: bool,
}
