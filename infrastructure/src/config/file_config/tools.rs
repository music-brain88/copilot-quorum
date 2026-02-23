//! Tools configuration from TOML (`[tools]` section)
//!
//! The tools configuration allows customizing how Quorum executes tools.
//! Tools come from multiple providers (builtin, CLI, MCP, scripts) and
//! can be configured to use enhanced CLI tools when available.
//!
//! Example configuration:
//!
//! ```toml
//! [tools]
//! providers = ["cli", "builtin"]
//! suggest_enhanced_tools = true
//!
//! [tools.cli.aliases]
//! grep_search = "rg"    # Use ripgrep
//! glob_search = "fd"    # Use fd-find
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    pub aliases: HashMap<String, String>,
    /// Enhanced tool definitions (for suggesting upgrades)
    #[serde(default)]
    pub enhanced: HashMap<String, EnhancedToolConfig>,
}

impl Default for FileCliToolsConfig {
    fn default() -> Self {
        let mut aliases = HashMap::new();
        // Default to standard tools (available everywhere)
        aliases.insert("grep_search".to_string(), "grep".to_string());
        aliases.insert("glob_search".to_string(), "find".to_string());
        aliases.insert("read_file".to_string(), "cat".to_string());

        let mut enhanced = HashMap::new();
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
    pub env: HashMap<String, String>,
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
    pub parameters: HashMap<String, FileCustomToolParameter>,
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
    pub custom: HashMap<String, FileCustomToolConfig>,
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
            custom: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.tools.custom.len(), 2);

        let gh = config.tools.custom.get("gh_create_issue").unwrap();
        assert_eq!(gh.description, "Create a GitHub issue");
        assert_eq!(gh.command, "gh issue create --title {title} --body {body}");
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
        let config: super::super::FileConfig = toml::from_str(toml_str).unwrap();
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
}
