//! Error types for the Copilot adapter.
//!
//! Provides structured error handling for all failure modes when
//! communicating with the GitHub Copilot CLI process.

use thiserror::Error;

/// Result type alias for Copilot operations.
pub type Result<T> = std::result::Result<T, CopilotError>;

/// Errors that can occur when communicating with Copilot CLI.
///
/// These cover process lifecycle, JSON-RPC protocol errors, and session management.
#[derive(Error, Debug)]
pub enum CopilotError {
    /// Failed to start the Copilot CLI process.
    #[error("Failed to spawn Copilot process: {0}")]
    SpawnError(#[from] std::io::Error),

    /// JSON serialization/deserialization failed.
    #[error("JSON serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    /// Could not parse a response from Copilot CLI.
    #[error("Failed to parse response: {error}\nRaw response: {raw}")]
    ParseError { error: String, raw: String },

    /// JSON-RPC error returned by Copilot CLI.
    #[error("JSON-RPC error (code {code}): {message}")]
    RpcError { code: i64, message: String },

    /// Received an unexpected response format.
    #[error("Unexpected response: {0}")]
    UnexpectedResponse(String),

    /// Attempted to use a session before initialization.
    #[error("Session not initialized")]
    SessionNotInitialized,

    /// The transport connection to Copilot CLI was closed.
    #[error("Transport closed")]
    TransportClosed,

    /// Request timed out waiting for response.
    #[error("Request timeout")]
    Timeout,

    /// Specified model is not supported.
    #[error("Invalid model: {0}")]
    InvalidModel(String),

    /// Operation was cancelled.
    #[error("Operation cancelled")]
    Cancelled,

    /// Error during tool call processing.
    #[error("Tool call error: {0}")]
    ToolCallError(String),
}

impl CopilotError {
    /// Check if this error represents a cancellation
    pub fn is_cancelled(&self) -> bool {
        matches!(self, CopilotError::Cancelled)
    }
}
