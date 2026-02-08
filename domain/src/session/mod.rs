//! LLM Session domain.
//!
//! - [`entities::Session`] — a conversation session with an LLM
//! - [`entities::Message`] — a single message within a session
//! - [`repository::LlmSessionRepository`] — trait for session persistence

pub mod entities;
pub mod repository;
pub mod stream;
