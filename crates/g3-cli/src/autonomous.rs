//! Autonomous mode for G3 CLI - coach-player feedback loop.

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Instant;
use tracing::debug;

use g3_core::error_handling::{classify_error, ErrorType, RecoverableError};
use g3_core::project::Project;
use g3_core::{Agent, DiscoveryOptions};

use crate::coach_feedback;
use crate::metrics::{format_elapsed_time, generate_turn_histogram, TurnMetrics};
use crate::simple_output::SimpleOutput;
use crate::ui_writer_impl::ConsoleUiWriter;
use g3_core::ui_writer::UiWriter;

/// Run autonomous mode with coach-player feedback loop (console output).
pub async fn run_autonomous(
    mut agent: Agent<ConsoleUiWriter>,
    project: Project,
    show_prompt: bool,
    show_code: bool,
    max_turns: usize,
    quiet: bool,
    codebase_fast_start: Option<PathBuf>,
) -> Result<Agent<ConsoleUiWriter>> {
    let start_time = std::time::Instant::now();
    let output = SimpleOutput::new();
    let mut turn_metrics: Vec<TurnMetrics> = Vec::new();

    output.print("g3 programming agent - autonomous mode");
    output.print(&format!(
        "📁 Using workspace: {}",
        project.workspace().display()
    ));

    // Check if requirements exist
    if !project.has_requirements() {
        print_no_requirements_error(&output, &agent, &turn_metrics, start_time, max_turns);
        return Ok(agent);
    }

    // Read requirements
    let requirements = match project.read_requirements()? {
        Some(content) => content,
        None => {
            print_cannot_read_requirements_error(
                &output,
                &agent,
                &turn_metrics,
                start_time,
                max_turns,
            );
            return Ok(agent);
        }
    };

    // Display appropriate message based on requirements source
    if project.requirements_text.is_some() {
        output.print("📋 Requirements loaded from --requirements flag");
    } else {
        output.print("📋 Requirements loaded from requirements.md");
    }

    // Calculate SHA256 of requirements
    let mut hasher = Sha256::new();
    hasher.update(requirements.as_bytes());
    let requirements_sha = hex::encode(hasher.finalize());

    output.print(&format!("🔒 Requirements SHA256: {}", requirements_sha));

    // Pass SHA to agent for staleness checking
    agent.set_requirements_sha(requirements_sha.clone());

    let loop_start = Instant::now();
    output.print("🔄 Starting coach-player feedback loop...");

    // Load fast-discovery messages before the loop starts (if enabled)
    let (discovery_messages, discovery_working_dir) =
        load_discovery_messages(&agent, &output, &codebase_fast_start, &requirements).await;
    let has_discovery = !discovery_messages.is_empty();

    let mut turn = 1;
    let mut coach_feedback_text = String::new();
    let mut implementation_approved = false;

    loop {
        let turn_start_time = Instant::now();
        let turn_start_tokens = agent.get_context_window().used_tokens;

        output.print(&format!(
            "\n=== TURN {}/{} - PLAYER MODE ===",
            turn, max_turns
        ));

        // Surface provider info for player agent
        agent.print_provider_banner("Player");

        // Player mode: implement requirements (with coach feedback if available)
        let player_prompt = build_player_prompt(&requirements, &requirements_sha, &coach_feedback_text);

        output.print(&format!(
            "🎯 Starting player implementation... (elapsed: {})",
            format_elapsed_time(loop_start.elapsed())
        ));

        // Display what feedback the player is receiving
        if coach_feedback_text.is_empty() {
            if turn > 1 {
                return Err(anyhow::anyhow!(
                    "Player mode error: No coach feedback received on turn {}",
                    turn
                ));
            }
            output.print("📋 Player starting initial implementation (no prior coach feedback)");
        } else {
            output.print(&format!(
                "📋 Player received coach feedback ({} chars):",
                coach_feedback_text.len()
            ));
            output.print(&coach_feedback_text);
        }
        output.print(""); // Empty line for readability

        // Execute player task with retry on error
        let player_result = execute_player_turn(
            &mut agent,
            &player_prompt,
            show_prompt,
            show_code,
            &output,
            has_discovery,
            &discovery_messages,
            discovery_working_dir.as_deref(),
            turn,
            &turn_metrics,
            start_time,
            max_turns,
        )
        .await;

        let player_failed = match player_result {
            PlayerTurnResult::Success => false,
            PlayerTurnResult::Failed => true,
            PlayerTurnResult::Panic(e) => return Err(e),
        };

        // If player failed after max retries, increment turn and continue
        if player_failed {
            output.print(&format!(
                "⚠️ Player turn {} failed after max retries. Moving to next turn.",
                turn
            ));
            record_turn_metrics(
                &mut turn_metrics,
                turn,
                turn_start_time,
                turn_start_tokens,
                &agent,
            );
            turn += 1;

            if turn > max_turns {
                output.print("\n=== SESSION COMPLETED - MAX TURNS REACHED ===");
                output.print(&format!("⏰ Maximum turns ({}) reached", max_turns));
                break;
            }

            coach_feedback_text = String::new();
            continue;
        }

        // Give some time for file operations to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Execute coach turn
        let coach_result = execute_coach_turn(
            &agent,
            &project,
            &requirements,
            show_prompt,
            show_code,
            quiet,
            &output,
            has_discovery,
            &discovery_messages,
            discovery_working_dir.as_deref(),
            turn,
            max_turns,
            &turn_metrics,
            start_time,
            loop_start,
        )
        .await;

        match coach_result {
            CoachTurnResult::Approved => {
                output.print("\n=== SESSION COMPLETED - IMPLEMENTATION APPROVED ===");
                output.print("✅ Coach approved the implementation!");
                implementation_approved = true;
                break;
            }
            CoachTurnResult::Feedback(feedback) => {
                output.print_smart(&format!("Coach feedback:\n{}", feedback));
                coach_feedback_text = feedback;
            }
            CoachTurnResult::Failed => {
                output.print(&format!(
                    "⚠️ Coach turn {} failed after max retries. Using default feedback.",
                    turn
                ));
                coach_feedback_text = "The implementation needs review. Please ensure all requirements are met and the code compiles without errors.".to_string();
            }
            CoachTurnResult::Panic(e) => return Err(e),
        }

        // Check if we've reached max turns
        if turn >= max_turns {
            output.print("\n=== SESSION COMPLETED - MAX TURNS REACHED ===");
            output.print(&format!("⏰ Maximum turns ({}) reached", max_turns));
            break;
        }

        record_turn_metrics(
            &mut turn_metrics,
            turn,
            turn_start_time,
            turn_start_tokens,
            &agent,
        );
        turn += 1;

        output.print("🔄 Coach provided feedback for next iteration");
    }

    // Generate final report
    print_final_report(
        &output,
        &agent,
        &turn_metrics,
        start_time,
        turn,
        max_turns,
        implementation_approved,
    );

    if implementation_approved {
        output.print(&format!(
            "\n🎉 Autonomous mode completed successfully (total loop time: {})",
            format_elapsed_time(loop_start.elapsed())
        ));
    } else {
        output.print(&format!(
            "\n🔄 Autonomous mode terminated (max iterations) (total loop time: {})",
            format_elapsed_time(loop_start.elapsed())
        ));
    }

    // Save session continuation for resume capability
    agent.save_session_continuation(None);

    Ok(agent)
}

