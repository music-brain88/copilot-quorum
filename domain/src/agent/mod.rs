//! Agent domain module
//!
//! Contains agent state, plans, tasks, and related entities
//! for the autonomous agent system.

pub mod entities;
pub mod value_objects;

pub use entities::{AgentConfig, AgentState, Plan, Task, TaskStatus};
pub use value_objects::{AgentContext, AgentId, TaskId, TaskResult, Thought};
