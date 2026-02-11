//! Run Agent use case
//!
//! Orchestrates the agent execution flow with quorum integration.
//! Phases are controlled by [`PhaseScope`](quorum_domain::PhaseScope):
//!
//! | Phase                    | Full | Fast  | PlanOnly    |
//! |--------------------------|------|-------|-------------|
//! | 1. Context Gathering     | yes  | yes   | yes         |
//! | 2. Planning              | yes  | yes   | yes         |
//! | 3. Plan Review (Quorum)  | yes  | skip  | skip        |
//! | 3b. Execution Confirm    | yes  | skip  | skip        |
//! | 4. Executing             | yes  | yes   | skip+return |
//! |    - Action Review       | yes  | skip  | N/A         |
//! | 5. Final Review          | opt  | skip  | N/A         |

mod hil;
mod planning;
mod review;
mod types;

pub use types::{RunAgentError, RunAgentInput, RunAgentOutput};

use types::{EnsemblePlanningOutcome, PlanningResult};

use crate::ports::agent_progress::{AgentProgressNotifier, NoAgentProgress};
use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::human_intervention::HumanInterventionPort;
use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession, ToolResultMessage};
use crate::ports::tool_executor::ToolExecutorPort;
use quorum_domain::core::string::truncate;
use quorum_domain::session::response::LlmResponse;
use quorum_domain::{
    AgentConfig, AgentContext, AgentPhase, AgentPromptTemplate, AgentState, HumanDecision, Model,
    ModelVote, ProjectContext, ReviewRound, StreamEvent, Task, TaskId, Thought,
};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Use case for running an autonomous agent
pub struct RunAgentUseCase<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static = NoContextLoader,
> {
    pub(super) gateway: Arc<G>,
    pub(super) tool_executor: Arc<T>,
    pub(super) context_loader: Option<Arc<C>>,
    pub(super) cancellation_token: Option<CancellationToken>,
    pub(super) human_intervention: Option<Arc<dyn HumanInterventionPort>>,
}

impl<G, T, C> Clone for RunAgentUseCase<G, T, C>
where
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static,
{
    fn clone(&self) -> Self {
        Self {
            gateway: self.gateway.clone(),
            tool_executor: self.tool_executor.clone(),
            context_loader: self.context_loader.clone(),
            cancellation_token: self.cancellation_token.clone(),
            human_intervention: self.human_intervention.clone(),
        }
    }
}

/// No-op context loader for backwards compatibility
pub struct NoContextLoader;

impl ContextLoaderPort for NoContextLoader {
    fn load_known_files(&self, _project_root: &Path) -> Vec<quorum_domain::LoadedContextFile> {
        Vec::new()
    }

    fn context_file_exists(&self, _project_root: &Path) -> bool {
        false
    }

    fn write_context_file(&self, _project_root: &Path, _content: &str) -> std::io::Result<()> {
        Ok(())
    }
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static>
    RunAgentUseCase<G, T, NoContextLoader>
{
    pub fn new(gateway: Arc<G>, tool_executor: Arc<T>) -> Self {
        Self {
            gateway,
            tool_executor,
            context_loader: None,
            cancellation_token: None,
            human_intervention: None,
        }
    }
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static, C: ContextLoaderPort + 'static>
    RunAgentUseCase<G, T, C>
{
    pub fn with_context_loader(
        gateway: Arc<G>,
        tool_executor: Arc<T>,
        context_loader: Arc<C>,
    ) -> Self {
        Self {
            gateway,
            tool_executor,
            context_loader: Some(context_loader),
            cancellation_token: None,
            human_intervention: None,
        }
    }

    /// Set a human intervention handler for when plan revision limit is exceeded
    pub fn with_human_intervention(mut self, intervention: Arc<dyn HumanInterventionPort>) -> Self {
        self.human_intervention = Some(intervention);
        self
    }

    /// Set a cancellation token for graceful interruption
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }

    /// Check if cancellation has been requested
    fn check_cancelled(&self) -> Result<(), RunAgentError> {
        if let Some(ref token) = self.cancellation_token
            && token.is_cancelled()
        {
            return Err(RunAgentError::Cancelled);
        }
        Ok(())
    }

    /// Send a prompt to LLM with cancellation support and streaming.
    ///
    /// Uses `send_streaming()` to receive incremental chunks, forwarding each
    /// to `progress.on_llm_chunk()` for real-time display.
    ///
    /// Note: Currently unused after migrating `create_plan()` to Native Tool Use.
    /// Kept for future text-only LLM interactions (e.g., plan review, summaries).
    #[allow(dead_code)]
    async fn send_with_cancellation(
        &self,
        session: &dyn LlmSession,
        prompt: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<String, RunAgentError> {
        let stream_handle = session
            .send_streaming(prompt)
            .await
            .map_err(RunAgentError::GatewayError)?;
        let mut receiver = stream_handle.receiver;
        let mut full_text = String::new();

        progress.on_llm_stream_start("");

        loop {
            let event = if let Some(ref token) = self.cancellation_token {
                tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        progress.on_llm_stream_end();
                        return Err(RunAgentError::Cancelled);
                    }
                    event = receiver.recv() => event,
                }
            } else {
                receiver.recv().await
            };

            match event {
                Some(StreamEvent::Delta(chunk)) => {
                    progress.on_llm_chunk(&chunk);
                    full_text.push_str(&chunk);
                }
                Some(StreamEvent::Completed(text)) => {
                    if full_text.is_empty() {
                        full_text = text;
                    }
                    break;
                }
                Some(StreamEvent::Error(e)) => {
                    progress.on_llm_stream_end();
                    return Err(RunAgentError::GatewayError(GatewayError::RequestFailed(e)));
                }
                Some(StreamEvent::CompletedResponse(response)) => {
                    let text = response.text_content();
                    if full_text.is_empty() {
                        full_text = text;
                    }
                    break;
                }
                Some(StreamEvent::ToolCallDelta { .. }) => {
                    // Tool call deltas handled in Native path — skip in text collection
                }
                None => break, // channel closed
            }
        }

        progress.on_llm_stream_end();
        Ok(full_text)
    }

    /// Send a prompt with tools to the LLM with cancellation support (Native Tool Use path).
    ///
    /// Returns the full `LlmResponse` with structured content blocks.
    /// This is the Native counterpart of `send_with_cancellation()`.
    async fn send_with_tools_cancellable(
        &self,
        session: &dyn LlmSession,
        prompt: &str,
        tools: &[serde_json::Value],
        progress: &dyn AgentProgressNotifier,
    ) -> Result<LlmResponse, RunAgentError> {
        self.check_cancelled()?;
        progress.on_llm_stream_start("native_tool_use");

        let response = session
            .send_with_tools(prompt, tools)
            .await
            .map_err(RunAgentError::GatewayError)?;

        // Forward any text content to progress
        let text = response.text_content();
        if !text.is_empty() {
            progress.on_llm_chunk(&text);
        }

        progress.on_llm_stream_end();
        Ok(response)
    }

    /// Execute the agent without progress reporting
    pub async fn execute(&self, input: RunAgentInput) -> Result<RunAgentOutput, RunAgentError> {
        self.execute_with_progress(input, &NoAgentProgress).await
    }