// --- Helper types and functions ---

enum PlayerTurnResult {
    Success,
    Failed,
    Panic(anyhow::Error),
}

enum CoachTurnResult {
    Approved,
    Feedback(String),
    Failed,
    Panic(anyhow::Error),
}

fn build_player_prompt(requirements: &str, requirements_sha: &str, coach_feedback: &str) -> String {
    if coach_feedback.is_empty() {
        format!(
            "You are G3 in implementation mode. Read and implement the following requirements:\n\n{}\n\nRequirements SHA256: {}\n\nImplement this step by step, creating all necessary files and code.",
            requirements, requirements_sha
        )
    } else {
        format!(
            "You are G3 in implementation mode. Address the following specific feedback from the coach:\n\n{}\n\nContext: You are improving an implementation based on these requirements:\n{}\n\nFocus on fixing the issues mentioned in the coach feedback above.",
            coach_feedback, requirements
        )
    }
}

fn build_coach_prompt(requirements: &str) -> String {
    format!(
        "You are G3 in coach mode. Your role is to critique and review implementations against requirements and provide concise, actionable feedback.

REQUIREMENTS:
{}

IMPLEMENTATION REVIEW:
Review the current state of the project and provide a concise critique focusing on:
1. Whether the requirements are correctly implemented
2. Whether the project compiles successfully
3. What requirements are missing or incorrect
4. Specific improvements needed to satisfy requirements
5. Use UI tools such as webdriver to test functionality thoroughly

CRITICAL INSTRUCTIONS:
1. Provide your feedback as your final response message
2. Your feedback should be CONCISE and ACTIONABLE
3. Focus ONLY on what needs to be fixed or improved
4. Do NOT include your analysis process, file contents, or compilation output in your final feedback

If the implementation thoroughly meets all requirements, compiles and is fully tested (especially UI flows) *WITHOUT* minor gaps or errors:
- Respond with: 'IMPLEMENTATION_APPROVED'

If improvements are needed:
- Respond with a brief summary listing ONLY the specific issues to fix

Remember: Be clear in your review and concise in your feedback. APPROVE iff the implementation works and thoroughly fits the requirements (implementation > 95% complete). Be rigorous, especially by testing that all UI features work.",
        requirements
    )
}

