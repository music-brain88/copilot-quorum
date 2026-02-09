//! Copilot session management.
//!
//! Provides [`CopilotSession`] which implements [`LlmSession`] for
//! maintaining a conversation with a specific LLM model through Copilot CLI.

use crate::copilot::error::{CopilotError, Result};
use crate::copilot::protocol::{
    CopilotToolDefinition, CreateSessionParams, JsonRpcRequest, JsonRpcResponseOut, SendParams,
    ToolCallResult,
};
use crate::copilot::transport::{StdioTransport, StreamingOutcome};
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmSession, StreamHandle, ToolResultMessage};
use quorum_domain::session::response::{ContentBlock, LlmResponse, StopReason};
use quorum_domain::{Model, StreamEvent};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Internal state for a tool-enabled session.
struct ToolSessionState {
    /// Session ID for the tool-enabled session (re-created with tools).
    session_id: String,
    /// Pending tool.call request awaiting a response from us.
    pending_tool_call: Option<PendingToolCall>,
}

/// A pending tool.call request that we need to respond to.
struct PendingToolCall {
    /// The JSON-RPC request ID we must echo back.
    request_id: u64,
}

/// An active conversation session with a specific Copilot model.
///
/// Maintains session state and allows sending prompts and receiving responses.
/// Implements [`LlmSession`] for use with the application layer.
pub struct CopilotSession {
    transport: Arc<StdioTransport>,
    session_id: String,
    model: Model,
    system_prompt: Option<String>,
    tool_session: Mutex<Option<ToolSessionState>>,
}

impl CopilotSession {
    /// Create a new session with the specified model
    pub async fn new(transport: Arc<StdioTransport>, model: Model) -> Result<Self> {
        Self::new_with_system_prompt(transport, model, None).await
    }

