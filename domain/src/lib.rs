//! Domain layer for copilot-quorum
//!
//! This crate contains the core business logic, entities, and value objects.
//! It has no dependencies on infrastructure or presentation concerns.

pub mod core;
pub mod orchestration;
pub mod prompt;
pub mod session;

// Re-export commonly used types
pub use core::{error::DomainError, model::Model, question::Question};
pub use orchestration::{
    entities::{Phase, QuorumRun},
    strategy::OrchestrationStrategy,
    value_objects::{ModelResponse, PeerReview, QuorumResult, SynthesisResult},
};
pub use prompt::PromptTemplate;
pub use session::{entities::Message, repository::LlmSessionRepository};
