//! Presentation-level configuration
//!
//! Configuration for output formatting and REPL behavior.
//! Provides conversions from infrastructure layer types.

use crate::cli::commands::OutputFormat;
use quorum_infrastructure::{FileOutputConfig, FileOutputFormat, FileReplConfig};

/// Output configuration for the presentation layer
#[derive(Debug, Clone)]
pub struct OutputConfig {
    /// Output format
    pub format: Option<OutputFormat>,
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

impl From<FileOutputConfig> for OutputConfig {
    fn from(file_config: FileOutputConfig) -> Self {
        Self {
            format: file_config.format.map(OutputFormat::from),
            color: file_config.color,
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

impl Default for ReplConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            history_file: None,
        }
    }
}

impl From<FileReplConfig> for ReplConfig {
    fn from(file_config: FileReplConfig) -> Self {
        Self {
            show_progress: file_config.show_progress,
            history_file: file_config.history_file,
        }
    }
}

// Conversion from infrastructure OutputFormat to presentation OutputFormat
impl From<FileOutputFormat> for OutputFormat {
    fn from(format: FileOutputFormat) -> Self {
        match format {
            FileOutputFormat::Full => OutputFormat::Full,
            FileOutputFormat::Synthesis => OutputFormat::Synthesis,
            FileOutputFormat::Json => OutputFormat::Json,
        }
    }
}
