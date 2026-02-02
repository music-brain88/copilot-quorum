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

    #[error("Operation cancelled")]
    Cancelled,
}

impl DomainError {
    /// Check if this error represents a cancellation
    pub fn is_cancelled(&self) -> bool {
        matches!(self, DomainError::Cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancelled_error_display() {
        let error = DomainError::Cancelled;
        assert_eq!(error.to_string(), "Operation cancelled");
    }

    #[test]
    fn test_is_cancelled_check() {
        assert!(DomainError::Cancelled.is_cancelled());
        assert!(!DomainError::NoModels.is_cancelled());
        assert!(!DomainError::AllModelsFailed.is_cancelled());
        assert!(!DomainError::InvalidQuestion("test".to_string()).is_cancelled());
    }
}