async fn load_discovery_messages(
    agent: &Agent<ConsoleUiWriter>,
    output: &SimpleOutput,
    codebase_fast_start: &Option<PathBuf>,
    requirements: &str,
) -> (Vec<g3_providers::Message>, Option<String>) {
    if let Some(ref codebase_path) = codebase_fast_start {
        let canonical_path = codebase_path
            .canonicalize()
            .unwrap_or_else(|_| codebase_path.clone());
        let path_str = canonical_path.to_string_lossy();
        output.print(&format!(
            "🔍 Fast-discovery mode: will explore codebase at {}",
            path_str
        ));

        match agent.get_provider() {
            Ok(provider) => {
                let output_clone = output.clone();
                let status_callback: g3_planner::StatusCallback = Box::new(move |msg: &str| {
                    output_clone.print(msg);
                });
                match g3_planner::get_initial_discovery_messages(
                    &path_str,
                    Some(requirements),
                    provider,
                    Some(&status_callback),
                )
                .await
                {
                    Ok(messages) => (messages, Some(path_str.to_string())),
                    Err(e) => {
                        output.print(&format!(
                            "⚠️ LLM discovery failed: {}, skipping fast-start",
                            e
                        ));
                        (Vec::new(), None)
                    }
                }
            }
            Err(e) => {
                output.print(&format!(
                    "⚠️ Could not get provider: {}, skipping fast-start",
                    e
                ));
                (Vec::new(), None)
            }
        }
    } else {
        (Vec::new(), None)
    }
}

