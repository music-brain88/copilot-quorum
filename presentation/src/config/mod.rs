//! Presentation-level configuration
//!
//! Configuration for output formatting and REPL behavior.
//! Constructed from primitives in the CLI layer.

use quorum_domain::OutputFormat;

/// Output configuration for the presentation layer
#[derive(Debug, Clone)]
pub struct OutputConfig {
    /// Output format
    pub format: OutputFormat,
    /// Enable colored terminal output
    pub color: bool,
}

impl OutputConfig {
    /// Create a new OutputConfig from primitives
    pub fn new(format: OutputFormat, color: bool) -> Self {
        Self { format, color }
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: OutputFormat::Synthesis,
            color: true,
        }
    }
}

/// REPL configuration for the presentation layer
#[derive(Debug, Clone)]
pub struct ReplConfig {
    /// Show progress indicators
    pub show_progress: bool,
    /// Path to history file
    pub history_file: Option<String>,
}

impl ReplConfig {
    /// Create a new ReplConfig from primitives
    pub fn new(show_progress: bool, history_file: Option<String>) -> Self {
        Self {
            show_progress,
            history_file,
        }
    }
}

impl Default for ReplConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            history_file: None,
        }
    }
}
