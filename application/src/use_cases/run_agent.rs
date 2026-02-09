//! Run Agent use case
//!
//! Orchestrates the agent execution flow with quorum integration:
//! 1. Context Gathering - Understand the project
//! 2. Planning - Create a task plan (single model)
//! 3. Plan Review - Quorum reviews the plan (REQUIRED)
//! 4. Executing - Execute tasks using tools
//!    - Action Review - Quorum reviews high-risk operations
//! 5. Final Review - Quorum reviews results (optional)

use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::human_intervention::{HumanInterventionError, HumanInterventionPort};
use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession, ToolResultMessage};
use crate::ports::tool_executor::ToolExecutorPort;
use quorum_domain::core::string::truncate;
use quorum_domain::session::response::LlmResponse;
use quorum_domain::{
    AgentConfig, AgentContext, AgentPhase, AgentPromptTemplate, AgentState, EnsemblePlanResult,
    HilMode, HumanDecision, Model, ModelVote, Plan, PlanCandidate, ProjectContext, ReviewRound,
    StreamEvent, Task, TaskId, Thought,
};
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

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

/// Input for the RunAgent use case
#[derive(Debug, Clone)]
pub struct RunAgentInput {
    /// The user's request
    pub request: String,
    /// Agent configuration
    pub config: AgentConfig,
}

impl RunAgentInput {
    pub fn new(request: impl Into<String>, config: AgentConfig) -> Self {
        Self {
            request: request.into(),
            config,
        }
    }

    pub fn with_model(request: impl Into<String>, model: Model) -> Self {
        Self {
            request: request.into(),
            config: AgentConfig::new(model),
        }
    }
}

/// Output from the RunAgent use case
#[derive(Debug, Clone)]
pub struct RunAgentOutput {
    /// Final state of the agent
    pub state: AgentState,
    /// Summary of what was accomplished
    pub summary: String,
    /// Whether the agent completed successfully
    pub success: bool,
}

/// Error category for display purposes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Tool doesn't exist (e.g., multi_tool_use.parallel)
    UnknownTool,
    /// Tool arguments are invalid
    ValidationError,
    /// Tool execution failed
    ExecutionError,
}

impl ErrorCategory {
    /// Get emoji for this error category
    pub fn emoji(&self) -> &'static str {
        match self {
            ErrorCategory::UnknownTool => "🔧",
            ErrorCategory::ValidationError => "⚠️",
            ErrorCategory::ExecutionError => "❌",
        }
    }

    /// Get description for this error category
    pub fn description(&self) -> &'static str {
        match self {
            ErrorCategory::UnknownTool => "Unknown tool requested",
            ErrorCategory::ValidationError => "Invalid arguments",
            ErrorCategory::ExecutionError => "Execution failed",
        }
    }

    /// Determine category from error code
    pub fn from_error_code(code: &str) -> Self {
        match code {
            "NOT_FOUND" => ErrorCategory::UnknownTool,
            "INVALID_ARGUMENT" => ErrorCategory::ValidationError,
            _ => ErrorCategory::ExecutionError,
        }
    }
}

/// Progress notifier for agent execution
///
/// This trait provides callbacks for various stages of agent execution,
/// allowing UI implementations to display progress to the user.
///
/// All methods have default no-op implementations, so implementers only
/// need to override the callbacks they care about.
///
/// # Callback Categories
///
/// - **Phase callbacks**: Track high-level execution phases
/// - **Task callbacks**: Track individual task execution
/// - **Tool callbacks**: Track tool calls and results
/// - **Quorum callbacks**: Track multi-model voting (Solo mode)
/// - **Ensemble callbacks**: Track multi-model planning (Ensemble mode)
///
/// # Example Implementation
///
/// ```ignore
/// use quorum_application::use_cases::run_agent::AgentProgressNotifier;
///
/// struct MyProgress;
///
/// impl AgentProgressNotifier for MyProgress {
///     fn on_phase_change(&self, phase: &AgentPhase) {
///         println!("Phase: {}", phase);
///     }
///
///     fn on_ensemble_complete(&self, model: &Model, score: f64) {
///         println!("Selected: {} ({:.1}/10)", model, score);
///     }
/// }
/// ```
pub trait AgentProgressNotifier: Send + Sync {
    /// Called when the agent transitions to a new phase
    fn on_phase_change(&self, _phase: &AgentPhase) {}

