//! Raw TOML configuration data types
//!
//! These structs represent the exact structure of the TOML config file.
//! They are deserialized directly and then converted to domain/application types.

use serde::{Deserialize, Serialize};

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
    /// Output format: "full", "synthesis", or "json"
    pub format: Option<String>,
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
        assert_eq!(config.output.format, Some("full".to_string()));
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
}
