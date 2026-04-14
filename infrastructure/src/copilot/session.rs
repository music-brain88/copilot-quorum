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
    SystemMessageConfig, ToolCallResult,
};
use crate::copilot::router::{MessageRouter, SessionChannel};
use crate::copilot::transport::StreamingOutcome;
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{
    GatewayError, LlmSession, StreamObserver, ToolResultMessage,
};
use quorum_domain::Model;
use quorum_domain::session::response::{ContentBlock, LlmResponse, StopReason};
use quorum_domain::util::truncate_str;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, trace, warn};

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

/// A pending tool invocation awaiting our result.
///
/// Copilot CLI delivers tool calls through two distinct mechanisms depending
/// on how the tool was registered and the CLI version:
///
/// - `Legacy` — built-in tools (and pre-1.0.25 user tools) arrive as a
///   JSON-RPC `tool.call` request; the reply is a JSON-RPC response with the
///   matching numeric id.
/// - `External` — user-defined tools on Copilot CLI 1.0.25+ arrive as
///   `external_tool.requested` session events; the reply is an outgoing
///   `session.tools.handlePendingToolCall` RPC keyed by a UUID string.
///
/// The variant determines which [`MessageRouter`](super::router::MessageRouter)
/// method `send_tool_results` dispatches to.
enum PendingToolCall {
    /// Legacy `tool.call` — respond via `send_response(JsonRpcResponseOut)`.
    Legacy { request_id: u64 },
    /// CLI 1.0.25+ `external_tool.requested` — respond via
    /// `respond_to_external_tool(session_id, request_id, result)`.
    External {
        session_id: String,
        request_id: String,
    },
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
    /// Optional observer that receives each streaming text chunk.
    /// Injected at construction time via [`new_with_observer`](Self::new_with_observer),
    /// immutable thereafter (no Mutex needed).
    stream_observer: Option<StreamObserver>,
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

        let system_message = system_prompt.as_ref().map(|content| SystemMessageConfig {
            mode: "append".to_string(),
            content: content.clone(),
        });

