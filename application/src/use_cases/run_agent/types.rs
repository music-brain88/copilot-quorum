//! Type definitions for the RunAgent use case.

use crate::config::ExecutionParams;
use crate::ports::llm_gateway::GatewayError;
use quorum_domain::agent::agent_policy::AgentPolicy;
use quorum_domain::agent::model_config::ModelConfig;
use quorum_domain::orchestration::session_mode::SessionMode;
use quorum_domain::{AgentId, AgentState, EnsemblePlanResult, Plan};
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

/// Input for the RunAgent use case.
///
/// # Config Split
///
/// Input groups configuration by concern:
///
/// | Field | Type | Purpose |
/// |-------|------|---------|
/// | `mode` | [`SessionMode`] | Runtime-mutable orchestration settings |
/// | `models` | [`ModelConfig`] | Role-based model selection |
/// | `policy` | [`AgentPolicy`] | Domain behavioral constraints |
/// | `execution` | [`ExecutionParams`] | Use case loop control |
#[derive(Debug, Clone)]
pub struct RunAgentInput {
    /// The user's request
    pub request: String,
    /// Runtime-mutable orchestration mode (consensus, scope, strategy)
    pub mode: SessionMode,
    /// Role-based model configuration
    pub models: ModelConfig,
    /// Domain behavioral policy (HiL, plan review, revision limits)
    pub policy: AgentPolicy,
    /// Execution loop control parameters
    pub execution: ExecutionParams,
}

impl RunAgentInput {
    pub fn new(
        request: impl Into<String>,
        mode: SessionMode,
        models: ModelConfig,
        policy: AgentPolicy,
        execution: ExecutionParams,
    ) -> Self {
        Self {
            request: request.into(),
            mode,
            models,
            policy,
            execution,
        }
    }

    /// Build an [`AgentState`] from this input, starting in the ContextGathering phase.
    pub fn to_agent_state(&self, id: impl Into<AgentId>) -> AgentState {
        AgentState::new(
            id,
            &self.request,
            self.mode.clone(),
            self.models.clone(),
            self.policy.clone(),
            self.execution.max_iterations,
        )
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