    /// Execute the agent with progress callbacks
    pub async fn execute_with_progress(
        &self,
        input: RunAgentInput,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<RunAgentOutput, RunAgentError> {
        // Check for cancellation before starting
        self.check_cancelled()?;

        info!("Starting agent for request: {}", input.request);

        // Initialize agent state
        let agent_id = format!("agent-{}", chrono_lite_timestamp());
        let mut state = AgentState::new(agent_id, &input.request, input.config.clone());

        // Create system prompt (shared across phases)
        let system_prompt = AgentPromptTemplate::agent_system();

        // ==================== Phase 1: Context Gathering ====================
        // Uses exploration_model (default: Haiku - cheap for info collection)
        progress.on_phase_change(&AgentPhase::ContextGathering);
        state.set_phase(AgentPhase::ContextGathering);

        let context_session = self
            .gateway
            .create_session_with_system_prompt(&input.config.exploration_model, &system_prompt)
            .await?;

        match self
            .gather_context(
                context_session.as_ref(),
                &input.request,
                &input.config,
                progress,
            )
            .await
        {
            Ok(context) => {
                state.context = context;
                state.add_thought(Thought::observation("Context gathered successfully"));
            }
            Err(e) => {
                warn!("Context gathering failed: {}", e);
                state.add_thought(Thought::observation(format!(
                    "Context gathering failed: {}",
                    e
                )));
                // Continue with empty context
            }
        }

        // ==================== Phase 2-3: Planning + Review Loop ====================
        // Mode determines planning approach:
        // - Single (Solo): decision_model creates plan, review_models vote
        // - Ensemble: review_models each create plans in parallel, then vote on each other's plans

        let mut plan_feedback: Option<String> = None;

        loop {
            // Check for cancellation at the start of each loop iteration
            self.check_cancelled()?;

            // Phase 2: Planning
            progress.on_phase_change(&AgentPhase::Planning);
            state.set_phase(AgentPhase::Planning);

            // Branch based on planning mode
            if input.config.planning_approach().is_ensemble() {
                // ==================== Ensemble Planning ====================
                // Multiple models create plans independently, then vote
                info!(
                    "Ensemble planning: {} models will generate plans",
                    input.config.review_models.len()
                );

                match self
                    .create_ensemble_plans(
                        &input,
                        &state.context,
                        &system_prompt,
                        plan_feedback.as_deref(),
                        progress,
                    )
                    .await
                {
                    Ok(EnsemblePlanningOutcome::Plans(result)) => {
                        // Get the selected plan
                        let selected = result.selected().ok_or_else(|| {
                            RunAgentError::EnsemblePlanningFailed(
                                "No plan was selected".to_string(),
                            )
                        })?;

                        state.add_thought(Thought::planning(format!(
                            "Ensemble selected plan from {} with score {:.1}/10: {}",
                            selected.model,
                            selected.average_score(),
                            selected.plan.objective
                        )));

                        // Log the summary
                        info!("Ensemble planning result:\n{}", result.summary());

                        state.set_plan(selected.plan.clone());

                        // Ensemble mode: voting is already done during plan generation
                        // Skip the separate review phase and mark as approved
                        state.approve_plan();
                        state.add_thought(Thought::observation(format!(
                            "Plan selected by ensemble voting (avg score: {:.1}/10)",
                            selected.average_score()
                        )));
                        break; // Exit loop and proceed to Phase 4
                    }
                    Ok(EnsemblePlanningOutcome::TextResponse(text)) => {
                        // All ensemble models returned text (no plans needed).
                        // The moderator has already synthesized the responses.
                        // This is the correct path for greetings, questions, etc.
                        state.add_thought(Thought::observation(
                            "No plan needed — ensemble text responses synthesized",
                        ));
                        state.complete();
                        return Ok(RunAgentOutput {
                            summary: text,
                            success: true,
                            state,
                        });
                    }
                    Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
                    Err(e) => {
                        // Fallback to Solo planning
                        warn!("Ensemble planning failed, falling back to solo: {}", e);
                        progress.on_ensemble_fallback(&e.to_string());
                        state.add_thought(Thought::observation(format!(
                            "Ensemble planning failed ({}), falling back to solo",
                            e
                        )));
                        // fall through to Solo Planning below
                    }
                }
            }

            // ==================== Single (Solo) Planning ====================
            // Also used as fallback when ensemble planning fails
            // Uses decision_model (default: Sonnet - needs strong reasoning for planning)
            let planning_session = self
                .gateway
                .create_session_with_system_prompt(&input.config.decision_model, &system_prompt)
                .await?;

            let plan = match self
                .create_plan(
                    planning_session.as_ref(),
                    &input.request,
                    &state.context,
                    plan_feedback.as_deref(),
                    progress,
                )
                .await
            {
                Ok(PlanningResult::Plan(plan)) => {
                    state.add_thought(Thought::planning(format!(
                        "Created plan with {} tasks: {}",
                        plan.tasks.len(),
                        plan.objective
                    )));
                    plan
                }
                Ok(PlanningResult::TextResponse(text)) => {
                    // LLM determined no plan is needed — return text response directly
                    state.add_thought(Thought::observation("No plan needed for this request"));
                    state.complete();
                    return Ok(RunAgentOutput {
                        summary: text,
                        success: true,
                        state,
                    });
                }
                Err(e) => {
                    state.fail(format!("Planning failed: {}", e));
                    return Ok(RunAgentOutput {
                        summary: format!("Agent failed during planning: {}", e),
                        success: false,
                        state,
                    });
                }
            };

            state.set_plan(plan);

            // Phase 3: Plan Review (Quorum) - controlled by PhaseScope
            if !input.config.phase_scope.includes_plan_review() {
                // Skip plan review (Fast/PlanOnly) — auto-approve
                state.approve_plan();
                state.add_thought(Thought::observation(format!(
                    "Plan review skipped (scope: {})",
                    input.config.phase_scope
                )));
                break;
            }

            progress.on_phase_change(&AgentPhase::PlanReview);
            state.set_phase(AgentPhase::PlanReview);

            let plan_review = self.review_plan(&input, &state, progress).await?;

            // Create review round for history
            let review_round = {
                let votes: Vec<ModelVote> = plan_review
                    .votes
                    .iter()
                    .map(|(model, approved, feedback)| ModelVote::new(model, *approved, feedback))
                    .collect();
                let round_num = state
                    .plan
                    .as_ref()
                    .map(|p| p.review_history.len() + 1)
                    .unwrap_or(1);
                ReviewRound::new(round_num, plan_review.approved, votes)
            };

            // Add review round to plan history
            if let Some(plan) = &mut state.plan {
                plan.add_review_round(review_round.clone());
            }

            // Notify with detailed vote information
            progress.on_quorum_complete_with_votes(
                "plan_review",
                plan_review.approved,
                &plan_review.votes,
                plan_review.feedback.as_deref(),
            );

            if plan_review.approved {
                state.approve_plan();
                state.add_thought(Thought::observation("Plan approved by quorum"));
                break; // Exit loop and proceed to Phase 4
            }

            // Plan was rejected - check if we can retry
            let feedback = plan_review
                .feedback
                .unwrap_or_else(|| "No specific feedback".to_string());
            state.reject_plan(&feedback);

            // Check plan revision limit for human intervention
            // Note: We use state.plan_revision_count instead of plan.revision_count()
            // because the Plan is recreated on each revision attempt, losing history.
            let revision_count = state.plan_revision_count;

            if revision_count >= input.config.max_plan_revisions {
                // Human intervention required
                let decision = self
                    .handle_human_intervention(&input, &state, progress)
                    .await?;

                match decision {
                    HumanDecision::Approve => {
                        info!("Human approved plan despite quorum rejection");
                        state.approve_plan();
                        state.add_thought(Thought::observation(
                            "Plan approved by human intervention",
                        ));
                        break; // Exit loop and proceed to Phase 4
                    }
                    HumanDecision::Reject => {
                        state.fail("Plan rejected by human");
                        return Err(RunAgentError::HumanRejected);
                    }
                    HumanDecision::Edit(new_plan) => {
                        info!("Human provided edited plan");
                        state.plan = Some(new_plan);
                        state.add_thought(Thought::observation("Plan edited by human"));
                        // Continue to re-review the edited plan
                        continue;
                    }
                }
            }

            // Check iteration limit before retrying
            if !state.increment_iteration() {
                state.fail("Max plan retries exceeded");
                return Ok(RunAgentOutput {
                    summary: format!(
                        "Plan rejected after {} attempts. Last feedback: {}",
                        state.iteration_count, feedback
                    ),
                    success: false,
                    state,
                });
            }

            // Notify about plan revision
            progress.on_plan_revision(state.iteration_count, &feedback);

            // Store feedback for next iteration and retry
            plan_feedback = Some(feedback.clone());
            state.add_thought(Thought::reflection(format!(
                "Plan rejected, retrying with feedback: {}",
                truncate(&feedback, 100)
            )));
            info!(
                "Plan rejected (attempt {}), retrying...",
                state.iteration_count
            );
        }

        // ==================== PlanOnly Early Return ====================
        if !input.config.phase_scope.includes_execution() {
            let plan_summary = state
                .plan
                .as_ref()
                .map(|p| p.objective.clone())
                .unwrap_or_default();
            state.complete();
            info!("PlanOnly scope: skipping execution, returning plan");
            return Ok(RunAgentOutput {
                summary: format!("Plan created (plan-only mode): {}", plan_summary),
                success: true,
                state,
            });
        }

        // ==================== Execution Confirmation Gate ====================
        if input.config.phase_scope.requires_execution_confirmation() {
            let decision = self
                .handle_execution_confirmation(&input, &state, progress)
                .await?;
            match decision {
                HumanDecision::Approve => {
                    info!("Execution confirmation: approved");
                }
                _ => {
                    // Reject or Edit — stop execution gracefully
                    info!("Execution confirmation: rejected, stopping");
                    state.complete();
                    return Ok(RunAgentOutput {
                        summary: "Plan approved but not executed (user declined execution)"
                            .to_string(),
                        success: true,
                        state,
                    });
                }
            }
        }

        // ==================== Phase 4: Task Execution ====================
        // Model selection is now dynamic based on tool risk level:
        // - Low-risk tools (read_file, glob_search, grep_search): exploration_model
        // - High-risk tools (write_file, run_command): decision_model
        progress.on_phase_change(&AgentPhase::Executing);
        state.set_phase(AgentPhase::Executing);

        let execution_result = self
            .execute_tasks_with_dynamic_model(&input, &mut state, &system_prompt, progress)
            .await;

        let summary = match execution_result {
            Ok(summary) => summary,
            Err(e) => {
                state.fail(e.to_string());
                return Ok(RunAgentOutput {
                    summary: format!("Agent failed during execution: {}", e),
                    success: false,
                    state,
                });
            }
        };

        // Phase 5: Final Review (optional, requires action review scope)
        if input.config.require_final_review && input.config.phase_scope.includes_action_review() {
            progress.on_phase_change(&AgentPhase::FinalReview);
            state.set_phase(AgentPhase::FinalReview);

            let final_review = self
                .final_review(&input, &state, &summary, progress)
                .await?;

            // UI notification for final review result
            progress.on_quorum_complete_with_votes(
                "final_review",
                final_review.approved,
                &final_review.votes,
                final_review.feedback.as_deref(),
            );

            if !final_review.approved {
                state.add_thought(Thought::observation(format!(
                    "Final review raised concerns: {}",
                    final_review.feedback.as_deref().unwrap_or("No details")
                )));
            } else {
                state.add_thought(Thought::conclusion("Final review passed"));
            }
        }

        state.complete();
        Ok(RunAgentOutput {
            summary,
            success: true,
            state,
        })
    }

    /// Gather context about the project using 3-stage fallback strategy
    ///
    /// Stage 1: Load known files directly (no LLM needed)
    /// Stage 2: If insufficient, run exploration agent
    /// Stage 3: Proceed with minimal context if exploration fails
    async fn gather_context(
        &self,
        session: &dyn LlmSession,
        request: &str,
        config: &AgentConfig,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<AgentContext, RunAgentError> {
        let mut context = AgentContext::new();

        if let Some(working_dir) = &config.working_dir {
            context = context.with_project_root(working_dir);
        }

        // ========== Stage 1: Load known files directly (no LLM needed) ==========
        if let Some(ref context_loader) = self.context_loader
            && let Some(ref working_dir) = config.working_dir
        {
            let project_root = Path::new(working_dir);
            let files = context_loader.load_known_files(project_root);
            let project_ctx = context_loader.build_project_context(files);

            if project_ctx.has_sufficient_context() {
                info!(
                    "Stage 1: Using existing context from: {}",
                    project_ctx.source_description()
                );
                return Ok(self.context_from_project_ctx(project_ctx, config));
            }

            // Even if not sufficient, preserve any partial context
            if !project_ctx.is_empty() {
                info!("Stage 1: Found partial context, proceeding to exploration");
                context = self.merge_project_context(context, &project_ctx);
            }
        }

        // ========== Stage 2: Run exploration agent ==========
        info!("Stage 2: Running exploration agent for additional context");

        match self
            .run_exploration_agent(session, request, config, progress)
            .await
        {
            Ok(enriched_ctx) => {
                info!("Stage 2: Exploration agent succeeded");
                return Ok(enriched_ctx);
            }
            Err(e) => {
                warn!("Stage 2: Exploration agent failed: {}", e);
                // Continue to Stage 3
            }
        }

        // ========== Stage 3: Proceed with minimal context ==========
        warn!("Stage 3: Proceeding with minimal context");
        Ok(context)
    }

    /// Convert ProjectContext to AgentContext
    fn context_from_project_ctx(
        &self,
        project_ctx: ProjectContext,
        config: &AgentConfig,
    ) -> AgentContext {
        let mut context = AgentContext::new();

        if let Some(ref working_dir) = config.working_dir {
            context = context.with_project_root(working_dir);
        }

        if let Some(ref project_type) = project_ctx.project_type {
            context = context.with_project_type(project_type);
        }

        // Build structure summary from project context
        let summary = project_ctx.to_summary();
        if !summary.is_empty() && summary != "No context available." {
            context.set_structure_summary(&summary);
        }

        context
    }

    /// Merge ProjectContext into existing AgentContext
    fn merge_project_context(
        &self,
        mut context: AgentContext,
        project_ctx: &ProjectContext,
    ) -> AgentContext {
        if let Some(ref project_type) = project_ctx.project_type {
            context = context.with_project_type(project_type);
        }

        let summary = project_ctx.to_summary();
        if !summary.is_empty() && summary != "No context available." {
            context.set_structure_summary(&summary);
        }

        context
    }

    /// Run the exploration agent to gather context (original behavior)
    async fn run_exploration_agent(
        &self,
        session: &dyn LlmSession,
        request: &str,
        config: &AgentConfig,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<AgentContext, RunAgentError> {
        let mut context = AgentContext::new();

        if let Some(working_dir) = &config.working_dir {
            context = context.with_project_root(working_dir);
        }

        // Ask the model to gather context using tools (Native multi-turn loop)
        let prompt = AgentPromptTemplate::context_gathering(request, config.working_dir.as_deref());
        let tools = self.tool_executor.tool_spec().to_api_tools();
        let max_turns = config.max_tool_turns;
        let mut turn_count = 0;
        let mut results = Vec::new();

        let mut response = match self
            .send_with_tools_cancellable(session, &prompt, &tools, progress)
            .await
        {
            Ok(response) => response,
            Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
            Err(e) => return Err(RunAgentError::ContextGatheringFailed(e.to_string())),
        };

        loop {
            let tool_calls = response.tool_calls();
            if tool_calls.is_empty() {
                break;
            }

            turn_count += 1;
            if turn_count > max_turns {
                break;
            }

            self.check_cancelled()?;

            let mut tool_result_messages = Vec::new();

            for call in &tool_calls {
                progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));

                let result = self.tool_executor.execute(call).await;
                let success = result.is_success();
                progress.on_tool_result(&call.tool_name, success);

                let (is_error, output) = if success {
                    let output = result.output().unwrap_or("").to_string();
                    results.push((call.tool_name.clone(), output.clone()));

                    // Try to detect project type from common files
                    if call.tool_name == "glob_search" || call.tool_name == "read_file" {
                        if output.contains("Cargo.toml") {
                            context = context.with_project_type("rust");
                        } else if output.contains("package.json") {
                            context = context.with_project_type("nodejs");
                        } else if output.contains("pyproject.toml") || output.contains("setup.py") {
                            context = context.with_project_type("python");
                        }
                    }

                    (false, output)
                } else {
                    let msg = result
                        .error()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string());
                    (true, msg)
                };

                if let Some(native_id) = call.native_id.clone() {
                    tool_result_messages.push(ToolResultMessage {
                        tool_use_id: native_id,
                        tool_name: call.tool_name.clone(),
                        output,
                        is_error,
                    });
                } else {
                    warn!(
                        "Missing native_id for tool call '{}'; skipping result.",
                        call.tool_name
                    );
                }
            }

            response = session
                .send_tool_results(&tool_result_messages)
                .await
                .map_err(|e| RunAgentError::ContextGatheringFailed(e.to_string()))?;
        }

        // Add gathered information to context
        if !results.is_empty() {
            let summary = results
                .iter()
                .map(|(tool, output)| format!("## {}\n{}", tool, truncate(output, 500)))
                .collect::<Vec<_>>()
                .join("\n\n");
            context.set_structure_summary(&summary);
        }

        Ok(context)
    }

