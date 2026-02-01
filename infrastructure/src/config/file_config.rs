//! Raw TOML configuration data types
//!
//! These structs represent the exact structure of the TOML config file.
//! They are deserialized directly and use domain types where appropriate.

use quorum_domain::OutputFormat;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// Re-export OutputFormat from domain for convenience
pub use quorum_domain::OutputFormat as FileOutputFormat;

/// Configuration validation errors
#[derive(Debug, Error)]
pub enum ConfigValidationError {
    #[error("timeout_seconds cannot be 0")]
    InvalidTimeout,

    #[error("model name cannot be empty")]
    EmptyModelName,

    #[error("moderator name cannot be empty")]
    EmptyModeratorName,
}

/// Raw council configuration from TOML
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileCouncilConfig {
    /// Model names as strings
    pub models: Vec<String>,
    /// Moderator model name
    pub moderator: Option<String>,
}

/// Raw behavior configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileBehaviorConfig {
    /// Enable peer review phase
    pub enable_review: bool,
    /// Timeout in seconds for API calls
    pub timeout_seconds: Option<u64>,
}

impl Default for FileBehaviorConfig {
    fn default() -> Self {
        Self {
            enable_review: true,
            timeout_seconds: None,
        }
    }
}

/// Raw output configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileOutputConfig {
    /// Output format (uses domain type)
    pub format: Option<OutputFormat>,
    /// Enable colored terminal output
    pub color: bool,
}

impl Default for FileOutputConfig {
    fn default() -> Self {
        Self {
            format: None,
            color: true,
        }
    }
}

/// Raw REPL configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileReplConfig {
    /// Show progress indicators
    pub show_progress: bool,
    /// Path to history file
    pub history_file: Option<String>,
}

impl Default for FileReplConfig {
    fn default() -> Self {
        Self {
            show_progress: true,
            history_file: None,
        }
    }
}

/// Complete file configuration (raw TOML structure)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    /// Council settings
    pub council: FileCouncilConfig,
    /// Behavior settings
    pub behavior: FileBehaviorConfig,
    /// Output settings
    pub output: FileOutputConfig,
    /// REPL settings
    pub repl: FileReplConfig,
}

impl FileConfig {
    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        // Timeout of 0 seconds doesn't make sense
        if let Some(0) = self.behavior.timeout_seconds {
            return Err(ConfigValidationError::InvalidTimeout);
        }

        // Check for empty model names
        for model in &self.council.models {
            if model.trim().is_empty() {
                return Err(ConfigValidationError::EmptyModelName);
            }
        }

        // Check for empty moderator name
        if let Some(ref moderator) = self.council.moderator {
            if moderator.trim().is_empty() {
                return Err(ConfigValidationError::EmptyModeratorName);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_full_config() {
        let toml_str = r#"
[council]
models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
moderator = "claude-sonnet-4.5"

[behavior]
enable_review = false
timeout_seconds = 120

[output]
format = "full"
color = false

[repl]
show_progress = false
history_file = "~/.local/share/quorum/history.txt"
"#;

        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.council.models.len(), 2);
        assert_eq!(
            config.council.moderator,
            Some("claude-sonnet-4.5".to_string())
        );
        assert!(!config.behavior.enable_review);
        assert_eq!(config.behavior.timeout_seconds, Some(120));
        assert_eq!(config.output.format, Some(OutputFormat::Full));
        assert!(!config.output.color);
        assert!(!config.repl.show_progress);
    }

    #[test]
    fn test_deserialize_partial_config() {
        let toml_str = r#"
[council]
models = ["gpt-5.2-codex"]
"#;

        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.council.models.len(), 1);
        assert!(config.council.moderator.is_none());
        // Defaults should apply
        assert!(config.behavior.enable_review);
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_default_config() {
        let config = FileConfig::default();
        assert!(config.council.models.is_empty());
        assert!(config.council.moderator.is_none());
        assert!(config.behavior.enable_review);
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_validate_valid_config() {
        let config = FileConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_zero_timeout() {
        let toml_str = r#"
[behavior]
timeout_seconds = 0
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            config.validate(),
            Err(ConfigValidationError::InvalidTimeout)
        ));
    }

    #[test]
    fn test_validate_empty_model_name() {
        let toml_str = r#"
[council]
models = ["gpt-5.2-codex", ""]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            config.validate(),
            Err(ConfigValidationError::EmptyModelName)
        ));
    }

    #[test]
    fn test_validate_empty_moderator() {
        let toml_str = r#"
[council]
moderator = "  "
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert!(matches!(
            config.validate(),
            Err(ConfigValidationError::EmptyModeratorName)
        ));
    }

    #[test]
    fn test_output_format_deserialize() {
        let toml_str = r#"
[output]
format = "json"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.output.format, Some(OutputFormat::Json));
    }
}
