//! Presentation layer for copilot-quorum
//!
//! This crate contains CLI definitions, output formatters,
//! progress reporters, and agent REPL interface.

pub mod agent;
pub mod cli;
pub mod config;
pub mod output;
pub mod progress;

// Re-export commonly used types
pub use cli::commands::{Cli, CliOutputFormat};
pub use config::{OutputConfig, ReplConfig};
pub use output::console::ConsoleFormatter;
pub use progress::reporter::{ProgressReporter, SimpleProgress};

// Agent-related exports
pub use agent::human_intervention::InteractiveHumanIntervention;
pub use agent::progress::{AgentProgressReporter, SimpleAgentProgress};
pub use agent::repl::AgentRepl;
pub use agent::thought::{format_thoughts, summarize_thoughts, ThoughtStream};

// Re-export OutputFormat from domain layer
pub use quorum_domain::OutputFormat;
