//! Application layer for copilot-quorum
//!
//! This crate contains use cases, port definitions, and application configuration.
//! It depends only on the domain layer.

pub mod config;
pub mod ports;
pub mod use_cases;

// Re-export commonly used types
pub use config::BehaviorConfig;
pub use ports::{llm_gateway::LlmGateway, progress::ProgressNotifier};
pub use use_cases::run_quorum::{RunQuorumInput, RunQuorumUseCase};
