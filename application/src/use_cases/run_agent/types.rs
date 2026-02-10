//! Type definitions for the RunAgent use case.

use crate::ports::llm_gateway::GatewayError;
use quorum_domain::{EnsemblePlanResult, Plan};
use thiserror::Error;

/// Errors that can occur during Agent execution
#[derive(Error, Debug)]
pub enum RunAgentError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Context gathering failed: {0}")]
    ContextGatheringFailed(String),

    #[error("Planning failed: {0}")]
    PlanningFailed(String),

    #[error("Ensemble planning failed: {0}")]
    EnsemblePlanningFailed(String),

    #[error("Plan rejected by quorum: {0}")]
    PlanRejected(String),

    #[error("Action rejected by quorum: {0}")]
    ActionRejected(String),

    #[error("Task execution failed: {0}")]
    TaskExecutionFailed(String),

    #[error("Max iterations exceeded")]
    MaxIterationsExceeded,

    #[error("Plan revision limit exceeded, human rejected")]
    HumanRejected,

    #[error("Human intervention failed: {0}")]
    HumanInterventionFailed(String),

    #[error("All quorum models failed")]
    QuorumFailed,

    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),

    #[error("Operation cancelled")]
    Cancelled,
}

impl RunAgentError {
    /// Check if this error represents a cancellation
    pub fn is_cancelled(&self) -> bool {
        matches!(self, RunAgentError::Cancelled)
    }
}

/// Result of the planning phase.
///
/// When the LLM determines a request doesn't need a plan (e.g., greetings,
/// questions), it returns text without calling `create_plan`. This enum
/// distinguishes that case from a successful plan creation.
pub(super) enum PlanningResult {
    /// LLM created a structured plan
    Plan(Plan),
    /// LLM responded with text only (no plan needed)
    TextResponse(String),
}

/// Result of the ensemble planning phase.
///
/// When all models fail to produce plans, `create_ensemble_plans()` returns
/// an error rather than a fallback text response. The caller
/// (`execute_with_progress`) handles this by falling back to Solo planning.
pub(super) enum EnsemblePlanningOutcome {
    /// Multiple models generated plans, voted, and selected one
    Plans(EnsemblePlanResult),
    /// All models returned text responses (no plan needed) — moderator-synthesized
    TextResponse(String),
}

/// Input for the RunAgent use case
#[derive(Debug, Clone)]
pub struct RunAgentInput {
    /// The user's request
    pub request: String,
    /// Agent configuration
    pub config: quorum_domain::AgentConfig,
}

impl RunAgentInput {
    pub fn new(request: impl Into<String>, config: quorum_domain::AgentConfig) -> Self {
        Self {
            request: request.into(),
            config,
        }
    }

    pub fn with_model(request: impl Into<String>, model: quorum_domain::Model) -> Self {
        Self {
            request: request.into(),
            config: quorum_domain::AgentConfig::new(model),
        }
    }
}

/// Output from the RunAgent use case
#[derive(Debug, Clone)]
pub struct RunAgentOutput {
    /// Final state of the agent
    pub state: quorum_domain::AgentState,
    /// Summary of what was accomplished
    pub summary: String,
    /// Whether the agent completed successfully
    pub success: bool,
}

/// Result of a quorum review
#[derive(Debug, Clone)]
pub struct QuorumReviewResult {
    /// Whether the quorum approved
    pub approved: bool,
    /// Individual model votes (model name, approved, feedback)
    pub votes: Vec<(String, bool, String)>,
    /// Aggregated feedback
    pub feedback: Option<String>,
}

impl QuorumReviewResult {
    /// Create from individual votes, requiring majority approval
    pub fn from_votes(votes: Vec<(String, bool, String)>) -> Self {
        let approve_count = votes.iter().filter(|(_, approved, _)| *approved).count();
        let total = votes.len();
        let approved = approve_count > total / 2; // Majority wins

        // Aggregate feedback from rejections
        let feedback = if !approved {
            let rejections: Vec<_> = votes
                .iter()
                .filter(|(_, approved, _)| !*approved)
                .map(|(model, _, feedback)| format!("{}: {}", model, feedback))
                .collect();
            if rejections.is_empty() {
                None
            } else {
                Some(rejections.join("\n\n"))
            }
        } else {
            None
        };

        Self {
            approved,
            votes,
            feedback,
        }
    }
}
