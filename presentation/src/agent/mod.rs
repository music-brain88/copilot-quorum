//! Agent presentation components
//!
//! This module provides UI components for agent execution:
//! - Progress reporting for agent phases and tasks
//! - Thought streaming for visualizing agent thinking
//! - Interactive REPL for agent mode
//! - Human intervention UI for plan revision limits

pub mod human_intervention;
pub mod progress;
pub mod repl;
pub mod thought;

pub use human_intervention::InteractiveHumanIntervention;
pub use progress::AgentProgressReporter;
pub use repl::AgentRepl;
pub use thought::ThoughtStream;