    /// Called when the agent records a reasoning step
    fn on_thought(&self, _thought: &Thought) {}

    /// Called when a task begins execution
    fn on_task_start(&self, _task: &Task) {}

    /// Called when a task completes (success or failure)
    fn on_task_complete(&self, _task: &Task, _success: bool) {}

    /// Called when a tool is invoked
    fn on_tool_call(&self, _tool_name: &str, _args: &str) {}

    /// Called when a tool returns a result
    fn on_tool_result(&self, _tool_name: &str, _success: bool) {}

    /// Called when a tool execution fails with details about the error
    fn on_tool_error(&self, _tool_name: &str, _category: ErrorCategory, _message: &str) {}

    /// Called when retrying a tool call after an error
    fn on_tool_retry(&self, _tool_name: &str, _attempt: usize, _max_retries: usize, _error: &str) {}

    /// Called when a tool name is not found in the registry
    fn on_tool_not_found(&self, _tool_name: &str, _available_tools: &[&str]) {}

    /// Called when an unknown tool name has been resolved to a valid tool
    fn on_tool_resolved(&self, _original_name: &str, _resolved_name: &str) {}

    // ==================== LLM Streaming Callbacks ====================

    /// Called for each text chunk received during LLM streaming.
    fn on_llm_chunk(&self, _chunk: &str) {}

    /// Called when LLM streaming begins.
    fn on_llm_stream_start(&self, _purpose: &str) {}

    /// Called when LLM streaming ends.
    fn on_llm_stream_end(&self) {}

    // ==================== Plan Revision Callbacks ====================

    /// Called when a plan revision is triggered after rejection
    fn on_plan_revision(&self, _revision: usize, _feedback: &str) {}

    /// Called when an action is being retried after rejection
    fn on_action_retry(&self, _task: &Task, _attempt: usize, _feedback: &str) {}

    // ==================== Quorum Callbacks (Solo Mode) ====================

    /// Called when quorum voting begins
    ///
    /// # Arguments
    /// * `phase` - The review phase (e.g., "plan_review", "action_review")
    /// * `model_count` - Number of models participating in the vote
    fn on_quorum_start(&self, _phase: &str, _model_count: usize) {}

    /// Called when a single model completes its vote
    fn on_quorum_model_complete(&self, _model: &Model, _approved: bool) {}

    /// Called when quorum voting completes
    fn on_quorum_complete(&self, _phase: &str, _approved: bool, _feedback: Option<&str>) {}

    /// Called when quorum voting completes with detailed vote information
    ///
    /// # Arguments
    /// * `votes` - Vec of (model_name, approved, reasoning) tuples
    fn on_quorum_complete_with_votes(
        &self,
        _phase: &str,
        _approved: bool,
        _votes: &[(String, bool, String)],
        _feedback: Option<&str>,
    ) {
    }

    /// Called when human intervention is required due to plan revision limit
    fn on_human_intervention_required(
        &self,
        _request: &str,
        _plan: &Plan,
        _review_history: &[ReviewRound],
        _max_revisions: usize,
    ) {
    }

    // ==================== Ensemble Planning Callbacks ====================

    /// Called when ensemble planning starts (Ensemble mode only)
    ///
    /// In ensemble mode, multiple models generate plans independently.
    /// This is called at the start of that process.
    ///
    /// # Arguments
    /// * `model_count` - Number of models that will generate plans
    fn on_ensemble_start(&self, _model_count: usize) {}

    /// Called when a model finishes generating its plan
    ///
    /// Called once per model as they complete plan generation.
    fn on_ensemble_plan_generated(&self, _model: &Model) {}

    /// Called when ensemble voting starts
    ///
    /// After all plans are generated, models vote on each other's plans.
    ///
    /// # Arguments
    /// * `plan_count` - Number of plans to be voted on
    fn on_ensemble_voting_start(&self, _plan_count: usize) {}

