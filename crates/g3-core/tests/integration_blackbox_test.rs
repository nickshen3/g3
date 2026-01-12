//! Integration Blackbox Tests
//!
//! CHARACTERIZATION: These tests verify end-to-end behavior through stable
//! public interfaces without encoding internal implementation details.
//!
//! What these tests protect:
//! - Background process lifecycle (start, check, stop)
//! - Unified diff application with complex multi-hunk scenarios
//! - Error classification boundary behavior
//!
//! What these tests intentionally do NOT assert:
//! - Internal state or implementation details
//! - Specific error message wording (only error types)
//! - Timing-dependent behavior (uses reasonable timeouts)

use g3_core::apply_unified_diff_to_string;
use g3_core::background_process::BackgroundProcessManager;
use g3_core::error_handling::{classify_error, ErrorType, RecoverableError};
use std::fs;
use std::thread;
use std::time::Duration;

// =============================================================================
// Background Process Lifecycle Tests
// =============================================================================

mod background_process_lifecycle {
    use super::*;

    /// Test the full lifecycle: start -> check running -> kill via signal -> verify stopped
    #[test]
    fn test_start_check_stop_lifecycle() {
        let test_dir = std::env::temp_dir().join("g3_bg_lifecycle_test");
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(&test_dir).unwrap();

        // Create a long-running script
        let script_path = test_dir.join("long_running.sh");
        fs::write(
            &script_path,
            r#"#!/bin/bash
while true; do
    echo "Still running..."
    sleep 1
done
"#,
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let log_dir = test_dir.join(".g3").join("background_processes");
        let manager = BackgroundProcessManager::new(log_dir);

        // Start the process
        let info = manager
            .start("lifecycle_test", "./long_running.sh", &test_dir)
            .expect("Should start process");

        assert!(info.pid > 0, "Should have valid PID");

        // Give it time to start
        thread::sleep(Duration::from_millis(200));

        // Verify it's running
        assert!(
            manager.is_running("lifecycle_test"),
            "Process should be running after start"
        );

        // Stop the process using kill (as designed - shell tool is used for stopping)
        #[cfg(unix)]
        {
            use std::process::Command;
            let _ = Command::new("kill")
                .arg(info.pid.to_string())
                .output();
        }

        // Give it time to stop
        thread::sleep(Duration::from_millis(500));

        // Verify it's no longer running
        assert!(
            !manager.is_running("lifecycle_test"),
            "Process should not be running after kill"
        );

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
    }

    /// Test that listing processes shows running processes
    #[test]
    fn test_list_running_processes() {
        let test_dir = std::env::temp_dir().join("g3_bg_list_test");
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(&test_dir).unwrap();

        let script_path = test_dir.join("sleeper.sh");
        fs::write(
            &script_path,
            r#"#!/bin/bash
sleep 30
"#,
        )
        .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let log_dir = test_dir.join(".g3").join("background_processes");
        let manager = BackgroundProcessManager::new(log_dir);

        // Start a process
        manager
            .start("list_test_proc", "./sleeper.sh", &test_dir)
            .expect("Should start");

        thread::sleep(Duration::from_millis(100));

        // List should include our process
        let list = manager.list();
        assert!(
            list.iter().any(|p| p.name == "list_test_proc"),
            "List should include our process"
        );

        // Stop the process using kill
        if let Some(proc_info) = manager.get("list_test_proc") {
            #[cfg(unix)]
            {
                use std::process::Command;
                let _ = Command::new("kill").arg(proc_info.pid.to_string()).output();
            }
        }
        let _ = fs::remove_dir_all(&test_dir);
    }

    /// Test that stopping a non-existent process is handled gracefully
    #[test]
    fn test_stop_nonexistent_process() {
        let test_dir = std::env::temp_dir().join("g3_bg_nonexistent_test");
        let _ = fs::remove_dir_all(&test_dir);
        fs::create_dir_all(&test_dir).unwrap();

        let log_dir = test_dir.join(".g3").join("background_processes");
        let manager = BackgroundProcessManager::new(log_dir);

        // Getting a process that doesn't exist should return None
        let result = manager.get("nonexistent_process");
        assert!(result.is_none(), "Should return None for nonexistent process");
        assert!(!manager.is_running("nonexistent_process"), "Should not be running");

        let _ = fs::remove_dir_all(&test_dir);
    }
}

// =============================================================================
// Unified Diff Edge Cases
// =============================================================================

mod unified_diff_edge_cases {
    use super::*;

