//! Presentation-level configuration
//!
//! Configuration for output formatting and REPL behavior.

use serde::{Deserialize, Serialize};

/// Output configuration for the presentation layer
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Output format: "full", "synthesis", or "json"
    pub format: Option<String>,
    /// Enable colored terminal output
    pub color: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: None,
            color: true,
        }
    }
}

/// REPL configuration for the presentation layer
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReplConfig {
    /// Show progress indicators
    pub show_progress: bool,
    /// Path to history file
    pub history_file: Option<String>,
}

impl Default for ReplConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            history_file: None,
        }
    }
}
