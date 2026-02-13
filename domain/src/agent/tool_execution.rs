//! Tool execution state machine.
//!
//! Tracks the lifecycle of individual tool calls within a task execution.
//! Inspired by OpenCode's ToolPart pattern: `Pending -> Running -> Completed/Error`.
//!
//! Each [`ToolExecution`] wraps a [`ToolExecutionState`] enum that enforces
//! valid state transitions at the type level — fields like `started_at` only
//! exist in states where they're meaningful.
//!
//! # State Transitions
//!
//! ```text
//! Pending ──> Running ──> Completed
//!                    └──> Error
//! ```

use crate::tool::value_objects::{ToolResult, ToolResultMetadata};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique identifier for a tool execution within a task.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolExecutionId(String);

impl ToolExecutionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolExecutionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<T: Into<String>> From<T> for ToolExecutionId {
    fn from(s: T) -> Self {
        Self::new(s)
    }
}

/// State of a tool execution — tagged union where each variant carries
/// only the fields valid for that state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolExecutionState {
    /// Tool call received, waiting to execute.
    Pending {
        tool_name: String,
        arguments: HashMap<String, serde_json::Value>,
        native_id: Option<String>,
    },
    /// Tool is currently executing.
    Running {
        tool_name: String,
        arguments: HashMap<String, serde_json::Value>,
        native_id: Option<String>,
        started_at: u64,
    },
    /// Tool completed successfully.
    Completed {
        tool_name: String,
        native_id: Option<String>,
        started_at: u64,
        completed_at: u64,
        output_preview: String,
        metadata: ToolResultMetadata,
    },
    /// Tool execution failed.
    Error {
        tool_name: String,
        native_id: Option<String>,
        started_at: u64,
        failed_at: u64,
        error_message: String,
    },
}

impl ToolExecutionState {
    /// Get the tool name regardless of state.
    pub fn tool_name(&self) -> &str {
        match self {
            Self::Pending { tool_name, .. }
            | Self::Running { tool_name, .. }
            | Self::Completed { tool_name, .. }
            | Self::Error { tool_name, .. } => tool_name,
        }
    }

    /// Whether this execution has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Error { .. })
    }

    /// Duration in milliseconds (only available for terminal states).
    pub fn duration_ms(&self) -> Option<u64> {
        match self {
            Self::Completed {
                started_at,
                completed_at,
                ..
            } => Some(completed_at.saturating_sub(*started_at)),
            Self::Error {
                started_at,
                failed_at,
                ..
            } => Some(failed_at.saturating_sub(*started_at)),
            _ => None,
        }
    }
}

/// A single tool execution tracked within a task.
///
/// Created when a tool call is received from the LLM, then transitioned
/// through Running -> Completed/Error as execution progresses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    /// Unique ID for this execution.
    pub id: ToolExecutionId,
    /// Current state (enum-based state machine).
    pub state: ToolExecutionState,
    /// Which turn of the Native Tool Use loop this belongs to (1-indexed).
    pub turn: usize,
}

impl ToolExecution {
    /// Create a new tool execution in Pending state from a tool call.
    pub fn new(
        id: impl Into<ToolExecutionId>,
        tool_name: impl Into<String>,
        arguments: HashMap<String, serde_json::Value>,
        native_id: Option<String>,
        turn: usize,
    ) -> Self {
        Self {
            id: id.into(),
            state: ToolExecutionState::Pending {
                tool_name: tool_name.into(),
                arguments,
                native_id,
            },
            turn,
        }
    }

    /// Transition from Pending to Running.
    ///
    /// No-op if already past Pending.
    pub fn mark_running(&mut self) {
        if let ToolExecutionState::Pending {
            tool_name,
            arguments,
            native_id,
        } = &self.state
        {
            self.state = ToolExecutionState::Running {
                tool_name: tool_name.clone(),
                arguments: arguments.clone(),
                native_id: native_id.clone(),
                started_at: current_timestamp(),
            };
        }
    }

    /// Transition from Running to Completed.
    ///
    /// No-op if not in Running state.
    pub fn mark_completed(&mut self, result: &ToolResult) {
        if let ToolExecutionState::Running {
            tool_name,
            native_id,
            started_at,
            ..
        } = &self.state
        {
            let output_preview = result
                .output()
                .map(|o| truncate_preview(o, 200))
                .unwrap_or_default();

            self.state = ToolExecutionState::Completed {
                tool_name: tool_name.clone(),
                native_id: native_id.clone(),
                started_at: *started_at,
                completed_at: current_timestamp(),
                output_preview,
                metadata: result.metadata.clone(),
            };
        }
    }

