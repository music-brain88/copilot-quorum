//! Raw TOML configuration data types
//!
//! These structs represent the exact structure of the TOML config file.
//! They are deserialized directly and use domain types where appropriate.

use quorum_domain::{ConsensusLevel, HilMode, Model, OutputFormat, PhaseScope, QuorumRule};
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
///
/// # Role-based Model Configuration
///
/// ```toml
/// [agent]
/// exploration_model = "claude-haiku-4.5"   # Context gathering + low-risk tools
/// decision_model = "claude-sonnet-4.5"     # Planning + high-risk tool decisions
/// review_models = ["claude-sonnet-4.5", "gpt-5.2-codex"]  # Reviews (quality)
/// consensus_level = "solo"                 # "solo" or "ensemble"
/// phase_scope = "full"                     # "full", "fast", "plan-only"
/// strategy = "quorum"                      # "quorum" or "debate"
/// ```
///
/// All model fields are optional; defaults are defined in `AgentConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileAgentConfig {
    /// Maximum plan revisions before human intervention
    pub max_plan_revisions: usize,
    /// Human-in-the-loop mode (interactive, auto_reject, auto_approve)
    pub hil_mode: String,
    /// Consensus level: "solo" or "ensemble"
    pub consensus_level: String,
    /// Phase scope: "full", "fast", "plan-only"
    pub phase_scope: String,
    /// Orchestration strategy: "quorum" or "debate"
    pub strategy: String,

    // ==================== Role-based Model Configuration ====================
    /// Model for exploration: context gathering + low-risk tools (optional)
    pub exploration_model: Option<String>,
    /// Model for decisions: planning + high-risk tool execution (optional)
    pub decision_model: Option<String>,
    /// Models for review phases (optional, uses default if not set)
    pub review_models: Option<Vec<String>>,
}

impl Default for FileAgentConfig {
    fn default() -> Self {
        Self {
            max_plan_revisions: 3,
            hil_mode: "interactive".to_string(),
            consensus_level: "solo".to_string(),
            phase_scope: "full".to_string(),
            strategy: "quorum".to_string(),
            // Role-based defaults are None - will use AgentConfig defaults
            exploration_model: None,
            decision_model: None,
            review_models: None,
        }
    }
}

impl FileAgentConfig {
    /// Parse hil_mode string into HilMode enum
    pub fn parse_hil_mode(&self) -> HilMode {
        self.hil_mode.parse().unwrap_or_default()
    }

    /// Parse consensus_level string into ConsensusLevel enum
    ///
    /// Accepts: "solo", "s", "ensemble", "ens", "e"
    pub fn parse_consensus_level(&self) -> ConsensusLevel {
        self.consensus_level.parse().unwrap_or_default()
    }

    /// Parse phase_scope string into PhaseScope enum
    ///
    /// Accepts: "full", "fast", "plan-only", "plan"
    pub fn parse_phase_scope(&self) -> PhaseScope {
        self.phase_scope.parse().unwrap_or_default()
    }

    /// Parse strategy string into strategy name
    ///
    /// Returns "quorum" or "debate". Used by CLI to configure OrchestrationStrategy.
    pub fn parse_strategy(&self) -> &str {
        match self.strategy.to_lowercase().as_str() {
            "debate" => "debate",
            _ => "quorum",
        }
    }

    /// Parse exploration_model string into Model enum
    pub fn parse_exploration_model(&self) -> Option<Model> {
        self.exploration_model.as_ref().and_then(|s| s.parse().ok())
    }

    /// Parse decision_model string into Model enum
    pub fn parse_decision_model(&self) -> Option<Model> {
        self.decision_model.as_ref().and_then(|s| s.parse().ok())
    }

    /// Parse review_models strings into `Vec<Model>`
    pub fn parse_review_models(&self) -> Option<Vec<Model>> {
        self.review_models
            .as_ref()
            .map(|models| models.iter().filter_map(|s| s.parse().ok()).collect())
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

// ==================== Quorum Configuration ====================
//
// The quorum configuration controls how multi-model consensus works.
// This is the core of the Quorum system - inspired by distributed systems
// where consensus ensures reliability.
//
// Example configuration:
//
// ```toml
// [quorum]
// rule = "majority"
// min_models = 2
//
// [quorum.discussion]
// models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro"]
// moderator = "claude-opus-4.5"
// enable_peer_review = true
// ```

/// Quorum discussion settings
///
/// Controls how Quorum Discussion (multi-model dialogue) operates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileQuorumDiscussionConfig {
    /// Models to use for discussion (optional, uses council.models if not set)
    pub models: Option<Vec<String>>,
    /// Moderator model for synthesis (optional, uses council.moderator if not set)
    pub moderator: Option<String>,
    /// Enable peer review phase in discussion
    pub enable_peer_review: bool,
}

impl Default for FileQuorumDiscussionConfig {
    fn default() -> Self {
        Self {
            models: None,
            moderator: None,
            enable_peer_review: true,
        }
    }
}

impl FileQuorumDiscussionConfig {
    /// Parse models into Model enums
    pub fn parse_models(&self) -> Option<Vec<Model>> {
        self.models
            .as_ref()
            .map(|models| models.iter().filter_map(|s| s.parse().ok()).collect())
    }

