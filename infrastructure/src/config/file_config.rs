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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileCouncilConfig {
    /// Model names as strings
    pub models: Vec<String>,
    /// Moderator model for synthesis
    #[serde(default)]
    pub moderator: Model,
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

// ==================== Tools Configuration ====================
//
// The tools configuration allows customizing how Quorum executes tools.
// Tools come from multiple providers (builtin, CLI, MCP, scripts) and
// can be configured to use enhanced CLI tools when available.
//
// Example configuration:
//
// ```toml
// [tools]
// providers = ["cli", "builtin"]
// suggest_enhanced_tools = true
//
// [tools.cli.aliases]
// grep_search = "rg"    # Use ripgrep
// glob_search = "fd"    # Use fd-find
// ```

/// Tool provider types that can be enabled
///
/// Providers are tried in order of their priority (highest first).
/// See [`crate::tools::ToolRegistry`] for details on priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolProviderType {
    /// Built-in tools (read_file, write_file, etc.) - always available
    Builtin,
    /// CLI tools (grep/rg, find/fd) - wraps system commands
    Cli,
    /// MCP server tools - connects to external servers
    Mcp,
    /// User scripts as tools
    Script,
}

/// Enhanced tool definition (for suggesting upgrades)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EnhancedToolConfig {
    /// CLI command to use
    pub command: String,
    /// Additional arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Description for user prompt
    #[serde(default)]
    pub description: String,
}

/// CLI tool provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileCliToolsConfig {
    /// Whether CLI tools are enabled
    pub enabled: bool,
    /// Tool aliases (tool_name -> CLI command)
    /// e.g., grep_search = "grep" or grep_search = "rg"
    #[serde(default)]
    pub aliases: std::collections::HashMap<String, String>,
    /// Enhanced tool definitions (for suggesting upgrades)
    #[serde(default)]
    pub enhanced: std::collections::HashMap<String, EnhancedToolConfig>,
}

impl Default for FileCliToolsConfig {
    fn default() -> Self {
        let mut aliases = std::collections::HashMap::new();
        // Default to standard tools (available everywhere)
        aliases.insert("grep_search".to_string(), "grep".to_string());
        aliases.insert("glob_search".to_string(), "find".to_string());
        aliases.insert("read_file".to_string(), "cat".to_string());

        let mut enhanced = std::collections::HashMap::new();
        enhanced.insert(
            "grep_search".to_string(),
            EnhancedToolConfig {
                command: "rg".to_string(),
                args: vec![],
                description: "10x faster than grep".to_string(),
            },
        );
        enhanced.insert(
            "glob_search".to_string(),
            EnhancedToolConfig {
                command: "fd".to_string(),
                args: vec![],
                description: "5x faster than find".to_string(),
            },
        );
        enhanced.insert(
            "read_file".to_string(),
            EnhancedToolConfig {
                command: "bat".to_string(),
                args: vec!["--plain".to_string(), "--paging=never".to_string()],
                description: "syntax highlighting".to_string(),
            },
        );

        Self {
            enabled: true,
            aliases,
            enhanced,
        }
    }
}

/// MCP server configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileMcpServerConfig {
    /// Server name
    pub name: String,
    /// Command to start the server
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// MCP tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileMcpToolsConfig {
    /// Whether MCP tools are enabled
    pub enabled: bool,
    /// MCP servers to connect to
    #[serde(default)]
    pub servers: Vec<FileMcpServerConfig>,
}

impl Default for FileMcpToolsConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default (requires setup)
            servers: vec![],
        }
    }
}

/// Builtin tools configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileBuiltinToolsConfig {
    /// Whether builtin tools are enabled
    pub enabled: bool,
}

impl Default for FileBuiltinToolsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Complete tools configuration
///
/// Controls which tool providers are enabled and how they behave.
///
/// # Example
///
/// ```toml
/// [tools]
/// providers = ["mcp", "cli", "builtin"]
/// suggest_enhanced_tools = true
///
/// [tools.cli.aliases]
/// grep_search = "rg"
/// glob_search = "fd"
///
/// [tools.mcp]
/// enabled = true
///
/// [[tools.mcp.servers]]
/// name = "filesystem"
/// command = "npx"
/// args = ["-y", "@anthropic/mcp-server-filesystem"]
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileToolsConfig {
    /// Which providers to enable (order matters for priority)
    ///
    /// Providers are listed from highest to lowest priority.
    /// Default: `["cli", "builtin"]`
    #[serde(default)]
    pub providers: Vec<ToolProviderType>,
    /// Whether to suggest enhanced tools when detected
    ///
    /// When enabled, Quorum will check for tools like `rg`, `fd`, `bat`
    /// and offer to configure them if found.
    #[serde(default = "default_suggest_enhanced")]
    pub suggest_enhanced_tools: bool,
    /// Builtin tools settings
    #[serde(default)]
    pub builtin: FileBuiltinToolsConfig,
    /// CLI tools settings
    #[serde(default)]
    pub cli: FileCliToolsConfig,
    /// MCP tools settings
    #[serde(default)]
    pub mcp: FileMcpToolsConfig,
}

fn default_suggest_enhanced() -> bool {
    true
}

impl Default for FileToolsConfig {
    fn default() -> Self {
        Self {
            providers: vec![ToolProviderType::Cli, ToolProviderType::Builtin],
            suggest_enhanced_tools: true,
            builtin: FileBuiltinToolsConfig::default(),
            cli: FileCliToolsConfig::default(),
            mcp: FileMcpToolsConfig::default(),
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
    /// Agent settings
    pub agent: FileAgentConfig,
    /// Integration settings
    pub integrations: FileIntegrationsConfig,
    /// Tools settings
    pub tools: FileToolsConfig,
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

    #[test]
    fn test_tools_config_default() {
        let config = FileToolsConfig::default();
        assert!(config.suggest_enhanced_tools);
        assert!(config.builtin.enabled);
        assert!(config.cli.enabled);
        assert!(!config.mcp.enabled);
        assert_eq!(config.cli.aliases.get("grep_search"), Some(&"grep".to_string()));
        assert_eq!(config.cli.aliases.get("glob_search"), Some(&"find".to_string()));
    }

    #[test]
    fn test_tools_config_deserialize() {
        let toml_str = r#"
[tools]
providers = ["cli", "builtin"]
suggest_enhanced_tools = false

[tools.cli]
enabled = true

[tools.cli.aliases]
grep_search = "rg"
glob_search = "fd"

[tools.mcp]
enabled = true

[[tools.mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@anthropic/mcp-server-filesystem"]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert!(!config.tools.suggest_enhanced_tools);
        assert!(config.tools.cli.enabled);
        assert_eq!(config.tools.cli.aliases.get("grep_search"), Some(&"rg".to_string()));
        assert!(config.tools.mcp.enabled);
        assert_eq!(config.tools.mcp.servers.len(), 1);
        assert_eq!(config.tools.mcp.servers[0].name, "filesystem");
    }

    #[test]
    fn test_tools_enhanced_config() {
        let config = FileCliToolsConfig::default();
        let rg = config.enhanced.get("grep_search").unwrap();
        assert_eq!(rg.command, "rg");
        assert!(rg.description.contains("faster"));
    }
}
