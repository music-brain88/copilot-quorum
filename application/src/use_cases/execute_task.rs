//! Execute Task use case.
//!
//! Responsible for Phase 4 of the agent execution flow: executing tasks
//! from the approved plan with dynamic model selection and action review.

use crate::ports::action_reviewer::{ActionReviewer, ReviewDecision};
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::conversation_logger::{ConversationEvent, ConversationLogger};
use crate::ports::llm_gateway::{LlmGateway, LlmSession, ToolResultMessage};
use crate::ports::tool_executor::ToolExecutorPort;
use crate::ports::tool_schema::ToolSchemaPort;
use crate::use_cases::run_agent::{RunAgentError, RunAgentInput};
use crate::use_cases::shared::{check_cancelled, send_with_tools_cancellable};
use quorum_domain::agent::model_config::ModelConfig;
use quorum_domain::context::context_budget::ContextBudget;
use quorum_domain::context::task_result_buffer::TaskResultBuffer;
use quorum_domain::util::truncate_str;
use quorum_domain::{AgentPromptTemplate, AgentState, Model, Task, TaskId, ToolExecution};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Use case for executing tasks from an approved plan (Phase 4).
///
/// Handles dynamic model selection based on tool risk level,
/// parallel execution of low-risk tools, and sequential execution
/// of high-risk tools with action review.
pub struct ExecuteTaskUseCase<G: LlmGateway, T: ToolExecutorPort> {
    gateway: Arc<G>,
    tool_executor: Arc<T>,
    tool_schema: Arc<dyn ToolSchemaPort>,
    cancellation_token: Option<CancellationToken>,
    action_reviewer: Arc<dyn ActionReviewer>,
    conversation_logger: Arc<dyn ConversationLogger>,
}

impl<G: LlmGateway + 'static, T: ToolExecutorPort + 'static> ExecuteTaskUseCase<G, T> {
    pub fn new(
        gateway: Arc<G>,
        tool_executor: Arc<T>,
        tool_schema: Arc<dyn ToolSchemaPort>,
        cancellation_token: Option<CancellationToken>,
        action_reviewer: Arc<dyn ActionReviewer>,
        conversation_logger: Arc<dyn ConversationLogger>,
    ) -> Self {
        Self {
            gateway,
            tool_executor,
            tool_schema,
            cancellation_token,
            action_reviewer,
            conversation_logger,
        }
    }

