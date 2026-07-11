//! Prompt domain
//!
//! Templates and utilities for generating prompts at each stage of the Quorum flow.

pub mod agent;
mod debate;
mod review;
mod template;

pub use agent::AgentPromptTemplate;
pub use debate::DebatePromptTemplate;
pub use review::ReviewPromptTemplate;
pub use template::PromptTemplate;