    /// Parse moderator into Model enum
    pub fn parse_moderator(&self) -> Option<Model> {
        self.moderator.as_ref().and_then(|s| s.parse().ok())
    }
}

/// Quorum consensus configuration
///
/// Controls how Quorum Consensus (voting for approval) works.
///
/// # Example
///
/// ```toml
/// [quorum]
/// rule = "majority"           # or "unanimous", "atleast:2", "75%"
/// min_models = 2              # minimum models required for valid consensus
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileQuorumConfig {
    /// Consensus rule: "majority", "unanimous", "atleast:N", "N%"
    pub rule: String,
    /// Minimum number of models required for valid consensus
    pub min_models: usize,
    /// Discussion settings
    pub discussion: FileQuorumDiscussionConfig,
}

impl Default for FileQuorumConfig {
    fn default() -> Self {
        Self {
            rule: "majority".to_string(),
            min_models: 2,
            discussion: FileQuorumDiscussionConfig::default(),
        }
    }
}

impl FileQuorumConfig {
    /// Parse the rule string into QuorumRule enum
    pub fn parse_rule(&self) -> QuorumRule {
        self.rule.parse().unwrap_or_default()
    }
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
///
/// MCP is disabled by default as it requires explicit server setup.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileMcpToolsConfig {
    /// Whether MCP tools are enabled (default: false, requires setup)
    pub enabled: bool,
    /// MCP servers to connect to
    #[serde(default)]
    pub servers: Vec<FileMcpServerConfig>,
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

/// Custom tool parameter definition
///
/// Defines a single parameter for a custom tool registered in `quorum.toml`.
///
/// # Example
///
/// ```toml
/// [tools.custom.gh_create_issue.parameters.title]
/// type = "string"
/// description = "Issue title"
/// required = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCustomToolParameter {
    /// Parameter type: "string", "number", "integer", "boolean"
    #[serde(rename = "type", default = "default_string_type")]
    pub param_type: String,
    /// Human-readable description of the parameter
    pub description: String,
    /// Whether this parameter is required (default: true)
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_string_type() -> String {
    "string".to_string()
}

fn default_true() -> bool {
    true
}

/// Custom tool definition from `quorum.toml`
///
/// Allows users to register external CLI commands as first-class tools.
/// The command template uses `{param_name}` placeholders that are replaced
/// with argument values at execution time.
///
/// # Security
///
/// All parameter values are shell-escaped before substitution to prevent
/// command injection.
///
/// # Example
///
/// ```toml
/// [tools.custom.gh_create_issue]
/// description = "Create a GitHub issue in the current repository"
/// command = "gh issue create --title {title} --body {body}"
/// risk_level = "high"
///
/// [tools.custom.gh_create_issue.parameters.title]
/// type = "string"
/// description = "Issue title"
/// required = true
///
/// [tools.custom.gh_create_issue.parameters.body]
/// type = "string"
/// description = "Issue body in markdown"
/// required = true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCustomToolConfig {
    /// Human-readable description of what this tool does
    pub description: String,
    /// Command template with `{param_name}` placeholders
    pub command: String,
    /// Risk level: "low" or "high" (default: "high" — safe by default)
    #[serde(default = "default_high_risk")]
    pub risk_level: String,
    /// Parameter definitions
    #[serde(default)]
    pub parameters: std::collections::HashMap<String, FileCustomToolParameter>,
}

fn default_high_risk() -> String {
    "high".to_string()
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
///
/// [tools.custom.gh_create_issue]
/// description = "Create a GitHub issue"
/// command = "gh issue create --title {title} --body {body}"
/// risk_level = "high"
///
/// [tools.custom.gh_create_issue.parameters.title]
/// type = "string"
/// description = "Issue title"
/// required = true
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
    /// Custom tools — user-defined CLI commands exposed as tools
    #[serde(default)]
    pub custom: std::collections::HashMap<String, FileCustomToolConfig>,
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
            custom: std::collections::HashMap::new(),
        }
    }
}