    /// Execute all tasks in the plan with dynamic model selection.
    ///
    /// Returns a summary string describing what was accomplished.
    pub async fn execute(
        &self,
        input: &RunAgentInput,
        state: &mut AgentState,
        system_prompt: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<String, RunAgentError> {
        let mut results = Vec::new();
        let mut result_buffer = TaskResultBuffer::new(input.execution.context_budget.clone());

        loop {
            // Check for cancellation at the start of each task
            check_cancelled(&self.cancellation_token)?;

            // Check iteration limit
            if !state.increment_iteration() {
                return Err(RunAgentError::MaxIterationsExceeded);
            }

            // Get next task and determine appropriate model
            let (task_id, selected_model, task_index, task_total) = {
                let plan = state.plan.as_ref().ok_or_else(|| {
                    RunAgentError::TaskExecutionFailed("No plan available".to_string())
                })?;

                match plan.next_task() {
                    Some(task) => {
                        let model = self.select_model_for_task(task, &input.models);
                        let index =
                            plan.tasks.iter().position(|t| t.id == task.id).unwrap_or(0) + 1;
                        let total = plan.tasks.len();
                        (task.id.clone(), model.clone(), index, total)
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
                progress.on_task_start(task, task_index, task_total);
            }

            // Execute the task with action retry support
            let max_action_retries = 2;
            let mut action_attempts = 0;
            let mut action_feedback: Option<String> = None;

            let task_result = loop {
                // Build context including any rejection feedback,
                // with optional per-task ContextMode budget override
                let task_budget = state
                    .plan
                    .as_ref()
                    .and_then(|p| p.tasks.iter().find(|t| t.id == task_id))
                    .and_then(|t| t.context_mode)
                    .map(ContextBudget::for_context_mode);

                let context_with_feedback = match &action_feedback {
                    Some(feedback) => {
                        result_buffer.render_with_feedback(feedback, task_budget.as_ref())
                    }
                    None => result_buffer.render_with_budget(task_budget.as_ref()),
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

            // Update task status and store tool executions
            let (success, output) = match task_result {
                Ok((output, tool_executions)) => {
                    // Store tool executions in the task
                    if let Some(plan) = &mut state.plan
                        && let Some(task) = plan.get_task_mut(&task_id)
                    {
                        task.tool_executions = tool_executions;
                    }
                    (true, output)
                }
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
                progress.on_task_complete(task, success, task_index, task_total);
            }

            results.push(format!(
                "Task {}: {}",
                task_id,
                if success { "OK" } else { "FAILED" }
            ));
            result_buffer.push(task_id.as_str(), &output);
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

    /// Determine the appropriate model for a task based on tool risk level.
    fn select_model_for_task<'a>(&self, task: &Task, models: &'a ModelConfig) -> &'a Model {
        if let Some(tool_name) = &task.tool_name {
            if self.action_reviewer.is_high_risk_tool(tool_name) {
                &models.decision
            } else {
                &models.exploration
            }
        } else {
            // Tool not specified yet - model will decide, so use decision_model
            &models.decision
        }
    }

    /// Execute a single task using the Native Tool Use API.
    ///
    /// Returns `(output_text, tool_executions)` on success.
    async fn execute_single_task(
        &self,
        session: &dyn LlmSession,
        input: &RunAgentInput,
        state: &AgentState,
        task_id: &TaskId,
        previous_results: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<(String, Vec<ToolExecution>), RunAgentError> {
        let task = state
            .plan
            .as_ref()
            .and_then(|p| p.tasks.iter().find(|t| &t.id == task_id))
            .ok_or_else(|| RunAgentError::TaskExecutionFailed("Task not found".to_string()))?;

        debug!("Executing task: {} - {}", task.id, task.description);

        self.execute_task_native(session, input, state, task, previous_results, progress)
            .await
    }

    /// Execute a task using the Native Tool Use API with multi-turn loop.
    ///
    /// Returns `(output_text, tool_executions)` — the output text is the
    /// joined LLM text blocks, and tool_executions tracks each tool call's
    /// lifecycle for state tracking and UI display.
    async fn execute_task_native(
        &self,
        session: &dyn LlmSession,
        input: &RunAgentInput,
        state: &AgentState,
        task: &Task,
        previous_results: &str,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<(String, Vec<ToolExecution>), RunAgentError> {
        let task_id_str = task.id.as_str();
        let prompt = AgentPromptTemplate::task_execution(task, &state.context, previous_results);

        self.conversation_logger.log(ConversationEvent::new(
            "llm_prompt",
            serde_json::json!({
                "task_id": task_id_str,
                "model": session.model().to_string(),
                "bytes": prompt.len(),
                "text": prompt,
            }),
        ));

        let tools = self
            .tool_schema
            .all_tools_schema(self.tool_executor.tool_spec());
        let max_turns = input.execution.max_tool_turns;
        let mut turn_count = 0;
        let mut all_outputs = Vec::new();
        let mut all_executions: Vec<ToolExecution> = Vec::new();
        let mut exec_counter: usize = 0;

        // Initial request
        let mut response = match send_with_tools_cancellable(
            session,
            &prompt,
            &tools,
            progress,
            &self.cancellation_token,
        )
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
                debug!(
                    "Task {}: LLM text response (first ~300 chars): {}",
                    task.id,
                    truncate_str(&text, 300)
                );
                self.conversation_logger.log(ConversationEvent::new(
                    "llm_response",
                    serde_json::json!({
                        "task_id": task.id.as_str(),
                        "model": session.model().to_string(),
                        "bytes": text.len(),
                        "text": text,
                    }),
                ));
                all_outputs.push(text);
            }

            // Extract tool calls
            let tool_calls = response.tool_calls();

            if tool_calls.is_empty() {
                // Tool specified but LLM responded with text only → retry once with nudge
                if task.tool_name.is_some() && turn_count == 0 {
                    warn!(
                        "Task {}: expected tool '{}' but got text-only response, retrying with nudge",
                        task.id,
                        task.tool_name.as_deref().unwrap_or("?")
                    );
                    turn_count += 1;

                    let nudge = format!(
                        "You responded with text only, but this task REQUIRES calling `{}`. \
                         Call the tool NOW. Do not respond with text.",
                        task.tool_name.as_deref().unwrap_or("?")
                    );
                    self.conversation_logger.log(ConversationEvent::new(
                        "llm_prompt",
                        serde_json::json!({
                            "task_id": task_id_str,
                            "model": session.model().to_string(),
                            "bytes": nudge.len(),
                            "text": nudge,
                            "nudge": true,
                        }),
                    ));
                    response = match send_with_tools_cancellable(
                        session,
                        &nudge,
                        &tools,
                        progress,
                        &self.cancellation_token,
                    )
                    .await
                    {
                        Ok(r) => r,
                        Err(_) => break, // nudge failed — keep original text
                    };
                    continue;
                }

                debug!(
                    "Task {}: no tool calls in response, ending execution loop",
                    task.id
                );
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
            check_cancelled(&self.cancellation_token)?;

            // Execute tool calls and collect results
            let mut tool_result_messages = Vec::new();

            // Separate into low-risk (can parallelize) and high-risk (sequential)
            let mut low_risk_calls = Vec::new();
            let mut high_risk_calls = Vec::new();

            for call in &tool_calls {
                if self.action_reviewer.is_high_risk_tool(&call.tool_name) {
                    high_risk_calls.push(call);
                } else {
                    low_risk_calls.push(call);
                }
            }

            // Execute low-risk calls in parallel
            if !low_risk_calls.is_empty() {
                // Create ToolExecutions for all low-risk calls (Pending state)
                let mut exec_indices = Vec::new();
                let mut futures = Vec::new();
                for call in &low_risk_calls {
                    exec_counter += 1;
                    let exec_id = format!("{}-exec-{}", task_id_str, exec_counter);
                    let mut exec = ToolExecution::new(
                        exec_id.clone(),
                        &call.tool_name,
                        call.arguments.clone(),
                        call.native_id.clone(),
                        turn_count,
                    );
                    progress.on_tool_execution_created(
                        task_id_str,
                        &exec_id,
                        &call.tool_name,
                        turn_count,
                    );

                    // Transition to Running
                    exec.mark_running();
                    progress.on_tool_execution_started(task_id_str, &exec_id, &call.tool_name);

                    all_executions.push(exec);
                    exec_indices.push(all_executions.len() - 1);

                    progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));
                    self.conversation_logger.log(ConversationEvent::new(
                        "tool_call",
                        serde_json::json!({
                            "task_id": task_id_str,
                            "tool": call.tool_name,
                            "args": call.arguments,
                            "risk": "low",
                        }),
                    ));
                    futures.push(self.tool_executor.execute(call));
                }

                let results: Vec<_> = futures::future::join_all(futures).await;

                for ((call, result), &exec_idx) in
                    low_risk_calls.iter().zip(results).zip(&exec_indices)
                {
                    let is_error = !result.is_success();
                    let output = if is_error {
                        result
                            .error()
                            .map(|e| e.message.clone())
                            .unwrap_or_else(|| "Unknown error".to_string())
                    } else {
                        result.output().unwrap_or("").to_string()
                    };

                    let mut tool_result_payload = serde_json::json!({
                        "task_id": task_id_str,
                        "tool": call.tool_name,
                        "success": !is_error,
                        "bytes": output.len(),
                        "duration_ms": result.metadata.duration_ms,
                    });
                    if is_error {
                        tool_result_payload["error"] = serde_json::Value::String(output.clone());
                    }
                    self.conversation_logger
                        .log(ConversationEvent::new("tool_result", tool_result_payload));

                    // Update ToolExecution state
                    let exec = &mut all_executions[exec_idx];
                    let exec_id = exec.id.to_string();
                    if is_error {
                        exec.mark_error(&output);
                        progress.on_tool_execution_failed(
                            task_id_str,
                            &exec_id,
                            &call.tool_name,
                            &output,
                        );
                    } else {
                        exec.mark_completed(&result);
                        let duration = exec.duration_ms().unwrap_or(0);
                        let preview = result
                            .output()
                            .unwrap_or("")
                            .chars()
                            .take(100)
                            .collect::<String>();
                        progress.on_tool_execution_completed(
                            task_id_str,
                            &exec_id,
                            &call.tool_name,
                            duration,
                            &preview,
                        );
                    }

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
                            is_rejected: false,
                        });
                    } else {
                        warn!(
                            "Missing native_id for tool call '{}'; skipping result.",
                            call.tool_name
                        );
                    }
                }
            }

            // Execute high-risk calls sequentially (with action review)
            for call in &high_risk_calls {
                exec_counter += 1;
                let exec_id = format!("{}-exec-{}", task_id_str, exec_counter);
                let mut exec = ToolExecution::new(
                    exec_id.clone(),
                    &call.tool_name,
                    call.arguments.clone(),
                    call.native_id.clone(),
                    turn_count,
                );
                progress.on_tool_execution_created(
                    task_id_str,
                    &exec_id,
                    &call.tool_name,
                    turn_count,
                );

                // Action review for high-risk operations
                let review_decision = {
                    let tool_call_json = serde_json::to_string_pretty(&serde_json::json!({
                        "tool": call.tool_name,
                        "args": call.arguments,
                    }))
                    .unwrap_or_default();

                    self.action_reviewer
                        .review_action(&tool_call_json, task, state, &input.models, progress)
                        .await?
                };

                match review_decision {
                    ReviewDecision::Rejected(_) => {
                        warn!("Tool call {} rejected by action review", call.tool_name);
                        exec.mark_running();
                        exec.mark_error("Action rejected by quorum review");
                        progress.on_tool_execution_failed(
                            task_id_str,
                            &exec_id,
                            &call.tool_name,
                            "Action rejected by quorum review",
                        );
                        all_executions.push(exec);

                        self.conversation_logger.log(ConversationEvent::new(
                            "tool_call",
                            serde_json::json!({
                                "task_id": task_id_str,
                                "tool": call.tool_name,
                                "args": call.arguments,
                                "risk": "high",
                                "rejected": true,
                            }),
                        ));
                        self.conversation_logger.log(ConversationEvent::new(
                            "tool_result",
                            serde_json::json!({
                                "task_id": task_id_str,
                                "tool": call.tool_name,
                                "success": false,
                                "rejected": true,
                                "reason": "Action rejected by quorum review",
                            }),
                        ));

                        if let Some(native_id) = call.native_id.clone() {
                            tool_result_messages.push(ToolResultMessage {
                                tool_use_id: native_id,
                                tool_name: call.tool_name.clone(),
                                output: "Action rejected by quorum review".to_string(),
                                is_error: false,
                                is_rejected: true,
                            });
                        } else {
                            warn!(
                                "Missing native_id for tool call '{}'; skipping result.",
                                call.tool_name
                            );
                        }
                        continue;
                    }
                    ReviewDecision::Approved | ReviewDecision::SkipReview => {
                        // Proceed with execution
                    }
                }

                // Transition to Running
                exec.mark_running();
                progress.on_tool_execution_started(task_id_str, &exec_id, &call.tool_name);

                progress.on_tool_call(&call.tool_name, &format!("{:?}", call.arguments));
                self.conversation_logger.log(ConversationEvent::new(
                    "tool_call",
                    serde_json::json!({
                        "task_id": task_id_str,
                        "tool": call.tool_name,
                        "args": call.arguments,
                        "risk": "high",
                    }),
                ));

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

                let mut tool_result_payload = serde_json::json!({
                    "task_id": task_id_str,
                    "tool": call.tool_name,
                    "success": !is_error,
                    "bytes": output.len(),
                    "duration_ms": result.metadata.duration_ms,
                });
                if is_error {
                    tool_result_payload["error"] = serde_json::Value::String(output.clone());
                }
                self.conversation_logger
                    .log(ConversationEvent::new("tool_result", tool_result_payload));

                // Update ToolExecution state
                if is_error {
                    exec.mark_error(&output);
                    progress.on_tool_execution_failed(
                        task_id_str,
                        &exec_id,
                        &call.tool_name,
                        &output,
                    );
                } else {
                    exec.mark_completed(&result);
                    let duration = exec.duration_ms().unwrap_or(0);
                    let preview = result
                        .output()
                        .unwrap_or("")
                        .chars()
                        .take(100)
                        .collect::<String>();
                    progress.on_tool_execution_completed(
                        task_id_str,
                        &exec_id,
                        &call.tool_name,
                        duration,
                        &preview,
                    );
                }
                all_executions.push(exec);

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
                        is_rejected: false,
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

        Ok((all_outputs.join("\n---\n"), all_executions))
    }
}
