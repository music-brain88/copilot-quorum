//! Agent domain module
//!
//! This module contains the core entities for the autonomous agent system,
//! which executes tasks with multi-model review (Quorum) for safety.
//!
//! # Overview
//!
//! The agent system supports two consensus levels:
//!
//! - **Solo** ([`ConsensusLevel::Solo`](crate::orchestration::mode::ConsensusLevel::Solo)):
//!   A single model creates the plan, which is then reviewed by multiple models
//!   (Quorum Consensus).
//!
//! - **Ensemble** ([`ConsensusLevel::Ensemble`](crate::orchestration::mode::ConsensusLevel::Ensemble)):
//!   Multiple models independently create plans in parallel, then vote on each
//!   other's plans. The highest-scoring plan is selected (see [`EnsemblePlanResult`]).
//!
//! # Agent Lifecycle
//!
//! ```text
//! ┌─────────────────┐
//! │ Context         │  exploration_model gathers project info
//! │ Gathering       │
//! └────────┬────────┘
//!          ↓
//! ┌─────────────────┐
//! │ Planning        │  Solo: decision_model creates plan
//! │                 │  Ensemble: review_models create plans + vote
//! └────────┬────────┘
//!          ↓
//! ┌─────────────────┐
//! │ Plan Review     │  review_models approve/reject (Solo only)
//! │ (Quorum)        │
//! └────────┬────────┘
//!          ↓
//! ┌─────────────────┐
//! │ Task Execution  │  decision_model executes, Quorum reviews risky ops
//! └────────┬────────┘
//!          ↓
//! ┌─────────────────┐
//! │ Completed       │
//! └─────────────────┘
//! ```
//!
//! # Key Types
//!
//! - [`AgentState`]: Tracks the complete state of an agent execution
//! - [`SessionMode`](crate::orchestration::session_mode::SessionMode): Runtime-mutable orchestration settings
//! - [`ModelConfig`]: Role-based model selection
//! - [`AgentPolicy`]: Domain behavioral constraints
//! - [`Plan`]: A plan consisting of [`Task`]s to execute
//! - [`EnsemblePlanResult`]: Result of ensemble planning with selected plan
//!
//! # Examples
//!
//! ## Solo Mode (Default)
//!
//! ```
//! use quorum_domain::SessionMode;
//! use quorum_domain::ConsensusLevel;
//!
//! let mode = SessionMode::default();
//! assert_eq!(mode.consensus_level, ConsensusLevel::Solo);
//! ```
//!
//! ## Ensemble Mode
//!
//! ```
//! use quorum_domain::{SessionMode, ConsensusLevel};
//!
//! let mode = SessionMode {
//!     consensus_level: ConsensusLevel::Ensemble,
//!     ..Default::default()
//! };
//! assert_eq!(mode.consensus_level, ConsensusLevel::Ensemble);
//! ```

pub mod agent_policy;
pub mod entities;
pub mod model_config;
pub mod plan_parser;
pub mod tool_execution;
pub mod validation;
pub mod value_objects;

pub use agent_policy::{AgentPolicy, HilAction};
pub use entities::{
    AgentPhase, AgentState, EnsemblePlanResult, HilMode, HumanDecision, Plan, PlanCandidate,
    ReviewRound, Task, TaskStatus,
};
pub use model_config::ModelConfig;
pub use plan_parser::{extract_plan_from_response, parse_plan, parse_plan_json};
pub use tool_execution::{ToolExecution, ToolExecutionId, ToolExecutionState};
pub use validation::{ConfigIssue, ConfigIssueCode, Severity};
pub use value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought};
