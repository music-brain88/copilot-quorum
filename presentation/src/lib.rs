//! Presentation layer for copilot-quorum
//!
//! This crate contains CLI definitions, output formatters,
//! and progress reporters.

pub mod cli;
pub mod output;
pub mod progress;

// Re-export commonly used types
pub use cli::commands::{Cli, OutputFormat};
pub use output::console::ConsoleFormatter;
pub use progress::reporter::{ProgressReporter, SimpleProgress};
