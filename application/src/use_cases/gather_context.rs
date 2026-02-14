//! Gather Context use case.
//!
//! Responsible for Phase 1 of the agent execution flow: collecting project
//! context through a 3-stage fallback strategy.
//!
//! 1. **Stage 1** — Load known files directly (no LLM needed)
//! 2. **Stage 2** — Run exploration agent with tool use
//! 3. **Stage 3** — Proceed with minimal context

use crate::config::ExecutionParams;
use crate::ports::agent_progress::AgentProgressNotifier;
use crate::ports::context_loader::ContextLoaderPort;
use crate::ports::llm_gateway::{LlmSession, ToolResultMessage};
use crate::ports::tool_executor::ToolExecutorPort;
use crate::use_cases::run_agent::RunAgentError;
use crate::use_cases::shared::{check_cancelled, send_with_tools_cancellable};
use quorum_domain::core::string::truncate;
use quorum_domain::{AgentContext, AgentPromptTemplate, ProjectContext};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Use case for gathering project context (Phase 1).
///
/// Uses a 3-stage fallback strategy:
/// 1. Load known files directly (no LLM needed)
/// 2. Run exploration agent with tool use
/// 3. Proceed with minimal context
pub struct GatherContextUseCase<T: ToolExecutorPort, C: ContextLoaderPort> {
    tool_executor: Arc<T>,
    context_loader: Option<Arc<C>>,
    cancellation_token: Option<CancellationToken>,
}

impl<T: ToolExecutorPort + 'static, C: ContextLoaderPort + 'static> GatherContextUseCase<T, C> {
    pub fn new(
        tool_executor: Arc<T>,
        context_loader: Option<Arc<C>>,
        cancellation_token: Option<CancellationToken>,
    ) -> Self {
        Self {
            tool_executor,
            context_loader,
            cancellation_token,
        }
    }

    /// Gather context about the project using 3-stage fallback strategy.
    ///
    /// # Arguments
    /// * `session` - LLM session for exploration (Stage 2)
    /// * `request` - The user's request (used to guide exploration)
    /// * `execution` - Execution parameters (working_dir, max_tool_turns)
    /// * `progress` - Progress notifier for UI updates
    pub async fn execute(
        &self,
        session: &dyn LlmSession,
        request: &str,
        execution: &ExecutionParams,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<AgentContext, RunAgentError> {
        let mut context = AgentContext::new();

        if let Some(working_dir) = &execution.working_dir {
            context = context.with_project_root(working_dir);
        }

        // ========== Stage 1: Load known files directly (no LLM needed) ==========
        if let Some(ref context_loader) = self.context_loader
            && let Some(ref working_dir) = execution.working_dir
        {
            let project_root = Path::new(working_dir);
            let files = context_loader.load_known_files(project_root);
            let project_ctx = context_loader.build_project_context(files);

            if project_ctx.has_sufficient_context() {
                info!(
                    "Stage 1: Using existing context from: {}",
                    project_ctx.source_description()
                );
                return Ok(Self::context_from_project_ctx(
                    project_ctx,
                    execution.working_dir.as_deref(),
                ));
            }

            // Even if not sufficient, preserve any partial context
            if !project_ctx.is_empty() {
                info!("Stage 1: Found partial context, proceeding to exploration");
                context = Self::merge_project_context(context, &project_ctx);
            }
        }

        // ========== Stage 2: Run exploration agent ==========
        info!("Stage 2: Running exploration agent for additional context");

        match self
            .run_exploration_agent(session, request, execution, progress)
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
        project_ctx: ProjectContext,
        working_dir: Option<&str>,
    ) -> AgentContext {
        let mut context = AgentContext::new();

        if let Some(working_dir) = working_dir {
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

    /// Run the exploration agent to gather context using Native Tool Use multi-turn loop.
    async fn run_exploration_agent(
        &self,
        session: &dyn LlmSession,
        request: &str,
        execution: &ExecutionParams,
        progress: &dyn AgentProgressNotifier,
    ) -> Result<AgentContext, RunAgentError> {
        let mut context = AgentContext::new();

        if let Some(working_dir) = &execution.working_dir {
            context = context.with_project_root(working_dir);
        }

        // Ask the model to gather context using tools (Native multi-turn loop)
        let prompt =
            AgentPromptTemplate::context_gathering(request, execution.working_dir.as_deref());
        let tools = self.tool_executor.tool_spec().to_api_tools();
        let max_turns = execution.max_tool_turns;
        let mut turn_count = 0;
        let mut results = Vec::new();

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

            check_cancelled(&self.cancellation_token)?;

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
}
