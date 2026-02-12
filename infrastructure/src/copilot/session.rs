//! Copilot session management.
//!
//! Provides [`CopilotSession`] which implements
//! [`LlmSession`] for
//! maintaining a conversation with a specific LLM model through the Copilot CLI.
//!
//! # Feature support
//!
//! A single `CopilotSession` encapsulates one LLM conversation. Depending on
//! the feature, sessions are used differently:
//!
//! | Feature | Session usage |
//! |---------|--------------|
//! | **Solo mode** | [`send`](CopilotSession) for text Q&A |
//! | **Quorum Discussion** | Multiple sessions in parallel, each with its own [`SessionChannel`] |
//! | **Ensemble Planning** | N plan-generation + N×(N−1) voting sessions |
//! | **Native Tool Use** | [`send_with_tools`](CopilotSession) creates a second tool-enabled session internally |
//! | **Agent System** | Multi-turn loop: `send_with_tools` → execute → [`send_tool_results`](CopilotSession) → repeat |
//!
//! # Tool session lifecycle
//!
//! When [`send_with_tools`](CopilotSession) is called, a **separate** Copilot
//! session is created with tool definitions attached. This tool session is
//! stored internally so that subsequent calls to
//! [`send_tool_results`](CopilotSession) can reuse the same session and
//! channel for the multi-turn tool-use loop.

use crate::copilot::error::{CopilotError, Result};
use crate::copilot::protocol::{
    CopilotToolDefinition, CreateSessionParams, JsonRpcRequest, JsonRpcResponseOut, SendParams,
    ToolCallResult,
};
use crate::copilot::router::{MessageRouter, SessionChannel};
use crate::copilot::transport::StreamingOutcome;
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmSession, ToolResultMessage};
use quorum_domain::Model;
use quorum_domain::session::response::{ContentBlock, LlmResponse, StopReason};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Internal state for a tool-enabled session (**Native Tool Use**).
///
/// Created by [`CopilotSession::create_tool_session_and_send`] when the
/// application layer calls [`send_with_tools`](LlmSession::send_with_tools).
/// Holds its own [`SessionChannel`] so that the multi-turn tool-call loop
/// can continue reading from the same session across multiple
/// [`send_tool_results`](LlmSession::send_tool_results) calls.
struct ToolSessionState {
    /// Session ID for the tool-enabled session (re-created with tools).
    #[allow(dead_code)]
    session_id: String,
    /// Dedicated channel for the tool-enabled session.
    channel: SessionChannel,
    /// Pending `tool.call` request awaiting our response.
    pending_tool_call: Option<PendingToolCall>,
}

/// A pending tool.call request that we need to respond to.
struct PendingToolCall {
    /// The JSON-RPC request ID we must echo back.
    request_id: u64,
}

/// An active conversation session with a specific Copilot model.
///
/// Implements [`LlmSession`] for use with the application layer.
/// Each instance holds its own [`SessionChannel`] for receiving routed
/// messages from the [`MessageRouter`], ensuring complete isolation from
/// other concurrent sessions.
///
/// # Feature support
///
/// - **Text Q&A** (Solo, Quorum, Ensemble): [`send`](LlmSession::send) /
///   [`ask_streaming`](Self::ask_streaming)
/// - **Native Tool Use**: [`send_with_tools`](LlmSession::send_with_tools)
///   creates an internal tool session; [`send_tool_results`](LlmSession::send_tool_results)
///   continues the multi-turn loop.
/// - **Cancellation**: [`ask_with_cancellation`](Self::ask_with_cancellation)
///   for user-interruptible Quorum Discussion phases.
pub struct CopilotSession {
    router: Arc<MessageRouter>,
    session_id: String,
    channel: Mutex<SessionChannel>,
    model: Model,
    system_prompt: Option<String>,
    tool_session: Mutex<Option<ToolSessionState>>,
}

impl CopilotSession {
    /// Create a new session with the specified model.
    ///
    /// Delegates to [`MessageRouter::create_session`] which handles the
    /// `session.create` → `session.start` handshake internally.
    pub async fn new(router: Arc<MessageRouter>, model: Model) -> Result<Self> {
        Self::new_with_system_prompt(router, model, None).await
    }

    /// Create a new session with a system prompt
    pub async fn new_with_system_prompt(
        router: Arc<MessageRouter>,
        model: Model,
        system_prompt: Option<String>,
    ) -> Result<Self> {
        info!("Creating session with model: {}", model);

        let params = CreateSessionParams {
            model: Some(model.to_string()),
            system_prompt: system_prompt.clone(),
            tools: None,
        };

        let (session_id, channel) = router.create_session(params).await?;
        debug!("Session created: {}", session_id);

        Ok(Self {
            router,
            session_id,
            channel: Mutex::new(channel),
            model,
            system_prompt,
            tool_session: Mutex::new(None),
        })
    }

    /// Returns the Copilot session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Sends a prompt and waits for the complete response.
    pub async fn ask(&self, content: &str) -> Result<String> {
        self.ask_streaming(content, |_| {}).await
    }

