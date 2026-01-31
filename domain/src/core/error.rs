//! Domain error types

use thiserror::Error;

/// Domain-level errors
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("No models configured for Quorum")]
    NoModels,

    #[error("All models failed to respond")]
    AllModelsFailed,

    #[error("Invalid question: {0}")]
    InvalidQuestion(String),

    #[error("Invalid model: {0}")]
    InvalidModel(String),

    #[error("Orchestration error: {0}")]
    OrchestrationError(String),
}
