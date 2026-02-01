//! Infrastructure layer for copilot-quorum
//!
//! This crate contains adapters that implement the ports defined
//! in the application layer, including configuration file loading.

pub mod config;
pub mod copilot;

// Re-export commonly used types
pub use config::{
    ConfigLoader, ConfigValidationError, FileConfig, FileCouncilConfig, FileOutputConfig,
    FileOutputFormat, FileReplConfig,
};
pub use copilot::{
    error::{CopilotError, Result},
    gateway::CopilotLlmGateway,
    session::CopilotSession,
    transport::StdioTransport,
};