/// Complete file configuration (raw TOML structure)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    /// Council settings (legacy, prefer [quorum.discussion])
    pub council: FileCouncilConfig,
    /// Quorum settings (new unified configuration)
    pub quorum: FileQuorumConfig,
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
        assert_eq!(
            config.cli.aliases.get("grep_search"),
            Some(&"grep".to_string())
        );
        assert_eq!(
            config.cli.aliases.get("glob_search"),
            Some(&"find".to_string())
        );
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
        assert_eq!(
            config.tools.cli.aliases.get("grep_search"),
            Some(&"rg".to_string())
        );
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

    #[test]
    fn test_agent_config_role_based_defaults() {
        let config = FileAgentConfig::default();
        assert!(config.exploration_model.is_none());
        assert!(config.decision_model.is_none());
        assert!(config.review_models.is_none());
    }

    #[test]
    fn test_agent_config_role_based_deserialize() {
        let toml_str = r#"
[agent]
max_plan_revisions = 5
hil_mode = "auto_reject"
consensus_level = "ensemble"
phase_scope = "fast"
strategy = "debate"
exploration_model = "claude-haiku-4.5"
decision_model = "claude-sonnet-4.5"
review_models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.max_plan_revisions, 5);
        assert_eq!(config.agent.hil_mode, "auto_reject");
        assert_eq!(config.agent.consensus_level, "ensemble");
        assert_eq!(
            config.agent.parse_consensus_level(),
            ConsensusLevel::Ensemble
        );
        assert_eq!(config.agent.phase_scope, "fast");
        assert_eq!(config.agent.parse_phase_scope(), PhaseScope::Fast);
        assert_eq!(config.agent.parse_strategy(), "debate");
        assert_eq!(
            config.agent.exploration_model,
            Some("claude-haiku-4.5".to_string())
        );
        assert_eq!(
            config.agent.decision_model,
            Some("claude-sonnet-4.5".to_string())
        );
        assert_eq!(
            config.agent.review_models,
            Some(vec![
                "claude-sonnet-4.5".to_string(),
                "gpt-5.2-codex".to_string()
            ])
        );
    }

    #[test]
    fn test_agent_config_consensus_level_deserialize() {
        // Test "solo" (default)
        let toml_str = r#"
[agent]
consensus_level = "solo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.consensus_level, "solo");
        assert_eq!(config.agent.parse_consensus_level(), ConsensusLevel::Solo);

        // Test "ensemble"
        let toml_str = r#"
[agent]
consensus_level = "ensemble"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.consensus_level, "ensemble");
        assert_eq!(
            config.agent.parse_consensus_level(),
            ConsensusLevel::Ensemble
        );

        // Test alias "ens" -> Ensemble
        let toml_str = r#"