    /// Transition from Running to Error.
    ///
    /// No-op if not in Running state.
    pub fn mark_error(&mut self, message: impl Into<String>) {
        if let ToolExecutionState::Running {
            tool_name,
            native_id,
            started_at,
            ..
        } = &self.state
        {
            self.state = ToolExecutionState::Error {
                tool_name: tool_name.clone(),
                native_id: native_id.clone(),
                started_at: *started_at,
                failed_at: current_timestamp(),
                error_message: message.into(),
            };
        }
    }

    /// Get the tool name.
    pub fn tool_name(&self) -> &str {
        self.state.tool_name()
    }

    /// Whether this execution has finished.
    pub fn is_terminal(&self) -> bool {
        self.state.is_terminal()
    }

    /// Duration in milliseconds (only available for terminal states).
    pub fn duration_ms(&self) -> Option<u64> {
        self.state.duration_ms()
    }
}

/// Truncate a string for preview display.
fn truncate_preview(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Get current timestamp in milliseconds.
fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pending() -> ToolExecution {
        ToolExecution::new(
            "exec-1",
            "read_file",
            {
                let mut args = HashMap::new();
                args.insert("path".into(), serde_json::json!("/test.rs"));
                args
            },
            Some("native-123".into()),
            1,
        )
    }

    #[test]
    fn test_new_is_pending() {
        let exec = make_pending();
        assert!(matches!(exec.state, ToolExecutionState::Pending { .. }));
        assert_eq!(exec.tool_name(), "read_file");
        assert_eq!(exec.turn, 1);
        assert!(!exec.is_terminal());
        assert!(exec.duration_ms().is_none());
    }

    #[test]
    fn test_pending_to_running() {
        let mut exec = make_pending();
        exec.mark_running();
        assert!(matches!(exec.state, ToolExecutionState::Running { .. }));
        assert!(!exec.is_terminal());
        assert!(exec.duration_ms().is_none());
    }

    #[test]
    fn test_running_to_completed() {
        let mut exec = make_pending();
        exec.mark_running();

        let result = ToolResult::success("read_file", "file contents here");
        exec.mark_completed(&result);

        assert!(matches!(exec.state, ToolExecutionState::Completed { .. }));
        assert!(exec.is_terminal());
        assert!(exec.duration_ms().is_some());

        if let ToolExecutionState::Completed { output_preview, .. } = &exec.state {
            assert_eq!(output_preview, "file contents here");
        }
    }

    #[test]
    fn test_running_to_error() {
        let mut exec = make_pending();
        exec.mark_running();
        exec.mark_error("Permission denied");

        assert!(matches!(exec.state, ToolExecutionState::Error { .. }));
        assert!(exec.is_terminal());
        assert!(exec.duration_ms().is_some());

        if let ToolExecutionState::Error { error_message, .. } = &exec.state {
            assert_eq!(error_message, "Permission denied");
        }
    }

    #[test]
    fn test_invalid_transition_pending_to_completed() {
        let mut exec = make_pending();
        // Trying to complete from Pending should be no-op
        let result = ToolResult::success("read_file", "output");
        exec.mark_completed(&result);
        assert!(matches!(exec.state, ToolExecutionState::Pending { .. }));
    }

    #[test]
    fn test_invalid_transition_pending_to_error() {
        let mut exec = make_pending();
        // Trying to error from Pending should be no-op
        exec.mark_error("oops");
        assert!(matches!(exec.state, ToolExecutionState::Pending { .. }));
    }

    #[test]
    fn test_invalid_transition_completed_to_running() {
        let mut exec = make_pending();
        exec.mark_running();
        exec.mark_completed(&ToolResult::success("read_file", "ok"));
        // Trying to go back to running should be no-op
        exec.mark_running();
        assert!(matches!(exec.state, ToolExecutionState::Completed { .. }));
    }

    #[test]
    fn test_truncate_preview() {
        assert_eq!(truncate_preview("short", 200), "short");
        let long = "a".repeat(300);
        let preview = truncate_preview(&long, 200);
        assert!(preview.len() <= 200);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn test_tool_execution_id() {
        let id = ToolExecutionId::new("test-id");
        assert_eq!(id.as_str(), "test-id");
        assert_eq!(id.to_string(), "test-id");

        let id2: ToolExecutionId = "from-str".into();
        assert_eq!(id2.as_str(), "from-str");
    }
}
