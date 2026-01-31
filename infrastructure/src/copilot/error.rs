//! Error types for the Copilot adapter

use thiserror::Error;

/// Result type alias for Copilot operations
pub type Result<T> = std::result::Result<T, CopilotError>;

/// Errors that can occur when communicating with Copilot CLI
#[derive(Error, Debug)]
pub enum CopilotError {
    #[error("Failed to spawn Copilot process: {0}")]
    SpawnError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Failed to parse response: {error}\nRaw response: {raw}")]
    ParseError { error: String, raw: String },

    #[error("JSON-RPC error (code {code}): {message}")]
    RpcError { code: i64, message: String },

    #[error("Unexpected response: {0}")]
    UnexpectedResponse(String),

    #[error("Session not initialized")]
    SessionNotInitialized,

    #[error("Transport closed")]
    TransportClosed,

    #[error("Request timeout")]
    Timeout,

    #[error("Invalid model: {0}")]
    InvalidModel(String),
}
