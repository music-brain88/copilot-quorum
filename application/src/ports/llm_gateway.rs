//! LLM Gateway port
//!
//! Defines the interface for communicating with LLM providers.

use async_trait::async_trait;
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
            }
        }
        // Channel closed without Completed — return what we have
        Ok(full_text)
    }
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
}