    /// Sends a prompt and streams the response, calling `on_chunk` for each piece.
    pub async fn ask_streaming<F>(&self, content: &str, on_chunk: F) -> Result<String>
    where
        F: FnMut(&str),
    {
        debug!("Sending to session {}: {}", self.session_id, content);

        let params = SendParams {
            session_id: self.session_id.clone(),
            prompt: content.to_string(),
        };

        let request = JsonRpcRequest::new("session.send", Some(serde_json::to_value(&params)?));

        let response = self.router.request(&request).await?;

        if let Some(error) = response.error {
            return Err(CopilotError::RpcError {
                code: error.code,
                message: error.message,
            });
        }

        debug!("session.send response: {:?}", response.result);

        let mut channel = self.channel.lock().await;
        let content = channel.read_streaming(on_chunk).await?;

        Ok(content)
    }

    /// Sends a prompt with cancellation support
    pub async fn ask_with_cancellation(
        &self,
        content: &str,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<String> {
        self.ask_streaming_with_cancellation(content, |_| {}, cancellation)
            .await
    }

    /// Sends a prompt and streams the response with cancellation support
    pub async fn ask_streaming_with_cancellation<F>(
        &self,
        content: &str,
        on_chunk: F,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        if cancellation.is_cancelled() {
            return Err(CopilotError::Cancelled);
        }

        debug!("Sending to session {}: {}", self.session_id, content);

        let params = SendParams {
            session_id: self.session_id.clone(),
            prompt: content.to_string(),
        };

        let request = JsonRpcRequest::new("session.send", Some(serde_json::to_value(&params)?));

        let response = self.router.request(&request).await?;

        if let Some(error) = response.error {
            return Err(CopilotError::RpcError {
                code: error.code,
                message: error.message,
            });
        }

        debug!("session.send response: {:?}", response.result);

        let mut channel = self.channel.lock().await;
        let content = channel
            .read_streaming_with_cancellation(on_chunk, cancellation)
            .await?;

        Ok(content)
    }

    /// Create a tool-enabled session and send the initial prompt.
    ///
    /// Called by the [`LlmSession::send_with_tools`] implementation.
    /// This creates a **new** Copilot session with tool definitions attached,
    /// sends the prompt, and reads the streaming response. If the LLM
    /// returns a `tool.call`, the tool session is stashed in
    /// [`ToolSessionState`] so that [`send_tool_results`](LlmSession::send_tool_results)
    /// can continue the conversation on the same session.
    ///
    /// # Native Tool Use flow
    ///
    /// ```text
    /// send_with_tools(prompt, tools)
    ///   └─ create_tool_session_and_send()
    ///        ├─ router.create_session(params_with_tools)
    ///        ├─ router.request(session.send)
    ///        └─ tool_channel.read_streaming_for_tools()
    ///             ├─ Idle → return text response
    ///             └─ ToolCall → stash channel + return ToolUse response
    /// ```
    async fn create_tool_session_and_send(
        &self,
        content: &str,
        tools: &[serde_json::Value],
    ) -> std::result::Result<LlmResponse, GatewayError> {
        let copilot_tools: Vec<CopilotToolDefinition> = tools
            .iter()
            .filter_map(CopilotToolDefinition::from_api_tool)
            .collect();

        debug!(
            "Tool conversion: {}/{} tools converted ({})",
            copilot_tools.len(),
            tools.len(),
            copilot_tools
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        if copilot_tools.is_empty() {
            warn!("No valid tools converted, falling back to text-only session");
            let text = self
                .ask(content)
                .await
                .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;
            return Ok(LlmResponse::from_text(text));
        }

        // Create a new session with tools
        let params = CreateSessionParams {
            model: Some(self.model.to_string()),
            system_prompt: self.system_prompt.clone(),
            tools: Some(copilot_tools),
        };

        let (tool_session_id, mut tool_channel) = self
            .router
            .create_session(params)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        debug!("Tool session created: {}", tool_session_id);

        // Send the prompt
        let send_params = SendParams {
            session_id: tool_session_id.clone(),
            prompt: content.to_string(),
        };

        let send_request = JsonRpcRequest::new(
            "session.send",
            Some(
                serde_json::to_value(&send_params)
                    .map_err(|e| GatewayError::RequestFailed(e.to_string()))?,
            ),
        );

        let response = self
            .router
            .request(&send_request)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        if let Some(error) = response.error {
            return Err(GatewayError::RequestFailed(format!(
                "RPC error ({}): {}",
                error.code, error.message
            )));
        }

        // Read streaming and build response
        let outcome = tool_channel
            .read_streaming_for_tools(|_chunk| {})
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        self.build_response_from_outcome(outcome, tool_session_id, tool_channel)
            .await
    }

    /// Build an [`LlmResponse`] from a streaming outcome, stashing tool session
    /// state when a tool call is received.
    ///
    /// Maps [`StreamingOutcome::Idle`] to a text response with
    /// [`StopReason::EndTurn`], and [`StreamingOutcome::ToolCall`] to a
    /// [`ContentBlock::ToolUse`] response with [`StopReason::ToolUse`] while
    /// saving the channel for the next [`send_tool_results`](LlmSession::send_tool_results).
    async fn build_response_from_outcome(
        &self,
        outcome: StreamingOutcome,
        tool_session_id: String,
        tool_channel: SessionChannel,
    ) -> std::result::Result<LlmResponse, GatewayError> {
        match outcome {
            StreamingOutcome::Idle(text) => {
                debug!("Tool session idle, text response received");
                // Channel drops here — session deregistered automatically
                Ok(LlmResponse {
                    content: vec![ContentBlock::Text(text)],
                    stop_reason: Some(StopReason::EndTurn),
                    model: Some(self.model.to_string()),
                })
            }
            StreamingOutcome::ToolCall {
                text_so_far,
                request_id,
                params,
            } => {
                debug!(
                    "Tool call received: {} (request_id={})",
                    params.tool_name, request_id
                );

                // Stash the tool session state (including channel) for
                // send_tool_results() to use later
                {
                    let mut tool_session = self.tool_session.lock().await;
                    *tool_session = Some(ToolSessionState {
                        session_id: tool_session_id,
                        channel: tool_channel,
                        pending_tool_call: Some(PendingToolCall { request_id }),
                    });
                }

                // Build response with text (if any) + tool use block
                let mut content = Vec::new();
                if !text_so_far.is_empty() {
                    content.push(ContentBlock::Text(text_so_far));
                }

                let input = if let Some(obj) = params.arguments.as_object() {
                    obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                } else {
                    std::collections::HashMap::new()
                };

                content.push(ContentBlock::ToolUse {
                    id: params.tool_call_id,
                    name: params.tool_name,
                    input,
                });

                Ok(LlmResponse {
                    content,
                    stop_reason: Some(StopReason::ToolUse),
                    model: Some(self.model.to_string()),
                })
            }
        }
    }
}

#[async_trait]
impl LlmSession for CopilotSession {
    fn model(&self) -> &Model {
        &self.model
    }

    async fn send(&self, content: &str) -> std::result::Result<String, GatewayError> {
        self.ask(content)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))
    }

