//! Domain layer for copilot-quorum
//!
//! This crate contains the core business logic, entities, and value objects.
//! It has no dependencies on infrastructure or presentation concerns.
//!
//! # Core Concepts
//!
//! ## Quorum
//!
//! Quorum is the central concept in copilot-quorum, inspired by distributed systems:
//!
//! - **Quorum Discussion**: Multiple models participate in equal discussion
//! - **Quorum Consensus**: Voting-based approval/rejection for plans and actions
//!
//! ## Solo / Ensemble Modes
//!
//! - **Solo Mode**: Single model (Agent) driven, quick execution (default)
//! - **Ensemble Mode**: Multi-model (Quorum) driven, for complex decisions

pub mod agent;
pub mod config;
pub mod context;
pub mod core;
pub mod orchestration;
pub mod prompt;
pub mod quorum;
pub mod session;
pub mod tool;

// Re-export commonly used types
pub use agent::{
    entities::{
        AgentConfig, AgentPhase, AgentState, HilMode, HumanDecision, ModelVote, Plan, ReviewRound,
        Task, TaskStatus,
    },
    value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought, ThoughtType},
};
pub use config::OutputFormat;
pub use context::{KnownContextFile, LoadedContextFile, ProjectContext};
pub use core::{error::DomainError, model::Model, question::Question};
pub use orchestration::{
    entities::{Phase, QuorumRun},
    mode::OrchestrationMode,
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

// Re-export quorum types
pub use quorum::{ConsensusOutcome, ConsensusRound, QuorumRule, Vote, VoteResult};
