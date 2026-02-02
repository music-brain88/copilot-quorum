//! Domain layer for copilot-quorum
//!
//! This crate contains the core business logic, entities, and value objects.
//! It has no dependencies on infrastructure or presentation concerns.

pub mod agent;
pub mod config;
pub mod context;
pub mod core;
pub mod orchestration;
pub mod prompt;
pub mod session;
pub mod tool;

// Re-export commonly used types
pub use agent::{
    entities::{AgentConfig, AgentPhase, AgentState, Plan, Task, TaskStatus},
    value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought, ThoughtType},
};
pub use config::OutputFormat;
pub use context::{KnownContextFile, LoadedContextFile, ProjectContext};
pub use core::{error::DomainError, model::Model, question::Question};
pub use orchestration::{
    entities::{Phase, QuorumRun},
    strategy::OrchestrationStrategy,
    value_objects::{ModelResponse, PeerReview, QuorumResult, SynthesisResult},
};
pub use prompt::{AgentPromptTemplate, PromptTemplate};
pub use session::{entities::Message, repository::LlmSessionRepository};
pub use tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter, ToolSpec},
    traits::{DefaultToolValidator, ToolValidator},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
