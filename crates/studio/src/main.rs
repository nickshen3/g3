use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use termimad::MadSkin;

mod git;
mod session;

use git::GitWorktree;
use session::{Session, SessionStatus};

/// Studio - Multi-agent workspace manager for g3
#[derive(Parser)]
#[command(name = "studio", subcommand_required = false)]
#[command(about = "Manage multiple g3 agent sessions using git worktrees")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a new interactive g3 session in an isolated worktree
    #[command(alias = "c")]
    Cli,

    /// Resume a paused interactive session
    #[command(alias = "r")]
    Resume {
        /// Session ID to resume
        session_id: String,
    },

    /// Run a new g3 session (tails output until complete)
    Run {
        /// Agent name (e.g., carmack, torvalds). If omitted, runs g3 in one-shot mode.
        #[arg(long)]
        agent: Option<String>,

        /// Automatically accept the session if it completes with commits
        #[arg(long)]
        accept: bool,

        /// Additional arguments to pass to g3
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        g3_args: Vec<String>,
    },

    /// Execute a g3 agent session in detached mode (for future use)
    Exec {
        /// Agent name (e.g., carmack, torvalds). If omitted, runs g3 in one-shot mode.
        #[arg(long)]
        agent: Option<String>,

        /// Additional arguments to pass to g3
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        g3_args: Vec<String>,
    },

    /// List all active sessions
    List,

    /// Show status of a session
    Status {
        /// Session ID
        session_id: String,
    },

    /// Accept a session: merge to main and cleanup
    Accept {
        /// Session ID
        session_id: String,
    },

    /// Discard a session: delete without merging
    Discard {
        /// Session ID
        session_id: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Cli) {
        Commands::Cli => cmd_cli(),
        Commands::Resume { session_id } => cmd_resume(&session_id),
        Commands::Run { agent, accept, g3_args } => cmd_run(agent.as_deref(), accept, &g3_args),
        Commands::Exec { agent, g3_args } => cmd_exec(agent.as_deref(), &g3_args),
        Commands::List => cmd_list(),
        Commands::Status { session_id } => cmd_status(&session_id),
        Commands::Accept { session_id } => cmd_accept(&session_id),
        Commands::Discard { session_id } => cmd_discard(&session_id),
    }
}

