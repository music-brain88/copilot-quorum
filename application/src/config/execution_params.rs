//! Execution parameters — use case loop control.
//!
//! [`ExecutionParams`] groups the static parameters that control the
//! execution loop in [`RunAgentUseCase`](crate::use_cases::run_agent::RunAgentUseCase).
//! These are application-layer concerns, not domain policy.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Execution loop control parameters.
///
/// Controls iteration limits, tool turn limits, timeouts, and working directory.
/// Used by RunAgentUseCase and ExecuteTaskUseCase.
///
/// # Buffer Usage
///
/// | Buffer | Uses ExecutionParams? |
/// |--------|----------------------|
/// | Agent  | Yes                  |
/// | Ask    | Yes                  |
/// | Discuss| No                   |
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionParams {
    /// Maximum number of execution iterations (plan+review loop).
    pub max_iterations: usize,
    /// Maximum tool use turns in a single Native Tool Use loop.
    pub max_tool_turns: usize,
    /// Maximum retries for tool validation errors.
    pub max_tool_retries: usize,
    /// Working directory for tool execution.
    pub working_dir: Option<String>,
    /// Timeout for each ensemble session's plan generation.
    pub ensemble_session_timeout: Option<Duration>,
}

impl Default for ExecutionParams {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            max_tool_turns: 10,
            max_tool_retries: 2,
            working_dir: None,
            ensemble_session_timeout: Some(Duration::from_secs(180)),
        }
    }
}

impl ExecutionParams {
    // ==================== Builder Methods ====================

    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_max_tool_turns(mut self, max: usize) -> Self {
        self.max_tool_turns = max;
        self
    }

    pub fn with_max_tool_retries(mut self, max: usize) -> Self {
        self.max_tool_retries = max;
        self
    }

    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn with_ensemble_session_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.ensemble_session_timeout = timeout;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default() {
        let params = ExecutionParams::default();
        assert_eq!(params.max_iterations, 50);
        assert_eq!(params.max_tool_turns, 10);
        assert_eq!(params.max_tool_retries, 2);
        assert!(params.working_dir.is_none());
        assert!(params.ensemble_session_timeout.is_some());
    }

    #[test]
    fn test_builder() {
        let params = ExecutionParams::default()
            .with_max_iterations(100)
            .with_max_tool_turns(20)
            .with_working_dir("/tmp/test");

        assert_eq!(params.max_iterations, 100);
        assert_eq!(params.max_tool_turns, 20);
        assert_eq!(params.working_dir, Some("/tmp/test".to_string()));
    }
}
