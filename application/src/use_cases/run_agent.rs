//! Run Agent use case
//!
//! Orchestrates the agent execution flow with quorum integration:
//! 1. Context Gathering - Understand the project
//! 2. Planning - Create a task plan (single model)
//! 3. Plan Review - Quorum reviews the plan (REQUIRED)
//! 4. Executing - Execute tasks using tools
//!    - Action Review - Quorum reviews high-risk operations
//! 5. Final Review - Quorum reviews results (optional)

use crate::ports::llm_gateway::{GatewayError, LlmGateway, LlmSession};
use crate::ports::tool_executor::ToolExecutorPort;
use quorum_domain::{
    AgentConfig, AgentContext, AgentPhase, AgentPromptTemplate, AgentState, Model, Plan, Task,
    TaskId, Thought, ToolCall,
};
use std::sync::Arc;
use thiserror::Error;
use tokio::task::JoinSet;
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

    #[error("All quorum models failed")]
    QuorumFailed,

    #[error("Gateway error: {0}")]
    GatewayError(#[from] GatewayError),
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

/// Progress notifier specific to agent execution
pub trait AgentProgressNotifier: Send + Sync {
    fn on_phase_change(&self, _phase: &AgentPhase) {}
    fn on_thought(&self, _thought: &Thought) {}
    fn on_task_start(&self, _task: &Task) {}
    fn on_task_complete(&self, _task: &Task, _success: bool) {}
    fn on_tool_call(&self, _tool_name: &str, _args: &str) {}
    fn on_tool_result(&self, _tool_name: &str, _success: bool) {}

    // Quorum-related callbacks
    fn on_quorum_start(&self, _phase: &str, _model_count: usize) {}
    fn on_quorum_model_complete(&self, _model: &Model, _approved: bool) {}
    fn on_quorum_complete(&self, _phase: &str, _approved: bool, _feedback: Option<&str>) {}
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
pub struct RunAgentUseCase<G: LlmGateway + 'static, T: ToolExecutorPort + 'static> {
    gateway: Arc<G>,
    tool_executor: Arc<T>,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static> RunAgentUseCase<G, T> {
    pub fn new(gateway: Arc<G>, tool_executor: Arc<T>) -> Self {
        Self {
            gateway,
            tool_executor,
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

        // Phase 2: Planning
        progress.on_phase_change(&AgentPhase::Planning);
        state.set_phase(AgentPhase::Planning);

        let plan = match self
            .create_plan(session.as_ref(), &input.request, &state.context, progress)
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

        let plan_review = self
            .review_plan(&input, &state, progress)
            .await?;

        if plan_review.approved {
            state.approve_plan();
            state.add_thought(Thought::observation("Plan approved by quorum"));
        } else {
            let feedback = plan_review.feedback.unwrap_or_else(|| "No specific feedback".to_string());
            state.reject_plan(&feedback);
            state.fail(format!("Plan rejected: {}", feedback));
            return Ok(RunAgentOutput {
                summary: format!("Plan rejected by quorum: {}", feedback),
                success: false,
                state,
            });
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

    /// Gather context about the project
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

        // Ask the model to gather context using tools
        let prompt =
            AgentPromptTemplate::context_gathering(request, config.working_dir.as_deref());

        let response = session
            .send(&prompt)
            .await
            .map_err(|e| RunAgentError::ContextGatheringFailed(e.to_string()))?;

        // Parse tool calls from response and execute them
        let tool_calls = parse_tool_calls(&response);
        let mut results = Vec::new();

        for call in tool_calls {
            progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));

            let result = self.tool_executor.execute(&call).await;
            let success = result.is_success();

            progress.on_tool_result(&call.tool_name, success);

            if success {
                if let Some(output) = result.output() {
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
        _progress: &dyn AgentProgressNotifier,
    ) -> Result<Plan, RunAgentError> {
        let prompt = AgentPromptTemplate::planning(request, context);

        let response = session
            .send(&prompt)
            .await
            .map_err(|e| RunAgentError::PlanningFailed(e.to_string()))?;

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
            if let Some(plan) = &mut state.plan {
                if let Some(task) = plan.get_task_mut(&task_id) {
                    task.mark_in_progress();
                    progress.on_task_start(task);
                }
            }

            // Execute the task
            let task_result = self
                .execute_single_task(session, input, state, &task_id, &previous_results, progress)
                .await;

            // Update task status
            let (success, output) = match task_result {
                Ok(output) => (true, output),
                Err(e) => (false, e.to_string()),
            };

            if let Some(plan) = &mut state.plan {
                if let Some(task) = plan.get_task_mut(&task_id) {
                    if success {
                        task.mark_completed(quorum_domain::TaskResult::success(&output));
                    } else {
                        task.mark_failed(quorum_domain::TaskResult::failure(&output));
                    }
                    progress.on_task_complete(task, success);
                }
            }

            results.push(format!("Task {}: {}", task_id, if success { "OK" } else { "FAILED" }));
            previous_results.push_str(&format!("\n---\nTask {}: {}\n", task_id, output));
        }

        // Generate summary
        let completed = state
            .plan
            .as_ref()
            .map(|p| p.progress())
            .unwrap_or((0, 0));

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
                })).unwrap_or_default();

                let review = self
                    .review_action(input, state, task, &tool_call_json, progress)
                    .await?;

                if !review.approved {
                    return Err(RunAgentError::ActionRejected(
                        review.feedback.unwrap_or_else(|| "Action rejected by quorum".to_string())
                    ));
                }
            }

            progress.on_tool_call(tool_name, &format!("{:?}", task.tool_args));

            let result = self.tool_executor.execute(&tool_call).await;
            let success = result.is_success();

            progress.on_tool_result(tool_name, success);

            if success {
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

        let response = session
            .send(&prompt)
            .await
            .map_err(|e| RunAgentError::TaskExecutionFailed(e.to_string()))?;

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
                })).unwrap_or_default();

                let review = self
                    .review_action(input, state, task, &tool_call_json, progress)
                    .await?;

                if !review.approved {
                    warn!("Tool call {} rejected by quorum", call.tool_name);
                    continue; // Skip this tool call
                }
            }

            progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));

            let result = self.tool_executor.execute(&call).await;
            let success = result.is_success();

            progress.on_tool_result(&call.tool_name, success);

            if success {
                if let Some(output) = result.output() {
                    outputs.push(output.to_string());
                }
            } else {
                warn!(
                    "Tool {} failed: {:?}",
                    call.tool_name,
                    result.error()
                );
            }
        }

        Ok(outputs.join("\n---\n"))
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
        let plan = state.plan.as_ref().ok_or_else(|| {
            RunAgentError::PlanningFailed("No plan to review".to_string())
        })?;

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

        // Collect votes
        let mut votes = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((model, Ok(response))) => {
                    let (approved, feedback) = parse_review_response(&response);
                    info!("Model {} voted: {}", model, if approved { "APPROVE" } else { "REJECT" });
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

        // Collect votes
        let mut votes = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((model, Ok(response))) => {
                    let (approved, feedback) = parse_review_response(&response);
                    info!("Model {} voted: {}", model, if approved { "APPROVE" } else { "REJECT" });
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

        // Collect results
        let mut votes = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((model, Ok(response))) => {
                    // For final review, we look for SUCCESS/PARTIAL/FAILURE
                    let (approved, feedback) = parse_final_review_response(&response);
                    info!("Model {} assessment: {}", model, if approved { "SUCCESS" } else { "ISSUES" });
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
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&current_block) {
                if let Some(tool_name) = parsed.get("tool").and_then(|v| v.as_str()) {
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
    let reasoning = json
        .get("reasoning")
        .and_then(|v| v.as_str())
        .unwrap_or("");

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

            if let Some(tool) = task_json.get("tool").and_then(|v| v.as_str()) {
                if tool != "null" && !tool.is_empty() {
                    task = task.with_tool(tool);
                }
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

/// Truncate a string to a maximum length
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
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
}
