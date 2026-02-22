//! Planning-related methods for the RunAgent use case.

use super::RunAgentUseCase;
use super::types::{EnsemblePlanningOutcome, PlanningResult, RunAgentError, RunAgentInput};
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::conversation_logger::ConversationEvent;
use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession, ToolResultMessage};
use crate::ports::tool_executor::ToolExecutorPort;
use crate::use_cases::shared::check_cancelled;
use quorum_domain::agent::plan_parser::extract_plan_from_response;
use quorum_domain::quorum::parsing::parse_vote_score;
use quorum_domain::session::response::LlmResponse;
use quorum_domain::{
    AgentContext, AgentPromptTemplate, EnsemblePlanResult, Model, PlanCandidate, PromptTemplate,
};
use std::sync::Arc;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

impl<G, T, C> RunAgentUseCase<G, T, C>
where
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
{
    pub(super) async fn create_plan(
        &self,
        session: &dyn LlmSession,
        request: &str,
        context: &AgentContext,
        previous_feedback: Option<&str>,
        _progress: &dyn AgentProgressNotifier,
    ) -> Result<PlanningResult, RunAgentError> {
        check_cancelled(&self.cancellation_token)?;

        match generate_plan_from_session(session, request, context, previous_feedback).await {
            Ok(result) => Ok(result),
            Err(e) => {
                // Check if the real cause was cancellation
                check_cancelled(&self.cancellation_token)?;
                Err(RunAgentError::PlanningFailed(e.to_string()))
            }
        }
    }

    /// Create plans using ensemble approach (multiple models generate independently, then vote)
    ///
    /// This implements the "Independent Generation + Voting" paradigm (ensemble-after-inference)
    /// based on recent research showing this approach outperforms iterative debate methods.
    ///
    /// # Algorithm
    ///
    /// 1. **Independent Generation**: Each `review_model` generates a plan in parallel,
    ///    without seeing other models' plans. This preserves diversity and avoids
    ///    "degeneration of thought" where models converge on potentially wrong answers.
    ///
    /// 2. **Voting**: Each model scores the other models' plans on a 1-10 scale.
    ///    Models do not vote on their own plans.
    ///
    /// 3. **Selection**: The plan with the highest average score is selected.
    ///
    /// # Research Background
    ///
    /// This approach is based on findings from:
    /// - "Debate or Vote" (ACL 2025): Voting matches debate performance with less cost
    /// - "Multi-Agent Debate" (ICLR 2025): Debate leads to "degeneration of thought"
    /// - "Beyond Majority Voting" (NeurIPS 2024): Advanced aggregation methods
    ///
    /// See `docs/concepts/ensemble-mode.md` for detailed design rationale.
    ///
    /// # Errors
    ///
    /// Returns [`RunAgentError::EnsemblePlanningFailed`] if:
    /// - No review models are configured
    /// - Fewer than 2 models are configured
    /// - All models fail to generate plans
    ///
    /// # Progress Callbacks
    ///
    /// Calls the following progress notifier methods:
    /// - [`AgentProgressNotifier::on_ensemble_start`] - At the beginning
    /// - [`AgentProgressNotifier::on_ensemble_plan_generated`] - For each plan
    /// - [`AgentProgressNotifier::on_ensemble_voting_start`] - Before voting
    /// - [`AgentProgressNotifier::on_ensemble_complete`] - With the selected plan
    pub(super) async fn create_ensemble_plans(
        &self,
        input: &RunAgentInput,
        context: &AgentContext,
        system_prompt: &str,
        previous_feedback: Option<&str>,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<EnsemblePlanningOutcome, RunAgentError> {
        let models = &input.models.review;

        if models.is_empty() {
            return Err(RunAgentError::EnsemblePlanningFailed(
                "No review models configured for ensemble planning".to_string(),
            ));
        }

        if models.len() < 2 {
            return Err(RunAgentError::EnsemblePlanningFailed(
                "Ensemble planning requires at least 2 models".to_string(),
            ));
        }

        // Step 1: Generate plans from each model in parallel
        info!(
            "Ensemble Step 1: Generating plans from {} models",
            models.len()
        );
        progress.on_ensemble_start(models.len());

        let session_timeout = input.execution.ensemble_session_timeout;
        let mut join_set = JoinSet::new();

        for model in models {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let request = input.request.clone();
            let context = context.clone();
            let system_prompt = system_prompt.to_string();
            let feedback = previous_feedback.map(|s| s.to_string());

            progress.on_ensemble_model_stream_start(&model.to_string());

            join_set.spawn(async move {
                let plan_future = async {
                    let session = gateway
                        .create_session_with_system_prompt(&model, &system_prompt)
                        .await
                        .map_err(|e| e.to_string())?;

                    generate_plan_from_session(
                        session.as_ref(),
                        &request,
                        &context,
                        feedback.as_deref(),
                    )
                    .await
                    .map_err(|e| e.to_string())
                };

                // Wrap with timeout if configured
                let result = if let Some(timeout) = session_timeout {
                    match tokio::time::timeout(timeout, plan_future).await {
                        Ok(r) => r,
                        Err(_) => Err(format!("session timed out after {}s", timeout.as_secs())),
                    }
                } else {
                    plan_future.await
                };

                (model, result)
            });
        }

        // Collect generated plans with cancellation support
        let mut candidates: Vec<PlanCandidate> = Vec::new();
        let mut text_responses: Vec<(String, String)> = Vec::new();
        let mut retryable_models: Vec<Model> = Vec::new();
        let mut failed_count = 0usize;

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
                Ok((model, Ok(PlanningResult::Plan(plan)))) => {
                    info!("Model {} generated plan: {}", model, plan.objective);
                    // Emit plan text as a stream chunk for live display
                    let model_str = model.to_string();
                    let summary = format!(
                        "Objective: {}\nTasks: {}",
                        plan.objective,
                        plan.tasks
                            .iter()
                            .map(|t| t.description.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    progress.on_ensemble_model_stream_chunk(&model_str, &summary);
                    progress.on_ensemble_model_stream_end(&model_str);
                    self.conversation_logger.log(ConversationEvent::new(
                        "plan_generated",
                        serde_json::json!({
                            "model": model_str,
                            "objective": plan.objective,
                            "task_count": plan.tasks.len(),
                        }),
                    ));
                    progress.on_ensemble_plan_generated(&model);
                    candidates.push(PlanCandidate::new(model, plan));
                }
                Ok((model, Ok(PlanningResult::TextResponse(text)))) => {
                    let model_str = model.to_string();
                    if text.trim().is_empty() {
                        warn!("Model {} returned empty text response, discarding", model);
                        progress.on_ensemble_model_stream_end(&model_str);
                        progress.on_ensemble_model_failed(&model, "empty response");
                        failed_count += 1;
                    } else {
                        info!("Model {} returned text response (no plan)", model);
                        progress.on_ensemble_model_stream_chunk(&model_str, &text);
                        progress.on_ensemble_model_stream_end(&model_str);
                        progress.on_ensemble_plan_generated(&model);
                        text_responses.push((model_str, text));
                    }
                }
                Ok((model, Err(e))) => {
                    // All errors are retryable (timeout, transport close, router stopped, etc.)
                    // since the Copilot CLI may serialize session.send internally, causing
                    // later sessions to fail when earlier ones complete and close the transport.
                    let model_str = model.to_string();
                    warn!("Model {} failed (will retry after backoff): {}", model, e);
                    progress.on_ensemble_model_stream_end(&model_str);
                    progress.on_ensemble_model_failed(&model, &e);
                    retryable_models.push(model);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                    failed_count += 1;
                }
            }
        }

        // Step 1b: Retry failed models sequentially after backoff
        // Copilot CLI serializes session.send internally, so later sessions often fail
        // when earlier ones complete and the CLI closes the transport. A brief backoff
        // followed by sequential retry gives each model a fresh chance.
        if !retryable_models.is_empty() {
            info!(
                "Retrying {} failed models after backoff",
                retryable_models.len()
            );
            // Brief backoff to let CLI finish processing and stabilize
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            for model in retryable_models {
                check_cancelled(&self.cancellation_token)?;
                info!("Retrying timed-out model: {}", model);

                let session = match self
                    .gateway
                    .create_session_with_system_prompt(&model, system_prompt)
                    .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Model {} retry: session creation failed: {}", model, e);
                        progress.on_ensemble_model_failed(&model, &e.to_string());
                        failed_count += 1;
                        continue;
                    }
                };

                match generate_plan_from_session(
                    session.as_ref(),
                    &input.request,
                    context,
                    previous_feedback,
                )
                .await
                {
                    Ok(PlanningResult::Plan(plan)) => {
                        info!(
                            "Model {} generated plan on retry: {}",
                            model, plan.objective
                        );
                        progress.on_ensemble_plan_generated(&model);
                        candidates.push(PlanCandidate::new(model, plan));
                    }
                    Ok(PlanningResult::TextResponse(text)) => {
                        if text.trim().is_empty() {
                            warn!(
                                "Model {} returned empty text response on retry, discarding",
                                model
                            );
                            progress.on_ensemble_model_failed(&model, "empty response");
                            failed_count += 1;
                        } else {
                            info!("Model {} returned text response on retry (no plan)", model);
                            progress.on_ensemble_plan_generated(&model);
                            text_responses.push((model.to_string(), text));
                        }
                    }
                    Err(e) => {
                        warn!("Model {} retry failed: {}", model, e);
                        progress.on_ensemble_model_failed(&model, &e.to_string());
                        failed_count += 1;
                    }
                }
            }
        }

        if candidates.is_empty() {
            if !text_responses.is_empty() {
                // All models returned text responses — synthesize via moderator
                info!(
                    "All models returned text responses ({} total), synthesizing via moderator",
                    text_responses.len()
                );
                let synthesized = self
                    .synthesize_text_responses(
                        &input.request,
                        &text_responses,
                        &input.models.decision,
                    )
                    .await?;
                return Ok(EnsemblePlanningOutcome::TextResponse(synthesized));
            }
            return Err(RunAgentError::EnsemblePlanningFailed(format!(
                "All {} models failed to generate plans",
                failed_count
            )));
        }

        if candidates.len() == 1 {
            // Only one plan succeeded, use it directly
            info!("Only one plan generated, selecting it directly");
            return Ok(EnsemblePlanningOutcome::Plans(EnsemblePlanResult::new(
                candidates, 0,
            )));
        }

        // Step 2: Each model votes on the other models' plans
        info!("Ensemble Step 2: Voting on {} plans", candidates.len());
        progress.on_ensemble_voting_start(candidates.len());

        // Voting timeout = session_timeout / 2 (voting is lighter than plan generation)
        let voting_timeout = session_timeout.map(|t| t / 2);

        // For each candidate, have other models vote on it
        for i in 0..candidates.len() {
            // Clone plan and model name for use in async tasks and logging
            let plan_to_vote = candidates[i].plan.clone();
            let plan_model_name = candidates[i].model.to_string();

            // Get votes from other models
            let mut voting_join_set = JoinSet::new();

            for (j, other_candidate) in candidates.iter().enumerate() {
                if i == j {
                    continue; // Don't vote on own plan
                }

                let gateway = Arc::clone(&self.gateway);
                let voter_model = other_candidate.model.clone();
                let voting_prompt = AgentPromptTemplate::plan_voting(&plan_to_vote);
                let system_prompt = system_prompt.to_string();

                voting_join_set.spawn(async move {
                    let voting_future = async {
                        let session = gateway
                            .create_session_with_system_prompt(&voter_model, &system_prompt)
                            .await?;
                        let response = session.send(&voting_prompt).await?;
                        let score = parse_vote_score(&response);
                        Ok::<(String, f64), GatewayError>((voter_model.to_string(), score))
                    };

                    if let Some(timeout) = voting_timeout {
                        match tokio::time::timeout(timeout, voting_future).await {
                            Ok(r) => r,
                            Err(_) => Err(GatewayError::Other(format!(
                                "voting timed out after {}s",
                                timeout.as_secs()
                            ))),
                        }
                    } else {
                        voting_future.await
                    }
                });
            }

            // Collect votes for this plan
            loop {
                let result = if let Some(ref token) = self.cancellation_token {
                    tokio::select! {
                        biased;
                        _ = token.cancelled() => {
                            voting_join_set.abort_all();
                            return Err(RunAgentError::Cancelled);
                        }
                        result = voting_join_set.join_next() => result,
                    }
                } else {
                    voting_join_set.join_next().await
                };

                let Some(result) = result else {
                    break;
                };

                match result {
                    Ok(Ok((voter, score))) => {
                        info!(
                            "Model {} voted {}/10 for plan from {}",
                            voter, score as i32, plan_model_name
                        );
                        self.conversation_logger.log(ConversationEvent::new(
                            "plan_vote",
                            serde_json::json!({
                                "voter": voter,
                                "plan_model": plan_model_name,
                                "score": score as i32,
                            }),
                        ));
                        candidates[i].add_vote(&voter, score);
                    }
                    Ok(Err(e)) => {
                        warn!("Voting failed for plan from {}: {}", plan_model_name, e);
                    }
                    Err(e) => {
                        warn!(
                            "Voting task join error for plan from {}: {}",
                            plan_model_name, e
                        );
                    }
                }
            }

            if candidates[i].vote_count() == 0 {
                warn!(
                    "Plan from {} received no votes — using score 0.0",
                    plan_model_name
                );
            }
        }

        // Step 3: Select the best plan
        let result = EnsemblePlanResult::select_best(candidates);

        if let Some(selected) = result.selected() {
            info!(
                "Selected plan from {} with average score {:.1}/10",
                selected.model,
                selected.average_score()
            );
            self.conversation_logger.log(ConversationEvent::new(
                "plan_selected",
                serde_json::json!({
                    "model": selected.model.to_string(),
                    "avg_score": selected.average_score(),
                    "summary": result.summary(),
                }),
            ));
            progress.on_ensemble_complete(&selected.model, selected.average_score());
        }

        Ok(EnsemblePlanningOutcome::Plans(result))
    }

    /// Synthesize text responses from multiple models using a moderator.
    ///
    /// Reuses the Quorum Discussion synthesis pattern
    /// ([`PromptTemplate::synthesis_system`] + [`PromptTemplate::synthesis_prompt_no_reviews`]).
    pub(super) async fn synthesize_text_responses(
        &self,
        question: &str,
        responses: &[(String, String)],
        moderator: &Model,
    ) -> Result<String, RunAgentError> {
        let session = self
            .gateway
            .create_session_with_system_prompt(moderator, PromptTemplate::synthesis_system())
            .await
            .map_err(|e| {
                RunAgentError::EnsemblePlanningFailed(format!(
                    "Failed to create synthesis session: {}",
                    e
                ))
            })?;

        let prompt = PromptTemplate::synthesis_prompt_no_reviews(question, responses);

        session.send(&prompt).await.map_err(|e| {
            RunAgentError::EnsemblePlanningFailed(format!("Text synthesis failed: {}", e))
        })
    }
}

