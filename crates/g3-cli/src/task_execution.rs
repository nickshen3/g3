//! Task execution with retry logic for G3 CLI.

use g3_core::error_handling::{classify_error, ErrorType, RecoverableError};
use g3_core::ui_writer::UiWriter;
use g3_core::Agent;
use tokio_util::sync::CancellationToken;
use tracing::error;

use crate::simple_output::SimpleOutput;

/// Maximum number of retry attempts for timeout errors
const MAX_TIMEOUT_RETRIES: u32 = 3;

/// Output mode for task execution feedback
pub enum OutputMode<'a> {
    /// Console mode with SimpleOutput for user-friendly messages
    Console(&'a SimpleOutput),
    /// Machine mode with structured output markers
    Machine,
}

impl<'a> OutputMode<'a> {
    fn print_thinking(&self) {
        match self {
            OutputMode::Console(output) => output.print("🤔 Thinking..."),
            OutputMode::Machine => {} // No thinking indicator in machine mode
        }
    }

    fn print_cancelled(&self) {
        match self {
            OutputMode::Console(output) => output.print("\n⚠️  Operation cancelled by user (Ctrl+C)"),
            OutputMode::Machine => println!("CANCELLED"),
        }
    }

    fn print_cancelled_simple(&self) {
        match self {
            OutputMode::Console(output) => output.print("⚠️  Operation cancelled by user"),
            OutputMode::Machine => println!("CANCELLED"),
        }
    }

    fn print_retry_success(&self, attempt: u32) {
        match self {
            OutputMode::Console(output) => {
                output.print(&format!("✅ Request succeeded after {} attempts", attempt))
            }
            OutputMode::Machine => println!("RETRY_SUCCESS: attempt {}", attempt),
        }
    }

    fn print_response(&self, response: &str, output: Option<&SimpleOutput>) {
        match self {
            OutputMode::Console(o) => o.print_smart(response),
            OutputMode::Machine => {
                println!("AGENT_RESPONSE:");
                println!("{}", response);
                println!("END_AGENT_RESPONSE");
                // Ignore the output parameter in machine mode
                let _ = output;
            }
        }
    }

    fn print_timeout_retry(&self, attempt: u32, delay: std::time::Duration) {
        match self {
            OutputMode::Console(output) => {
                output.print(&format!(
                    "⏱️  Timeout error detected (attempt {}/{}). Retrying in {:?}...",
                    attempt, MAX_TIMEOUT_RETRIES, delay
                ));
            }
            OutputMode::Machine => {
                println!(
                    "TIMEOUT: attempt {} of {}, retrying in {:?}",
                    attempt, MAX_TIMEOUT_RETRIES, delay
                );
            }
        }
    }

    fn print_error(&self, e: &anyhow::Error, attempt: u32) {
        match self {
            OutputMode::Console(_) => {} // Handled by handle_execution_error
            OutputMode::Machine => {
                println!("ERROR: {}", e);
                if attempt > 1 {
                    println!("FAILED_AFTER_RETRIES: {}", attempt);
                }
            }
        }
    }
}

/// Execute a task with retry logic for timeout errors.
///
/// This is the unified implementation used by both console and machine modes.
pub async fn execute_task_with_retry<W: UiWriter>(
    agent: &mut Agent<W>,
    input: &str,
    show_prompt: bool,
    show_code: bool,
    mode: OutputMode<'_>,
) {
    let mut attempt = 0;

    mode.print_thinking();

    // Create cancellation token for this request
    let cancellation_token = CancellationToken::new();
    let cancel_token_clone = cancellation_token.clone();

    loop {
        attempt += 1;

        // Execute task with cancellation support
        let execution_result = tokio::select! {
            result = agent.execute_task_with_timing_cancellable(
                input, None, false, show_prompt, show_code, true, cancellation_token.clone(), None
            ) => {
                result
            }
            _ = tokio::signal::ctrl_c() => {
                cancel_token_clone.cancel();
                mode.print_cancelled();
                return;
            }
        };

        match execution_result {
            Ok(result) => {
                if attempt > 1 {
                    mode.print_retry_success(attempt);
                }
                mode.print_response(&result.response, match &mode {
                    OutputMode::Console(o) => Some(*o),
                    OutputMode::Machine => None,
                });
                return;
            }
            Err(e) => {
                if e.to_string().contains("cancelled") {
                    mode.print_cancelled_simple();
                    return;
                }

                // Check if this is a timeout error that we should retry
                let error_type = classify_error(&e);

                if matches!(
                    error_type,
                    ErrorType::Recoverable(RecoverableError::Timeout)
                ) && attempt < MAX_TIMEOUT_RETRIES
                {
                    // Calculate retry delay with exponential backoff
                    let delay_ms = 1000 * (2_u64.pow(attempt - 1));
                    let delay = std::time::Duration::from_millis(delay_ms);

                    mode.print_timeout_retry(attempt, delay);

                    // Wait before retrying
                    tokio::time::sleep(delay).await;
                    continue;
                }

                // For non-timeout errors or after max retries
                match &mode {
                    OutputMode::Console(output) => {
                        handle_execution_error(&e, input, output, attempt);
                    }
                    OutputMode::Machine => {
                        mode.print_error(&e, attempt);
                    }
                }
                return;
            }
        }
    }
}

/// Handle execution errors with detailed logging and user-friendly output.
pub fn handle_execution_error(e: &anyhow::Error, input: &str, output: &SimpleOutput, attempt: u32) {
    // Enhanced error logging with detailed information
    error!("=== TASK EXECUTION ERROR ===");
    error!("Error: {}", e);
    if attempt > 1 {
        error!("Failed after {} attempts", attempt);
    }

    // Log error chain
    let mut source = e.source();
    let mut depth = 1;
    while let Some(err) = source {
        error!("  Caused by [{}]: {}", depth, err);
        source = err.source();
        depth += 1;
    }

    // Log additional context
    error!("Task input: {}", input);
    error!("Error type: {}", std::any::type_name_of_val(&e));

    // Display user-friendly error message
    output.print(&format!("❌ Error: {}", e));

    // If it's a stream error, provide helpful guidance
    if e.to_string().contains("No response received") || e.to_string().contains("timed out") {
        output.print("💡 This may be a temporary issue. Please try again or check the logs for more details.");
        output.print("   Log files are saved in the '.g3/sessions/' directory.");
    }
}
