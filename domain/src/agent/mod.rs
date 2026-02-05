//! Agent domain module
//!
//! Contains agent state, plans, tasks, and related entities
//! for the autonomous agent system.

pub mod entities;
pub mod value_objects;

pub use entities::{
    AgentConfig, AgentPhase, AgentState, EnsemblePlanResult, HilMode, HumanDecision, ModelVote,
    Plan, PlanCandidate, PlanningMode, ReviewRound, Task, TaskStatus,
};
pub use value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought};
