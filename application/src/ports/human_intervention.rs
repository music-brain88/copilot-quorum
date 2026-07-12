//! Human intervention port for handling plan revision limits.
//!
//! This module defines the port (interface) for requesting human decisions
//! when quorum cannot reach consensus after `max_plan_revisions` attempts.
//!
//! # Architecture
//!
//! Following the Ports and Adapters pattern:
//! - **Port**: [`HumanInterventionPort`] - defined here in application layer
//! - **Adapter**: `InteractiveHumanIntervention` - implemented in presentation layer
//!
//! # Flow
//!
//! ```text
//! Plan Review REJECTED (revision 1)
//!        ↓
//! Plan Review REJECTED (revision 2)
//!        ↓
//! Plan Review REJECTED (revision 3)
//!        ↓
//! max_plan_revisions reached
//!        ↓
//! HumanInterventionPort::request_intervention()
//!        ↓
//! User decides: Approve / Reject / Edit
//! ```
//!
//! # Built-in Implementations
//!
//! - [`AutoRejectIntervention`] - Always returns `HumanDecision::Reject`
//! - [`AutoApproveIntervention`] - Always returns `HumanDecision::Approve`
//!
//! For interactive use, see `InteractiveHumanIntervention` in the presentation layer.

use async_trait::async_trait;
use quorum_domain::{HumanDecision, Plan, ReviewRound};

/// Error type for human intervention operations.
///
/// These errors represent failures during the intervention process,
/// not decisions made by the user.
#[derive(Debug, Clone)]
pub enum HumanInterventionError {
    /// User cancelled the operation (e.g., via Ctrl+C).
    Cancelled,
    /// Input/output error (e.g., terminal read failure).
    IoError(String),
    /// Invalid user input (e.g., unrecognized command).
    InvalidInput(String),
}

impl std::fmt::Display for HumanInterventionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HumanInterventionError::Cancelled => write!(f, "Operation cancelled"),
            HumanInterventionError::IoError(msg) => write!(f, "I/O error: {}", msg),
            HumanInterventionError::InvalidInput(msg) => write!(f, "Invalid input: {}", msg),
        }
    }
}

impl std::error::Error for HumanInterventionError {}

/// Port for requesting human intervention during agent execution.
///
/// This trait defines the contract for human intervention handlers.
/// Implementations are responsible for:
/// 1. Displaying intervention context to the user
/// 2. Collecting user input
/// 3. Returning the user's decision
///
/// # Implementations
///
/// - **Interactive (CLI)**: `InteractiveHumanIntervention` in presentation layer
/// - **Auto-reject**: [`AutoRejectIntervention`]
/// - **Auto-approve**: [`AutoApproveIntervention`]
///
/// # Example
///
/// ```ignore
/// // In presentation layer
/// pub struct InteractiveHumanIntervention;
///
/// #[async_trait]
/// impl HumanInterventionPort for InteractiveHumanIntervention {
///     async fn request_intervention(
///         &self,
///         request: &str,
///         plan: &Plan,
///         review_history: &[ReviewRound],
///     ) -> Result<HumanDecision, HumanInterventionError> {
///         // Display UI
///         // Read user input (/approve, /reject, /edit)
///         // Return decision
///     }
/// }
/// ```
#[async_trait]
pub trait HumanInterventionPort: Send + Sync {
    /// Request human decision when plan revision limit is exceeded.
    ///
    /// This method is called by `RunAgentUseCase` when:
    /// 1. `AgentPolicy.hil_mode` is `HilMode::Interactive`
    /// 2. Plan revision count >= `AgentPolicy.max_plan_revisions`
    ///
    /// # Arguments
    ///
    /// * `request` - The original user request
    /// * `plan` - The current plan that failed to get approval
    /// * `review_history` - History of review rounds with feedback from each model
    ///
    /// # Returns
    ///
    /// * `Ok(HumanDecision::Approve)` - Execute the current plan
    /// * `Ok(HumanDecision::Reject)` - Abort the agent
    /// * `Ok(HumanDecision::Edit(plan))` - Use an edited plan (future)
    /// * `Err(HumanInterventionError)` - Error during intervention
    async fn request_intervention(
        &self,
        request: &str,
        plan: &Plan,
        review_history: &[ReviewRound],
    ) -> Result<HumanDecision, HumanInterventionError>;

    /// Request execution confirmation before running the approved plan.
    ///
    /// Called when `PhaseScope::Full` is active, after plan review approval
    /// but before task execution begins. This gives the user a final gate
    /// to abort before any tools are actually invoked.
    ///
    /// # Default
    ///
    /// Defaults to `Approve` (proceed with execution). Override in
    /// interactive implementations to prompt the user.
    async fn request_execution_confirmation(
        &self,
        _request: &str,
        _plan: &Plan,
    ) -> Result<HumanDecision, HumanInterventionError> {
        Ok(HumanDecision::Approve)
    }

