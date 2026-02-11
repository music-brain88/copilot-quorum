//! Presentation layer for copilot-quorum
//!
//! This crate contains CLI definitions, output formatters,
//! progress reporters, TUI interface, and agent presentation components.

pub mod agent;
pub mod cli;
pub mod config;
pub mod output;
pub mod progress;
pub mod tui;

// Re-export commonly used types
pub use cli::commands::{Cli, CliOutputFormat};
pub use config::{OutputConfig, ReplConfig};
pub use output::console::ConsoleFormatter;
pub use progress::reporter::{ProgressReporter, SimpleProgress};

// Agent-related exports (used by one-shot mode)
pub use agent::human_intervention::InteractiveHumanIntervention;
pub use agent::presenter::ReplPresenter;
pub use agent::progress::{AgentProgressReporter, SimpleAgentProgress};
pub use agent::thought::{ThoughtStream, format_thoughts, summarize_thoughts};

// TUI exports
pub use tui::TuiApp;
pub use tui::TuiHumanIntervention;
pub use tui::TuiInputConfig;
pub use tui::TuiPresenter;
pub use tui::TuiProgressBridge;

// Re-export OutputFormat from domain layer
pub use quorum_domain::OutputFormat;
