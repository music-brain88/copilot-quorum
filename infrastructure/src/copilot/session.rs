//! Copilot session management.
//!
//! Provides [`CopilotSession`] which implements [`LlmSession`] for
//! maintaining a conversation with a specific LLM model through Copilot CLI.

use crate::copilot::error::{CopilotError, Result};
use crate::copilot::protocol::{CreateSessionParams, JsonRpcRequest, SendParams};
use crate::copilot::transport::StdioTransport;
use async_trait::async_trait;
use quorum_application::ports::llm_gateway::{GatewayError, LlmSession, StreamHandle};
use quorum_domain::{Model, StreamEvent};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

/// An active conversation session with a specific Copilot model.
///
/// Maintains session state and allows sending prompts and receiving responses.
/// Implements [`LlmSession`] for use with the application layer.
pub struct CopilotSession {
    transport: Arc<StdioTransport>,
    session_id: String,
    model: Model,
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
            system_prompt,
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
}