        let params = CreateSessionParams {
            model: Some(model.to_string()),
            system_prompt: system_prompt.clone(),
            system_message,
            tools: None,
            available_tools: None,
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
            stream_observer: None,
        })
    }

    /// Create a new session with a stream observer for real-time chunk delivery.
    ///
    /// The observer receives each text chunk as it arrives from the LLM,
    /// enabling per-model live streaming in the TUI.
    ///
    /// Sends `availableTools: []` to disable Copilot CLI built-in tools,
    /// preventing the model from executing commands during streaming sessions
    /// (e.g., Ensemble Planning, Quorum Discussion). User-defined tools
    /// passed via `send_with_tools()` still work normally.
    pub async fn new_with_observer(
        router: Arc<MessageRouter>,
        model: Model,
        system_prompt: Option<String>,
        observer: StreamObserver,
    ) -> Result<Self> {
        info!("Creating streaming session with model: {}", model);

        let system_message = system_prompt.as_ref().map(|content| SystemMessageConfig {
            mode: "append".to_string(),
            content: content.clone(),
        });

        let params = CreateSessionParams {
            model: Some(model.to_string()),
            system_prompt: system_prompt.clone(),
            system_message,
            tools: None,
            available_tools: Some(vec![]),
        };

        let (session_id, channel) = router.create_session(params).await?;
        debug!("Streaming session created: {}", session_id);

        Ok(Self {
            router,
            session_id,
            channel: Mutex::new(channel),
            model,
            system_prompt,
            tool_session: Mutex::new(None),
            stream_observer: Some(observer),
        })
    }

    /// Create a text-only session that disables all Copilot CLI built-in tools.
    ///
    /// Sends `availableTools: []` to the CLI, preventing the model from
    /// executing any tools (file operations, `gh` commands, subagents, etc.).
    /// Used for review sessions where the model should only produce text.
    pub async fn new_text_only(
        router: Arc<MessageRouter>,
        model: Model,
        system_prompt: Option<String>,
    ) -> Result<Self> {
        info!("Creating text-only session with model: {}", model);

        let system_message = system_prompt.as_ref().map(|content| SystemMessageConfig {
            mode: "append".to_string(),
            content: content.clone(),
        });

        let params = CreateSessionParams {
            model: Some(model.to_string()),
            system_prompt: system_prompt.clone(),
            system_message,
            tools: None,
            available_tools: Some(vec![]),
        };

        let (session_id, channel) = router.create_session(params).await?;
        debug!("Text-only session created: {}", session_id);

        Ok(Self {
            router,
            session_id,
            channel: Mutex::new(channel),
            model,
            system_prompt,
            tool_session: Mutex::new(None),
            stream_observer: None,
        })
    }

    /// Returns the Copilot session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Sends a prompt and waits for the complete response.
    ///
    /// If a stream observer was injected at construction time, each chunk
    /// is forwarded to it for real-time display.
    pub async fn ask(&self, content: &str) -> Result<String> {
        let observer = self.stream_observer.clone();
        self.ask_streaming(content, move |chunk| {
            if let Some(ref obs) = observer {
                obs(chunk);
            }
        })
        .await
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

        for tool in &copilot_tools {
            let size = serde_json::to_string(tool).map(|s| s.len()).unwrap_or(0);
            debug!("Tool definition '{}': {} bytes", tool.name, size);
        }

        if copilot_tools.is_empty() {
            warn!("No valid tools converted, falling back to text-only session");
            let text = self
                .ask(content)
                .await
                .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;
            return Ok(LlmResponse::from_text(text));
        }

        // Create a new session with tools
        let system_message = self
            .system_prompt
            .as_ref()
            .map(|content| SystemMessageConfig {
                mode: "append".to_string(),
                content: content.clone(),
            });

        let params = CreateSessionParams {
            model: Some(self.model.to_string()),
            system_prompt: self.system_prompt.clone(),
            system_message,
            tools: Some(copilot_tools),
            available_tools: None,
        };

        let payload_size = serde_json::to_string(&params).map(|s| s.len()).unwrap_or(0);
        debug!("session.create payload: {} bytes", payload_size);

        let (tool_session_id, mut tool_channel) = self
            .router
            .create_session(params)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        debug!("Tool session created: {}", tool_session_id);

        // Send the prompt
        debug!(
            "Tool session prompt (first ~500 chars): {}",
            truncate_str(content, 500)
        );
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

        // Read streaming and build response — forward chunks to observer if present
        let observer = self.stream_observer.clone();
        let outcome = tool_channel
            .read_streaming_for_tools(move |chunk| {
                if let Some(ref obs) = observer {
                    obs(chunk);
                }
            })
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
                        pending_tool_call: Some(PendingToolCall::Legacy { request_id }),
                    });
                }

                Ok(build_tool_use_response(
                    text_so_far,
                    params.tool_call_id,
                    params.tool_name,
                    params.arguments,
                    self.model.to_string(),
                ))
            }
            StreamingOutcome::ExternalToolCall {
                text_so_far,
                request_id,
                session_id,
                tool_call_id,
                tool_name,
                arguments,
            } => {
                debug!(
                    "External tool call received: {} (request_id={})",
                    tool_name, request_id
                );

                {
                    let mut tool_session = self.tool_session.lock().await;
                    *tool_session = Some(ToolSessionState {
                        session_id: tool_session_id,
                        channel: tool_channel,
                        pending_tool_call: Some(PendingToolCall::External {
                            session_id,
                            request_id,
                        }),
                    });
                }

                Ok(build_tool_use_response(
                    text_so_far,
                    tool_call_id,
                    tool_name,
                    arguments,
                    self.model.to_string(),
                ))
            }
        }
    }
}

/// Assemble an [`LlmResponse`] that carries any pre-tool-call text followed
/// by a [`ContentBlock::ToolUse`] block.
///
/// Shared between the legacy `tool.call` path and the CLI 1.0.25+
/// `external_tool.requested` path, which only differ in how the reply is
/// sent back (see [`PendingToolCall`]).
fn build_tool_use_response(
    text_so_far: String,
    tool_call_id: String,
    tool_name: String,
    arguments: serde_json::Value,
    model: String,
) -> LlmResponse {
    let mut content = Vec::new();
    if !text_so_far.is_empty() {
        content.push(ContentBlock::Text(text_so_far));
    }
    let input = if let Some(obj) = arguments.as_object() {
        obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    } else {
        std::collections::HashMap::new()
    };
    content.push(ContentBlock::ToolUse {
        id: tool_call_id,
        name: tool_name,
        input,
    });
    LlmResponse {
        content,
        stop_reason: Some(StopReason::ToolUse),
        model: Some(model),
    }
}

