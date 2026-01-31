//! Application layer for copilot-quorum
//!
//! This crate contains use cases and port definitions.
//! It depends only on the domain layer.

pub mod ports;
pub mod use_cases;

// Re-export commonly used types
pub use ports::{llm_gateway::LlmGateway, progress::ProgressNotifier};
pub use use_cases::run_quorum::{RunQuorumInput, RunQuorumUseCase};
