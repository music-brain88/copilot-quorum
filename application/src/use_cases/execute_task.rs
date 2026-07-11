//! Execute Task use case.
//!
//! Responsible for Phase 4 of the agent execution flow: executing tasks
//! from the approved plan with dynamic model selection and action review.

use crate::ports::action_reviewer::{ActionReviewer, ReviewDecision};
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::conversation_logger::{ConversationEvent, ConversationLogger};
use crate::ports::llm_gateway::{LlmGateway, LlmSession, ToolResultMessage};
use crate::ports::scripting_engine::ScriptingEnginePort;
use crate::ports::tool_executor::ToolExecutorPort;
use crate::ports::tool_schema::ToolSchemaPort;
use crate::use_cases::run_agent::{RunAgentError, RunAgentInput};
use crate::use_cases::shared::{check_cancelled, send_with_tools_cancellable};
use crate::use_cases::tool_helpers::tool_args_preview;
use quorum_domain::agent::model_config::ModelConfig;
use quorum_domain::context::context_budget::ContextBudget;
use quorum_domain::context::task_result_buffer::TaskResultBuffer;
use quorum_domain::util::truncate_str;
use quorum_domain::{
    AgentPromptTemplate, AgentState, Model, Task, TaskId, ToolExecution, looks_like_tool_call_json,
};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Extract a brief meaningful line from task output.
///
/// Skips common noise lines (thinking prefixes, tool headers, separators)
/// and returns the first substantive line, truncated to `max_bytes`.
fn extract_task_brief(output: &str, max_bytes: usize) -> Option<String> {
    let meaningful_line = output.lines().find(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        // Skip common noise patterns
        if trimmed.starts_with("**Thinking**") || trimmed.starts_with("**thinking**") {
            return false;
        }
        if trimmed.starts_with("[") && trimmed.contains("]:") {
            // e.g. "[read_file]: ..." — tool output header
            return false;
        }
        if trimmed.chars().all(|c| c == '-' || c == '=' || c == '─') {
            return false;
        }
        if trimmed.len() <= 3 {
            // Skip very short lines (e.g. "##", "ok", "...") — too terse to be a useful brief
            return false;
        }
        true
    });

    meaningful_line.map(|line| {
        let truncated = truncate_str(line.trim(), max_bytes);
        truncated.to_string()
    })
}

/// Use case for executing tasks from an approved plan (Phase 4).
///
/// Handles dynamic model selection based on tool risk level,
/// parallel execution of low-risk tools, and sequential execution
/// of high-risk tools with action review.
pub struct ExecuteTaskUseCase {
    gateway: Arc<dyn LlmGateway>,
    tool_executor: Arc<dyn ToolExecutorPort>,
    tool_schema: Arc<dyn ToolSchemaPort>,
    cancellation_token: Option<CancellationToken>,
    action_reviewer: Arc<dyn ActionReviewer>,
    conversation_logger: Arc<dyn ConversationLogger>,
    scripting_engine: Option<Arc<dyn ScriptingEnginePort>>,
}