    // send_streaming: uses default impl (delegates to send())

    async fn send_with_tools(
        &self,
        content: &str,
        tools: &[serde_json::Value],
    ) -> std::result::Result<LlmResponse, GatewayError> {
        self.create_tool_session_and_send(content, tools).await
    }

    async fn send_tool_results(
        &self,
        results: &[ToolResultMessage],
    ) -> std::result::Result<LlmResponse, GatewayError> {
        // Get the pending tool call state
        let mut tool_session_guard = self.tool_session.lock().await;
        let state = tool_session_guard.as_mut().ok_or_else(|| {
            GatewayError::RequestFailed(
                "No tool session active — call send_with_tools() first".to_string(),
            )
        })?;

        let pending = state.pending_tool_call.take().ok_or_else(|| {
            GatewayError::RequestFailed("No pending tool call to respond to".to_string())
        })?;

        let request_id = pending.request_id;

        // Build the tool call result from the first result
        // (Copilot CLI sends one tool.call at a time)
        let tool_result = if let Some(result) = results.first() {
            if result.is_error {
                ToolCallResult::error(&result.output)
            } else {
                ToolCallResult::success(&result.output)
            }
        } else {
            ToolCallResult::success("")
        };

        // Send the JSON-RPC response back to the CLI
        let response = JsonRpcResponseOut::new(
            request_id,
            serde_json::to_value(&tool_result)
                .map_err(|e| GatewayError::RequestFailed(e.to_string()))?,
        );

        self.router
            .send_response(&response)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        debug!("Tool result sent for request_id={}", request_id);

        // Read the next streaming response from the tool session channel
        let outcome = state
            .channel
            .read_streaming_for_tools(|_chunk| {})
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        match outcome {
            StreamingOutcome::Idle(text) => {
                debug!("Tool session idle, text response received");
                Ok(LlmResponse {
                    content: vec![ContentBlock::Text(text)],
                    stop_reason: Some(StopReason::EndTurn),
                    model: Some(self.model.to_string()),
                })
            }
            StreamingOutcome::ToolCall {
                text_so_far,
                request_id: new_request_id,
                params,
            } => {
                debug!(
                    "Tool call received: {} (request_id={})",
                    params.tool_name, new_request_id
                );

                // Store the new pending tool call
                state.pending_tool_call = Some(PendingToolCall {
                    request_id: new_request_id,
                });

                let mut content = Vec::new();
                if !text_so_far.is_empty() {
                    content.push(ContentBlock::Text(text_so_far));
                }

                let input = if let Some(obj) = params.arguments.as_object() {
                    obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                } else {
                    std::collections::HashMap::new()
                };

                content.push(ContentBlock::ToolUse {
                    id: params.tool_call_id,
                    name: params.tool_name,
                    input,
                });

                Ok(LlmResponse {
                    content,
                    stop_reason: Some(StopReason::ToolUse),
                    model: Some(self.model.to_string()),
                })
            }
        }
    }
}
