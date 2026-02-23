//! Quorum review methods for the RunAgent use case.
//!
//! Contains plan review, final review orchestration, and the
//! [`QuorumActionReviewer`] implementation of [`ActionReviewer`].

use super::RunAgentUseCase;
use super::types::{QuorumReviewResult, RunAgentError, RunAgentInput};
use crate::ports::action_reviewer::{ActionReviewer, ReviewDecision};
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::conversation_logger::ConversationEvent;
use crate::ports::llm_gateway::{GatewayError, LlmGateway};
use crate::ports::tool_executor::ToolExecutorPort;
use async_trait::async_trait;
use quorum_domain::agent::model_config::ModelConfig;
use quorum_domain::quorum::parsing::{parse_final_review_response, parse_review_response};
use quorum_domain::{AgentPromptTemplate, AgentState, Model, Task};
use std::sync::Arc;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

// ==================== Free Functions ====================

/// Query a single model for review.
///
/// Used by `review_plan`, `review_action`, and `final_review`.
pub(crate) async fn query_model_for_review(
    gateway: &dyn LlmGateway,
    model: &Model,
    prompt: &str,
) -> Result<String, GatewayError> {
    let system_prompt = "You are a code reviewer evaluating plans and actions. \
        Provide your assessment with a clear APPROVE or REJECT/REVISE recommendation.";

    let session = gateway
        .create_text_only_session(model, system_prompt)
        .await?;

    session.send(prompt).await
}

// ==================== QuorumActionReviewer ====================

/// Action reviewer that uses quorum (multi-model voting) to review high-risk tool calls.
pub(crate) struct QuorumActionReviewer {
    gateway: Arc<dyn LlmGateway>,
    tool_executor: Arc<dyn ToolExecutorPort>,
    cancellation_token: Option<CancellationToken>,
}

impl QuorumActionReviewer {
    pub(crate) fn new(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
        cancellation_token: Option<CancellationToken>,
    ) -> Self {
        Self {
            gateway,
            tool_executor,
            cancellation_token,
        }
    }
}