    /// Called when ensemble planning completes with the selected plan
    ///
    /// # Arguments
    /// * `selected_model` - The model whose plan was selected
    /// * `score` - The average score (1-10) the selected plan received
    fn on_ensemble_complete(&self, _selected_model: &Model, _score: f64) {}
}

/// No-op implementation for when progress isn't needed
pub struct NoAgentProgress;

impl AgentProgressNotifier for NoAgentProgress {}

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

/// Use case for running an autonomous agent
pub struct RunAgentUseCase<
    G: LlmGateway + 'static,
    T: ToolExecutorPort + 'static,
    C: ContextLoaderPort + 'static = NoContextLoader,
> {
    gateway: Arc<G>,
    tool_executor: Arc<T>,
    context_loader: Option<Arc<C>>,
    cancellation_token: Option<CancellationToken>,
    human_intervention: Option<Arc<dyn HumanInterventionPort>>,
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

                let ensemble_result = match self
                    .create_ensemble_plans(
                        &input,
                        &state.context,
                        &system_prompt,
                        plan_feedback.as_deref(),
                        progress,
                    )
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        state.fail(format!("Ensemble planning failed: {}", e));
                        return Ok(RunAgentOutput {
                            summary: format!("Agent failed during ensemble planning: {}", e),
                            success: false,
                            state,
                        });
                    }
                };

                // Get the selected plan
                let selected = ensemble_result.selected().ok_or_else(|| {
                    RunAgentError::EnsemblePlanningFailed("No plan was selected".to_string())
                })?;

                state.add_thought(Thought::planning(format!(
                    "Ensemble selected plan from {} with score {:.1}/10: {}",
                    selected.model,
                    selected.average_score(),
                    selected.plan.objective
                )));

                // Log the summary
                info!("Ensemble planning result:\n{}", ensemble_result.summary());

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

            // ==================== Single (Solo) Planning ====================
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
                Ok(plan) => {
                    state.add_thought(Thought::planning(format!(
                        "Created plan with {} tasks: {}",
                        plan.tasks.len(),
                        plan.objective
                    )));
                    plan
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

            // Phase 3: Plan Review (Quorum) - REQUIRED for Solo mode
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

        // Phase 5: Final Review (optional)
        if input.config.require_final_review {
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

    /// Create a plan for the task
    async fn create_plan(
        &self,
        session: &dyn LlmSession,
        request: &str,
        context: &AgentContext,
        previous_feedback: Option<&str>,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<Plan, RunAgentError> {
        let prompt =
            AgentPromptTemplate::planning_with_feedback(request, context, previous_feedback);

        let response = match self
            .send_with_cancellation(session, &prompt, progress)
            .await
        {
            Ok(response) => response,
            Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
            Err(e) => return Err(RunAgentError::PlanningFailed(e.to_string())),
        };

        // Parse the plan from the response
        parse_plan(&response).ok_or_else(|| {
            RunAgentError::PlanningFailed("Failed to parse plan from model response".to_string())
        })
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
    /// See `docs/features/ensemble-mode.md` for detailed design rationale.
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
    async fn create_ensemble_plans(
        &self,
        input: &RunAgentInput,
        context: &AgentContext,
        system_prompt: &str,
        previous_feedback: Option<&str>,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<EnsemblePlanResult, RunAgentError> {
        let models = &input.config.review_models;

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

        let prompt =
            AgentPromptTemplate::planning_with_feedback(&input.request, context, previous_feedback);

        let mut join_set = JoinSet::new();

        for model in models {
            let gateway = Arc::clone(&self.gateway);
            let model = model.clone();
            let prompt = prompt.clone();
            let system_prompt = system_prompt.to_string();

            join_set.spawn(async move {
                let session = gateway
                    .create_session_with_system_prompt(&model, &system_prompt)
                    .await?;
                let response = session.send(&prompt).await?;
                let plan = parse_plan(&response)
                    .ok_or_else(|| GatewayError::Other("Failed to parse plan".to_string()))?;
                Ok::<(Model, Plan), GatewayError>((model, plan))
            });
        }

        // Collect generated plans with cancellation support
        let mut candidates: Vec<PlanCandidate> = Vec::new();

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
                Ok(Ok((model, plan))) => {
                    info!("Model {} generated plan: {}", model, plan.objective);
                    progress.on_ensemble_plan_generated(&model);
                    candidates.push(PlanCandidate::new(model, plan));
                }
                Ok(Err(e)) => {
                    warn!("Model failed to generate plan: {}", e);
                }
                Err(e) => {
                    warn!("Task join error: {}", e);
                }
            }
        }

        if candidates.is_empty() {
            return Err(RunAgentError::EnsemblePlanningFailed(
                "All models failed to generate plans".to_string(),
            ));
        }

        if candidates.len() == 1 {
            // Only one plan succeeded, use it directly
            info!("Only one plan generated, selecting it directly");
            return Ok(EnsemblePlanResult::new(candidates, 0));
        }

        // Step 2: Each model votes on the other models' plans
        info!("Ensemble Step 2: Voting on {} plans", candidates.len());
        progress.on_ensemble_voting_start(candidates.len());

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
                    let session = gateway
                        .create_session_with_system_prompt(&voter_model, &system_prompt)
                        .await?;
                    let response = session.send(&voting_prompt).await?;
                    let score = parse_vote_score(&response);
                    Ok::<(String, f64), GatewayError>((voter_model.to_string(), score))
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
                        candidates[i].add_vote(&voter, score);
                    }
                    Ok(Err(e)) => {
                        warn!("Voting failed: {}", e);
                    }
                    Err(e) => {
                        warn!("Voting task join error: {}", e);
                    }
                }
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
            progress.on_ensemble_complete(&selected.model, selected.average_score());
        }

        Ok(result)
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

    /// Handle human intervention when plan revision limit is exceeded
    async fn handle_human_intervention(
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
            input.config.max_plan_revisions,
        );

        // Determine decision based on HiL mode
        match input.config.hil_mode {
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

    /// Check if a tool is high-risk (requires quorum review)
    fn is_high_risk_tool(&self, tool_name: &str) -> bool {
        if let Some(definition) = self.tool_executor.get_tool(tool_name) {
            definition.is_high_risk()
        } else {
            // Unknown tools are considered high-risk by default
            true
        }
    }

    // ==================== Quorum Review Methods ====================

    /// Review the plan using quorum (multiple models vote)
    async fn review_plan(
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
        if !input.config.require_plan_review {
            info!("Plan review disabled, auto-approving plan");
            return Ok(QuorumReviewResult {
                approved: true,
                votes: vec![],
                feedback: None,
            });
        }

        let models = &input.config.review_models;
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
                let result = Self::query_model_for_review(&gateway, &model, &prompt).await;
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
        // Note: UI notification is handled by the caller (execute_with_progress)
        // to maintain separation between business logic and presentation

        Ok(result)
    }

    /// Review a high-risk action using quorum
    async fn review_action(
        &self,
        input: &RunAgentInput,
        state: &AgentState,
        task: &Task,
        tool_call_json: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<QuorumReviewResult, RunAgentError> {
        let models = &input.config.review_models;
        if models.is_empty() {
            // No quorum models configured, auto-approve
            return Ok(QuorumReviewResult {
                approved: true,
                votes: vec![],
                feedback: None,
            });
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
                let result = Self::query_model_for_review(&gateway, &model, &prompt).await;
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
        // Note: UI notification is handled by the caller (execute_single_task)
        // to maintain separation between business logic and presentation

        Ok(result)
    }

    /// Final review of agent results using quorum (optional)
    async fn final_review(
        &self,
        input: &RunAgentInput,
        state: &AgentState,
        results_summary: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<QuorumReviewResult, RunAgentError> {
        let plan = state.plan.as_ref().ok_or_else(|| {
            RunAgentError::TaskExecutionFailed("No plan for final review".to_string())
        })?;

        let models = &input.config.review_models;
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
                let result = Self::query_model_for_review(&gateway, &model, &prompt).await;
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

    /// Query a single model for review
    async fn query_model_for_review(
        gateway: &G,
        model: &Model,
        prompt: &str,
    ) -> Result<String, GatewayError> {
        let system_prompt = "You are a code reviewer evaluating plans and actions. \
            Provide your assessment with a clear APPROVE or REJECT/REVISE recommendation.";

        let session = gateway
            .create_session_with_system_prompt(model, system_prompt)
            .await?;

        session.send(prompt).await
    }
}

/// Parse a review response to extract approval status and feedback
fn parse_review_response(response: &str) -> (bool, String) {
    let response_upper = response.to_uppercase();

    // Check for explicit approval/rejection keywords
    let approved = response_upper.contains("APPROVE")
        && !response_upper.contains("NOT APPROVE")
        && !response_upper.contains("DON'T APPROVE")
        && !response_upper.contains("CANNOT APPROVE");

    let rejected = response_upper.contains("REJECT")
        || response_upper.contains("REVISE")
        || response_upper.contains("NOT APPROVE")
        || response_upper.contains("CANNOT APPROVE");

    // If explicitly rejected, return false
    // If explicitly approved and not rejected, return true
    // Otherwise, default to false (conservative)
    let is_approved = approved && !rejected;

    (is_approved, response.to_string())
}

/// Parse a final review response
fn parse_final_review_response(response: &str) -> (bool, String) {
    let response_upper = response.to_uppercase();

    // Look for SUCCESS/PARTIAL/FAILURE
    let success = response_upper.contains("SUCCESS")
        && !response_upper.contains("PARTIAL")
        && !response_upper.contains("FAILURE");

    (success, response.to_string())
}

/// Parse a vote score from ensemble voting response
///
/// Parses the model's voting response to extract a numerical score (1-10).
/// Supports multiple response formats for robustness.
///
/// # Supported Formats
///
/// 1. **JSON** (preferred): `{"score": 8, "reasoning": "..."}`
/// 2. **Fraction**: `8/10` or `Score: 7/10`
/// 3. **Standalone number**: `9` (if in valid range 1-10)
///
/// # Return Value
///
/// - Returns the parsed score clamped to 1.0-10.0
/// - Returns 5.0 (neutral) if parsing fails
///
/// # Examples
///
/// ```ignore
/// assert_eq!(parse_vote_score(r#"{"score": 8, "reasoning": "Good"}"#), 8.0);
/// assert_eq!(parse_vote_score("I rate this 7/10"), 7.0);
/// assert_eq!(parse_vote_score("Score: 9"), 9.0);
/// assert_eq!(parse_vote_score("No numbers here"), 5.0); // fallback
/// ```
fn parse_vote_score(response: &str) -> f64 {
    // Try to find JSON in the response
    if let Some(start) = response.find('{')
        && let Some(end) = response[start..].rfind('}')
    {
        let json_str = &response[start..start + end + 1];
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str)
            && let Some(score) = parsed.get("score").and_then(|v| v.as_f64())
        {
            // Clamp to valid range
            return score.clamp(1.0, 10.0);
        }
    }

    // Fallback: try to find a number that looks like a score
    // Look for patterns like "8/10" or "score: 8" or just a standalone number
    for word in response.split_whitespace() {
        // Check for "N/10" pattern
        if let Some(num_str) = word.strip_suffix("/10")
            && let Ok(num) = num_str.parse::<f64>()
        {
            return num.clamp(1.0, 10.0);
        }
        // Check for standalone number (1-10)
        if let Ok(num) = word
            .trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<f64>()
            && (1.0..=10.0).contains(&num)
        {
            return num;
        }
    }

    // Default to middle score if parsing fails
    5.0
}

/// Parse a plan from model response
fn parse_plan(response: &str) -> Option<Plan> {
    // Look for ```plan ... ``` blocks
    let mut in_plan_block = false;
    let mut current_block = String::new();

    for line in response.lines() {
        if line.trim() == "```plan" {
            in_plan_block = true;
            current_block.clear();
        } else if in_plan_block && line.trim() == "```" {
            in_plan_block = false;
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_block) {
                return parse_plan_json(&parsed);
            }
        } else if in_plan_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    // Try parsing the entire response as JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(response) {
        return parse_plan_json(&parsed);
    }

    // Fallback: create a simple plan from the response
    Some(Plan::new(
        "Execute user request",
        response.chars().take(200).collect::<String>(),
    ))
}

fn parse_plan_json(json: &serde_json::Value) -> Option<Plan> {
    let objective = json.get("objective")?.as_str()?;
    let reasoning = json.get("reasoning").and_then(|v| v.as_str()).unwrap_or("");

    let mut plan = Plan::new(objective, reasoning);

    if let Some(tasks) = json.get("tasks").and_then(|v| v.as_array()) {
        for task_json in tasks {
            let id = task_json
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let description = task_json
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("No description");

            let mut task = Task::new(id, description);

            if let Some(tool) = task_json.get("tool").and_then(|v| v.as_str())
                && tool != "null"
                && !tool.is_empty()
            {
                task = task.with_tool(tool);
            }

            if let Some(args) = task_json.get("args").and_then(|v| v.as_object()) {
                for (key, value) in args {
                    task = task.with_arg(key, value.clone());
                }
            }

            if let Some(deps) = task_json.get("depends_on").and_then(|v| v.as_array()) {
                for dep in deps {
                    if let Some(dep_id) = dep.as_str() {
                        task = task.with_dependency(dep_id);
                    }
                }
            }

            plan.add_task(task);
        }
    }

    Some(plan)
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

    #[test]
    fn test_parse_plan() {
        let response = r#"
Here's my plan:

```plan
{
  "objective": "Update the README",
  "reasoning": "The README needs updating",
  "tasks": [
    {
      "id": "1",
      "description": "Read current README",
      "tool": "read_file",
      "args": {"path": "README.md"},
      "depends_on": []
    },
    {
      "id": "2",
      "description": "Write updated README",
      "tool": "write_file",
      "args": {"path": "README.md", "content": "..."},
      "depends_on": ["1"]
    }
  ]
}
```
"#;

        let plan = parse_plan(response).unwrap();
        assert_eq!(plan.objective, "Update the README");
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[0].tool_name, Some("read_file".to_string()));
        assert_eq!(plan.tasks[1].depends_on, vec![TaskId::new("1")]);
    }

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

    // ==================== Ensemble Planning Tests ====================

    #[test]
    fn test_parse_vote_score_json() {
        // Standard JSON response
        let response = r#"{"score": 8, "reasoning": "Good plan"}"#;
        assert_eq!(parse_vote_score(response), 8.0);

        // With markdown code block
        let response = r#"
Here is my evaluation:
```json
{"score": 7, "reasoning": "Solid but could improve"}
```
"#;
        assert_eq!(parse_vote_score(response), 7.0);
    }

    #[test]
    fn test_parse_vote_score_pattern() {
        // "N/10" pattern
        assert_eq!(parse_vote_score("I rate this 8/10"), 8.0);
        assert_eq!(parse_vote_score("Score: 6/10"), 6.0);

        // Standalone number
        assert_eq!(parse_vote_score("My score is 9"), 9.0);
    }

    #[test]
    fn test_parse_vote_score_clamp() {
        // Clamps to valid range
        let response = r#"{"score": 15, "reasoning": "Too high"}"#;
        assert_eq!(parse_vote_score(response), 10.0);

        let response = r#"{"score": -5, "reasoning": "Too low"}"#;
        assert_eq!(parse_vote_score(response), 1.0);
    }

    #[test]
    fn test_parse_vote_score_fallback() {
        // Fallback to 5.0 when parsing fails
        assert_eq!(parse_vote_score("No numbers here"), 5.0);
        assert_eq!(parse_vote_score(""), 5.0);
    }

    #[test]
    fn test_ensemble_planning_error() {
        let error = RunAgentError::EnsemblePlanningFailed("test error".to_string());
        assert_eq!(error.to_string(), "Ensemble planning failed: test error");
        assert!(!error.is_cancelled());
    }
}
