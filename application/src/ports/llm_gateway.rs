//! LLM Gateway port
//!
//! Defines the interface for communicating with LLM providers.

use async_trait::async_trait;
use quorum_domain::Model;
use thiserror::Error;

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

/// An active LLM session
#[async_trait]
pub trait LlmSession: Send + Sync {
    /// Get the model used by this session
    fn model(&self) -> &Model;

    /// Send a message and get a response
    async fn send(&self, content: &str) -> Result<String, GatewayError>;
}