/// Start a new interactive g3 session
fn cmd_cli() -> Result<()> {
    let g3_binary = get_g3_binary_path()?;
    let repo_root = get_repo_root()?;
    let session = Session::new_interactive();

    // Create worktree
    let worktree = GitWorktree::new(&repo_root);
    let worktree_path = worktree.create(&session)?;

    println!("📁 Worktree: {}", worktree_path.display());
    println!("🌿 Branch: {}", session.branch_name());
    println!("🆔 Session: {}", session.id);
    println!();

    // Save session metadata
    session.save(&repo_root, &worktree_path)?;

    // Build g3 command with inherited stdio for interactive use
    let mut cmd = Command::new(&g3_binary);
    cmd.arg("--workspace").arg(&worktree_path);
    cmd.current_dir(&worktree_path);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // Spawn and wait
    let status = cmd.status().context("Failed to run g3")?;

    // Mark session as paused (user can resume later or accept/discard)
    session.mark_paused(&repo_root)?;

    println!();
    println!("Session {} paused.", session.id);
    println!();
    println!("Next steps:");
    println!("  studio resume {}  - Continue working", session.id);
    println!("  studio accept {}  - Merge changes to main", session.id);
    println!("  studio discard {} - Discard changes", session.id);

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Resume a paused interactive session
fn cmd_resume(session_id: &str) -> Result<()> {
    let g3_binary = get_g3_binary_path()?;
    let repo_root = get_repo_root()?;
    let session = Session::load(&repo_root, session_id)?;

    let worktree_path = session.worktree_path.as_ref()
        .ok_or_else(|| anyhow!("Session {} has no worktree path", session_id))?;

    if !worktree_path.exists() {
        bail!("Worktree for session {} no longer exists at {}", session_id, worktree_path.display());
    }

    println!("📁 Resuming session {} in {}", session_id, worktree_path.display());
    println!();

    // Build g3 command with inherited stdio
    let mut cmd = Command::new(&g3_binary);
    cmd.arg("--workspace").arg(worktree_path);
    cmd.current_dir(worktree_path);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    // Spawn and wait
    let status = cmd.status().context("Failed to run g3")?;

    // Mark session as paused again
    session.mark_paused(&repo_root)?;

    println!();
    println!("Session {} paused.", session_id);
    println!();
    println!("Next steps:");
    println!("  studio resume {}  - Continue working", session_id);
    println!("  studio accept {}  - Merge changes to main", session_id);
    println!("  studio discard {} - Discard changes", session_id);

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Get the path to the g3 binary (same directory as studio)
fn get_g3_binary_path() -> Result<PathBuf> {
    let current_exe = env::current_exe().context("Failed to get current executable path")?;
    let exe_dir = current_exe
        .parent()
        .ok_or_else(|| anyhow!("Failed to get executable directory"))?;
    let g3_path = exe_dir.join("g3");

    if !g3_path.exists() {
        bail!(
            "g3 binary not found at {:?}. Ensure g3 is built and in the same directory as studio.",
            g3_path
        );
    }

    Ok(g3_path)
}

/// Get the repository root (where .git is)
fn get_repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git rev-parse")?;

    if !output.status.success() {
        bail!("Not in a git repository");
    }

    let path = String::from_utf8(output.stdout)
        .context("Invalid UTF-8 in git output")?
        .trim()
        .to_string();

    Ok(PathBuf::from(path))
}

/// Filter --accept from g3_args and return (filtered_args, was_accept_present)
/// This handles the case where --accept is passed after positional args,
/// which clap's trailing_var_arg captures instead of parsing as our flag.
fn filter_accept_flag(g3_args: &[String]) -> (Vec<String>, bool) {
    let accept_in_args = g3_args.iter().any(|arg| arg == "--accept");
    let filtered: Vec<String> = g3_args
        .iter()
        .filter(|arg| *arg != "--accept")
        .cloned()
        .collect();
    (filtered, accept_in_args)
}

/// Run a new g3 session (foreground, tails output)
fn cmd_run(agent: Option<&str>, auto_accept: bool, g3_args: &[String]) -> Result<()> {
    let (g3_args, accept_in_args) = filter_accept_flag(g3_args);
    let auto_accept = auto_accept || accept_in_args;

    let g3_binary = get_g3_binary_path()?;
    let repo_root = get_repo_root()?;
    // Use "single" as the agent name for non-agent runs
    let agent_name = agent.unwrap_or("single");
    let session = Session::new(agent_name);

    // Create worktree
    let worktree = GitWorktree::new(&repo_root);
    let worktree_path = worktree.create(&session)?;

    // Print compact session info: "studio:" in bold green, session id in inline-code orange
    println!("\x1b[1;32mstudio:\x1b[0m new session \x1b[38;2;216;177;114m{}\x1b[0m", session.id);

    // Build g3 command with --workspace prepended
    let mut cmd = Command::new(&g3_binary);
    cmd.arg("--workspace").arg(&worktree_path);
    // Only add --agent if an agent was specified
    if let Some(a) = agent {
        cmd.arg("--agent").arg(a);
    }
    cmd.args(g3_args);
    cmd.current_dir(&worktree_path);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Save session metadata
    session.save(&repo_root, &worktree_path)?;

    println!();

    // Spawn and tail output
    let mut child = cmd.spawn().context("Failed to spawn g3 process")?;

    // Update session with PID
    session.update_pid(&repo_root, child.id())?;

    // Tail stdout in a separate thread
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stderr = child.stderr.take().expect("Failed to capture stderr");

    let stdout_handle = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            if let Ok(line) = line {
                println!("{}", line);
            }
        }
    });

    let stderr_handle = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            if let Ok(line) = line {
                eprintln!("{}", line);
            }
        }
    });

    // Wait for process to complete
    let status = child.wait().context("Failed to wait for g3 process")?;

    stdout_handle.join().ok();
    stderr_handle.join().ok();

    println!();
    // Update session status
    session.mark_complete(&repo_root, status.success())?;

    if status.success() {
        println!("✅ Session {} completed successfully", session.id);

        // Auto-accept if flag is set and there are commits on the branch
        if auto_accept {
            if has_commits_on_branch(&worktree_path, &session.branch_name())? {
                println!();
                println!("🔄 Auto-accepting session (commits detected)...");
                return cmd_accept(&session.id);
            } else {
                println!();
                println!("⚠️  --accept flag set but no commits on branch, skipping auto-accept");
            }
        }

        println!();
        println!("Next steps:");
        println!("  studio accept {}  - Merge changes to main", session.id);
        println!("  studio discard {} - Discard changes", session.id);
    } else {
        println!("❌ Session {} failed (exit code: {:?})", session.id, status.code());
    }

    Ok(())
}

