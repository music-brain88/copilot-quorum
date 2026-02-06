//! Core domain concepts shared across all subdomains.
//!
//! - [`model::Model`] — available AI models (Claude, GPT, Gemini, etc.)
//! - [`question::Question`] — a validated question to pose to the Quorum
//! - [`error::DomainError`] — domain-level errors

pub mod error;
pub mod model;
pub mod question;
pub mod string;