/// Map a batch of tool-execution results into the single `Result<String, String>`
/// that `session.tools.handlePendingToolCall` expects for CLI 1.0.25+ external
/// tools. Only the first message is considered because a single
/// `external_tool.requested` event corresponds to exactly one tool call.
///
/// - rejected → `Err("Tool rejected: ...")` so the LLM sees a distinct message
/// - error    → `Err(output)`
/// - success  → `Ok(output)`
/// - empty    → `Ok("")` (keeps the turn alive rather than stalling the LLM)
fn tool_results_to_external_outcome(
    results: &[ToolResultMessage],
) -> std::result::Result<String, String> {
    match results.first() {
        Some(r) if r.is_rejected => Err(format!("Tool rejected: {}", r.output)),
        Some(r) if r.is_error => Err(r.output.clone()),
        Some(r) => Ok(r.output.clone()),
        None => Ok(String::new()),
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

        let first = results.first();
        let first_tool_name = first.map(|r| r.tool_name.as_str()).unwrap_or("unknown");
        let first_output_bytes = first.map(|r| r.output.len()).unwrap_or(0);

        // Dispatch by pending variant: legacy `tool.call` goes back as a
        // JSON-RPC response, while CLI 1.0.25+ `external_tool.requested`
        // needs `session.tools.handlePendingToolCall` with the UUID.
        match pending {
            PendingToolCall::Legacy { request_id } => {
                let tool_result = match first {
                    Some(r) if r.is_rejected => ToolCallResult::rejected(&r.output),
                    Some(r) if r.is_error => ToolCallResult::error(&r.output),
                    Some(r) => ToolCallResult::success(&r.output),
                    None => ToolCallResult::success(""),
                };
                let result_type = tool_result.result_type.clone();
                let response =
                    JsonRpcResponseOut::new(request_id, tool_result.into_rpc_value());
                let response_json = serde_json::to_string(&response).unwrap_or_default();

                self.router
                    .send_response(&response)
                    .await
                    .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

                debug!(
                    "Tool result sent for request_id={}: tool={}, type={}, output_bytes={}",
                    request_id, first_tool_name, result_type, first_output_bytes,
                );
                trace!(
                    "Tool result payload for request_id={}: {}",
                    request_id,
                    truncate_str(&response_json, 2048),
                );
            }
            PendingToolCall::External {
                session_id,
                request_id,
            } => {
                let outcome = tool_results_to_external_outcome(results);

                self.router
                    .respond_to_external_tool(&session_id, &request_id, outcome)
                    .await
                    .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

                debug!(
                    "External tool result sent for request_id={}: tool={}, output_bytes={}",
                    request_id, first_tool_name, first_output_bytes,
                );
            }
        }

        // Read the next streaming response — forward chunks to observer if present
        let observer = self.stream_observer.clone();
        let outcome = state
            .channel
            .read_streaming_for_tools(move |chunk| {
                if let Some(ref obs) = observer {
                    obs(chunk);
                }
            })
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
                state.pending_tool_call = Some(PendingToolCall::Legacy {
                    request_id: new_request_id,
                });
                Ok(build_tool_use_response(
                    text_so_far,
                    params.tool_call_id,
                    params.tool_name,
                    params.arguments,
                    self.model.to_string(),
                ))
            }
            StreamingOutcome::ExternalToolCall {
                text_so_far,
                request_id: new_request_id,
                session_id: new_session_id,
                tool_call_id,
                tool_name,
                arguments,
            } => {
                debug!(
                    "External tool call received: {} (request_id={})",
                    tool_name, new_request_id
                );
                state.pending_tool_call = Some(PendingToolCall::External {
                    session_id: new_session_id,
                    request_id: new_request_id,
                });
                Ok(build_tool_use_response(
                    text_so_far,
                    tool_call_id,
                    tool_name,
                    arguments,
                    self.model.to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(output: &str, is_error: bool, is_rejected: bool) -> ToolResultMessage {
        ToolResultMessage {
            tool_use_id: "call_x".to_string(),
            tool_name: "create_plan".to_string(),
            output: output.to_string(),
            is_error,
            is_rejected,
        }
    }

    #[test]
    fn external_outcome_success() {
        let results = vec![msg("plan saved", false, false)];
        assert_eq!(
            tool_results_to_external_outcome(&results),
            Ok("plan saved".to_string())
        );
    }

    #[test]
    fn external_outcome_error_maps_to_err() {
        let results = vec![msg("boom", true, false)];
        assert_eq!(
            tool_results_to_external_outcome(&results),
            Err("boom".to_string())
        );
    }

    #[test]
    fn external_outcome_rejected_is_prefixed() {
        let results = vec![msg("user said no", false, true)];
        // Rejection deserves a distinct prefix so the LLM can distinguish
        // "I declined" from "the tool crashed".
        assert_eq!(
            tool_results_to_external_outcome(&results),
            Err("Tool rejected: user said no".to_string())
        );
    }

    #[test]
    fn external_outcome_empty_is_ok_empty() {
        // An empty results slice shouldn't stall the turn — send Ok("") to
        // let the LLM continue reasoning with a blank tool response.
        let results: Vec<ToolResultMessage> = vec![];
        assert_eq!(
            tool_results_to_external_outcome(&results),
            Ok(String::new())
        );
    }

    #[test]
    fn external_outcome_rejected_beats_error() {
        // Guard the precedence: if both flags somehow got set, rejected wins
        // because it carries more user-intent than a generic error.
        let results = vec![msg("weird", true, true)];
        assert_eq!(
            tool_results_to_external_outcome(&results),
            Err("Tool rejected: weird".to_string())
        );
    }
}
