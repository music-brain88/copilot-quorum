//! LLM Session repository trait

use crate::core::model::Model;
use async_trait::async_trait;

/// Repository trait for LLM sessions
///
/// This is a domain-level abstraction that defines how sessions
/// are created and managed. Implementations live in the infrastructure layer.
#[async_trait]
pub trait LlmSessionRepository: Send + Sync {
    /// Error type for repository operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Session handle type returned by create_session
    type Session: LlmSession;

    /// Create a new session with the specified model
    async fn create_session(&self, model: &Model) -> Result<Self::Session, Self::Error>;

    /// Create a new session with a system prompt
    async fn create_session_with_system_prompt(
        &self,
        model: &Model,
        system_prompt: &str,
    ) -> Result<Self::Session, Self::Error>;

    /// Get available models
    async fn available_models(&self) -> Result<Vec<Model>, Self::Error>;
}

/// Trait representing an active LLM session
#[async_trait]
pub trait LlmSession: Send + Sync {
    /// Error type for session operations
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get the model used by this session
    fn model(&self) -> &Model;

    /// Send a message and get a response
    async fn send(&self, content: &str) -> Result<String, Self::Error>;
}
