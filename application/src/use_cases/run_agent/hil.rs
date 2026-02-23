//! Human-in-the-Loop methods for the RunAgent use case.
//!
//! Contains human intervention and execution confirmation handlers.

use super::types::RunAgentError;
use super::RunAgentUseCase;
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::human_intervention::HumanInterventionError;
use quorum_domain::{AgentState, HilMode, HumanDecision};
use tracing::{info, warn};

use super::types::RunAgentInput;

impl RunAgentUseCase {
    /// Handle human intervention when plan revision limit is exceeded
    pub(super) async fn handle_human_intervention(
        &self,
        input: &RunAgentInput,
        state: &AgentState,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<HumanDecision, RunAgentError> {
        let plan = state
            .plan
            .as_ref()
            .ok_or_else(|| RunAgentError::PlanningFailed("No plan available".to_string()))?;

        let review_history = &plan.review_history;

        // Notify that human intervention is required
        progress.on_human_intervention_required(
            &input.request,
            plan,
            review_history,
            input.policy.max_plan_revisions,
        );

        // Determine decision based on HiL mode
        match input.policy.hil_mode {
            HilMode::AutoReject => {
                info!("Auto-rejecting due to HilMode::AutoReject");
                Ok(HumanDecision::Reject)
            }
            HilMode::AutoApprove => {
                warn!("Auto-approving due to HilMode::AutoApprove - use with caution!");
                Ok(HumanDecision::Approve)
            }
            HilMode::Interactive => {
                // Use the human intervention port if available
                if let Some(ref intervention) = self.human_intervention {
                    intervention
                        .request_intervention(&input.request, plan, review_history)
                        .await
                        .map_err(|e| match e {
                            HumanInterventionError::Cancelled => RunAgentError::Cancelled,
                            _ => RunAgentError::HumanInterventionFailed(e.to_string()),
                        })
                } else {
                    // No intervention handler, fall back to auto_reject
                    warn!("No human intervention handler configured, auto-rejecting");
                    Ok(HumanDecision::Reject)
                }
            }
        }
    }

    /// Handle execution confirmation gate (PhaseScope::Full only)
    ///
    /// This is the "are you sure?" gate between plan approval and task execution.
    /// The decision source depends on `HilMode`:
    /// - `Interactive` → `HumanInterventionPort::request_execution_confirmation()`
    /// - `AutoApprove` → automatically approve
    /// - `AutoReject` → automatically reject (plan created but not executed)
    pub(super) async fn handle_execution_confirmation(
        &self,
        input: &RunAgentInput,
        state: &AgentState,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<HumanDecision, RunAgentError> {
        let plan = state
            .plan
            .as_ref()
            .ok_or_else(|| RunAgentError::PlanningFailed("No plan available".to_string()))?;

        progress.on_execution_confirmation_required(&input.request, plan);

        match input.policy.hil_mode {
            HilMode::AutoApprove => {
                info!("Execution confirmation auto-approved (HilMode::AutoApprove)");
                Ok(HumanDecision::Approve)
            }
            HilMode::AutoReject => {
                info!("Execution confirmation auto-rejected (HilMode::AutoReject)");
                Ok(HumanDecision::Reject)
            }
            HilMode::Interactive => {
                if let Some(ref intervention) = self.human_intervention {
                    intervention
                        .request_execution_confirmation(&input.request, plan)
                        .await
                        .map_err(|e| match e {
                            HumanInterventionError::Cancelled => RunAgentError::Cancelled,
                            _ => RunAgentError::HumanInterventionFailed(e.to_string()),
                        })
                } else {
                    // No intervention handler → auto-approve (backwards compatible)
                    info!("No intervention handler for execution confirmation, auto-approving");
                    Ok(HumanDecision::Approve)
                }
            }
        }
    }
}