    /// Determine the appropriate model for a task based on tool risk level
    ///
    /// - Low-risk tools (read_file, glob_search, grep_search): exploration_model
    /// - High-risk tools (write_file, run_command) or unknown: decision_model
    fn select_model_for_task<'a>(&self, task: &Task, config: &'a AgentConfig) -> &'a Model {
        if let Some(tool_name) = &task.tool_name {
            if self.is_high_risk_tool(tool_name) {
                &config.decision_model
            } else {
                &config.exploration_model
            }
        } else {
            // Tool not specified yet - model will decide, so use decision_model
            &config.decision_model
        }
    }

    /// Execute all tasks in the plan with dynamic model selection based on risk level
    async fn execute_tasks_with_dynamic_model(
        &self,
        input: &RunAgentInput,
        state: &mut AgentState,
        system_prompt: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<String, RunAgentError> {
        let mut results = Vec::new();
        let mut previous_results = String::new();

        loop {
            // Check for cancellation at the start of each task
            self.check_cancelled()?;

            // Check iteration limit
            if !state.increment_iteration() {
                return Err(RunAgentError::MaxIterationsExceeded);
            }

            // Get next task and determine appropriate model
            let (task_id, selected_model) = {
                let plan = state.plan.as_ref().ok_or_else(|| {
                    RunAgentError::TaskExecutionFailed("No plan available".to_string())
                })?;

                match plan.next_task() {
                    Some(task) => {
                        let model = self.select_model_for_task(task, &input.config);
                        (task.id.clone(), model.clone())
                    }
                    None => break, // All tasks complete
                }
            };

            // Create session with the selected model
            let session = self
                .gateway
                .create_session_with_system_prompt(&selected_model, system_prompt)
                .await?;

            debug!(
                "Task {} using model {} (risk-based selection)",
                task_id, selected_model
            );

            // Mark task as in progress
            if let Some(plan) = &mut state.plan
                && let Some(task) = plan.get_task_mut(&task_id)
            {
                if task.status.is_terminal() {
                    warn!("Task {} already in terminal state, skipping", task_id);
                    continue;
                }
                task.mark_in_progress();
                progress.on_task_start(task);
            }

            // Execute the task with action retry support
            let max_action_retries = 2;
            let mut action_attempts = 0;
            let mut action_feedback: Option<String> = None;

            let task_result = loop {
                // Build context including any rejection feedback
                let context_with_feedback = if let Some(ref feedback) = action_feedback {
                    format!(
                        "{}\n\n---\n[Previous action was rejected]\nFeedback: {}\nPlease try a different approach.",
                        previous_results, feedback
                    )
                } else {
                    previous_results.clone()
                };

                match self
                    .execute_single_task(
                        session.as_ref(),
                        input,
                        state,
                        &task_id,
                        &context_with_feedback,
                        progress,
                    )
                    .await
                {
                    Err(RunAgentError::ActionRejected(feedback)) => {
                        action_attempts += 1;
                        if action_attempts >= max_action_retries {
                            break Err(RunAgentError::ActionRejected(format!(
                                "Action rejected after {} attempts. Last feedback: {}",
                                action_attempts, feedback
                            )));
                        }

                        // Get task for notification
                        if let Some(plan) = state.plan.as_ref()
                            && let Some(task) = plan.tasks.iter().find(|t| t.id == task_id)
                        {
                            progress.on_action_retry(task, action_attempts, &feedback);
                        }

                        info!(
                            "Action rejected (attempt {}), retrying with feedback...",
                            action_attempts
                        );
                        action_feedback = Some(feedback);
                    }
                    other => break other,
                }
            };

            // Update task status
            let (success, output) = match task_result {
                Ok(output) => (true, output),
                Err(e) => (false, e.to_string()),
            };

            if let Some(plan) = &mut state.plan
                && let Some(task) = plan.get_task_mut(&task_id)
                && !task.status.is_terminal()
            {
                if success {
                    task.mark_completed(quorum_domain::TaskResult::success(&output));
                } else {
                    task.mark_failed(quorum_domain::TaskResult::failure(&output));
                }
                progress.on_task_complete(task, success);
            }

            results.push(format!(
                "Task {}: {}",
                task_id,
                if success { "OK" } else { "FAILED" }
            ));
            previous_results.push_str(&format!("\n---\nTask {}: {}\n", task_id, output));
        }

        // Generate summary
        let completed = state.plan.as_ref().map(|p| p.progress()).unwrap_or((0, 0));

        Ok(format!(
            "Completed {}/{} tasks.\n\n{}",
            completed.0,
            completed.1,
            results.join("\n")
        ))
    }

    /// Execute a single task using the Native Tool Use API.
    async fn execute_single_task(
        &self,
        session: &dyn LlmSession,
        input: &RunAgentInput,
        state: &AgentState,
        task_id: &TaskId,
        previous_results: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<String, RunAgentError> {
        let task = state
            .plan
            .as_ref()
            .and_then(|p| p.tasks.iter().find(|t| &t.id == task_id))
            .ok_or_else(|| RunAgentError::TaskExecutionFailed("Task not found".to_string()))?;

        debug!("Executing task: {} - {}", task.id, task.description);

        // Always use Native Tool Use API
        self.execute_task_native(session, input, state, task, previous_results, progress)
            .await
    }

    /// Execute a task using the Native Tool Use API with multi-turn loop.
    ///
    /// Implements the standard Native Tool Use conversation loop:
    /// ```text
    /// send_with_tools(prompt, tools)
    ///   → stop_reason == ToolUse?
    ///     → yes: execute tool calls → send_tool_results() → loop
    ///     → no: return text content (done)
    /// ```
    ///
    /// Tool calls are extracted directly from the structured response — no
    /// text parsing or alias resolution needed (the API guarantees valid names).
    ///
    /// # Parallel Execution
    ///
    /// Low-risk tool calls within the same turn are executed in parallel
    /// using `futures::join_all`. High-risk tools are executed sequentially
    /// to maintain quorum review ordering.
    async fn execute_task_native(
        &self,
        session: &dyn LlmSession,
        input: &RunAgentInput,
        state: &AgentState,
        task: &Task,
        previous_results: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<String, RunAgentError> {
        let prompt = AgentPromptTemplate::task_execution(task, &state.context, previous_results);
        let tools = self.tool_executor.tool_spec().to_api_tools();
        let max_turns = input.config.max_tool_turns;
        let mut turn_count = 0;
        let mut all_outputs = Vec::new();

        // Initial request
        let mut response = match self
            .send_with_tools_cancellable(session, &prompt, &tools, progress)
            .await
        {
            Ok(response) => response,
            Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
            Err(e) => return Err(RunAgentError::TaskExecutionFailed(e.to_string())),
        };

        loop {
            // Collect any text from this turn
            let text = response.text_content();
            if !text.is_empty() {
                all_outputs.push(text);
            }

            // Extract tool calls
            let tool_calls = response.tool_calls();

            if tool_calls.is_empty() {
                // No tool calls — model is done
                break;
            }

            // Check turn limit
            turn_count += 1;
            if turn_count > max_turns {
                warn!(
                    "Native tool use loop exceeded max_tool_turns ({})",
                    max_turns
                );
                break;
            }

            // Check cancellation
            self.check_cancelled()?;

            // Execute tool calls and collect results
            let mut tool_result_messages = Vec::new();

            // Separate into low-risk (can parallelize) and high-risk (sequential)
            let mut low_risk_calls = Vec::new();
            let mut high_risk_calls = Vec::new();

            for call in &tool_calls {
                if self.is_high_risk_tool(&call.tool_name) {
                    high_risk_calls.push(call);
                } else {
                    low_risk_calls.push(call);
                }
            }

            // Execute low-risk calls in parallel
            if !low_risk_calls.is_empty() {
                let mut futures = Vec::new();
                for call in &low_risk_calls {
                    progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));
                    futures.push(self.tool_executor.execute(call));
                }

                let results: Vec<_> = futures::future::join_all(futures).await;

                for (call, result) in low_risk_calls.iter().zip(results) {
                    let is_error = !result.is_success();
                    let output = if is_error {
                        result
                            .error()
                            .map(|e| e.message.clone())
                            .unwrap_or_else(|| "Unknown error".to_string())
                    } else {
                        result.output().unwrap_or("").to_string()
                    };

                    progress.on_tool_result(&call.tool_name, !is_error);

                    if !is_error {
                        all_outputs.push(format!("[{}]: {}", call.tool_name, &output));
                    }

                    if let Some(native_id) = call.native_id.clone() {
                        tool_result_messages.push(ToolResultMessage {
                            tool_use_id: native_id,
                            tool_name: call.tool_name.clone(),
                            output,
                            is_error,
                        });
                    } else {
                        warn!(
                            "Missing native_id for tool call '{}'; skipping result.",
                            call.tool_name
                        );
                    }
                }
            }

            // Execute high-risk calls sequentially (with quorum review)
            for call in &high_risk_calls {
                // Quorum review for high-risk operations
                if !input.config.review_models.is_empty() {
                    let tool_call_json = serde_json::to_string_pretty(&serde_json::json!({
                        "tool": call.tool_name,
                        "args": call.arguments,
                    }))
                    .unwrap_or_default();

                    let review = self
                        .review_action(input, state, task, &tool_call_json, progress)
                        .await?;

                    progress.on_quorum_complete_with_votes(
                        "action_review",
                        review.approved,
                        &review.votes,
                        review.feedback.as_deref(),
                    );

                    if !review.approved {
                        warn!("Tool call {} rejected by quorum", call.tool_name);
                        if let Some(native_id) = call.native_id.clone() {
                            tool_result_messages.push(ToolResultMessage {
                                tool_use_id: native_id,
                                tool_name: call.tool_name.clone(),
                                output: "Action rejected by quorum review".to_string(),
                                is_error: true,
                            });
                        } else {
                            warn!(
                                "Missing native_id for tool call '{}'; skipping result.",
                                call.tool_name
                            );
                        }
                        continue;
                    }
                }

                progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));

                let result = self.tool_executor.execute(call).await;
                let is_error = !result.is_success();
                let output = if is_error {
                    result
                        .error()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string())
                } else {
                    result.output().unwrap_or("").to_string()
                };

                progress.on_tool_result(&call.tool_name, !is_error);

                if !is_error {
                    all_outputs.push(format!("[{}]: {}", call.tool_name, &output));
                }

                if let Some(native_id) = call.native_id.clone() {
                    tool_result_messages.push(ToolResultMessage {
                        tool_use_id: native_id,
                        tool_name: call.tool_name.clone(),
                        output,
                        is_error,
                    });
                } else {
                    warn!(
                        "Missing native_id for tool call '{}'; skipping result.",
                        call.tool_name
                    );
                }
            }

            // Send tool results back to LLM for next turn
            debug!(
                "Native tool use turn {}/{}: sending {} tool results",
                turn_count,
                max_turns,
                tool_result_messages.len()
            );

            response = session
                .send_tool_results(&tool_result_messages)
                .await
                .map_err(RunAgentError::GatewayError)?;
        }

        Ok(all_outputs.join("\n---\n"))
    }
}