/// Generate a plan from an existing LLM session using Native Tool Use.
///
/// Sends a planning prompt with a `create_plan` tool schema, then extracts
/// the structured plan from the tool-use response.  If the LLM calls
/// `create_plan` with empty/invalid arguments, one retry is attempted.
/// If no plan is produced at all, the text content is returned instead.
pub(super) async fn generate_plan_from_session(
    session: &dyn LlmSession,
    request: &str,
    context: &AgentContext,
    previous_feedback: Option<&str>,
) -> Result<PlanningResult, GatewayError> {
    let prompt = AgentPromptTemplate::planning_with_feedback(request, context, previous_feedback);
    let plan_tool = AgentPromptTemplate::plan_tool_schema();

    let response = session.send_with_tools(&prompt, &[plan_tool]).await?;

    if let Some(plan) = extract_plan_from_response(&response) {
        return Ok(PlanningResult::Plan(plan));
    }

    // create_plan was called with empty/invalid arguments — send error and retry once
    let mut retry_response: Option<LlmResponse> = None;
    if response.has_tool_use("create_plan")
        && let Some(tool_use_id) = response.first_tool_use_id()
    {
        debug!("create_plan called with empty arguments, sending error for retry");
        let results = vec![ToolResultMessage {
            tool_use_id: tool_use_id.to_string(),
            tool_name: "create_plan".to_string(),
            output: "Error: create_plan requires 'objective', 'reasoning', and 'tasks' \
                     fields. Please call create_plan again with all required arguments."
                .to_string(),
            is_error: true,
            is_rejected: false,
        }];
        let retry = session.send_tool_results(&results).await?;
        if let Some(plan) = extract_plan_from_response(&retry) {
            return Ok(PlanningResult::Plan(plan));
        }
        retry_response = Some(retry);
    }

    // No plan found — LLM responded with text only
    // Prefer retry response text (if retry happened), fall back to original response
    let text = retry_response
        .as_ref()
        .map(|r| r.text_content())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| response.text_content());
    if !text.is_empty() {
        return Ok(PlanningResult::TextResponse(text));
    }

    Ok(PlanningResult::TextResponse(String::new()))
}
