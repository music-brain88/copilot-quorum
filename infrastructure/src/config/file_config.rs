//! Raw TOML configuration data types
//!
//! These structs represent the exact structure of the TOML config file.
//! They are deserialized directly and use domain types where appropriate.

use quorum_domain::{HilMode, Model, OutputFormat};
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
}

/// Raw council configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileCouncilConfig {
    /// Model names as strings
    pub models: Vec<String>,
    /// Moderator model for synthesis
    #[serde(default)]
    pub moderator: Model,
}

impl Default for FileCouncilConfig {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            moderator: Model::default(),
        }
    }
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

/// Raw agent configuration from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileAgentConfig {
    /// Maximum plan revisions before human intervention
    pub max_plan_revisions: usize,
    /// Human-in-the-loop mode (interactive, auto_reject, auto_approve)
    pub hil_mode: String,
}

impl Default for FileAgentConfig {
    fn default() -> Self {
        Self {
            max_plan_revisions: 3,
            hil_mode: "interactive".to_string(),
        }
    }
}

impl FileAgentConfig {
    /// Parse hil_mode string into HilMode enum
    pub fn parse_hil_mode(&self) -> HilMode {
        self.hil_mode.parse().unwrap_or_default()
    }
}

/// Raw GitHub integration configuration from TOML
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileGitHubConfig {
    /// Enable GitHub Discussions integration
    pub enabled: bool,
    /// Repository (owner/name) - auto-detected if not set
    pub repo: Option<String>,
    /// Discussion category for escalations
    pub category: Option<String>,
}

/// Raw integrations configuration from TOML
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileIntegrationsConfig {
    /// GitHub integration settings
    pub github: FileGitHubConfig,
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
    /// Agent settings
    pub agent: FileAgentConfig,
    /// Integration settings
    pub integrations: FileIntegrationsConfig,
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
        assert_eq!(config.council.moderator, Model::ClaudeSonnet45);
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
        assert_eq!(config.council.moderator, Model::default());
        // Defaults should apply
        assert!(config.behavior.enable_review);
        assert!(config.output.color);
        assert!(config.repl.show_progress);
    }

    #[test]
    fn test_default_config() {
        let config = FileConfig::default();
        assert!(config.council.models.is_empty());
        assert_eq!(config.council.moderator, Model::default());
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
    fn test_output_format_deserialize() {
        let toml_str = r#"
[output]
format = "json"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.output.format, Some(OutputFormat::Json));
    }
}