    /// Request human escalation when a Debate strategy moderator tries to
    /// settle while critical/major [`quorum_domain::quorum::Objection`]s
    /// remain unresolved.
    ///
    /// Called at *every* settle checkpoint gated this way — not only the
    /// final round. An early premature settle attempt and the final round
    /// forcing a settle both route through here; `can_continue` tells the
    /// implementation which one it's showing the user.
    ///
    /// # Arguments
    ///
    /// * `question` - The original question/topic being debated
    /// * `unresolved` - The objections that remain unresolved
    /// * `transcript_summary` - A condensed (head+tail truncated to a fixed
    ///   byte budget — see `DebateStrategyExecutor`'s
    ///   `TRANSCRIPT_SUMMARY_MAX_BYTES`) summary of the debate transcript,
    ///   safe to render inline (e.g. a single TUI modal line) without
    ///   growing unbounded across rounds
    /// * `can_continue` - `true` if this is a non-final round: rejecting
    ///   declines the premature settle and the debate continues to the next
    ///   round. `false` if this is the final round (`max_rounds` reached):
    ///   rejecting aborts the debate entirely, since there is no next round
    ///   to continue to.
    ///
    /// # Default
    ///
    /// Defaults to `Reject` (fail-secure). This matches the philosophy of
    /// [`AutoRejectIntervention`]: an unresolved adversarial debate should
    /// not silently settle. When `can_continue` is `true`, `Reject` simply
    /// continues the debate rather than aborting it. Override in interactive
    /// implementations to prompt the user, or in [`AutoApproveIntervention`]
    /// to bypass.
    async fn request_debate_escalation(
        &self,
        _question: &str,
        _unresolved: &[quorum_domain::quorum::Objection],
        _transcript_summary: &str,
        _can_continue: bool,
    ) -> Result<HumanDecision, HumanInterventionError> {
        Ok(HumanDecision::Reject)
    }
}

/// Auto-reject implementation for `HilMode::AutoReject`.
///
/// This implementation always returns `HumanDecision::Reject`,
/// causing the agent to abort when plan revision limit is exceeded.
///
/// This is the safest non-interactive mode.
pub struct AutoRejectIntervention;

#[async_trait]
impl HumanInterventionPort for AutoRejectIntervention {
    async fn request_intervention(
        &self,
        _request: &str,
        _plan: &Plan,
        _review_history: &[ReviewRound],
    ) -> Result<HumanDecision, HumanInterventionError> {
        Ok(HumanDecision::Reject)
    }
}

/// Auto-approve implementation for `HilMode::AutoApprove`.
///
/// This implementation always returns `HumanDecision::Approve`,
/// causing the agent to proceed with the rejected plan.
///
/// # Warning
///
/// **Use with caution!** This bypasses quorum consensus and may
/// execute a plan that multiple models rejected. Only use when:
/// - Running in a sandboxed environment
/// - You're confident the rejections are false positives
/// - The consequences of a bad plan are acceptable
pub struct AutoApproveIntervention;

#[async_trait]
impl HumanInterventionPort for AutoApproveIntervention {
    async fn request_intervention(
        &self,
        _request: &str,
        _plan: &Plan,
        _review_history: &[ReviewRound],
    ) -> Result<HumanDecision, HumanInterventionError> {
        Ok(HumanDecision::Approve)
    }

    /// # Warning
    ///
    /// **Use with caution!** This bypasses the fail-secure default and
    /// approves proceeding even though the Debate strategy could not
    /// resolve all objections. Only use when the consequences of
    /// proceeding despite unresolved objections are acceptable.
    async fn request_debate_escalation(
        &self,
        _question: &str,
        _unresolved: &[quorum_domain::quorum::Objection],
        _transcript_summary: &str,
        _can_continue: bool,
    ) -> Result<HumanDecision, HumanInterventionError> {
        Ok(HumanDecision::Approve)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_reject_intervention() {
        let intervention = AutoRejectIntervention;
        let plan = Plan::new("test", "test");
        let result = intervention
            .request_intervention("test", &plan, &[])
            .await
            .unwrap();
        assert!(matches!(result, HumanDecision::Reject));
    }

    #[tokio::test]
    async fn test_auto_approve_intervention() {
        let intervention = AutoApproveIntervention;
        let plan = Plan::new("test", "test");
        let result = intervention
            .request_intervention("test", &plan, &[])
            .await
            .unwrap();
        assert!(matches!(result, HumanDecision::Approve));
    }

    #[tokio::test]
    async fn test_auto_reject_execution_confirmation_defaults_to_approve() {
        let intervention = AutoRejectIntervention;
        let plan = Plan::new("test", "test");
        // Default implementation returns Approve
        let result = intervention
            .request_execution_confirmation("test", &plan)
            .await
            .unwrap();
        assert!(matches!(result, HumanDecision::Approve));
    }

    #[tokio::test]
    async fn test_auto_approve_execution_confirmation_defaults_to_approve() {
        let intervention = AutoApproveIntervention;
        let plan = Plan::new("test", "test");
        let result = intervention
            .request_execution_confirmation("test", &plan)
            .await
            .unwrap();
        assert!(matches!(result, HumanDecision::Approve));
    }

    fn sample_objection() -> quorum_domain::quorum::Objection {
        let mut ledger = quorum_domain::quorum::ObjectionLedger::new();
        let id = ledger.add(
            1,
            "The plan ignores concurrent writes",
            "See race in step 3",
            quorum_domain::quorum::ObjectionSeverity::Major,
        );
        ledger
            .open_objections()
            .into_iter()
            .find(|o| o.id == id)
            .cloned()
            .expect("objection should exist after add")
    }

    #[tokio::test]
    async fn test_auto_reject_debate_escalation_defaults_to_reject() {
        let intervention = AutoRejectIntervention;
        let objection = sample_objection();
        let result = intervention
            .request_debate_escalation("test question", &[objection], "transcript summary", true)
            .await
            .unwrap();
        assert!(matches!(result, HumanDecision::Reject));
    }

    #[tokio::test]
    async fn test_auto_approve_debate_escalation_returns_approve() {
        let intervention = AutoApproveIntervention;
        let objection = sample_objection();
        let result = intervention
            .request_debate_escalation("test question", &[objection], "transcript summary", true)
            .await
            .unwrap();
        assert!(matches!(result, HumanDecision::Approve));
    }
}
