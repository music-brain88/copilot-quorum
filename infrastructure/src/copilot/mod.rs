//! Copilot CLI adapter — [`LlmGateway`](quorum_application::ports::llm_gateway::LlmGateway)
//! implementation using GitHub Copilot.
//!
//! This module provides the infrastructure to communicate with LLMs through
//! the GitHub Copilot CLI, implementing the `LlmGateway` port from the
//! application layer. It is the **sole LLM backend** for copilot-quorum and
//! underpins every user-facing feature:
//!
//! | Feature | How this module supports it |
//! |---------|---------------------------|
//! | **Solo mode** | One [`CopilotSession`](session::CopilotSession) for ask/answer |
//! | **Quorum Discussion** | Multiple sessions created in parallel via [`MessageRouter`](router::MessageRouter) |
//! | **Ensemble Planning** | N² concurrent sessions — plans + cross-model voting |
//! | **Native Tool Use** | [`send_with_tools`](session::CopilotSession) / [`send_tool_results`](session::CopilotSession) multi-turn loop |
//! | **Agent System** | Builds on Native Tool Use for autonomous task execution |
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────┐    ┌────────────────┐    ┌──────────────────┐
//! │ CopilotLlmGateway │───>│ CopilotSession │───>│  MessageRouter   │
//! │   (gateway.rs)    │    │  (session.rs)  │    │   (router.rs)    │
//! └───────────────────┘    └────────────────┘    └──────────────────┘
//!                                                   │ background task
//!                                                   ▼
//!                                            ┌─────────────┐
//!                                            │ Copilot CLI  │
//!                                            │ (TCP / JSON- │
//!                                            │  RPC 2.0)    │
//!                                            └─────────────┘
//! ```
//!
//! The [`MessageRouter`](router::MessageRouter) owns the single TCP connection
//! and demultiplexes incoming messages by `session_id` to per-session
//! [`SessionChannel`](router::SessionChannel)s, enabling safe concurrent access
//! without `Mutex` contention on the reader.
//!
//! # Modules
//!
//! - [`gateway`] — Entry point implementing `LlmGateway`; shared by Solo / Ensemble / Quorum
//! - [`session`] — Active conversation with an LLM implementing [`LlmSession`](quorum_application::ports::llm_gateway::LlmSession)
//! - [`router`] — Transport demultiplexer for concurrent sessions (Quorum Discussion, Ensemble Planning)
//! - [`transport`] — Message classification types used by the router's background reader task
//! - [`protocol`] — JSON-RPC message types and tool-call structures (Native Tool Use)
//! - [`error`] — Error types for Copilot operations

pub mod error;
pub mod gateway;
pub mod protocol;
pub mod router;
pub mod session;
pub mod transport;
