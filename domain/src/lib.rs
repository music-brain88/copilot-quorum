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
//! ## Solo / Ensemble (ConsensusLevel)
//!
//! - **Solo** (default): Single model driven, quick execution
//! - **Ensemble**: Multi-model driven, for complex decisions

pub mod agent;
pub mod config;
pub mod context;
pub mod core;
pub mod orchestration;
pub mod prompt;
pub mod quorum;
pub mod session;
pub mod tool;
pub mod util;

// Re-export commonly used types
pub use agent::{
    agent_policy::{AgentPolicy, HilAction},
    context_mode::ContextMode,
    entities::{
        AgentPhase, AgentState, EnsemblePlanResult, HilMode, HumanDecision, ModelVote, Plan,
        PlanCandidate, ReviewRound, Task, TaskStatus,
    },
    model_config::ModelConfig,
    tool_execution::{ToolExecution, ToolExecutionId, ToolExecutionState},
    validation::{ConfigIssue, ConfigIssueCode, Severity},
    value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought, ThoughtType},
};
pub use config::OutputFormat;
pub use context::{KnownContextFile, LoadedContextFile, ProjectContext, ResourceReference, extract_references};
pub use core::{error::DomainError, model::Model, question::Question};
pub use orchestration::{
    entities::{Phase, QuorumRun},
    mode::{ConsensusLevel, PlanningApproach},
    scope::PhaseScope,
    session_mode::SessionMode,
    strategy::{DebateConfig, DebateIntensity, OrchestrationStrategy, StrategyExecutor},
    value_objects::{ModelResponse, PeerReview, QuorumResult, SynthesisResult},
};
pub use prompt::{AgentPromptTemplate, PromptTemplate};
pub use session::{
    entities::Message,
    repository::LlmSessionRepository,
    response::{ContentBlock, LlmResponse, StopReason},
    stream::StreamEvent,
};
pub use tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter, ToolSpec},
    traits::{DefaultToolValidator, ToolValidator},
    value_objects::{ErrorCategory, ToolError, ToolResult, ToolResultMetadata},
};

// Re-export quorum types
pub use quorum::{
    ConsensusOutcome, ConsensusRound, QuorumRule, Vote, VoteResult, parse_final_review_response,
    parse_review_response, parse_vote_score,
};

// Re-export plan parser
pub use agent::plan_parser::{extract_plan_from_response, parse_plan, parse_plan_json};