impl ExecuteTaskUseCase {
    pub fn new(
        gateway: Arc<dyn LlmGateway>,
        tool_executor: Arc<dyn ToolExecutorPort>,
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
            scripting_engine: None,
        }
    }

    /// Set the scripting engine for ToolCallBefore events.
    pub fn with_scripting_engine(mut self, engine: Arc<dyn ScriptingEnginePort>) -> Self {
        self.scripting_engine = Some(engine);
        self
    }

    /// Check ToolCallBefore: returns true if the tool call should proceed.
    fn check_tool_call_before(
        &self,
        tool_name: &str,
        args: &std::collections::HashMap<String, serde_json::Value>,
    ) -> bool {
        if let Some(engine) = &self.scripting_engine {
            let args_json = serde_json::to_string(args).unwrap_or_default();
            engine.on_tool_call_before(tool_name, &args_json)
        } else {
            true
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
            let (task_id, task_description, selected_model, task_index, task_total) = {
                let plan = state.plan.as_ref().ok_or_else(|| {
                    RunAgentError::TaskExecutionFailed("No plan available".to_string())
                })?;

                match plan.next_task() {
                    Some(task) => {
                        let model = self.select_model_for_task(task, &input.models);
                        let index =
                            plan.tasks.iter().position(|t| t.id == task.id).unwrap_or(0) + 1;
                        let total = plan.tasks.len();
                        (
                            task.id.clone(),
                            task.description.clone(),
                            model.clone(),
                            index,
                            total,
                        )
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

                        // Track consecutive rejections for cascade detection
                        let rejection_count = state.record_action_rejection();
                        let cascade_action = input.policy.action_rejection_action(rejection_count);

                        match cascade_action {
                            quorum_domain::agent::agent_policy::HilAction::Abort => {
                                warn!(
                                    "Action rejection cascade detected ({} consecutive). Aborting.",
                                    rejection_count
                                );
                                break Err(RunAgentError::ActionRejected(format!(
                                    "Rejection cascade: {} consecutive rejections exceeded limit. \
                                     Last feedback: {}",
                                    rejection_count, feedback
                                )));
                            }
                            quorum_domain::agent::agent_policy::HilAction::ForceApprove => {
                                info!(
                                    "Action rejection cascade ({} consecutive): auto-approve mode, \
                                     skipping review for next attempt.",
                                    rejection_count
                                );
                                // Reset and continue — next attempt won't be reviewed
                                // (the tool-level review is still in place, but the cascade
                                // count signals that we should let it through)
                                action_feedback = Some(format!(
                                    "{}\n[NOTE: Cascade limit reached. Proceeding without review.]",
                                    feedback
                                ));
                                continue;
                            }
                            quorum_domain::agent::agent_policy::HilAction::RequestIntervention => {
                                warn!(
                                    "Action rejection cascade ({} consecutive): requesting human intervention.",
                                    rejection_count
                                );
                                break Err(RunAgentError::ActionRejected(format!(
                                    "Rejection cascade: {} consecutive rejections. \
                                     Human intervention required. Last feedback: {}",
                                    rejection_count, feedback
                                )));
                            }
                            quorum_domain::agent::agent_policy::HilAction::Continue => {
                                // Within retry limits — normal retry flow
                            }
                        }

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
                    // Reset cascade counter on success
                    state.reset_action_rejections();
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

            let status = if success { "OK" } else { "FAILED" };
            let brief = if success {
                extract_task_brief(&output, 150)
                    .map(|b| format!(" — {}", b))
                    .unwrap_or_default()
            } else {
                // For failures, the output IS the error message
                let error_brief = truncate_str(&output, 150);
                if error_brief.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", error_brief)
                }
            };
            results.push(format!(
                "Task {} ({}): {}{}",
                task_id,
                truncate_str(&task_description, 60),
                status,
                brief,
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
    ///
    /// Note: At task-selection time we only know the tool name, not arguments,
    /// so `run_command` falls through to decision model (conservative).
    fn select_model_for_task<'a>(&self, task: &Task, models: &'a ModelConfig) -> &'a Model {
        if let Some(tool_name) = &task.tool_name {
            if self
                .action_reviewer
                .is_high_risk_tool(tool_name, &std::collections::HashMap::new())
            {
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
        // Retry budget for text-only / leaked-tool-call responses (#268)
        const MAX_TOOL_NUDGES: usize = 2;
        let mut nudge_count = 0;
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
            }

            // Extract tool calls
            let tool_calls = response.tool_calls();

            if tool_calls.is_empty() {
                // Detect a tool call the LLM wrote as raw JSON text instead of
                // actually invoking it via the Native Tool Use API (#268).
                let tool_call_leak = !text.is_empty()
                    && looks_like_tool_call_json(&text, self.tool_executor.tool_spec());

                // Text-only response where a tool was expected, or a leaked
                // tool-call JSON → retry with a nudge. Leaked JSON is dropped
                // from the output; ordinary text-only responses are kept.
                let expected_tool = task.tool_name.is_some() && turn_count == 0;
                if (tool_call_leak || expected_tool) && nudge_count < MAX_TOOL_NUDGES {
                    if !tool_call_leak && !text.is_empty() {
                        all_outputs.push(text.clone());
                    }
                    nudge_count += 1;
                    turn_count += 1;
                    warn!(
                        "Task {}: {} (nudge {}/{})",
                        task.id,
                        if tool_call_leak {
                            "LLM emitted a tool call as JSON text instead of invoking it"
                        } else {
                            "expected a tool call but got text-only response"
                        },
                        nudge_count,
                        MAX_TOOL_NUDGES,
                    );

                    let nudge = if tool_call_leak {
                        "You wrote a tool invocation as JSON text instead of calling \
                         the tool. Do NOT print JSON. Invoke the tool NOW using the \
                         native tool-calling mechanism, then answer in natural language."
                            .to_string()
                    } else {
                        format!(
                            "You responded with text only, but this task REQUIRES calling `{}`. \
                             Call the tool NOW. Do not respond with text.",
                            task.tool_name.as_deref().unwrap_or("?")
                        )
                    };
                    self.conversation_logger.log(ConversationEvent::new(
                        "llm_prompt",
                        serde_json::json!({
                            "task_id": task_id_str,
                            "model": session.model().to_string(),
                            "bytes": nudge.len(),
                            "text": nudge,
                            "nudge": true,
                            "tool_call_leak": tool_call_leak,
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
                        // Nudge failed — keep already-collected output
                        // (leaked JSON was never pushed, so it cannot surface)
                        Err(_) => break,
                    };
                    continue;
                }

                if tool_call_leak {
                    // Nudge budget exhausted — suppress the raw JSON so it never
                    // reaches the user as the final answer (#268).
                    warn!(
                        "Task {}: suppressing leaked tool-call JSON from output ({} bytes)",
                        task.id,
                        text.len()
                    );
                    self.conversation_logger.log(ConversationEvent::new(
                        "tool_call_leak_suppressed",
                        serde_json::json!({
                            "task_id": task_id_str,
                            "model": session.model().to_string(),
                            "bytes": text.len(),
                            "text": text,
                        }),
                    ));
                    if all_outputs.is_empty() {
                        return Err(RunAgentError::TaskExecutionFailed(
                            "LLM returned a raw tool-call JSON instead of executing the tool \
                             (retries exhausted)"
                                .to_string(),
                        ));
                    }
                    break;
                }

                if !text.is_empty() {
                    all_outputs.push(text);
                }
                debug!(
                    "Task {}: no tool calls in response, ending execution loop",
                    task.id
                );
                break;
            }

            // Turn has tool calls — keep any accompanying text
            if !text.is_empty() {
                all_outputs.push(text);
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
                if self
                    .action_reviewer
                    .is_high_risk_tool(&call.tool_name, &call.arguments)
                {
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
                    // ToolCallBefore check: Lua filter can cancel tool calls
                    if !self.check_tool_call_before(&call.tool_name, &call.arguments) {
                        debug!(
                            "Tool call {} cancelled by ToolCallBefore listener",
                            call.tool_name
                        );
                        if let Some(native_id) = call.native_id.clone() {
                            tool_result_messages.push(ToolResultMessage {
                                tool_use_id: native_id,
                                tool_name: call.tool_name.clone(),
                                output: "Tool call cancelled by ToolCallBefore listener"
                                    .to_string(),
                                is_error: false,
                                is_rejected: true,
                            });
                        }
                        continue;
                    }

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
                        &tool_args_preview(call),
                    );

                    // Transition to Running
                    exec.mark_running();
                    progress.on_tool_execution_started(task_id_str, &exec_id, &call.tool_name);

                    all_executions.push(exec);
                    exec_indices.push(all_executions.len() - 1);

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

                    if !is_error {
                        all_outputs.push(format!("[{}]: {}", call.tool_name, output));
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
            let mut high_risk_rejected_count = 0;
            for call in &high_risk_calls {
                // ToolCallBefore check: Lua filter can cancel tool calls before HiL
                if !self.check_tool_call_before(&call.tool_name, &call.arguments) {
                    debug!(
                        "High-risk tool call {} cancelled by ToolCallBefore listener",
                        call.tool_name
                    );
                    high_risk_rejected_count += 1;
                    if let Some(native_id) = call.native_id.clone() {
                        tool_result_messages.push(ToolResultMessage {
                            tool_use_id: native_id,
                            tool_name: call.tool_name.clone(),
                            output: "Tool call cancelled by ToolCallBefore listener".to_string(),
                            is_error: false,
                            is_rejected: true,
                        });
                    }
                    continue;
                }

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
                    &tool_args_preview(call),
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
                        high_risk_rejected_count += 1;
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

                if !is_error {
                    all_outputs.push(format!("[{}]: {}", call.tool_name, output));
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

            // If ALL high-risk calls were rejected (and there were some),
            // propagate ActionRejected to the outer retry loop.
            if !high_risk_calls.is_empty()
                && high_risk_rejected_count == high_risk_calls.len()
                && low_risk_calls.is_empty()
            {
                return Err(RunAgentError::ActionRejected(
                    "All tool calls rejected by quorum review".to_string(),
                ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_brief_skips_thinking_prefix() {
        let output = "**Thinking**: let me analyze this\nThe config uses TOML format.";
        let brief = extract_task_brief(output, 150);
        assert_eq!(brief.unwrap(), "The config uses TOML format.");
    }

    #[test]
    fn extract_brief_skips_tool_headers() {
        let output = "[read_file]: contents of foo.rs\n---\nFound 3 functions in the module.";
        let brief = extract_task_brief(output, 150);
        assert_eq!(brief.unwrap(), "Found 3 functions in the module.");
    }

    #[test]
    fn extract_brief_skips_separator_lines() {
        let output = "---\n===\nActual meaningful content here.";
        let brief = extract_task_brief(output, 150);
        assert_eq!(brief.unwrap(), "Actual meaningful content here.");
    }

    #[test]
    fn extract_brief_truncates_long_lines() {
        let output = "A".repeat(300);
        let brief = extract_task_brief(&output, 150).unwrap();
        assert!(brief.len() <= 150);
    }

    #[test]
    fn extract_brief_returns_none_for_empty() {
        assert!(extract_task_brief("", 150).is_none());
        assert!(extract_task_brief("---\n===", 150).is_none());
    }

    #[test]
    fn extract_brief_returns_first_meaningful_line() {
        let output = "\n\n  No matches found in .rs files\nSome other info";
        let brief = extract_task_brief(output, 150);
        assert_eq!(brief.unwrap(), "No matches found in .rs files");
    }

    // ==================== Tool-call leak tests (#268) ====================

    use crate::config::ExecutionParams;
    use crate::ports::conversation_logger::NoConversationLogger;
    use crate::ports::llm_gateway::GatewayError;
    use async_trait::async_trait;
    use quorum_domain::session::response::{ContentBlock, LlmResponse, StopReason};
    use quorum_domain::tool::entities::{
        RiskLevel, ToolCall, ToolDefinition, ToolParameter, ToolSpec,
    };
    use quorum_domain::tool::value_objects::ToolResult;
    use quorum_domain::{AgentPolicy, ConsensusLevel, PhaseScope, Plan, SessionMode};
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    /// Session that pops pre-scripted responses in order.
    struct QueueSession {
        model: Model,
        responses: Arc<Mutex<VecDeque<LlmResponse>>>,
    }

    impl QueueSession {
        fn pop(&self) -> LlmResponse {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| LlmResponse::from_text("(exhausted)"))
        }
    }

    #[async_trait]
    impl LlmSession for QueueSession {
        fn model(&self) -> &Model {
            &self.model
        }

        async fn send(&self, _content: &str) -> Result<String, GatewayError> {
            Ok(self.pop().text_content())
        }

        async fn send_with_tools(
            &self,
            _content: &str,
            _tools: &[serde_json::Value],
        ) -> Result<LlmResponse, GatewayError> {
            Ok(self.pop())
        }

        async fn send_tool_results(
            &self,
            _results: &[ToolResultMessage],
        ) -> Result<LlmResponse, GatewayError> {
            Ok(self.pop())
        }
    }

    struct QueueGateway {
        responses: Arc<Mutex<VecDeque<LlmResponse>>>,
    }

    #[async_trait]
    impl LlmGateway for QueueGateway {
        async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError> {
            Ok(Box::new(QueueSession {
                model: model.clone(),
                responses: self.responses.clone(),
            }))
        }

        async fn create_session_with_system_prompt(
            &self,
            model: &Model,
            _system_prompt: &str,
        ) -> Result<Box<dyn LlmSession>, GatewayError> {
            self.create_session(model).await
        }

        async fn available_models(&self) -> Result<Vec<Model>, GatewayError> {
            Ok(vec![])
        }
    }

    /// Executor whose spec has `run_command` (required param `command`),
    /// matching the #268 reproduction. Records executed tool names.
    struct RecordingToolExecutor {
        spec: ToolSpec,
        calls: Mutex<Vec<String>>,
    }

    impl RecordingToolExecutor {
        fn new() -> Self {
            Self {
                spec: ToolSpec::new().register(
                    ToolDefinition::new("run_command", "Run a shell command", RiskLevel::High)
                        .with_parameter(ToolParameter::new("command", "Command to run", true)),
                ),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ToolExecutorPort for RecordingToolExecutor {
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

    struct StubToolSchema;

    impl ToolSchemaPort for StubToolSchema {
        fn tool_to_schema(&self, tool: &ToolDefinition) -> serde_json::Value {
            serde_json::json!({ "name": tool.name })
        }

        fn all_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value> {
            spec.all().map(|t| self.tool_to_schema(t)).collect()
        }

        fn low_risk_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value> {
            spec.low_risk_tools()
                .map(|t| self.tool_to_schema(t))
                .collect()
        }
    }

    /// Reviewer that treats everything as low-risk (no review round-trips).
    struct LowRiskReviewer;

    #[async_trait]
    impl ActionReviewer for LowRiskReviewer {
        async fn review_action(
            &self,
            _tool_call_json: &str,
            _task: &Task,
            _state: &AgentState,
            _models: &ModelConfig,
            _progress: &dyn AgentProgressNotifier,
        ) -> Result<ReviewDecision, RunAgentError> {
            Ok(ReviewDecision::SkipReview)
        }

        fn is_high_risk_tool(
            &self,
            _tool_name: &str,
            _arguments: &HashMap<String, serde_json::Value>,
        ) -> bool {
            false
        }
    }

    struct NoopProgress;
    impl AgentProgressNotifier for NoopProgress {}

    fn make_use_case(
        responses: Vec<LlmResponse>,
        executor: Arc<RecordingToolExecutor>,
    ) -> ExecuteTaskUseCase {
        let gateway = Arc::new(QueueGateway {
            responses: Arc::new(Mutex::new(responses.into())),
        });
        ExecuteTaskUseCase::new(
            gateway,
            executor,
            Arc::new(StubToolSchema),
            None,
            Arc::new(LowRiskReviewer),
            Arc::new(NoConversationLogger),
        )
    }

    fn test_input() -> RunAgentInput {
        RunAgentInput::new(
            "List the crates in this workspace",
            SessionMode {
                consensus_level: ConsensusLevel::Solo,
                phase_scope: PhaseScope::Fast,
                strategy: Default::default(),
            },
            ModelConfig::default(),
            AgentPolicy::default(),
            ExecutionParams {
                max_iterations: 10,
                max_tool_turns: 5,
                max_tool_retries: 2,
                working_dir: None,
                ensemble_session_timeout: None,
                context_budget: ContextBudget::default(),
            },
        )
    }

    fn test_state(input: &RunAgentInput, task: Task) -> AgentState {
        let mut state = input.to_agent_state("agent-test");
        let mut plan = Plan::new("List workspace crates", "test");
        plan.add_task(task);
        state.set_plan(plan);
        state
    }

    /// The exact leak shape from the #268 reproduction.
    fn leak_response() -> LlmResponse {
        LlmResponse::from_text(
            "```json\n{\n  \"command\": \"cargo metadata --no-deps --format-version 1\"\n}\n```",
        )
    }

    fn tool_use_response() -> LlmResponse {
        let mut arguments = HashMap::new();
        arguments.insert("command".to_string(), serde_json::json!("ls"));
        LlmResponse {
            content: vec![ContentBlock::ToolUse {
                id: "toolu_1".to_string(),
                name: "run_command".to_string(),
                input: arguments,
            }],
            stop_reason: Some(StopReason::ToolUse),
            model: None,
        }
    }

    #[tokio::test]
    async fn leaked_tool_call_json_is_nudged_into_real_tool_call() {
        let executor = Arc::new(RecordingToolExecutor::new());
        let use_case = make_use_case(
            vec![
                leak_response(),     // turn 0: JSON as text → nudge
                tool_use_response(), // nudge response: actual tool call
                LlmResponse::from_text("The workspace has 5 crates."),
            ],
            executor.clone(),
        );
        let input = test_input();
        let mut state = test_state(&input, Task::new("1", "List crates"));

        let summary = use_case
            .execute(&input, &mut state, "system", &NoopProgress)
            .await
            .expect("should succeed");

        assert!(summary.contains("Completed 1/1"), "summary: {}", summary);
        assert!(summary.contains("The workspace has 5 crates."));
        assert!(
            !summary.contains("cargo metadata"),
            "leaked JSON must not surface: {}",
            summary
        );
        assert_eq!(
            executor.calls.lock().unwrap().as_slice(),
            &["run_command".to_string()]
        );
    }

    #[tokio::test]
    async fn leaked_json_fails_task_when_nudges_exhausted() {
        let executor = Arc::new(RecordingToolExecutor::new());
        let use_case = make_use_case(
            vec![leak_response(), leak_response(), leak_response()],
            executor.clone(),
        );
        let input = test_input();
        let mut state = test_state(&input, Task::new("1", "List crates"));

        let summary = use_case
            .execute(&input, &mut state, "system", &NoopProgress)
            .await
            .expect("execute() itself should not fail");

        // Task fails, but the raw JSON never surfaces
        assert!(summary.contains("FAILED"), "summary: {}", summary);
        assert!(
            !summary.contains("cargo metadata"),
            "leaked JSON must not surface: {}",
            summary
        );
        assert!(executor.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn leak_after_tool_turn_is_suppressed_but_earlier_output_kept() {
        let executor = Arc::new(RecordingToolExecutor::new());
        let use_case = make_use_case(
            vec![
                tool_use_response(), // turn 1: real tool call
                leak_response(),     // after tool result: leak → nudge
                leak_response(),     // still leaking → nudge
                leak_response(),     // budget exhausted → suppress
            ],
            executor.clone(),
        );
        let input = test_input();
        let mut state = test_state(&input, Task::new("1", "List crates"));

        let summary = use_case
            .execute(&input, &mut state, "system", &NoopProgress)
            .await
            .expect("should succeed");

        // Tool output is kept; the trailing leak is dropped
        assert!(summary.contains("OK"), "summary: {}", summary);
        assert!(
            !summary.contains("cargo metadata"),
            "leaked JSON must not surface: {}",
            summary
        );
        let task = &state.plan.as_ref().unwrap().tasks[0];
        let output = task.result.as_ref().unwrap().output.clone();
        assert!(output.contains("[run_command]: ok"), "output: {}", output);
        assert!(!output.contains("cargo metadata"), "output: {}", output);
    }

    #[tokio::test]
    async fn plain_text_answer_is_kept_as_before() {
        let executor = Arc::new(RecordingToolExecutor::new());
        let use_case = make_use_case(
            vec![LlmResponse::from_text(
                "The workspace contains 5 crates: domain, application, ...",
            )],
            executor.clone(),
        );
        let input = test_input();
        let mut state = test_state(&input, Task::new("1", "List crates"));

        let summary = use_case
            .execute(&input, &mut state, "system", &NoopProgress)
            .await
            .expect("should succeed");

        assert!(summary.contains("Completed 1/1"), "summary: {}", summary);
        assert!(summary.contains("The workspace contains 5 crates"));
    }
}