/// Generate a simple timestamp-based ID
fn chrono_lite_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::{HilMode, Plan};

    #[test]
    fn test_run_agent_error_cancelled() {
        let error = RunAgentError::Cancelled;
        assert_eq!(error.to_string(), "Operation cancelled");
        assert!(error.is_cancelled());
    }

    #[test]
    fn test_run_agent_error_is_cancelled_false_for_other_errors() {
        let errors = vec![
            RunAgentError::InvalidConfig("test".to_string()),
            RunAgentError::PlanningFailed("test".to_string()),
            RunAgentError::MaxIterationsExceeded,
            RunAgentError::QuorumFailed,
        ];

        for error in errors {
            assert!(!error.is_cancelled(), "{:?} should not be cancelled", error);
        }
    }

    #[test]
    fn test_ensemble_planning_error() {
        let error = RunAgentError::EnsemblePlanningFailed("test error".to_string());
        assert_eq!(error.to_string(), "Ensemble planning failed: test error");
        assert!(!error.is_cancelled());
    }

    // ==================== Flow Test Infrastructure ====================

    use crate::ports::human_intervention::{HumanInterventionError, HumanInterventionPort};
    use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession, ToolResultMessage};
    use crate::ports::tool_executor::ToolExecutorPort;
    use async_trait::async_trait;
    use quorum_domain::session::response::{ContentBlock, LlmResponse, StopReason};
    use quorum_domain::tool::entities::{ToolCall, ToolDefinition, ToolSpec};
    use quorum_domain::tool::value_objects::ToolResult;
    use quorum_domain::{ConsensusLevel, ContextMode, InteractionType, PhaseScope};
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    /// A scripted response for the mock session
    #[derive(Debug, Clone)]
    enum ScriptedResponse {
        /// Plain text response (for send() / send_streaming())
        Text(String),
        /// Structured LlmResponse (for send_with_tools() / send_tool_results())
        Response(LlmResponse),
        /// Return an error
        Error(String),
    }

    /// Mock session that returns scripted responses in order
    struct ScriptedSession {
        model: Model,
        responses: Mutex<VecDeque<ScriptedResponse>>,
    }

    impl ScriptedSession {
        fn new(model: Model, responses: Vec<ScriptedResponse>) -> Self {
            Self {
                model,
                responses: Mutex::new(responses.into()),
            }
        }

        fn next_response(&self) -> ScriptedResponse {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ScriptedResponse::Text("(no more responses)".to_string()))
        }
    }

    #[async_trait]
    impl LlmSession for ScriptedSession {
        fn model(&self) -> &Model {
            &self.model
        }

        async fn send(&self, _content: &str) -> Result<String, GatewayError> {
            match self.next_response() {
                ScriptedResponse::Text(t) => Ok(t),
                ScriptedResponse::Response(r) => Ok(r.text_content()),
                ScriptedResponse::Error(e) => Err(GatewayError::RequestFailed(e)),
            }
        }

        async fn send_with_tools(
            &self,
            _content: &str,
            _tools: &[serde_json::Value],
        ) -> Result<LlmResponse, GatewayError> {
            match self.next_response() {
                ScriptedResponse::Text(t) => Ok(LlmResponse::from_text(t)),
                ScriptedResponse::Response(r) => Ok(r),
                ScriptedResponse::Error(e) => Err(GatewayError::RequestFailed(e)),
            }
        }

        async fn send_tool_results(
            &self,
            _results: &[ToolResultMessage],
        ) -> Result<LlmResponse, GatewayError> {
            match self.next_response() {
                ScriptedResponse::Text(t) => Ok(LlmResponse::from_text(t)),
                ScriptedResponse::Response(r) => Ok(r),
                ScriptedResponse::Error(e) => Err(GatewayError::RequestFailed(e)),
            }
        }
    }

    /// Mock gateway that creates ScriptedSessions based on model matching
    struct ScriptedGateway {
        /// Sessions keyed by model name — each key maps to a queue of response sets
        session_queues: Mutex<HashMap<String, VecDeque<Vec<ScriptedResponse>>>>,
        /// Fallback responses for any model not explicitly configured
        fallback_responses: Mutex<VecDeque<Vec<ScriptedResponse>>>,
        /// Track which sessions were created (for test assertions)
        created_sessions: Mutex<Vec<String>>,
    }

    impl ScriptedGateway {
        fn new() -> Self {
            Self {
                session_queues: Mutex::new(HashMap::new()),
                fallback_responses: Mutex::new(VecDeque::new()),
                created_sessions: Mutex::new(Vec::new()),
            }
        }

        /// Add a session script for a specific model
        fn add_session(&mut self, model: &str, responses: Vec<ScriptedResponse>) {
            self.session_queues
                .lock()
                .unwrap()
                .entry(model.to_string())
                .or_default()
                .push_back(responses);
        }

        /// Add a fallback session script (used when no model-specific session exists)
        fn add_fallback_session(&mut self, responses: Vec<ScriptedResponse>) {
            self.fallback_responses.lock().unwrap().push_back(responses);
        }

        fn get_session_responses(&self, model: &str) -> Vec<ScriptedResponse> {
            // Try model-specific queue first
            if let Some(queue) = self.session_queues.lock().unwrap().get_mut(model) {
                if let Some(responses) = queue.pop_front() {
                    return responses;
                }
            }
            // Try fallback
            if let Some(responses) = self.fallback_responses.lock().unwrap().pop_front() {
                return responses;
            }
            // Default: return a simple text response
            vec![ScriptedResponse::Text("(default response)".to_string())]
        }
    }

    #[async_trait]
    impl LlmGateway for ScriptedGateway {
        async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
            let model_str = model.to_string();
            self.created_sessions
                .lock()
                .unwrap()
                .push(model_str.clone());
            let responses = self.get_session_responses(&model_str);
            Ok(Box::new(ScriptedSession::new(model.clone(), responses)))
        }

        async fn create_session_with_system_prompt(
            &self,
            model: &Model,
            _system_prompt: &str,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.create_session(model).await
        }

        async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
            Ok(vec![Model::ClaudeSonnet45])
        }
    }

    /// Mock tool executor that records calls and returns success
    struct MockToolExecutor {
        spec: ToolSpec,
        calls: Mutex<Vec<String>>,
    }

    impl MockToolExecutor {
        fn new() -> Self {
            let spec = ToolSpec::new()
                .register(ToolDefinition::new(
                    "read_file",
                    "Read a file",
                    quorum_domain::RiskLevel::Low,
                ))
                .register(ToolDefinition::new(
                    "write_file",
                    "Write a file",
                    quorum_domain::RiskLevel::High,
                ));
            Self {
                spec,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ToolExecutorPort for MockToolExecutor {
        fn tool_spec(&self) -> &ToolSpec {
            &self.spec
        }

        async fn execute(&self, call: &ToolCall) -> ToolResult {
            self.calls.lock().unwrap().push(call.tool_name.clone());
            ToolResult::success(&call.tool_name, "ok")
        }

        fn execute_sync(&self, call: &ToolCall) -> ToolResult {
            self.calls.lock().unwrap().push(call.tool_name.clone());
            ToolResult::success(&call.tool_name, "ok")
        }
    }

    /// Mock HumanIntervention that returns a pre-configured decision
    struct MockHumanIntervention {
        intervention_decision: Mutex<HumanDecision>,
        execution_confirmation_decision: Mutex<HumanDecision>,
        /// Track how many times each method was called
        intervention_calls: Mutex<usize>,
        execution_confirmation_calls: Mutex<usize>,
    }

    impl MockHumanIntervention {
        fn with_execution_confirmation(decision: HumanDecision) -> Self {
            Self {
                intervention_decision: Mutex::new(HumanDecision::Approve),
                execution_confirmation_decision: Mutex::new(decision),
                intervention_calls: Mutex::new(0),
                execution_confirmation_calls: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl HumanInterventionPort for MockHumanIntervention {
        async fn request_intervention(
            &self,
            _request: &str,
            _plan: &Plan,
            _review_history: &[ReviewRound],
        ) -> Result<HumanDecision, HumanInterventionError> {
            *self.intervention_calls.lock().unwrap() += 1;
            Ok(self.intervention_decision.lock().unwrap().clone())
        }

        async fn request_execution_confirmation(
            &self,
            _request: &str,
            _plan: &Plan,
        ) -> Result<HumanDecision, HumanInterventionError> {
            *self.execution_confirmation_calls.lock().unwrap() += 1;
            Ok(self.execution_confirmation_decision.lock().unwrap().clone())
        }
    }

    /// Tracking progress notifier that records phase transitions
    struct TrackingProgress {
        phases: Mutex<Vec<AgentPhase>>,
        execution_confirmation_count: Mutex<usize>,
    }

    impl TrackingProgress {
        fn new() -> Self {
            Self {
                phases: Mutex::new(Vec::new()),
                execution_confirmation_count: Mutex::new(0),
            }
        }

        fn phases(&self) -> Vec<AgentPhase> {
            self.phases.lock().unwrap().clone()
        }

        fn has_phase(&self, phase: &AgentPhase) -> bool {
            self.phases.lock().unwrap().contains(phase)
        }

        fn execution_confirmation_count(&self) -> usize {
            *self.execution_confirmation_count.lock().unwrap()
        }
    }

    impl AgentProgressNotifier for TrackingProgress {
        fn on_phase_change(&self, phase: &AgentPhase) {
            self.phases.lock().unwrap().push(phase.clone());
        }

        fn on_execution_confirmation_required(&self, _request: &str, _plan: &Plan) {
            *self.execution_confirmation_count.lock().unwrap() += 1;
        }
    }

    /// Helper to create a plan as a ToolUse LlmResponse (Native Tool Use path)
    fn make_plan_response(objective: &str) -> ScriptedResponse {
        let mut input = HashMap::new();
        input.insert("objective".to_string(), serde_json::json!(objective));
        input.insert("reasoning".to_string(), serde_json::json!("test reasoning"));
        input.insert(
            "tasks".to_string(),
            serde_json::json!([
                {
                    "id": "1",
                    "description": "Read file",
                    "tool": "read_file",
                    "args": {"path": "test.txt"},
                    "depends_on": []
                }
            ]),
        );

        ScriptedResponse::Response(LlmResponse {
            content: vec![ContentBlock::ToolUse {
                id: "toolu_plan_001".to_string(),
                name: "create_plan".to_string(),
                input,
            }],
            stop_reason: Some(StopReason::ToolUse),
            model: None,
        })
    }

    /// Helper to create an "APPROVE" review response
    fn approve_response() -> String {
        "I APPROVE this plan. It looks good.".to_string()
    }

    /// Helper to create a "REJECT" review response
    fn reject_response() -> String {
        "I REJECT this plan. It needs changes.".to_string()
    }

    /// Builder for configuring and executing flow tests
    struct FlowTestBuilder {
        config: AgentConfig,
        gateway: ScriptedGateway,
        tool_executor: MockToolExecutor,
        human_intervention: Option<Arc<dyn HumanInterventionPort>>,
    }

    impl FlowTestBuilder {
        /// Solo + Full minimal configuration
        fn solo_full() -> Self {
            let config = AgentConfig {
                exploration_model: Model::ClaudeHaiku45,
                decision_model: Model::ClaudeSonnet45,
                review_models: vec![Model::ClaudeSonnet45],
                consensus_level: ConsensusLevel::Solo,
                phase_scope: PhaseScope::Full,
                interaction_type: InteractionType::Ask,
                context_mode: ContextMode::Shared,
                require_plan_review: true,
                require_final_review: false,
                max_iterations: 50,
                working_dir: None,
                max_tool_retries: 2,
                max_plan_revisions: 3,
                hil_mode: HilMode::Interactive,
                max_tool_turns: 3,
                orchestration_strategy: Default::default(),
                ensemble_session_timeout: None,
            };
            let mut gateway = ScriptedGateway::new();

            // Context gathering session (exploration model) - ends immediately
            gateway.add_session(
                &Model::ClaudeHaiku45.to_string(),
                vec![ScriptedResponse::Response(LlmResponse::from_text(
                    "Context gathered",
                ))],
            );

            // Planning session (decision model) - returns a plan
            gateway.add_session(
                &Model::ClaudeSonnet45.to_string(),
                vec![make_plan_response("Test plan")],
            );

            // Plan review session (review model) - APPROVE
            gateway.add_session(
                &Model::ClaudeSonnet45.to_string(),
                vec![ScriptedResponse::Text(approve_response())],
            );

            // Execution session (decision model) - simple completion
            gateway.add_session(
                &Model::ClaudeSonnet45.to_string(),
                vec![ScriptedResponse::Response(LlmResponse::from_text(
                    "Task completed successfully",
                ))],
            );

            Self {
                config,
                gateway,
                tool_executor: MockToolExecutor::new(),
                human_intervention: None,
            }
        }

        /// Solo + PlanOnly minimal configuration
        fn solo_plan_only() -> Self {
            let mut builder = Self::solo_full();
            builder.config.phase_scope = PhaseScope::PlanOnly;
            builder
        }

        /// Solo + Fast minimal configuration
        fn solo_fast() -> Self {
            let mut builder = Self::solo_full();
            builder.config.phase_scope = PhaseScope::Fast;
            // Fast skips plan review, so remove the review session
            // and ensure execution session is available
            builder
        }

        /// Ensemble + Fast minimal configuration
        ///
        /// 2 review models (ClaudeHaiku45, ClaudeSonnet45), each gets a planning session.
        /// By default, both return plans — override with `with_ensemble_plan_responses()`.
        fn ensemble_fast() -> Self {
            let config = AgentConfig {
                exploration_model: Model::ClaudeHaiku45,
                decision_model: Model::ClaudeSonnet45,
                review_models: vec![Model::ClaudeHaiku45, Model::ClaudeSonnet45],
                consensus_level: ConsensusLevel::Ensemble,
                phase_scope: PhaseScope::Fast,
                interaction_type: InteractionType::Ask,
                context_mode: ContextMode::Shared,
                require_plan_review: false,
                require_final_review: false,
                max_iterations: 50,
                working_dir: None,
                max_tool_retries: 2,
                max_plan_revisions: 3,
                hil_mode: HilMode::Interactive,
                max_tool_turns: 3,
                orchestration_strategy: Default::default(),
                ensemble_session_timeout: None,
            };
            let mut gateway = ScriptedGateway::new();

            // Context gathering session (exploration model)
            gateway.add_session(
                &Model::ClaudeHaiku45.to_string(),
                vec![ScriptedResponse::Response(LlmResponse::from_text(
                    "Context gathered",
                ))],
            );

            // Planning sessions for each review model (default: both return plans)
            gateway.add_session(
                &Model::ClaudeHaiku45.to_string(),
                vec![make_plan_response("Plan from Haiku")],
            );
            gateway.add_session(
                &Model::ClaudeSonnet45.to_string(),
                vec![make_plan_response("Plan from Sonnet")],
            );

            // Voting sessions (each model votes on the other's plan)
            gateway.add_session(
                &Model::ClaudeHaiku45.to_string(),
                vec![ScriptedResponse::Text("Score: 7/10".to_string())],
            );
            gateway.add_session(
                &Model::ClaudeSonnet45.to_string(),
                vec![ScriptedResponse::Text("Score: 8/10".to_string())],
            );

            // Execution session (decision model)
            gateway.add_session(
                &Model::ClaudeSonnet45.to_string(),
                vec![ScriptedResponse::Response(LlmResponse::from_text(
                    "Task completed successfully",
                ))],
            );

            Self {
                config,
                gateway,
                tool_executor: MockToolExecutor::new(),
                human_intervention: None,
            }
        }

        /// Replace ensemble planning responses for all review models
        fn with_ensemble_plan_responses(
            mut self,
            responses: Vec<(Model, ScriptedResponse)>,
        ) -> Self {
            let mut gateway = ScriptedGateway::new();

            // Context gathering session
            gateway.add_session(
                &self.config.exploration_model.to_string(),
                vec![ScriptedResponse::Response(LlmResponse::from_text(
                    "Context gathered",
                ))],
            );

            // Custom planning sessions for each model
            // For error responses, add two sessions: one for the initial attempt in the
            // JoinSet, and one for the sequential retry (retryable_models backoff).
            for (model, response) in responses {
                match &response {
                    ScriptedResponse::Error(_) => {
                        // Initial attempt session
                        gateway.add_session(&model.to_string(), vec![response.clone()]);
                        // Retry attempt session (same error)
                        gateway.add_session(&model.to_string(), vec![response]);
                    }
                    _ => {
                        gateway.add_session(&model.to_string(), vec![response]);
                    }
                }
            }

            self.gateway = gateway;
            self
        }

        fn with_phase_scope(mut self, scope: PhaseScope) -> Self {
            self.config.phase_scope = scope;
            self
        }

        fn with_human_intervention(mut self, intervention: Arc<dyn HumanInterventionPort>) -> Self {
            self.human_intervention = Some(intervention);
            self
        }

        fn with_hil_mode(mut self, mode: HilMode) -> Self {
            self.config.hil_mode = mode;
            self
        }

        /// Replace planning response with a custom one (for testing parse failures)
        fn with_plan_response(mut self, response: ScriptedResponse) -> Self {
            // Rebuild gateway: context session + custom plan response (Fast skips review)
            let mut gateway = ScriptedGateway::new();

            // Context gathering session
            gateway.add_session(
                &self.config.exploration_model.to_string(),
                vec![ScriptedResponse::Response(LlmResponse::from_text(
                    "Context gathered",
                ))],
            );

            // Custom planning session
            gateway.add_session(&self.config.decision_model.to_string(), vec![response]);

            self.gateway = gateway;
            self
        }

        async fn execute(self) -> (Result<RunAgentOutput, RunAgentError>, TrackingProgress) {
            let progress = TrackingProgress::new();
            let gateway = Arc::new(self.gateway);
            let executor = Arc::new(self.tool_executor);

            let mut use_case = RunAgentUseCase::new(gateway, executor);

            if let Some(intervention) = self.human_intervention {
                use_case = use_case.with_human_intervention(intervention);
            }

            let input = RunAgentInput::new("Test request", self.config);
            let result = use_case.execute_with_progress(input, &progress).await;

            (result, progress)
        }
    }

    // ==================== Flow Tests ====================

    #[tokio::test]
    async fn test_solo_full_flow_happy_path() {
        let (result, progress) = FlowTestBuilder::solo_full().execute().await;

        let output = result.expect("should succeed");
        assert!(output.success);
        assert_eq!(output.state.phase, AgentPhase::Completed);

        // Verify expected phase transitions
        assert!(progress.has_phase(&AgentPhase::ContextGathering));
        assert!(progress.has_phase(&AgentPhase::Planning));
        assert!(progress.has_phase(&AgentPhase::PlanReview));
        assert!(progress.has_phase(&AgentPhase::Executing));
    }

    #[tokio::test]
    async fn test_plan_only_skips_execution() {
        let (result, progress) = FlowTestBuilder::solo_plan_only().execute().await;

        let output = result.expect("should succeed");
        assert!(output.success);
        assert!(output.summary.contains("plan-only"));
        assert!(output.state.plan.is_some());
        assert_eq!(output.state.phase, AgentPhase::Completed);

        // Plan is created but execution never happens
        assert!(progress.has_phase(&AgentPhase::ContextGathering));
        assert!(progress.has_phase(&AgentPhase::Planning));
        // PlanOnly skips both plan review and execution
        assert!(!progress.has_phase(&AgentPhase::PlanReview));
        assert!(!progress.has_phase(&AgentPhase::Executing));
    }

    #[tokio::test]
    async fn test_fast_skips_plan_review() {
        let (result, progress) = FlowTestBuilder::solo_fast().execute().await;

        let output = result.expect("should succeed");
        assert!(output.success);
        assert_eq!(output.state.phase, AgentPhase::Completed);

        // Fast mode: planning happens, review is skipped, execution proceeds
        assert!(progress.has_phase(&AgentPhase::ContextGathering));
        assert!(progress.has_phase(&AgentPhase::Planning));
        assert!(!progress.has_phase(&AgentPhase::PlanReview));
        assert!(progress.has_phase(&AgentPhase::Executing));
    }

    #[tokio::test]
    async fn test_full_includes_plan_review() {
        let (result, progress) = FlowTestBuilder::solo_full().execute().await;

        let output = result.expect("should succeed");
        assert!(output.success);

        // Full mode includes plan review
        assert!(progress.has_phase(&AgentPhase::PlanReview));
    }

    #[tokio::test]
    async fn test_full_execution_confirmation_reject_stops() {
        let mock_hil = Arc::new(MockHumanIntervention::with_execution_confirmation(
            HumanDecision::Reject,
        ));

        let (result, progress) = FlowTestBuilder::solo_full()
            .with_human_intervention(mock_hil.clone())
            .execute()
            .await;

        let output = result.expect("should succeed (graceful stop, not error)");
        assert!(output.success);
        assert!(output.summary.contains("not executed"));

        // Plan review happened but execution did not
        assert!(progress.has_phase(&AgentPhase::PlanReview));
        assert!(!progress.has_phase(&AgentPhase::Executing));
        assert_eq!(output.state.phase, AgentPhase::Completed);

        // Execution confirmation was called
        assert_eq!(*mock_hil.execution_confirmation_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_full_execution_confirmation_approve_continues() {
        let mock_hil = Arc::new(MockHumanIntervention::with_execution_confirmation(
            HumanDecision::Approve,
        ));

        let (result, progress) = FlowTestBuilder::solo_full()
            .with_human_intervention(mock_hil.clone())
            .execute()
            .await;

        let output = result.expect("should succeed");
        assert!(output.success);

        // Both plan review and execution happened
        assert!(progress.has_phase(&AgentPhase::PlanReview));
        assert!(progress.has_phase(&AgentPhase::Executing));

        // Execution confirmation was called
        assert_eq!(*mock_hil.execution_confirmation_calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn test_fast_skips_execution_confirmation() {
        let mock_hil = Arc::new(MockHumanIntervention::with_execution_confirmation(
            HumanDecision::Reject, // Would reject if called
        ));

        let (result, _progress) = FlowTestBuilder::solo_fast()
            .with_human_intervention(mock_hil.clone())
            .execute()
            .await;

        let output = result.expect("should succeed");
        assert!(output.success);

        // Fast mode never calls execution confirmation
        assert_eq!(*mock_hil.execution_confirmation_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_plan_only_skips_execution_confirmation() {
        let mock_hil = Arc::new(MockHumanIntervention::with_execution_confirmation(
            HumanDecision::Reject,
        ));

        let (result, _progress) = FlowTestBuilder::solo_plan_only()
            .with_human_intervention(mock_hil.clone())
            .execute()
            .await;

        let output = result.expect("should succeed");
        assert!(output.success);

        // PlanOnly never calls execution confirmation
        assert_eq!(*mock_hil.execution_confirmation_calls.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_hil_auto_approve_skips_execution_confirmation_prompt() {
        // With auto_approve hil_mode, execution confirmation should auto-approve
        let (result, _progress) = FlowTestBuilder::solo_full()
            .with_hil_mode(HilMode::AutoApprove)
            .execute()
            .await;

        let output = result.expect("should succeed");
        assert!(output.success);
        assert!(output.state.phase == AgentPhase::Completed);
    }

    // ==================== Plan Parse Failure Flow Tests ====================

    #[tokio::test]
    async fn test_text_response_without_plan_succeeds() {
        // LLM がプラン不要と判断してテキストだけ返した場合、正常終了するべき
        let (result, progress) = FlowTestBuilder::solo_fast()
            .with_plan_response(ScriptedResponse::Text(
                "Hello! How can I help you today?".into(),
            ))
            .execute()
            .await;

        let output = result.expect("should return output (not panic)");
        assert!(
            output.success,
            "Agent should succeed with text-only response, got: {}",
            output.summary
        );
        assert_eq!(output.summary, "Hello! How can I help you today?");
        // Planning should have been attempted
        assert!(progress.has_phase(&AgentPhase::Planning));
        // Execution should NOT have been reached (no plan = no execution)
        assert!(
            !progress.has_phase(&AgentPhase::Executing),
            "Should not reach execution with text-only response"
        );
        // State should be completed
        assert_eq!(output.state.phase, AgentPhase::Completed);
    }

    #[tokio::test]
    async fn test_empty_tasks_native_tool_use_falls_back_to_text() {
        // Native Tool Use で create_plan を呼んだがタスク 0 個の場合、
        // extract_plan_from_response が None → テキストフォールバック
        let mut input = HashMap::new();
        input.insert("objective".to_string(), serde_json::json!("Do something"));
        input.insert("reasoning".to_string(), serde_json::json!("because"));
        input.insert("tasks".to_string(), serde_json::json!([]));

        // ToolUse with empty tasks + text content
        let response = LlmResponse {
            content: vec![
                ContentBlock::Text("I'll help with that.".to_string()),
                ContentBlock::ToolUse {
                    id: "toolu_001".to_string(),
                    name: "create_plan".to_string(),
                    input,
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
            model: None,
        };

        // After extract fails, generate_plan_from_session retries by sending
        // a tool_result error. Provide a second response for that retry —
        // the LLM gives up on tools and responds with text only.
        let mut builder = FlowTestBuilder::solo_fast();
        let mut gateway = ScriptedGateway::new();
        gateway.add_session(
            &builder.config.exploration_model.to_string(),
            vec![ScriptedResponse::Response(LlmResponse::from_text(
                "Context gathered",
            ))],
        );
        gateway.add_session(
            &builder.config.decision_model.to_string(),
            vec![
                ScriptedResponse::Response(response),
                // Retry response: LLM gives up on tool use, returns text
                ScriptedResponse::Text("I'll help with that.".to_string()),
            ],
        );
        builder.gateway = gateway;

        let (result, progress) = builder.execute().await;

        let output = result.expect("should return output (not panic)");
        // Empty tasks → extract fails → retry → text fallback → success
        assert!(
            output.success,
            "Agent should succeed with text fallback, got: {}",
            output.summary
        );
        assert_eq!(output.summary, "I'll help with that.");
        assert!(progress.has_phase(&AgentPhase::Planning));
        assert!(!progress.has_phase(&AgentPhase::Executing));
    }

    #[tokio::test]
    async fn test_hil_auto_reject_stops_at_execution_confirmation() {
        let (result, progress) = FlowTestBuilder::solo_full()
            .with_hil_mode(HilMode::AutoReject)
            .execute()
            .await;

        let output = result.expect("should succeed (graceful stop)");
        assert!(output.success);
        assert!(output.summary.contains("not executed"));

        // Execution phase was never entered
        assert!(!progress.has_phase(&AgentPhase::Executing));
    }

    // ==================== Ensemble Planning Flow Tests ====================

    #[tokio::test]
    async fn test_ensemble_all_text_response_synthesized() {
        // 全モデルがテキストのみ返した場合、モデレーター合成で成功するべき
        let mut builder = FlowTestBuilder::ensemble_fast().with_ensemble_plan_responses(vec![
            (
                Model::ClaudeHaiku45,
                ScriptedResponse::Text("Hello! I can help you with that.".into()),
            ),
            (
                Model::ClaudeSonnet45,
                ScriptedResponse::Text("Hi there! What do you need?".into()),
            ),
        ]);

        // Moderator synthesis session (decision_model = ClaudeSonnet45)
        builder.gateway.add_session(
            &Model::ClaudeSonnet45.to_string(),
            vec![ScriptedResponse::Text(
                "Synthesized: Both models offered to help.".into(),
            )],
        );

        let (result, progress) = builder.execute().await;

        let output = result.expect("should succeed via text synthesis");
        assert!(
            output.success,
            "Should succeed with synthesized text response, got: {}",
            output.summary
        );
        assert!(progress.has_phase(&AgentPhase::Planning));
        // No plan should be set — text responses don't generate plans
        assert!(
            output.state.plan.is_none(),
            "No plan should be set for text-only responses"
        );
        assert!(
            output.summary.contains("Synthesized"),
            "Summary should contain synthesized text, got: {}",
            output.summary
        );
    }

    #[tokio::test]
    async fn test_ensemble_partial_plan_success() {
        // 1モデルがプラン、1モデルがテキスト → プランが使われる
        let (result, progress) = FlowTestBuilder::ensemble_fast()
            .with_ensemble_plan_responses(vec![
                (
                    Model::ClaudeHaiku45,
                    ScriptedResponse::Text("I don't think we need a plan for this.".into()),
                ),
                (Model::ClaudeSonnet45, make_plan_response("Sonnet's plan")),
            ])
            .execute()
            .await;

        let output = result.expect("should succeed with partial plan");
        assert!(
            output.success,
            "Ensemble should succeed when at least one model returns a plan, got: {}",
            output.summary
        );
        // Plan should have been set (the one that succeeded)
        assert!(
            output.state.plan.is_some(),
            "Plan should be set from the successful model"
        );
        assert!(progress.has_phase(&AgentPhase::Planning));
        // With Fast mode, execution should proceed
        assert!(progress.has_phase(&AgentPhase::Executing));
    }

    #[tokio::test]
    async fn test_ensemble_all_models_fail_falls_back_to_solo() {
        // 全モデルがエラー → Solo フォールバック → Solo で成功
        let mut builder = FlowTestBuilder::ensemble_fast().with_ensemble_plan_responses(vec![
            (
                Model::ClaudeHaiku45,
                ScriptedResponse::Error("API error".into()),
            ),
            (
                Model::ClaudeSonnet45,
                ScriptedResponse::Error("API error".into()),
            ),
        ]);

        // Solo fallback needs a planning session for decision_model
        builder.gateway.add_session(
            &Model::ClaudeSonnet45.to_string(),
            vec![make_plan_response("Solo fallback plan")],
        );
        // Solo execution session
        builder.gateway.add_session(
            &Model::ClaudeSonnet45.to_string(),
            vec![ScriptedResponse::Response(LlmResponse::from_text(
                "Task completed",
            ))],
        );

        let (result, progress) = builder.execute().await;

        let output = result.expect("should succeed via solo fallback");
        assert!(
            output.success,
            "Should succeed via solo fallback when ensemble fails, got: {}",
            output.summary
        );
        assert!(progress.has_phase(&AgentPhase::Planning));
        assert!(
            output.state.plan.is_some(),
            "Plan should be set from solo fallback"
        );
    }

    #[tokio::test]
    async fn test_ensemble_and_solo_both_fail_returns_error() {
        // 全 ensemble モデルがエラー + Solo もエラー → 失敗
        let mut builder = FlowTestBuilder::ensemble_fast().with_ensemble_plan_responses(vec![
            (
                Model::ClaudeHaiku45,
                ScriptedResponse::Error("API error".into()),
            ),
            (
                Model::ClaudeSonnet45,
                ScriptedResponse::Error("API error".into()),
            ),
        ]);

        // Solo fallback also fails
        builder.gateway.add_session(
            &Model::ClaudeSonnet45.to_string(),
            vec![ScriptedResponse::Error("Solo also failed".into())],
        );

        let (result, _progress) = builder.execute().await;

        let output = result.expect("should return output (not panic)");
        assert!(
            !output.success,
            "Should fail when both ensemble and solo fail"
        );
    }
}