    /// Test applying a diff with multiple separate hunks
    #[test]
    fn test_multi_hunk_separate_locations() {
        let original = "header\n\nfunction_a() {\n  old_a\n}\n\nmiddle\n\nfunction_b() {\n  old_b\n}\n\nfooter\n";

        // Diff that modifies two separate locations
        let diff = r#"@@ -3,3 +3,3 @@
 function_a() {
-  old_a
+  new_a
 }
@@ -9,3 +9,3 @@
 function_b() {
-  old_b
+  new_b
 }
"#;

        let result = apply_unified_diff_to_string(original, diff, None, None).unwrap();

        assert!(result.contains("new_a"), "First hunk should be applied");
        assert!(result.contains("new_b"), "Second hunk should be applied");
        assert!(!result.contains("old_a"), "Old content should be replaced");
        assert!(!result.contains("old_b"), "Old content should be replaced");
    }

    /// Test diff with only additions (no deletions)
    #[test]
    fn test_diff_additions_only() {
        let original = "line 1\nline 2\nline 3\n";

        let diff = r#"@@ -1,3 +1,5 @@
 line 1
+inserted after 1
 line 2
+inserted after 2
 line 3
"#;

        let result = apply_unified_diff_to_string(original, diff, None, None).unwrap();

        assert!(result.contains("inserted after 1"));
        assert!(result.contains("inserted after 2"));
        // Original lines should still be present
        assert!(result.contains("line 1"));
        assert!(result.contains("line 2"));
        assert!(result.contains("line 3"));
    }

    /// Test diff with only deletions (no additions)
    #[test]
    fn test_diff_deletions_only() {
        let original = "keep 1\ndelete me\nkeep 2\nalso delete\nkeep 3\n";

        let diff = r#"@@ -1,5 +1,3 @@
 keep 1
-delete me
 keep 2
-also delete
 keep 3
"#;

        let result = apply_unified_diff_to_string(original, diff, None, None).unwrap();

        assert!(!result.contains("delete me"));
        assert!(!result.contains("also delete"));
        assert!(result.contains("keep 1"));
        assert!(result.contains("keep 2"));
        assert!(result.contains("keep 3"));
    }

    /// Test diff with CRLF line endings (should be normalized)
    #[test]
    fn test_diff_crlf_normalization() {
        let original = "line 1\r\nold line\r\nline 3\r\n";

        let diff = "@@ -1,3 +1,3 @@\n line 1\n-old line\n+new line\n line 3\n";

        let result = apply_unified_diff_to_string(original, diff, None, None).unwrap();

        assert!(result.contains("new line"));
        assert!(!result.contains("old line"));
    }

    /// Test diff with empty context (minimal diff)
    #[test]
    fn test_minimal_diff_no_context() {
        let original = "unchanged\nold\nunchanged\n";

        // Minimal diff without context lines
        let diff = "-old\n+new\n";

        let result = apply_unified_diff_to_string(original, diff, None, None).unwrap();

        assert!(result.contains("new"));
        assert!(!result.contains("\nold\n"));
    }

    /// Test that invalid diff format returns an error
    #[test]
    fn test_invalid_diff_format_error() {
        let original = "some content\n";
        let invalid_diff = "this is not a valid diff format";

        let result = apply_unified_diff_to_string(original, invalid_diff, None, None);

        assert!(result.is_err(), "Invalid diff should return error");
    }

