//! Presentation layer for copilot-quorum
//!
//! This crate contains CLI definitions, output formatters,
//! progress reporters, and interactive chat interface.

pub mod chat;
pub mod cli;
pub mod config;
pub mod output;
pub mod progress;

// Re-export commonly used types
pub use chat::ChatRepl;
pub use cli::commands::{Cli, OutputFormat};
pub use config::{OutputConfig, ReplConfig};
pub use output::console::ConsoleFormatter;
pub use progress::reporter::{ProgressReporter, SimpleProgress};
