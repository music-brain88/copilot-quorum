//! Agent domain module
//!
//! This module contains the core entities for the autonomous agent system,
//! which executes tasks with multi-model review (Quorum) for safety.
//!
//! # Overview
//!
//! The agent system supports two planning modes:
//!
//! - **Solo Mode** ([`PlanningMode::Single`]): A single model creates the plan,
//!   which is then reviewed by multiple models (Quorum Consensus).
//!
//! - **Ensemble Mode** ([`PlanningMode::Ensemble`]): Multiple models independently
//!   create plans in parallel, then vote on each other's plans. The highest-scoring
//!   plan is selected (see [`EnsemblePlanResult`]).
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
//! - [`AgentConfig`]: Configuration including model selection and planning mode
//! - [`Plan`]: A plan consisting of [`Task`]s to execute
//! - [`PlanningMode`]: Single (Solo) or Ensemble planning
//! - [`EnsemblePlanResult`]: Result of ensemble planning with selected plan
//!
//! # Examples
//!
//! ## Solo Mode (Default)
//!
//! ```
//! use quorum_domain::agent::{AgentConfig, PlanningMode};
//!
//! let config = AgentConfig::default();
//! assert_eq!(config.planning_mode, PlanningMode::Single);
//! ```
//!
//! ## Ensemble Mode
//!
//! ```
//! use quorum_domain::agent::{AgentConfig, PlanningMode};
//!
//! let config = AgentConfig::default().with_ensemble_planning();
//! assert_eq!(config.planning_mode, PlanningMode::Ensemble);
//! ```

pub mod entities;
pub mod value_objects;

pub use entities::{
    AgentConfig, AgentPhase, AgentState, EnsemblePlanResult, HilMode, HumanDecision, ModelVote,
    Plan, PlanCandidate, PlanningMode, ReviewRound, Task, TaskStatus,
};
pub use value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought};
