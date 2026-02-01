//! Prompt domain
//!
//! Templates and utilities for generating prompts at each stage of the Quorum flow.

pub mod agent;
mod template;

pub use agent::AgentPromptTemplate;
pub use template::PromptTemplate;
