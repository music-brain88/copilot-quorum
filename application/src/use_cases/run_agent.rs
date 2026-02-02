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
use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use crate::ports::tool_executor::ToolExecutorPort;
use quorum_domain::core::string::truncate;
use quorum_domain::{
    AgentConfig, AgentContext, AgentPhase, AgentPromptTemplate, AgentState, HilMode, HumanDecision,
    Model, ModelVote, Plan, ProjectContext, ReviewRound, Task, TaskId, Thought, ToolCall,
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

/// Progress notifier specific to agent execution
pub trait AgentProgressNotifier: Send + Sync {
    fn on_phase_change(&self, _phase: &AgentPhase) {}
    fn on_thought(&self, _thought: &Thought) {}
    fn on_task_start(&self, _task: &Task) {}
    fn on_task_complete(&self, _task: &Task, _success: bool) {}
    fn on_tool_call(&self, _tool_name: &str, _args: &str) {}
    fn on_tool_result(&self, _tool_name: &str, _success: bool) {}

    /// Called when a tool execution fails with details about the error
    fn on_tool_error(&self, _tool_name: &str, _category: ErrorCategory, _message: &str) {}

    /// Called when retrying a tool call after an error
    fn on_tool_retry(&self, _tool_name: &str, _attempt: usize, _max_retries: usize, _error: &str) {}

    // Plan revision callbacks
    /// Called when a plan revision is triggered after rejection
    fn on_plan_revision(&self, _revision: usize, _feedback: &str) {}

    /// Called when an action is being retried after rejection
    fn on_action_retry(&self, _task: &Task, _attempt: usize, _feedback: &str) {}

    // Quorum-related callbacks
    fn on_quorum_start(&self, _phase: &str, _model_count: usize) {}
    fn on_quorum_model_complete(&self, _model: &Model, _approved: bool) {}
    fn on_quorum_complete(&self, _phase: &str, _approved: bool, _feedback: Option<&str>) {}

    /// Called when quorum voting completes with detailed vote information
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

    /// Send a prompt to LLM with cancellation support
    async fn send_with_cancellation(
        &self,
        session: &dyn LlmSession,
        prompt: &str,
    ) -> Result<String, RunAgentError> {
        if let Some(ref token) = self.cancellation_token {
            tokio::select! {
                biased;
                _ = token.cancelled() => {
                    Err(RunAgentError::Cancelled)
                }
                result = session.send(prompt) => {
                    result.map_err(RunAgentError::GatewayError)
                }
            }
        } else {
            session
                .send(prompt)
                .await
                .map_err(RunAgentError::GatewayError)
        }
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

        // Create a session with the primary model
        let system_prompt = AgentPromptTemplate::agent_system(self.tool_executor.tool_spec());
        let session = self
            .gateway
            .create_session_with_system_prompt(&input.config.primary_model, &system_prompt)
            .await?;

        // Phase 1: Context Gathering
        progress.on_phase_change(&AgentPhase::ContextGathering);
        state.set_phase(AgentPhase::ContextGathering);

        match self
            .gather_context(session.as_ref(), &input.request, &input.config, progress)
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

        // Phase 2-3: Planning + Review Loop
        let mut plan_feedback: Option<String> = None;

        loop {
            // Check for cancellation at the start of each loop iteration
            self.check_cancelled()?;

            // Phase 2: Planning
            progress.on_phase_change(&AgentPhase::Planning);
            state.set_phase(AgentPhase::Planning);

            let plan = match self
                .create_plan(
                    session.as_ref(),
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

            // Phase 3: Plan Review (Quorum) - REQUIRED
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

        // Phase 4: Task Execution
        progress.on_phase_change(&AgentPhase::Executing);
        state.set_phase(AgentPhase::Executing);

        let execution_result = self
            .execute_tasks(session.as_ref(), &input, &mut state, progress)
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

        // Ask the model to gather context using tools
        let prompt = AgentPromptTemplate::context_gathering(request, config.working_dir.as_deref());

        let response = match self.send_with_cancellation(session, &prompt).await {
            Ok(response) => response,
            Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
            Err(e) => return Err(RunAgentError::ContextGatheringFailed(e.to_string())),
        };

        // Parse tool calls from response and execute them
        let tool_calls = parse_tool_calls(&response);
        let mut results = Vec::new();

        for call in tool_calls {
            progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));

            let result = self.tool_executor.execute(&call).await;
            let success = result.is_success();

            progress.on_tool_result(&call.tool_name, success);

            if success && let Some(output) = result.output() {
                results.push((call.tool_name.clone(), output.to_string()));

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
            }
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
        _progress: &dyn AgentProgressNotifier,
    ) -> Result<Plan, RunAgentError> {
        let prompt =
            AgentPromptTemplate::planning_with_feedback(request, context, previous_feedback);

        let response = match self.send_with_cancellation(session, &prompt).await {
            Ok(response) => response,
            Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
            Err(e) => return Err(RunAgentError::PlanningFailed(e.to_string())),
        };

        // Parse the plan from the response
        parse_plan(&response).ok_or_else(|| {
            RunAgentError::PlanningFailed("Failed to parse plan from model response".to_string())
        })
    }

    /// Execute all tasks in the plan
    async fn execute_tasks(
        &self,
        session: &dyn LlmSession,
        input: &RunAgentInput,
        state: &mut AgentState,
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

            // Get next task
            let task_id = {
                let plan = state.plan.as_ref().ok_or_else(|| {
                    RunAgentError::TaskExecutionFailed("No plan available".to_string())
                })?;

                match plan.next_task() {
                    Some(task) => task.id.clone(),
                    None => break, // All tasks complete
                }
            };

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
                        session,
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

    /// Execute a single task
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

        // If task has a predefined tool call, execute it directly
        if let Some(tool_name) = &task.tool_name {
            // Convert task args to tool call args
            let mut tool_call = ToolCall::new(tool_name);
            for (key, value) in &task.tool_args {
                tool_call = tool_call.with_arg(key, value.clone());
            }

            // Check if this is a high-risk tool that needs review
            let needs_review = task.requires_review || self.is_high_risk_tool(tool_name);

            if needs_review && !input.config.quorum_models.is_empty() {
                let tool_call_json = serde_json::to_string_pretty(&serde_json::json!({
                    "tool": tool_name,
                    "args": task.tool_args,
                }))
                .unwrap_or_default();

                let review = self
                    .review_action(input, state, task, &tool_call_json, progress)
                    .await?;

                if !review.approved {
                    return Err(RunAgentError::ActionRejected(
                        review
                            .feedback
                            .unwrap_or_else(|| "Action rejected by quorum".to_string()),
                    ));
                }
            }

            // Execute with retry for validation errors
            let result = self
                .execute_tool_with_retry(
                    session,
                    &tool_call,
                    input.config.max_tool_retries,
                    progress,
                )
                .await;

            if result.is_success() {
                return Ok(result.output().unwrap_or("").to_string());
            } else {
                return Err(RunAgentError::TaskExecutionFailed(
                    result
                        .error()
                        .map(|e| e.message.clone())
                        .unwrap_or_else(|| "Unknown error".to_string()),
                ));
            }
        }

        // Otherwise, ask the model to execute the task
        let prompt = AgentPromptTemplate::task_execution(task, &state.context, previous_results);

        let response = match self.send_with_cancellation(session, &prompt).await {
            Ok(response) => response,
            Err(RunAgentError::Cancelled) => return Err(RunAgentError::Cancelled),
            Err(e) => return Err(RunAgentError::TaskExecutionFailed(e.to_string())),
        };

        // Parse and execute any tool calls in the response
        let tool_calls = parse_tool_calls(&response);

        if tool_calls.is_empty() {
            // No tool calls, the response itself is the result
            return Ok(response);
        }

        let mut outputs = Vec::new();

        for call in tool_calls {
            // Check if this is a high-risk tool that needs review
            let needs_review = self.is_high_risk_tool(&call.tool_name);

            if needs_review && !input.config.quorum_models.is_empty() {
                let tool_call_json = serde_json::to_string_pretty(&serde_json::json!({
                    "tool": call.tool_name,
                    "args": call.arguments,
                }))
                .unwrap_or_default();

                let review = self
                    .review_action(input, state, task, &tool_call_json, progress)
                    .await?;

                if !review.approved {
                    warn!("Tool call {} rejected by quorum", call.tool_name);
                    continue; // Skip this tool call
                }
            }

            // Execute with retry for validation errors
            let result = self
                .execute_tool_with_retry(session, &call, input.config.max_tool_retries, progress)
                .await;

            if result.is_success() {
                if let Some(output) = result.output() {
                    outputs.push(output.to_string());
                }
            } else {
                warn!("Tool {} failed: {:?}", call.tool_name, result.error());
            }
        }

        Ok(outputs.join("\n---\n"))
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

    /// Execute a tool with retry on retryable errors
    ///
    /// If the tool execution fails with a retryable error (INVALID_ARGUMENT, NOT_FOUND),
    /// this method will:
    /// 1. Notify progress of the retry attempt
    /// 2. Send the error back to the LLM with the tool_retry prompt
    /// 3. Parse the corrected tool call from the response
    /// 4. Retry execution up to max_tool_retries times
    async fn execute_tool_with_retry(
        &self,
        session: &dyn LlmSession,
        tool_call: &ToolCall,
        max_retries: usize,
        progress: &dyn AgentProgressNotifier,
    ) -> quorum_domain::ToolResult {
        let mut current_call = tool_call.clone();
        let mut attempts = 0;

        loop {
            progress.on_tool_call(
                &current_call.tool_name,
                &format!("{:?}", current_call.arguments),
            );

            let result = self.tool_executor.execute(&current_call).await;

            progress.on_tool_result(&current_call.tool_name, result.is_success());

            // If successful or not a retryable error, return immediately
            if result.is_success() || !is_retryable_error(&result) {
                // Report non-retryable errors
                if !result.is_success()
                    && let Some(err) = result.error()
                {
                    let category = ErrorCategory::from_error_code(&err.code);
                    progress.on_tool_error(&current_call.tool_name, category, &err.message);
                }
                return result;
            }

            // Get error info for retry
            let (error_code, error_message) = result
                .error()
                .map(|e| (e.code.clone(), e.message.clone()))
                .unwrap_or_else(|| ("UNKNOWN".to_string(), "Unknown error".to_string()));

            let category = ErrorCategory::from_error_code(&error_code);

            // Check if we've exceeded retry limit
            attempts += 1;
            if attempts >= max_retries {
                debug!(
                    "Tool {} failed after {} retry attempts",
                    current_call.tool_name, attempts
                );
                progress.on_tool_error(&current_call.tool_name, category, &error_message);
                return result;
            }

            // Notify about retry attempt
            progress.on_tool_retry(
                &current_call.tool_name,
                attempts,
                max_retries,
                &error_message,
            );

            info!(
                "Tool {} error (attempt {}): {}. Requesting corrected call from LLM.",
                current_call.tool_name, attempts, error_message
            );

            // Ask LLM to fix the tool call
            let retry_prompt = AgentPromptTemplate::tool_retry(
                &current_call.tool_name,
                &error_message,
                &current_call.arguments,
            );

            let response = match self.send_with_cancellation(session, &retry_prompt).await {
                Ok(response) => response,
                Err(RunAgentError::Cancelled) => {
                    // Return the previous result if cancelled
                    return result;
                }
                Err(e) => {
                    warn!("Failed to get retry response from LLM: {}", e);
                    return result;
                }
            };

            // Parse the corrected tool call from the response
            let corrected_calls = parse_tool_calls(&response);

            if let Some(corrected) = corrected_calls.into_iter().next() {
                debug!(
                    "LLM provided corrected tool call with args: {:?}",
                    corrected.arguments
                );
                current_call = corrected;
            } else {
                warn!("LLM did not provide a valid corrected tool call");
                return result;
            }
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

        let models = &input.config.quorum_models;
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
        progress.on_quorum_complete("plan_review", result.approved, result.feedback.as_deref());

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
        let models = &input.config.quorum_models;
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
        progress.on_quorum_complete("action_review", result.approved, result.feedback.as_deref());

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

        let models = &input.config.quorum_models;
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
        progress.on_quorum_complete("final_review", result.approved, result.feedback.as_deref());

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

/// Check if a ToolResult error is retryable
///
/// Retryable errors include:
/// - INVALID_ARGUMENT: The tool was called with invalid arguments
/// - NOT_FOUND: The requested tool doesn't exist (e.g., multi_tool_use.parallel)
fn is_retryable_error(result: &quorum_domain::ToolResult) -> bool {
    result
        .error()
        .map(|e| matches!(e.code.as_str(), "INVALID_ARGUMENT" | "NOT_FOUND"))
        .unwrap_or(false)
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

/// Parse tool calls from model response
fn parse_tool_calls(response: &str) -> Vec<ToolCall> {
    let mut calls = Vec::new();

    // Look for ```tool ... ``` blocks
    let mut in_tool_block = false;
    let mut current_block = String::new();

    for line in response.lines() {
        if line.trim() == "```tool" {
            in_tool_block = true;
            current_block.clear();
        } else if in_tool_block && line.trim() == "```" {
            in_tool_block = false;
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_block)
                && let Some(tool_name) = parsed.get("tool").and_then(|v| v.as_str())
            {
                let mut call = ToolCall::new(tool_name);

                if let Some(args) = parsed.get("args").and_then(|v| v.as_object()) {
                    for (key, value) in args {
                        call = call.with_arg(key, value.clone());
                    }
                }

                if let Some(reasoning) = parsed.get("reasoning").and_then(|v| v.as_str()) {
                    call = call.with_reasoning(reasoning);
                }

                calls.push(call);
            }
        } else if in_tool_block {
            current_block.push_str(line);
            current_block.push('\n');
        }
    }

    calls
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
    fn test_parse_tool_calls() {
        let response = r#"
Let me read the file.

```tool
{
  "tool": "read_file",
  "args": {
    "path": "/test/file.txt"
  },
  "reasoning": "Need to check the contents"
}
```

Done!
"#;

        let calls = parse_tool_calls(response);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_name, "read_file");
        assert_eq!(calls[0].get_string("path"), Some("/test/file.txt"));
    }

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
    fn test_parse_empty_tool_calls() {
        let response = "Just some text without any tool calls.";
        let calls = parse_tool_calls(response);
        assert!(calls.is_empty());
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
}
