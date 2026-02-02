//! Infrastructure layer for copilot-quorum
//!
//! This crate contains adapters that implement the ports defined
//! in the application layer, including configuration file loading.

pub mod config;
pub mod context;
pub mod copilot;
pub mod tools;

// Re-export commonly used types
pub use config::{
    ConfigLoader, ConfigValidationError, FileConfig, FileCouncilConfig, FileOutputConfig,
    FileOutputFormat, FileReplConfig,
};
pub use context::LocalContextLoader;
pub use copilot::{
    error::{CopilotError, Result},
    gateway::CopilotLlmGateway,
    session::CopilotSession,
    transport::StdioTransport,
};
pub use tools::{default_tool_spec, read_only_tool_spec, LocalToolExecutor};
