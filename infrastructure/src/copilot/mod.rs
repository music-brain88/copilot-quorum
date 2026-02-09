//! Copilot CLI adapter - LlmGateway implementation using GitHub Copilot.
//!
//! This module provides the infrastructure to communicate with LLMs through
//! the GitHub Copilot CLI, implementing the `LlmGateway` port from the application layer.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌────────────────┐     ┌─────────────────┐
//! │ CopilotLlmGateway │ --> │ CopilotSession │ --> │ CopilotTransport │
//! │   (gateway.rs)   │     │  (session.rs)   │     │  (transport.rs)  │
//! └─────────────────┘     └────────────────┘     └─────────────────┘
//!                                                        │
//!                                                        ▼
//!                                                 ┌─────────────┐
//!                                                 │ Copilot CLI │
//!                                                 │  (gh copilot)│
//!                                                 └─────────────┘
//! ```
//!
//! # Modules
//!
//! - [`gateway`] - Main entry point implementing `LlmGateway`
//! - [`session`] - Active conversation session with an LLM
//! - [`transport`] - Low-level JSON-RPC communication with Copilot CLI
//! - [`protocol`] - JSON-RPC message types and structures
//! - [`error`] - Error types for Copilot operations

pub mod error;
pub mod gateway;
pub mod protocol;
pub mod router;
pub mod session;
pub mod transport;