#[async_trait]
impl ActionReviewer for QuorumActionReviewer {
    async fn review_action(
        &self,
        tool_call_json: &str,
        task: &Task,
        state: &AgentState,
        models: &ModelConfig,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<ReviewDecision, RunAgentError> {
        let models = &models.review;
        if models.is_empty() {
            return Ok(ReviewDecision::SkipReview);
        }

        info!("Starting action review for task: {}", task.description);
        progress.on_quorum_start("action_review", models.len());

        let prompt = AgentPromptTemplate::action_review(task, tool_call_json, &state.context);

        // Query all quorum models in parallel
        let mut join_set = JoinSet::new();

        for model in models {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let prompt = prompt.clone();

            join_set.spawn(async move {
                let result = query_model_for_review(gateway.as_ref(), &model, &prompt).await;
                (model, result)
            });
        }

        // Collect votes with cancellation support
        let mut votes = Vec::new();

        loop {
            let result = if let Some(ref token) = self.cancellation_token {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        join_set.abort_all();
                        return Err(RunAgentError::Cancelled);
                    }
                    result = join_set.join_next() => result,
                }
            } else {
                join_set.join_next().await
            };

            let Some(result) = result else {
                break;
            };

            match result {
                Ok((model, Ok(response))) => {
                    let (approved, feedback) = parse_review_response(&response);
                    info!(
                        "Model {} voted: {}",
                        model,
                        if approved { "APPROVE" } else { "REJECT" }
                    );
                    progress.on_quorum_model_complete(&model, approved);
                    votes.push((model.to_string(), approved, feedback));
                }
                Ok((model, Err(e))) => {
                    warn!("Model {} failed to review: {}", model, e);
                    progress.on_quorum_model_complete(&model, false);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        if votes.is_empty() {
            return Err(RunAgentError::QuorumFailed);
        }

        let review = QuorumReviewResult::from_votes(votes);

        // Notify with detailed vote information
        progress.on_quorum_complete_with_votes(
            "action_review",
            review.approved,
            &review.votes,
            review.feedback.as_deref(),
        );

        if review.approved {
            Ok(ReviewDecision::Approved)
        } else {
            Ok(ReviewDecision::Rejected(
                review
                    .feedback
                    .unwrap_or_else(|| "Rejected by quorum".to_string()),
            ))
        }
    }

    fn is_high_risk_tool(&self, tool_name: &str) -> bool {
        if let Some(definition) = self.tool_executor.get_tool(tool_name) {
            definition.is_high_risk()
        } else {
            // Unknown tools are considered high-risk by default
            true
        }
    }
}

// ==================== RunAgentUseCase Review Methods ====================

impl RunAgentUseCase {
    /// Review the plan using quorum (multiple models vote)
    pub(super) async fn review_plan(
        &self,
        input: &RunAgentInput,
        state: &AgentState,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<QuorumReviewResult, RunAgentError> {
        let plan = state
            .plan
            .as_ref()
            .ok_or_else(|| RunAgentError::PlanningFailed("No plan to review".to_string()))?;

        // Skip plan review if configured to do so (e.g., --no-quorum flag)
        if !input.policy.require_plan_review {
            info!("Plan review disabled, auto-approving plan");
            return Ok(QuorumReviewResult {
                approved: true,
                votes: vec![],
                feedback: None,
            });
        }

        let models = &input.models.review;
        if models.is_empty() {
            // No quorum models configured, auto-approve
            info!("No quorum models configured, auto-approving plan");
            return Ok(QuorumReviewResult {
                approved: true,
                votes: vec![],
                feedback: None,
            });
        }

        info!("Starting plan review with {} models", models.len());
        progress.on_quorum_start("plan_review", models.len());

        let prompt = AgentPromptTemplate::plan_review(&input.request, plan, &state.context);

        // Query all quorum models in parallel
        let mut join_set = JoinSet::new();

        for model in models {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let prompt = prompt.clone();

            join_set.spawn(async move {
                let result = query_model_for_review(gateway.as_ref(), &model, &prompt).await;
                (model, result)
            });
        }

        // Collect votes with cancellation support
        let mut votes = Vec::new();

        loop {
            // Check for cancellation using select! if token exists
            let result = if let Some(ref token) = self.cancellation_token {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        join_set.abort_all();
                        return Err(RunAgentError::Cancelled);
                    }
                    result = join_set.join_next() => result,
                }
            } else {
                join_set.join_next().await
            };

            let Some(result) = result else {
                break; // All tasks complete
            };

            match result {
                Ok((model, Ok(response))) => {
                    let (approved, feedback) = parse_review_response(&response);
                    info!(
                        "Model {} voted: {}",
                        model,
                        if approved { "APPROVE" } else { "REJECT" }
                    );
                    progress.on_quorum_model_complete(&model, approved);
                    votes.push((model.to_string(), approved, feedback));
                }
                Ok((model, Err(e))) => {
                    warn!("Model {} failed to review: {}", model, e);
                    progress.on_quorum_model_complete(&model, false);
                    // Treat failure as abstain (don't count)
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        if votes.is_empty() {
            return Err(RunAgentError::QuorumFailed);
        }

        let result = QuorumReviewResult::from_votes(votes);

        self.conversation_logger.log(ConversationEvent::new(
            "quorum_result",
            serde_json::json!({
                "topic": "plan_review",
                "approved": result.approved,
                "votes": result.votes.iter().map(|(model, approved, feedback)| {
                    serde_json::json!({
                        "model": model,
                        "approved": approved,
                        "feedback": feedback,
                    })
                }).collect::<Vec<_>>(),
            }),
        ));

        // Note: UI notification is handled by the caller (execute_with_progress)
        // to maintain separation between business logic and presentation

        Ok(result)
    }

    /// Final review of agent results using quorum (optional)
    pub(super) async fn final_review(
        &self,
        input: &RunAgentInput,
        state: &AgentState,
        results_summary: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<QuorumReviewResult, RunAgentError> {
        let plan = state.plan.as_ref().ok_or_else(|| {
            RunAgentError::TaskExecutionFailed("No plan for final review".to_string())
        })?;

        let models = &input.models.review;
        if models.is_empty() {
            return Ok(QuorumReviewResult {
                approved: true,
                votes: vec![],
                feedback: None,
            });
        }

        info!("Starting final review with {} models", models.len());
        progress.on_quorum_start("final_review", models.len());

        let prompt = AgentPromptTemplate::final_review(&input.request, plan, results_summary);

        // Query all quorum models in parallel
        let mut join_set = JoinSet::new();

        for model in models {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let prompt = prompt.clone();

            join_set.spawn(async move {
                let result = query_model_for_review(gateway.as_ref(), &model, &prompt).await;
                (model, result)
            });
        }

        // Collect results with cancellation support
        let mut votes = Vec::new();

        loop {
            // Check for cancellation using select! if token exists
            let result = if let Some(ref token) = self.cancellation_token {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        join_set.abort_all();
                        return Err(RunAgentError::Cancelled);
                    }
                    result = join_set.join_next() => result,
                }
            } else {
                join_set.join_next().await
            };

            let Some(result) = result else {
                break; // All tasks complete
            };

            match result {
                Ok((model, Ok(response))) => {
                    // For final review, we look for SUCCESS/PARTIAL/FAILURE
                    let (approved, feedback) = parse_final_review_response(&response);
                    info!(
                        "Model {} assessment: {}",
                        model,
                        if approved { "SUCCESS" } else { "ISSUES" }
                    );
                    progress.on_quorum_model_complete(&model, approved);
                    votes.push((model.to_string(), approved, feedback));
                }
                Ok((model, Err(e))) => {
                    warn!("Model {} failed to review: {}", model, e);
                    progress.on_quorum_model_complete(&model, false);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        if votes.is_empty() {
            return Err(RunAgentError::QuorumFailed);
        }

        let result = QuorumReviewResult::from_votes(votes);
        // Note: UI notification is handled by the caller (execute_with_progress)
        // to maintain separation between business logic and presentation

        Ok(result)
    }
}