    /// Create a new session with a system prompt
    pub async fn new_with_system_prompt(
        transport: Arc<StdioTransport>,
        model: Model,
        system_prompt: Option<String>,
    ) -> Result<Self> {
        info!("Creating session with model: {}", model);

        let params = CreateSessionParams {
            model: Some(model.to_string()),
            system_prompt: system_prompt.clone(),
            tools: None,
        };

        let request = JsonRpcRequest::new("session.create", Some(serde_json::to_value(&params)?));

        // Send the request and wait for session.start event
        transport.send_request(&request).await?;

        // Wait for session.start notification to get session_id
        let session_id = transport.wait_for_session_start().await?;

        debug!("Session created: {}", session_id);

        Ok(Self {
            transport,
            session_id,
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
        Self::ask_streaming_inner(&self.transport, &self.session_id, content, on_chunk).await
    }

    /// Static inner implementation that doesn't borrow `self`.
    ///
    /// This allows spawning streaming work in a `tokio::spawn` task
    /// where `&self` cannot be sent across threads.
    async fn ask_streaming_inner<F>(
        transport: &StdioTransport,
        session_id: &str,
        content: &str,
        on_chunk: F,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        debug!("Sending to session {}: {}", session_id, content);

        let params = SendParams {
            session_id: session_id.to_string(),
            prompt: content.to_string(),
        };

        let request = JsonRpcRequest::new("session.send", Some(serde_json::to_value(&params)?));

        // Send the request
        let response = transport.request(&request).await?;

        if let Some(error) = response.error {
            return Err(CopilotError::RpcError {
                code: error.code,
                message: error.message,
            });
        }

        debug!("session.send response: {:?}", response.result);

        // Read streaming notifications until session.idle
        let content = transport.read_streaming(on_chunk).await?;

        Ok(content)
    }

    /// Sends a prompt with cancellation support
    pub async fn ask_with_cancellation(
        &self,
        content: &str,
        cancellation: CancellationToken,
    ) -> Result<String> {
        self.ask_streaming_with_cancellation(content, |_| {}, cancellation)
            .await
    }

    /// Sends a prompt and streams the response with cancellation support
    pub async fn ask_streaming_with_cancellation<F>(
        &self,
        content: &str,
        on_chunk: F,
        cancellation: CancellationToken,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        // Check for cancellation before starting
        if cancellation.is_cancelled() {
            return Err(CopilotError::Cancelled);
        }

        debug!("Sending to session {}: {}", self.session_id, content);

        let params = SendParams {
            session_id: self.session_id.clone(),
            prompt: content.to_string(),
        };

        let request = JsonRpcRequest::new("session.send", Some(serde_json::to_value(&params)?));

        // Send the request
        let response = self.transport.request(&request).await?;

        if let Some(error) = response.error {
            return Err(CopilotError::RpcError {
                code: error.code,
                message: error.message,
            });
        }

        debug!("session.send response: {:?}", response.result);

        // Read streaming notifications until session.idle with cancellation support
        let content = self
            .transport
            .read_streaming_with_cancellation(on_chunk, cancellation)
            .await?;

        Ok(content)
    }

    /// Create a tool-enabled session and send the initial prompt.
    ///
    /// This creates a **new** Copilot session with tool definitions attached,
    /// then sends the prompt and reads the streaming response.
    async fn create_tool_session_and_send(
        &self,
        content: &str,
        tools: &[serde_json::Value],
    ) -> std::result::Result<LlmResponse, GatewayError> {
        let copilot_tools: Vec<CopilotToolDefinition> = tools
            .iter()
            .filter_map(CopilotToolDefinition::from_api_tool)
            .collect();

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

        let request = JsonRpcRequest::new("session.create", Some(
            serde_json::to_value(&params)
                .map_err(|e| GatewayError::RequestFailed(e.to_string()))?,
        ));

        self.transport
            .send_request(&request)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        let tool_session_id = self
            .transport
            .wait_for_session_start()
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        debug!("Tool session created: {}", tool_session_id);

        // Send the prompt
        let send_params = SendParams {
            session_id: tool_session_id.clone(),
            prompt: content.to_string(),
        };

        let send_request = JsonRpcRequest::new("session.send", Some(
            serde_json::to_value(&send_params)
                .map_err(|e| GatewayError::RequestFailed(e.to_string()))?,
        ));

        let response = self
            .transport
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
        let llm_response = self.read_and_build_response(&tool_session_id).await?;

        Ok(llm_response)
    }

    /// Read streaming output and build an `LlmResponse`.
    ///
    /// Shared logic between `send_with_tools()` and `send_tool_results()`.
    async fn read_and_build_response(
        &self,
        tool_session_id: &str,
    ) -> std::result::Result<LlmResponse, GatewayError> {
        let outcome = self
            .transport
            .read_streaming_for_tools(|_chunk| {
                // We could forward chunks here for streaming, but for now
                // we just let the content accumulate.
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
                request_id,
                params,
            } => {
                debug!(
                    "Tool call received: {} (request_id={})",
                    params.tool_name, request_id
                );

                // Store the pending tool call state
                {
                    let mut tool_session = self.tool_session.lock().await;
                    *tool_session = Some(ToolSessionState {
                        session_id: tool_session_id.to_string(),
                        pending_tool_call: Some(PendingToolCall { request_id }),
                    });
                }

                // Build response with text (if any) + tool use block
                let mut content = Vec::new();
                if !text_so_far.is_empty() {
                    content.push(ContentBlock::Text(text_so_far));
                }

                // Parse arguments from the tool call
                let input = if let Some(obj) = params.arguments.as_object() {
                    obj.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
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

    async fn send_streaming(
        &self,
        content: &str,
    ) -> std::result::Result<StreamHandle, GatewayError> {
        let (tx, rx) = mpsc::channel::<StreamEvent>(32);
        let transport = self.transport.clone();
        let session_id = self.session_id.clone();
        let content = content.to_string();

        tokio::spawn(async move {
            let tx_for_cb = tx.clone();
            let result =
                Self::ask_streaming_inner(&transport, &session_id, &content, move |chunk| {
                    let _ = tx_for_cb.try_send(StreamEvent::Delta(chunk.to_string()));
                })
                .await;

            match result {
                Ok(full) => {
                    let _ = tx.send(StreamEvent::Completed(full)).await;
                }
                Err(e) => {
                    let _ = tx.send(StreamEvent::Error(e.to_string())).await;
                }
            }
        });

        Ok(StreamHandle::new(rx))
    }

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
        let (request_id, tool_session_id) = {
            let mut tool_session = self.tool_session.lock().await;
            let state = tool_session.as_mut().ok_or_else(|| {
                GatewayError::RequestFailed(
                    "No tool session active — call send_with_tools() first".to_string(),
                )
            })?;

            let pending = state.pending_tool_call.take().ok_or_else(|| {
                GatewayError::RequestFailed(
                    "No pending tool call to respond to".to_string(),
                )
            })?;

            (pending.request_id, state.session_id.clone())
        };

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

        self.transport
            .send_response(&response)
            .await
            .map_err(|e| GatewayError::RequestFailed(e.to_string()))?;

        debug!("Tool result sent for request_id={}", request_id);

        // Read the next streaming response
        self.read_and_build_response(&tool_session_id).await
    }
}