async fn execute_player_turn(
    agent: &mut Agent<ConsoleUiWriter>,
    player_prompt: &str,
    show_prompt: bool,
    show_code: bool,
    output: &SimpleOutput,
    has_discovery: bool,
    discovery_messages: &[g3_providers::Message],
    discovery_working_dir: Option<&str>,
    turn: usize,
    turn_metrics: &[TurnMetrics],
    start_time: Instant,
    max_turns: usize,
) -> PlayerTurnResult {
    const MAX_PLAYER_RETRIES: u32 = 3;
    let mut retry_count = 0;

    loop {
        let discovery_opts = if has_discovery {
            Some(DiscoveryOptions {
                messages: discovery_messages,
                fast_start_path: discovery_working_dir,
            })
        } else {
            None
        };

        match agent
            .execute_task_with_timing(
                player_prompt,
                None,
                false,
                show_prompt,
                show_code,
                true,
                discovery_opts,
            )
            .await
        {
            Ok(result) => {
                output.print("📝 Player implementation completed:");
                // Only print response if it's not empty (streaming already displayed it)
                if !result.response.trim().is_empty() {
                    output.print_smart(&result.response);
                }
                return PlayerTurnResult::Success;
            }
            Err(e) => {
                let error_type = classify_error(&e);

                if matches!(
                    error_type,
                    ErrorType::Recoverable(RecoverableError::ContextLengthExceeded)
                ) {
                    output.print(&format!("⚠️ Context length exceeded in player turn: {}", e));
                    output.print("📝 Logging error to session and ending current turn...");

                    let forensic_context = format!(
                        "Turn: {}\nRole: Player\nContext tokens: {}\nTotal available: {}\nPercentage used: {:.1}%\nPrompt length: {} chars\nError occurred at: {}",
                        turn,
                        agent.get_context_window().used_tokens,
                        agent.get_context_window().total_tokens,
                        agent.get_context_window().percentage_used(),
                        player_prompt.len(),
                        chrono::Utc::now().to_rfc3339()
                    );

                    agent.log_error_to_session(&e, "assistant", Some(forensic_context));
                    return PlayerTurnResult::Failed;
                } else if e.to_string().contains("panic") {
                    output.print(&format!("💥 Player panic detected: {}", e));
                    print_panic_report(output, agent, turn_metrics, start_time, turn, max_turns, "PLAYER PANIC");
                    return PlayerTurnResult::Panic(e);
                }

                retry_count += 1;
                output.print(&format!(
                    "⚠️ Player error (attempt {}/{}): {}",
                    retry_count, MAX_PLAYER_RETRIES, e
                ));

                if retry_count >= MAX_PLAYER_RETRIES {
                    output.print("🔄 Max retries reached for player, marking turn as failed...");
                    return PlayerTurnResult::Failed;
                }
                output.print("🔄 Retrying player implementation...");
            }
        }
    }
}