/// Check if a branch has commits ahead of main
fn has_commits_on_branch(worktree_path: &Path, branch_name: &str) -> Result<bool> {
    // Count commits on branch that aren't on main
    let output = Command::new("git")
        .current_dir(worktree_path)
        .args(["rev-list", "--count", &format!("main..{}", branch_name)])
        .output()
        .context("Failed to check commits on branch")?;

    if !output.status.success() {
        return Ok(false);
    }

    let count: u32 = String::from_utf8_lossy(&output.stdout).trim().parse().unwrap_or(0);
    Ok(count > 0)
}

/// Execute a g3 session in detached mode (placeholder for future)
fn cmd_exec(agent: Option<&str>, g3_args: &[String]) -> Result<()> {
    // For now, just print what would happen
    println!("exec command not yet implemented");
    match agent {
        Some(a) => println!("Would run agent '{}' with args: {:?}", a, g3_args),
        None => println!("Would run one-shot session with args: {:?}", g3_args),
    }
    Ok(())
}

/// List all sessions
fn cmd_list() -> Result<()> {
    let repo_root = get_repo_root()?;
    let sessions = Session::list_all(&repo_root)?;

    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }

    println!("{:<12} {:<12} {:<10} {:<20}", "SESSION", "AGENT", "STATUS", "CREATED");
    println!("{}", "─".repeat(60));

    for session in sessions {
        let status_str = match session.status {
            SessionStatus::Running => "🔄 running",
            SessionStatus::Paused => "⏸️  paused",
            SessionStatus::Complete => "✅ complete",
            SessionStatus::Failed => "❌ failed",
        };
        println!(
            "{:<12} {:<12} {:<10} {:<20}",
            session.id,
            session.agent,
            status_str,
            session.created_at.format("%Y-%m-%d %H:%M")
        );
    }

    Ok(())
}

/// Show status of a specific session
fn cmd_status(session_id: &str) -> Result<()> {
    let repo_root = get_repo_root()?;
    let session = Session::load(&repo_root, session_id)?;

    println!("Session: {}", session.id);
    println!("Agent:   {}", session.agent);
    println!("Branch:  {}", session.branch_name());
    println!("Created: {}", session.created_at.format("%Y-%m-%d %H:%M:%S"));
    println!("Status:  {:?}", session.status);

    if let Some(path) = &session.worktree_path {
        println!("Worktree: {}", path.display());
    }

    // Check if process is still running
    if session.status == SessionStatus::Running {
        if let Some(pid) = session.pid {
            let is_running = is_process_running(pid);
            if is_running {
                println!("Process:  Running (PID {})", pid);
            } else {
                println!("Process:  Not running (stale session)");
            }
        }
    }

    // Try to extract summary from session logs if complete
    if session.status != SessionStatus::Running {
        if let Some(summary) = extract_session_summary(&session) {
            println!();
            println!("Summary:");
            println!("{}", "─".repeat(60));
            let skin = MadSkin::default();
            skin.print_text(&summary);
        }
    }

    Ok(())
}

/// Accept a session: merge to main and cleanup
fn cmd_accept(session_id: &str) -> Result<()> {
    let repo_root = get_repo_root()?;
    let session = Session::load(&repo_root, session_id)?;

    // Check session is not still running
    if session.status == SessionStatus::Running {
        if let Some(pid) = session.pid {
            if is_process_running(pid) {
                bail!("Session {} is still running (PID {}). Wait for it to complete or kill it first.", session_id, pid);
            }
        }
    }

    let worktree = GitWorktree::new(&repo_root);
    let branch_name = session.branch_name();

    // Print status line without newline, then complete after operations
    use std::io::Write;
    print!("> session {} ... ", session_id);
    std::io::stdout().flush().ok();

    // Merge the branch to main
    worktree.merge_to_main(&branch_name)?;

    // Remove worktree and branch
    worktree.remove(&session)?;

    // Remove session metadata
    session.delete(&repo_root)?;

    println!("[\x1b[1;32mmerged\x1b[0m]");

    Ok(())
}

