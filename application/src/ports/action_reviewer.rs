//! Action reviewer port for high-risk tool call review.
//!
//! [`ActionReviewer`] abstracts the review mechanism for high-risk tool calls
//! during task execution. This allows different review strategies (quorum voting,
//! single-model review, human approval) to be plugged in.

use crate::ports::agent_progress::AgentProgressNotifier;
use crate::use_cases::run_agent::RunAgentError;
use async_trait::async_trait;
use quorum_domain::agent::model_config::ModelConfig;
use quorum_domain::{AgentState, Task};

/// Decision from the action review process
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewDecision {
    /// The action is approved for execution
    Approved,
    /// The action is rejected with a reason
    Rejected(String),
    /// No review needed (e.g., no review models configured)
    SkipReview,
}

/// Port for reviewing high-risk tool calls before execution.
///
/// Implementations decide whether a given tool call should be allowed,
/// typically by consulting multiple LLM models (quorum review).
#[async_trait]
pub trait ActionReviewer: Send + Sync {
    /// Review a high-risk tool call and return a decision.
    ///
    /// # Arguments
    /// * `tool_call_json` - JSON representation of the tool call
    /// * `task` - The task that triggered this tool call
    /// * `state` - Current agent state (for context)
    /// * `models` - Model configuration (for review models)
    /// * `progress` - Progress notifier for UI updates
    async fn review_action(
        &self,
        tool_call_json: &str,
        task: &Task,
        state: &AgentState,
        models: &ModelConfig,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<ReviewDecision, RunAgentError>;

    /// Check if a tool is high-risk (requires review before execution).
    fn is_high_risk_tool(&self, tool_name: &str) -> bool;
}