async fn execute_coach_turn(
    player_agent: &Agent<ConsoleUiWriter>,
    project: &Project,
    requirements: &str,
    show_prompt: bool,
    show_code: bool,
    quiet: bool,
    output: &SimpleOutput,
    has_discovery: bool,
    discovery_messages: &[g3_providers::Message],
    discovery_working_dir: Option<&str>,
    turn: usize,
    max_turns: usize,
    turn_metrics: &[TurnMetrics],
    start_time: Instant,
    loop_start: Instant,
) -> CoachTurnResult {
    const MAX_COACH_RETRIES: u32 = 3;

    // Create a new agent instance for coach mode to ensure fresh context
    let base_config = player_agent.get_config().clone();
    let coach_config = match base_config.for_coach() {
        Ok(c) => c,
        Err(e) => return CoachTurnResult::Panic(e),
    };

    // Reset filter suppression state before creating coach agent
    crate::filter_json::reset_json_tool_state();

    let ui_writer = ConsoleUiWriter::new();
    ui_writer.set_workspace_path(project.workspace().to_path_buf());
    let mut coach_agent =
        match Agent::new_autonomous_with_project_context_and_quiet(coach_config, ui_writer, None, quiet)
            .await
        {
            Ok(a) => a,
            Err(e) => return CoachTurnResult::Panic(e),
        };

    coach_agent.print_provider_banner("Coach");

    if let Err(e) = project.enter_workspace() {
        return CoachTurnResult::Panic(e);
    }

    output.print(&format!(
        "\n=== TURN {}/{} - COACH MODE ===",
        turn, max_turns
    ));

    let coach_prompt = build_coach_prompt(requirements);

    output.print(&format!(
        "🎓 Starting coach review... (elapsed: {})",
        format_elapsed_time(loop_start.elapsed())
    ));

    let mut retry_count = 0;

    loop {
        let discovery_opts = if has_discovery {
            Some(DiscoveryOptions {
                messages: discovery_messages,
                fast_start_path: discovery_working_dir,
            })
        } else {
            None
        };

        match coach_agent
            .execute_task_with_timing(
                &coach_prompt,
                None,
                false,
                show_prompt,
                show_code,
                true,
                discovery_opts,
            )
            .await
        {
            Ok(result) => {
                output.print("🎓 Coach review completed");

                let feedback_text =
                    match coach_feedback::extract_from_logs(&result, &coach_agent, output) {
                        Ok(f) => f,
                        Err(e) => return CoachTurnResult::Panic(e),
                    };

                debug!(
                    "Coach feedback extracted: {} characters (from {} total)",
                    feedback_text.len(),
                    result.response.len()
                );

                if feedback_text.is_empty() {
                    output.print("⚠️ Coach did not provide feedback. This may be a model issue.");
                    return CoachTurnResult::Failed;
                }

                if result.is_approved() || feedback_text.contains("IMPLEMENTATION_APPROVED") {
                    return CoachTurnResult::Approved;
                }

                return CoachTurnResult::Feedback(feedback_text);
            }
            Err(e) => {
                let error_type = classify_error(&e);

                if matches!(
                    error_type,
                    ErrorType::Recoverable(RecoverableError::ContextLengthExceeded)
                ) {
                    output.print(&format!("⚠️ Context length exceeded in coach turn: {}", e));
                    output.print("📝 Logging error to session and ending current turn...");

                    let forensic_context = format!(
                        "Turn: {}\nRole: Coach\nContext tokens: {}\nTotal available: {}\nPercentage used: {:.1}%\nPrompt length: {} chars\nError occurred at: {}",
                        turn,
                        coach_agent.get_context_window().used_tokens,
                        coach_agent.get_context_window().total_tokens,
                        coach_agent.get_context_window().percentage_used(),
                        coach_prompt.len(),
                        chrono::Utc::now().to_rfc3339()
                    );

                    coach_agent.log_error_to_session(&e, "assistant", Some(forensic_context));
                    return CoachTurnResult::Failed;
                } else if e.to_string().contains("panic") {
                    output.print(&format!("💥 Coach panic detected: {}", e));
                    print_panic_report(output, player_agent, turn_metrics, start_time, turn, max_turns, "COACH PANIC");
                    return CoachTurnResult::Panic(e);
                }

                retry_count += 1;
                output.print(&format!(
                    "⚠️ Coach error (attempt {}/{}): {}",
                    retry_count, MAX_COACH_RETRIES, e
                ));

                if retry_count >= MAX_COACH_RETRIES {
                    output.print("🔄 Max retries reached for coach, using default feedback...");
                    return CoachTurnResult::Failed;
                }
                output.print("🔄 Retrying coach review...");
            }
        }
    }
}

fn record_turn_metrics(
    turn_metrics: &mut Vec<TurnMetrics>,
    turn: usize,
    turn_start_time: Instant,
    turn_start_tokens: u32,
    agent: &Agent<ConsoleUiWriter>,
) {
    let turn_duration = turn_start_time.elapsed();
    let turn_tokens = agent
        .get_context_window()
        .used_tokens
        .saturating_sub(turn_start_tokens);
    turn_metrics.push(TurnMetrics {
        turn_number: turn,
        tokens_used: turn_tokens,
        wall_clock_time: turn_duration,
    });
}

