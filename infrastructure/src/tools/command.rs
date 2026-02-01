//! Command execution tool: run_command

use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Tool name constant
pub const RUN_COMMAND: &str = "run_command";

/// Default timeout for command execution (60 seconds)
const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// Maximum output size (1 MB)
const MAX_OUTPUT_SIZE: usize = 1024 * 1024;

/// Get the tool definition for run_command
pub fn run_command_definition() -> ToolDefinition {
    ToolDefinition::new(
        RUN_COMMAND,
        "Execute a shell command and return its output. Use with caution.",
        RiskLevel::High,
    )
    .with_parameter(
        ToolParameter::new("command", "The command to execute", true).with_type("string"),
    )
    .with_parameter(
        ToolParameter::new("working_dir", "Working directory for the command", false)
            .with_type("path"),
    )
    .with_parameter(
        ToolParameter::new("timeout_secs", "Timeout in seconds (default: 60)", false)
            .with_type("number"),
    )
}

/// Execute the run_command tool
pub fn execute_run_command(call: &ToolCall) -> ToolResult {
    let start = Instant::now();

    // Get the command argument
    let command_str = match call.require_string("command") {
        Ok(c) => c,
        Err(e) => return ToolResult::failure(RUN_COMMAND, ToolError::invalid_argument(e)),
    };

    // Get optional working directory
    let working_dir = call.get_string("working_dir");

    // Get timeout
    let timeout_secs = call
        .get_i64("timeout_secs")
        .unwrap_or(DEFAULT_TIMEOUT_SECS as i64) as u64;

    // Build the command
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("cmd");
        c.args(["/C", command_str]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", command_str]);
        c
    };

    // Set working directory if specified
    if let Some(dir) = working_dir {
        let path = std::path::Path::new(dir);
        if !path.exists() {
            return ToolResult::failure(
                RUN_COMMAND,
                ToolError::not_found(format!("Working directory does not exist: {}", dir)),
            );
        }
        if !path.is_dir() {
            return ToolResult::failure(
                RUN_COMMAND,
                ToolError::invalid_argument(format!("'{}' is not a directory", dir)),
            );
        }
        cmd.current_dir(path);
    }

    // Configure stdio
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    // Spawn the process
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return ToolResult::failure(
                RUN_COMMAND,
                ToolError::execution_failed(format!("Failed to spawn command: {}", e)),
            )
        }
    };

    // Wait for the command with timeout
    let output = match wait_with_timeout(child, Duration::from_secs(timeout_secs)) {
        Ok(o) => o,
        Err(e) => {
            return ToolResult::failure(
                RUN_COMMAND,
                ToolError::timeout(format!(
                    "Command timed out after {} seconds: {}",
                    timeout_secs, e
                )),
            )
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let exit_code = output.status.code().unwrap_or(-1);

    // Combine stdout and stderr
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut combined_output = String::new();
    if !stdout.is_empty() {
        combined_output.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !combined_output.is_empty() {
            combined_output.push_str("\n--- stderr ---\n");
        }
        combined_output.push_str(&stderr);
    }

    // Truncate if too large
    if combined_output.len() > MAX_OUTPUT_SIZE {
        combined_output.truncate(MAX_OUTPUT_SIZE);
        combined_output.push_str("\n... (output truncated)");
    }

    let bytes = combined_output.len();

    // Return success even if exit code is non-zero (let the agent decide what to do)
    let metadata = ToolResultMetadata {
        duration_ms: Some(duration_ms),
        bytes: Some(bytes),
        exit_code: Some(exit_code),
        ..Default::default()
    };

    if output.status.success() {
        ToolResult::success(RUN_COMMAND, combined_output).with_metadata(metadata)
    } else {
        // Still return success from tool perspective, but include exit code
        ToolResult::success(
            RUN_COMMAND,
            format!(
                "Command exited with code {}\n{}",
                exit_code, combined_output
            ),
        )
        .with_metadata(metadata)
    }
}

/// Wait for a child process with timeout
fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    // Simple implementation: try to wait and kill if timeout
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process has exited
                let stdout = child
                    .stdout
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                let stderr = child
                    .stderr
                    .take()
                    .map(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok();
                        buf
                    })
                    .unwrap_or_default();

                return Ok(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                // Process still running
                if start.elapsed() > timeout {
                    // Kill the process
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Command timed out".to_string());
                }
                // Sleep a bit before checking again
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(format!("Failed to wait for process: {}", e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_command_echo() {
        let call = ToolCall::new(RUN_COMMAND).with_arg("command", "echo hello");
        let result = execute_run_command(&call);

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("hello"));
    }

    #[test]
    fn test_run_command_with_working_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let dir_path = temp_dir.path().to_str().unwrap();

        let call = ToolCall::new(RUN_COMMAND)
            .with_arg("command", "pwd")
            .with_arg("working_dir", dir_path);
        let result = execute_run_command(&call);

        assert!(result.is_success());
        // The output should contain the temp dir path
        let output = result.output().unwrap();
        // Normalize paths for comparison
        assert!(output.contains(temp_dir.path().file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn test_run_command_nonzero_exit() {
        let call = ToolCall::new(RUN_COMMAND).with_arg("command", "exit 1");
        let result = execute_run_command(&call);

        // Tool should still succeed, but include exit code info
        assert!(result.is_success());
        assert_eq!(result.metadata.exit_code, Some(1));
    }

    #[test]
    fn test_run_command_invalid_working_dir() {
        let call = ToolCall::new(RUN_COMMAND)
            .with_arg("command", "echo test")
            .with_arg("working_dir", "/nonexistent/directory");
        let result = execute_run_command(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "NOT_FOUND");
    }

    #[test]
    fn test_run_command_missing_command() {
        let call = ToolCall::new(RUN_COMMAND);
        let result = execute_run_command(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "INVALID_ARGUMENT");
    }
}
