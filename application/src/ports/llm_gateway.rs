//! LLM Gateway port
//!
//! Defines the interface for communicating with LLM providers.

use async_trait::async_trait;
use quorum_domain::session::response::LlmResponse;
use quorum_domain::{Model, StreamEvent};
use thiserror::Error;
use tokio::sync::mpsc;

/// Errors that can occur during LLM gateway operations
#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Session error: {0}")]
    SessionError(String),

    #[error("Model not available: {0}")]
    ModelNotAvailable(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    #[error("Timeout")]
    Timeout,

    #[error("Transport closed")]
    TransportClosed,

    #[error("Other error: {0}")]
    Other(String),
}

/// Gateway for LLM communication
///
/// This port defines how the application layer communicates with LLM providers.
/// Implementations (adapters) live in the infrastructure layer.
#[async_trait]
pub trait LlmGateway: Send + Sync {
    /// Create a new session with the specified model
    async fn create_session(&self, model: &Model) -> Result<Box<dyn LlmSession>, GatewayError>;

    /// Create a new session with a system prompt
    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError>;

    /// Create a text-only session that cannot execute any tools.
    ///
    /// Used for review sessions where the model should only produce text
    /// and must not trigger any side-effects (file writes, commands, etc.).
    ///
    /// Default implementation delegates to `create_session_with_system_prompt`.
    /// The Copilot adapter overrides this to send `availableTools: []`,
    /// disabling CLI built-in tools.
    async fn create_text_only_session(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Box<dyn LlmSession>, GatewayError> {
        self.create_session_with_system_prompt(model, system_prompt)
            .await
    }

    /// Get available models
    async fn available_models(&self) -> Result<Vec<Model>, GatewayError>;
}

/// Handle for receiving streaming events from an LLM session.
///
/// Wraps an `mpsc::Receiver<StreamEvent>` and provides convenience methods
/// for consuming the stream.
pub struct StreamHandle {
    pub receiver: mpsc::Receiver<StreamEvent>,
}

impl StreamHandle {
    pub fn new(receiver: mpsc::Receiver<StreamEvent>) -> Self {
        Self { receiver }
    }

    /// Consume the stream and collect all text into a single string.
    ///
    /// Useful when you want streaming at the transport level but only need
    /// the final text (e.g., for the default `send_streaming` fallback).
    pub async fn collect_text(mut self) -> Result<String, GatewayError> {
        let mut full_text = String::new();
        while let Some(event) = self.receiver.recv().await {
            match event {
                StreamEvent::Delta(chunk) => full_text.push_str(&chunk),
                StreamEvent::Completed(text) => {
                    if full_text.is_empty() {
                        return Ok(text);
                    }
                    return Ok(full_text);
                }
                StreamEvent::Error(e) => {
                    return Err(GatewayError::RequestFailed(e));
                }
                // Native Tool Use events — extract text from completed response
                StreamEvent::CompletedResponse(response) => {
                    let text = response.text_content();
                    if full_text.is_empty() {
                        return Ok(text);
                    }
                    full_text.push_str(&text);
                    return Ok(full_text);
                }
                // Tool call deltas are not text — skip
                StreamEvent::ToolCallDelta { .. } => {}
            }
        }
        // Channel closed without Completed — return what we have
        Ok(full_text)
    }
}

/// Result of a tool execution, sent back to the LLM in the Native Tool Use loop.
///
/// Used with [`LlmSession::send_tool_results()`] to continue the multi-turn
/// tool use conversation.
///
/// # Example
///
/// ```ignore
/// let result = ToolResultMessage {
///     tool_use_id: "toolu_abc123".to_string(),
///     tool_name: "read_file".to_string(),
///     output: "fn main() { ... }".to_string(),
///     is_error: false,
///     is_rejected: false,
/// };
/// let next_response = session.send_tool_results(&[result]).await?;
/// ```
#[derive(Debug, Clone)]
pub struct ToolResultMessage {
    /// The `id` from the original `ContentBlock::ToolUse` (correlates request → result).
    pub tool_use_id: String,
    /// Canonical tool name (for logging/debugging).
    pub tool_name: String,
    /// Tool output or error message.
    pub output: String,
    /// Whether this result represents an error.
    pub is_error: bool,
    /// Whether this result was rejected by HiL / action review.
    ///
    /// When `true`, the transport layer sends `resultType: "rejected"` instead of
    /// `"failure"`, allowing the LLM to distinguish policy rejections from tool errors.
    pub is_rejected: bool,
}

/// An active LLM session
#[async_trait]
pub trait LlmSession: Send + Sync {
    /// Get the model used by this session
    fn model(&self) -> &Model;

    /// Send a message and get a response
    async fn send(&self, content: &str) -> Result<String, GatewayError>;

    /// Send a message and get a streaming response.
    ///
    /// Default implementation calls `send()` and wraps the result in a single
    /// `Completed` event, so existing implementations work without changes.
    async fn send_streaming(&self, content: &str) -> Result<StreamHandle, GatewayError> {
        let result = self.send(content).await?;
        let (tx, rx) = mpsc::channel(1);
        // Send Completed event — if the receiver is dropped, that's fine
        let _ = tx.send(StreamEvent::Completed(result)).await;
        Ok(StreamHandle::new(rx))
    }

    // ==================== Native Tool Use API ====================

    /// Send a message with tool definitions, getting a structured response.
    ///
    /// The `tools` parameter is passed to the API as structured JSON schemas.
    /// The API response contains `ContentBlock::ToolUse` blocks for tool calls.
    ///
    /// # Default fallback
    /// Delegates to `send()` and wraps the result in `LlmResponse::from_text()`.
    async fn send_with_tools(
        &self,
        content: &str,
        _tools: &[serde_json::Value],
    ) -> Result<LlmResponse, GatewayError> {
        let text = self.send(content).await?;
        Ok(LlmResponse::from_text(text))
    }

    /// Send tool execution results back to the LLM.
    ///
    /// Used in the multi-turn Native Tool Use loop:
    /// ```text
    /// send_with_tools() → ToolUse stop → execute tools → send_tool_results() → ...
    /// ```
    ///
    /// # Default fallback
    /// Formats results as text and sends via `send()`.
    async fn send_tool_results(
        &self,
        results: &[ToolResultMessage],
    ) -> Result<LlmResponse, GatewayError> {
        let text = format_tool_results_as_text(results);
        let response = self.send(&text).await?;
        Ok(LlmResponse::from_text(response))
    }
}

/// Format tool results as plain text for the default fallback path.
fn format_tool_results_as_text(results: &[ToolResultMessage]) -> String {
    results
        .iter()
        .map(|r| {
            let status = if r.is_error { "ERROR" } else { "OK" };
            format!(
                "## Tool Result: {} [{}]\n\n{}",
                r.tool_name, status, r.output
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}