fn print_no_requirements_error(
    output: &SimpleOutput,
    agent: &Agent<ConsoleUiWriter>,
    turn_metrics: &[TurnMetrics],
    start_time: Instant,
    max_turns: usize,
) {
    output.print("❌ Error: requirements.md not found in workspace directory");
    output.print("   Please either:");
    output.print("   1. Create a requirements.md file with your project requirements");
    output.print("   2. Or use the --requirements flag to provide requirements text directly:");
    output.print("      g3 --autonomous --requirements \"Your requirements here\"");
    output.print("");

    print_final_report(output, agent, turn_metrics, start_time, 0, max_turns, false);
}

fn print_cannot_read_requirements_error(
    output: &SimpleOutput,
    agent: &Agent<ConsoleUiWriter>,
    turn_metrics: &[TurnMetrics],
    start_time: Instant,
    max_turns: usize,
) {
    output.print("❌ Error: Could not read requirements (neither --requirements flag nor requirements.md file provided)");
    print_final_report(output, agent, turn_metrics, start_time, 0, max_turns, false);
}

fn print_panic_report(
    output: &SimpleOutput,
    agent: &Agent<ConsoleUiWriter>,
    turn_metrics: &[TurnMetrics],
    start_time: Instant,
    turn: usize,
    max_turns: usize,
    status: &str,
) {
    let elapsed = start_time.elapsed();
    let context_window = agent.get_context_window();

    output.print(&format!("\n{}", "=".repeat(60)));
    output.print("📊 AUTONOMOUS MODE SESSION REPORT");
    output.print(&"=".repeat(60));

    output.print(&format!("⏱️  Total Duration: {:.2}s", elapsed.as_secs_f64()));
    output.print(&format!("🔄 Turns Taken: {}/{}", turn, max_turns));
    output.print(&format!("📝 Final Status: 💥 {}", status));

    output.print("\n📈 Token Usage Statistics:");
    output.print(&format!("   • Used Tokens: {}", context_window.used_tokens));
    output.print(&format!("   • Total Available: {}", context_window.total_tokens));
    output.print(&format!("   • Cumulative Tokens: {}", context_window.cumulative_tokens));
    output.print(&format!("   • Usage Percentage: {:.1}%", context_window.percentage_used()));
    output.print(&generate_turn_histogram(turn_metrics));
    output.print(&"=".repeat(60));
}

fn print_final_report(
    output: &SimpleOutput,
    agent: &Agent<ConsoleUiWriter>,
    turn_metrics: &[TurnMetrics],
    start_time: Instant,
    turn: usize,
    max_turns: usize,
    implementation_approved: bool,
) {
    let elapsed = start_time.elapsed();
    let context_window = agent.get_context_window();

    output.print(&format!("\n{}", "=".repeat(60)));
    output.print("📊 AUTONOMOUS MODE SESSION REPORT");
    output.print(&"=".repeat(60));

    output.print(&format!("⏱️  Total Duration: {:.2}s", elapsed.as_secs_f64()));
    output.print(&format!("🔄 Turns Taken: {}/{}", turn, max_turns));
    output.print(&format!(
        "📝 Final Status: {}",
        if implementation_approved {
            "✅ APPROVED"
        } else if turn >= max_turns {
            "⏰ MAX TURNS REACHED"
        } else {
            "⚠️ INCOMPLETE"
        }
    ));

    output.print("\n📈 Token Usage Statistics:");
    output.print(&format!("   • Used Tokens: {}", context_window.used_tokens));
    output.print(&format!("   • Total Available: {}", context_window.total_tokens));
    output.print(&format!("   • Cumulative Tokens: {}", context_window.cumulative_tokens));
    output.print(&format!("   • Usage Percentage: {:.1}%", context_window.percentage_used()));
    output.print(&generate_turn_histogram(turn_metrics));
    output.print(&"=".repeat(60));
}
