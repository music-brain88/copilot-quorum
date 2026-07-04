//! Prompt domain
//!
//! Templates and utilities for generating prompts at each stage of the Quorum flow.

pub mod agent;
mod review;
mod template;

pub use agent::AgentPromptTemplate;
pub use review::ReviewPromptTemplate;
pub use template::PromptTemplate;