    /// Test diff with range constraint
    #[test]
    fn test_diff_with_range_constraint() {
        let original = "A\nold\nB\nold\nC\n";

        // This diff should only apply to the first "old" due to range
        let diff = "@@ -1,3 +1,3 @@\n A\n-old\n+NEW\n B\n";

        // Range covers only the first part of the file
        let end_pos = original.find("B\n").unwrap() + 2;
        let result = apply_unified_diff_to_string(original, diff, Some(0), Some(end_pos)).unwrap();

        // First "old" should be replaced
        assert!(result.starts_with("A\nNEW\nB"));
        // Second "old" should remain unchanged
        assert!(result.contains("\nold\nC"));
    }

    /// Test diff pattern not found error
    #[test]
    fn test_diff_pattern_not_found() {
        let original = "actual content\n";
        let diff = "@@ -1,1 +1,1 @@\n-nonexistent pattern\n+replacement\n";

        let result = apply_unified_diff_to_string(original, diff, None, None);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found") || err_msg.contains("Pattern"),
            "Error should indicate pattern not found: {}",
            err_msg
        );
    }
}

// =============================================================================
// Error Classification Boundary Tests
// =============================================================================

mod error_classification_boundaries {
    use super::*;

    /// Test that various rate limit error formats are correctly classified
    #[test]
    fn test_rate_limit_variations() {
        let variations = vec![
            "Rate limit exceeded",
            "rate_limit_exceeded",
            "HTTP 429 Too Many Requests",
            "Error 429: rate limited",
            "API rate limit hit",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::Recoverable(RecoverableError::RateLimit),
                "Should classify '{}' as RateLimit",
                msg
            );
        }
    }

    /// Test that various server error formats are correctly classified
    #[test]
    fn test_server_error_variations() {
        let variations = vec![
            "HTTP 500 Internal Server Error",
            "502 Bad Gateway",
            "503 Service Unavailable",
            "504 Gateway Timeout",
            "Server error occurred",
            "Internal error: database unavailable",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::Recoverable(RecoverableError::ServerError),
                "Should classify '{}' as ServerError",
                msg
            );
        }
    }

    /// Test that timeout errors are correctly classified
    #[test]
    fn test_timeout_variations() {
        let variations = vec![
            "Request timed out",
            "Request timeout",
            "Timed out waiting for server",
            "Operation timed out after 30s",
            "Timeout waiting for response",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::Recoverable(RecoverableError::Timeout),
                "Should classify '{}' as Timeout",
                msg
            );
        }
    }

    /// Test that network errors are correctly classified
    #[test]
    fn test_network_error_variations() {
        let variations = vec![
            "Network connection failed",
            "DNS resolution failed",
            "Connection refused",
            "Network unreachable",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::Recoverable(RecoverableError::NetworkError),
                "Should classify '{}' as NetworkError",
                msg
            );
        }
    }

    /// Test that context length exceeded is correctly classified
    #[test]
    fn test_context_length_exceeded_variations() {
        let variations = vec![
            "HTTP 400 Bad Request: context length exceeded",
            "Error 400: prompt is too long",
            "Bad request: maximum context length exceeded",
            "400: context_length_exceeded",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::Recoverable(RecoverableError::ContextLengthExceeded),
                "Should classify '{}' as ContextLengthExceeded",
                msg
            );
        }
    }

    /// Test that non-recoverable errors are correctly classified
    #[test]
    fn test_non_recoverable_errors() {
        let variations = vec![
            "Invalid API key",
            "Authentication failed",
            "Malformed request body",
            "Unknown error occurred",
            "Permission denied",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::NonRecoverable,
                "Should classify '{}' as NonRecoverable",
                msg
            );
        }
    }

    /// Test model busy/overloaded classification
    #[test]
    fn test_model_busy_variations() {
        let variations = vec![
            "Model is busy",
            "Server overloaded",
            "At capacity, please retry",
            "Service temporarily unavailable",
        ];

        for msg in variations {
            let error = anyhow::anyhow!("{}", msg);
            let classification = classify_error(&error);
            assert_eq!(
                classification,
                ErrorType::Recoverable(RecoverableError::ModelBusy),
                "Should classify '{}' as ModelBusy",
                msg
            );
        }
    }
}
