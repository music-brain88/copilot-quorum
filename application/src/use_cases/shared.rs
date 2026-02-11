//! Shared utilities for use cases.
//!
//! Contains cancellation checking and cancellable LLM interaction helpers
//! used across multiple use cases (GatherContext, ExecuteTask, RunAgent).

use crate::ports::agent_progress::AgentProgressNotifier;
use crate::use_cases::run_agent::RunAgentError;
use quorum_domain::session::response::LlmResponse;
use tokio_util::sync::CancellationToken;

use crate::ports::llm_gateway::LlmSession;

/// Check if cancellation has been requested.
///
/// Returns `Err(RunAgentError::Cancelled)` if the token exists and is cancelled.
pub(crate) fn check_cancelled(token: &Option<CancellationToken>) -> Result<(), RunAgentError> {
    if let Some(token) = token
        && token.is_cancelled()
    {
        return Err(RunAgentError::Cancelled);
    }
    Ok(())
}

/// Send a prompt with tools to the LLM with cancellation support (Native Tool Use path).
///
/// Returns the full `LlmResponse` with structured content blocks.
/// Checks for cancellation before sending and forwards text content to progress.
pub(crate) async fn send_with_tools_cancellable(
    session: &dyn LlmSession,
    prompt: &str,
    tools: &[serde_json::Value],
    progress: &dyn AgentProgressNotifier,
    cancellation_token: &Option<CancellationToken>,
) -> Result<LlmResponse, RunAgentError> {
    check_cancelled(cancellation_token)?;
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
