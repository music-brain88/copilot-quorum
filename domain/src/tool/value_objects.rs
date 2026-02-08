//! Tool domain value objects — immutable result and error types
//!
//! These types form the **output side** of the Tool System pipeline.
//! Every tool execution produces a [`ToolResult`] with optional
//! [`ToolResultMetadata`] (timing, byte counts, paths, etc.).
//!
//! Error codes in [`ToolError`] drive the **retry strategy**:
//! `INVALID_ARGUMENT` and `NOT_FOUND` are retryable (up to 2 attempts),
//! while `EXECUTION_FAILED` and others terminate immediately.

use serde::{Deserialize, Serialize};

/// Error that occurred during tool execution.
///
/// Error codes determine retry behavior in the agent execution loop:
///
/// | Code | Retryable? | Description |
/// |------|-----------|-------------|
/// | `INVALID_ARGUMENT` | Yes | Missing/wrong parameters — LLM can fix |
/// | `NOT_FOUND` | Yes | Unknown tool or resource — LLM can correct |
/// | `EXECUTION_FAILED` | No | Runtime failure (I/O error, HTTP error) |
/// | `PERMISSION_DENIED` | No | Access denied |
/// | `TIMEOUT` | No | Operation timed out |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolError {
    /// Error code (e.g., "NOT_FOUND", "PERMISSION_DENIED")
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ToolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    // Common error constructors
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::new(
            "NOT_FOUND",
            format!("Resource not found: {}", resource.into()),
        )
    }

    pub fn permission_denied(resource: impl Into<String>) -> Self {
        Self::new(
            "PERMISSION_DENIED",
            format!("Permission denied: {}", resource.into()),
        )
    }

    pub fn invalid_argument(message: impl Into<String>) -> Self {
        Self::new("INVALID_ARGUMENT", message)
    }

    pub fn execution_failed(message: impl Into<String>) -> Self {
        Self::new("EXECUTION_FAILED", message)
    }

    pub fn timeout(operation: impl Into<String>) -> Self {
        Self::new(
            "TIMEOUT",
            format!("Operation timed out: {}", operation.into()),
        )
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)?;
        if let Some(details) = &self.details {
            write!(f, " ({})", details)?;
        }
        Ok(())
    }
}

impl std::error::Error for ToolError {}

/// Result of a tool execution, carrying output or error information.
///
/// Produced by tool executors (file, command, search, web) and consumed
/// by the agent loop for context injection and error handling.
///
/// The [`metadata`](Self::metadata) field provides structured execution data
/// (timing, byte counts, paths) used for progress reporting and diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Name of the tool that was executed
    pub tool_name: String,
    /// Whether the execution was successful
    pub success: bool,
    /// Output content (for successful execution)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Error information (for failed execution)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolError>,
    /// Metadata about the execution
    #[serde(default)]
    pub metadata: ToolResultMetadata,
}

/// Structured metadata about tool execution.
///
/// Each tool populates the relevant fields:
///
/// | Tool | `duration_ms` | `bytes` | `path` | `exit_code` | `match_count` |
/// |------|:---:|:---:|:---:|:---:|:---:|
/// | `read_file` | - | yes | yes | - | - |
/// | `write_file` | - | yes | yes | - | - |
/// | `run_command` | yes | - | - | yes | - |
/// | `glob_search` | yes | - | - | - | yes |
/// | `grep_search` | yes | - | - | - | yes |
/// | `web_fetch` | yes | yes | - | - | - |
/// | `web_search` | yes | - | - | - | - |
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolResultMetadata {
    /// Duration of execution in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Number of bytes processed/returned
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<usize>,
    /// For file operations: the affected path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// For command execution: exit code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// For search operations: number of matches
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_count: Option<usize>,
}

impl ToolResult {
    /// Create a successful result
    pub fn success(tool_name: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: true,
            output: Some(output.into()),
            error: None,
            metadata: ToolResultMetadata::default(),
        }
    }

    /// Create a failed result
    pub fn failure(tool_name: impl Into<String>, error: ToolError) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: false,
            output: None,
            error: Some(error),
            metadata: ToolResultMetadata::default(),
        }
    }

    /// Add metadata to the result
    pub fn with_metadata(mut self, metadata: ToolResultMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Add duration metadata
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.metadata.duration_ms = Some(duration_ms);
        self
    }

    /// Add path metadata
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.metadata.path = Some(path.into());
        self
    }

    /// Check if execution was successful
    pub fn is_success(&self) -> bool {
        self.success
    }

    /// Get the output content
    pub fn output(&self) -> Option<&str> {
        self.output.as_deref()
    }

    /// Get the error
    pub fn error(&self) -> Option<&ToolError> {
        self.error.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_error() {
        let err = ToolError::not_found("/path/to/file").with_details("File does not exist");

        assert_eq!(err.code, "NOT_FOUND");
        assert!(err.message.contains("/path/to/file"));
        assert!(err.details.is_some());
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("read_file", "file contents").with_path("/test/file.txt");

        assert!(result.is_success());
        assert_eq!(result.output(), Some("file contents"));
        assert!(result.error().is_none());
        assert_eq!(result.metadata.path, Some("/test/file.txt".to_string()));
    }

    #[test]
    fn test_tool_result_failure() {
        let result = ToolResult::failure("write_file", ToolError::permission_denied("/etc/passwd"));

        assert!(!result.is_success());
        assert!(result.output().is_none());
        assert!(result.error().is_some());
        assert_eq!(result.error().unwrap().code, "PERMISSION_DENIED");
    }
}