/// Discard a session: delete without merging
fn cmd_discard(session_id: &str) -> Result<()> {
    let repo_root = get_repo_root()?;
    let session = Session::load(&repo_root, session_id)?;

    // Check session is not still running
    if session.status == SessionStatus::Running {
        if let Some(pid) = session.pid {
            if is_process_running(pid) {
                bail!("Session {} is still running (PID {}). Wait for it to complete or kill it first.", session_id, pid);
            }
        }
    }

    let worktree = GitWorktree::new(&repo_root);

    // Print status line without newline, then complete after operations
    use std::io::Write;
    print!("> session {} ... ", session_id);
    std::io::stdout().flush().ok();

    // Remove worktree and branch
    worktree.remove(&session)?;

    // Remove session metadata
    session.delete(&repo_root)?;

    println!("[\x1b[1;33mdiscarded\x1b[0m]");

    Ok(())
}

/// Check if a process is running by PID
fn is_process_running(pid: u32) -> bool {
    // Use kill -0 to check if process exists
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Extract summary from session logs
fn extract_session_summary(session: &Session) -> Option<String> {
    // Look for session logs in the worktree's .g3 directory
    let worktree_path = session.worktree_path.as_ref()?;
    let session_dir = worktree_path.join(".g3").join("sessions");

    if !session_dir.exists() {
        return None;
    }

    // Find the most recent session log
    let mut latest_log: Option<(PathBuf, std::time::SystemTime)> = None;

    if let Ok(entries) = fs::read_dir(&session_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let log_file = path.join("session.json");
                if log_file.exists() {
                    if let Ok(metadata) = log_file.metadata() {
                        if let Ok(modified) = metadata.modified() {
                            if latest_log.is_none() || modified > latest_log.as_ref().unwrap().1 {
                                latest_log = Some((log_file, modified));
                            }
                        }
                    }
                }
            }
        }
    }

    let log_file = latest_log?.0;
    let content = fs::read_to_string(&log_file).ok()?;

    // Parse JSON and extract the last assistant message as summary
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    
    // Try the new format first: context_window.conversation_history
    // Fall back to old format: messages
    let messages = json
        .get("context_window")
        .and_then(|cw| cw.get("conversation_history"))
        .and_then(|ch| ch.as_array())
        .or_else(|| json.get("messages").and_then(|m| m.as_array()))?;

    // Find the last assistant message
    for msg in messages.iter().rev() {
        if msg.get("role")?.as_str()? == "assistant" {
            if let Some(content) = msg.get("content") {
                if let Some(text) = content.as_str() {
                    // Return the full summary text
                    return Some(text.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_accept_flag_removes_accept() {
        let args = vec!["task".to_string(), "--accept".to_string()];
        let (filtered, found) = filter_accept_flag(&args);
        assert!(found);
        assert_eq!(filtered, vec!["task".to_string()]);
    }

    #[test]
    fn test_filter_accept_flag_no_accept() {
        let args = vec!["task".to_string(), "--verbose".to_string()];
        let (filtered, found) = filter_accept_flag(&args);
        assert!(!found);
        assert_eq!(filtered, vec!["task".to_string(), "--verbose".to_string()]);
    }

    #[test]
    fn test_filter_accept_flag_empty() {
        let args: Vec<String> = vec![];
        let (filtered, found) = filter_accept_flag(&args);
        assert!(!found);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_accept_flag_accept_in_middle() {
        let args = vec![
            "task".to_string(),
            "--accept".to_string(),
            "--other".to_string(),
        ];
        let (filtered, found) = filter_accept_flag(&args);
        assert!(found);
        assert_eq!(filtered, vec!["task".to_string(), "--other".to_string()]);
    }

    #[test]
    fn test_filter_accept_flag_only_accept() {
        let args = vec!["--accept".to_string()];
        let (filtered, found) = filter_accept_flag(&args);
        assert!(found);
        assert!(filtered.is_empty());
    }
}