[agent]
consensus_level = "ens"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(
            config.agent.parse_consensus_level(),
            ConsensusLevel::Ensemble
        );
    }

    #[test]
    fn test_agent_config_phase_scope_deserialize() {
        let toml_str = r#"
[agent]
phase_scope = "plan-only"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.agent.parse_phase_scope(), PhaseScope::PlanOnly);
    }

    #[test]
    fn test_agent_config_parse_role_models() {
        let toml_str = r#"
[agent]
exploration_model = "claude-haiku-4.5"
decision_model = "claude-sonnet-4.5"
review_models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();

        assert_eq!(
            config.agent.parse_exploration_model(),
            Some(Model::ClaudeHaiku45)
        );
        assert_eq!(
            config.agent.parse_decision_model(),
            Some(Model::ClaudeSonnet45)
        );

        let review_models = config.agent.parse_review_models().unwrap();
        assert_eq!(review_models.len(), 2);
        assert!(review_models.contains(&Model::ClaudeSonnet45));
        assert!(review_models.contains(&Model::Gpt52Codex));
    }

    #[test]
    fn test_agent_config_partial_role_models() {
        // Only some models specified
        let toml_str = r#"
[agent]
decision_model = "claude-opus-4.5"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();

        // Only decision model is set
        assert!(config.agent.exploration_model.is_none());
        assert_eq!(
            config.agent.parse_decision_model(),
            Some(Model::ClaudeOpus45)
        );
        assert!(config.agent.review_models.is_none());
    }

    #[test]
    fn test_quorum_config_default() {
        let config = FileQuorumConfig::default();
        assert_eq!(config.rule, "majority");
        assert_eq!(config.min_models, 2);
        assert!(config.discussion.enable_peer_review);
        assert!(config.discussion.models.is_none());
    }

    #[test]
    fn test_quorum_config_deserialize() {
        let toml_str = r#"
[quorum]
rule = "unanimous"
min_models = 3

[quorum.discussion]
models = ["claude-sonnet-4.5", "gpt-5.2-codex", "gemini-3-pro-preview"]
moderator = "claude-opus-4.5"
enable_peer_review = false
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.quorum.rule, "unanimous");
        assert_eq!(config.quorum.min_models, 3);
        assert!(!config.quorum.discussion.enable_peer_review);

        let models = config.quorum.discussion.parse_models().unwrap();
        assert_eq!(models.len(), 3);
        assert!(models.contains(&Model::ClaudeSonnet45));
        assert!(models.contains(&Model::Gpt52Codex));
        assert!(models.contains(&Model::Gemini3Pro));

        let moderator = config.quorum.discussion.parse_moderator().unwrap();
        assert_eq!(moderator, Model::ClaudeOpus45);
    }

    #[test]
    fn test_quorum_config_parse_rule() {
        let mut config = FileQuorumConfig::default();

        config.rule = "majority".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Majority);

        config.rule = "unanimous".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Unanimous);

        config.rule = "atleast:2".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::AtLeast(2));

        config.rule = "75%".to_string();
        assert_eq!(config.parse_rule(), QuorumRule::Percentage(75));
    }

    #[test]
    fn test_custom_tools_config_deserialize() {
        let toml_str = r#"
[tools.custom.gh_create_issue]
description = "Create a GitHub issue"
command = "gh issue create --title {title} --body {body}"
risk_level = "high"

[tools.custom.gh_create_issue.parameters.title]
type = "string"
description = "Issue title"
required = true

[tools.custom.gh_create_issue.parameters.body]
type = "string"
description = "Issue body in markdown"
required = true

[tools.custom.aws_s3_ls]
description = "List S3 objects"
command = "aws s3 ls {path}"
risk_level = "low"

[tools.custom.aws_s3_ls.parameters.path]
type = "string"
description = "S3 path (e.g. s3://my-bucket/)"
required = true
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tools.custom.len(), 2);

        let gh = config.tools.custom.get("gh_create_issue").unwrap();
        assert_eq!(gh.description, "Create a GitHub issue");
        assert_eq!(
            gh.command,
            "gh issue create --title {title} --body {body}"
        );
        assert_eq!(gh.risk_level, "high");
        assert_eq!(gh.parameters.len(), 2);

        let title_param = gh.parameters.get("title").unwrap();
        assert_eq!(title_param.param_type, "string");
        assert!(title_param.required);

        let s3 = config.tools.custom.get("aws_s3_ls").unwrap();
        assert_eq!(s3.risk_level, "low");
    }

    #[test]
    fn test_custom_tools_config_defaults() {
        let toml_str = r#"
[tools.custom.simple_tool]
description = "A simple tool"
command = "echo {message}"

[tools.custom.simple_tool.parameters.message]
description = "Message to echo"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();
        let tool = config.tools.custom.get("simple_tool").unwrap();

        // risk_level defaults to "high"
        assert_eq!(tool.risk_level, "high");

        let param = tool.parameters.get("message").unwrap();
        // param_type defaults to "string"
        assert_eq!(param.param_type, "string");
        // required defaults to true
        assert!(param.required);
    }

    #[test]
    fn test_custom_tools_empty_by_default() {
        let config = FileToolsConfig::default();
        assert!(config.custom.is_empty());
    }

    #[test]
    fn test_quorum_config_with_council_fallback() {
        // When quorum.discussion is not set, should use council settings
        let toml_str = r#"
[council]
models = ["claude-sonnet-4.5", "gpt-5.2-codex"]
moderator = "claude-opus-4.5"

[quorum]
rule = "majority"
"#;
        let config: FileConfig = toml::from_str(toml_str).unwrap();

        // quorum.discussion should be None (using defaults)
        assert!(config.quorum.discussion.models.is_none());
        assert!(config.quorum.discussion.moderator.is_none());

        // But council is set
        assert_eq!(config.council.models.len(), 2);
        assert_eq!(config.council.moderator, Model::ClaudeOpus45);
    }
}
