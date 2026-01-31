//! Infrastructure layer for copilot-quorum
//!
//! This crate contains adapters that implement the ports defined
//! in the application layer.

pub mod copilot;

// Re-export commonly used types
pub use copilot::{
    error::{CopilotError, Result},
    gateway::CopilotLlmGateway,
    session::CopilotSession,
    transport::StdioTransport,
};
