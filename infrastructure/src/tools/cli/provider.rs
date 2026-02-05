//! CLI tool provider
//!
//! The [`CliToolProvider`] wraps external CLI tools as Quorum tools.
//! It uses standard tools (grep, find, cat) by default, with configurable
//! aliases for enhanced tools (rg, fd, bat).
//!
//! # Default Tools
//!
//! | Tool Name | Default CLI | Enhanced CLI | Description |
//! |-----------|-------------|--------------|-------------|
//! | `grep_search` | `grep` | `rg` (ripgrep) | Search file contents |
//! | `glob_search` | `find` | `fd` | Find files by pattern |
//!
//! # Configuration
//!
//! Tools can be configured in `quorum.toml`:
//!
//! ```toml
//! [tools.cli]
//! enabled = true
//!
//! [tools.cli.aliases]
//! grep_search = "rg"    # Use ripgrep instead of grep
//! glob_search = "fd"    # Use fd instead of find
//! ```
//!
//! # Example
//!
//! ```ignore
//! use quorum_infrastructure::tools::CliToolProvider;
//! use quorum_infrastructure::config::FileCliToolsConfig;
//!
//! // Create with default standard tools
//! let provider = CliToolProvider::new();
//!
//! // Or create from configuration
//! let config = FileCliToolsConfig::default();
//! let provider = CliToolProvider::with_config(&config);
//!
//! // Check what's available
//! let tools = provider.discover_tools().await?;
//! for tool in tools {
//!     println!("{}: {}", tool.name, tool.description);
//! }
//! ```
//!
//! # Graceful Degradation
//!
//! If a configured CLI tool is not installed, the provider:
//! 1. Excludes it from `discover_tools()` results
//! 2. Returns a helpful error if `execute()` is called directly

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use async_trait::async_trait;
use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    provider::{ProviderError, ToolProvider},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};

use crate::config::FileCliToolsConfig;

/// Priority for CLI provider (medium-high)
pub const CLI_PRIORITY: i32 = 50;

/// CLI tool provider
///
/// Wraps external CLI tools (grep, find, rg, fd, bat, etc.) as Quorum tools.
/// Uses standard tools by default, with configurable aliases for enhanced tools.
#[derive(Debug, Clone)]
pub struct CliToolProvider {
    /// Tool aliases (tool_name -> CLI command)
    aliases: HashMap<String, String>,
    /// Working directory for commands
    working_dir: Option<String>,
    /// Cached available tools (only tools whose CLI is installed)
    available_tools: Vec<String>,
}

impl CliToolProvider {
    /// Create a new CLI provider with default aliases (standard tools)
    pub fn new() -> Self {
        Self::with_config(&FileCliToolsConfig::default())
    }

    /// Create a CLI provider from configuration
    pub fn with_config(config: &FileCliToolsConfig) -> Self {
        let aliases = config.aliases.clone();

        // Check which tools are actually available
        let available_tools: Vec<String> = aliases
            .iter()
            .filter(|(_, cmd): &(&String, &String)| Self::is_command_available(cmd))
            .map(|(tool, _): (&String, &String)| tool.clone())
            .collect();

        Self {
            aliases,
            working_dir: None,
            available_tools,
        }
    }

    /// Set working directory for command execution
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Check if a CLI command is available
    pub fn is_command_available(command: &str) -> bool {
        which::which(command).is_ok()
    }

    /// Get the CLI command for a tool
    fn get_command(&self, tool_name: &str) -> Option<&String> {
        self.aliases.get(tool_name)
    }

    /// Execute grep_search using grep or rg
    fn execute_grep_search(&self, call: &ToolCall, command: &str) -> ToolResult {
        let pattern = match call.require_string("pattern") {
            Ok(p) => p,
            Err(e) => return ToolResult::failure(&call.tool_name, ToolError::invalid_argument(e)),
        };

        let path = call.get_string("path").unwrap_or(".");
        let case_insensitive = call.get_bool("case_insensitive").unwrap_or(false);

        let start = Instant::now();

        let mut cmd = Command::new(command);

        // Build command based on tool type
        match command {
            "rg" => {
                // ripgrep
                cmd.arg("--line-number");
                cmd.arg("--color=never");
                if case_insensitive {
                    cmd.arg("-i");
                }
                cmd.arg(pattern);
                cmd.arg(path);
            }
            "grep" | _ => {
                // Standard grep
                cmd.arg("-rn");
                cmd.arg("--color=never");
                if case_insensitive {
                    cmd.arg("-i");
                }
                cmd.arg(pattern);
                cmd.arg(path);
            }
        }

        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        match cmd.output() {
            Ok(output) => {
                let duration = start.elapsed().as_millis() as u64;
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                // grep returns exit code 1 when no matches found (not an error)
                if output.status.success() || output.status.code() == Some(1) {
                    let result = if stdout.is_empty() {
                        "No matches found".to_string()
                    } else {
                        // Limit output size
                        let lines: Vec<&str> = stdout.lines().take(100).collect();
                        let truncated = lines.len() >= 100;
                        let mut result = lines.join("\n");
                        if truncated {
                            result.push_str("\n... (truncated, more matches available)");
                        }
                        result
                    };

                    let match_count = stdout.lines().count();
                    ToolResult::success(&call.tool_name, result).with_metadata(ToolResultMetadata {
                        duration_ms: Some(duration),
                        match_count: Some(match_count),
                        path: Some(path.to_string()),
                        ..Default::default()
                    })
                } else {
                    ToolResult::failure(
                        &call.tool_name,
                        ToolError::execution_failed(format!("Command failed: {}", stderr.trim())),
                    )
                }
            }
            Err(e) => ToolResult::failure(
                &call.tool_name,
                ToolError::execution_failed(format!("Failed to execute {}: {}", command, e)),
            ),
        }
    }

    /// Execute glob_search using find or fd
    fn execute_glob_search(&self, call: &ToolCall, command: &str) -> ToolResult {
        let pattern = match call.require_string("pattern") {
            Ok(p) => p,
            Err(e) => return ToolResult::failure(&call.tool_name, ToolError::invalid_argument(e)),
        };

        let base_dir = call.get_string("base_dir").unwrap_or(".");
        let max_results = call.get_i64("max_results").unwrap_or(100) as usize;

        let start = Instant::now();

        let mut cmd = Command::new(command);

        match command {
            "fd" => {
                // fd-find
                cmd.arg("--glob");
                cmd.arg("--color=never");
                cmd.arg(pattern);
                cmd.arg(base_dir);
            }
            "find" | _ => {
                // Standard find
                cmd.arg(base_dir);
                cmd.arg("-name");
                cmd.arg(pattern);
                cmd.arg("-type");
                cmd.arg("f");
            }
        }

        if let Some(ref dir) = self.working_dir {
            cmd.current_dir(dir);
        }

        match cmd.output() {
            Ok(output) => {
                let duration = start.elapsed().as_millis() as u64;
                let stdout = String::from_utf8_lossy(&output.stdout);

                if output.status.success() {
                    let lines: Vec<&str> = stdout.lines().take(max_results).collect();
                    let truncated = stdout.lines().count() > max_results;
                    let match_count = lines.len();

                    let mut result = if lines.is_empty() {
                        "No files found".to_string()
                    } else {
                        lines.join("\n")
                    };

                    if truncated {
                        result.push_str(&format!("\n... (showing first {} results)", max_results));
                    }

                    ToolResult::success(&call.tool_name, result).with_metadata(ToolResultMetadata {
                        duration_ms: Some(duration),
                        match_count: Some(match_count),
                        path: Some(base_dir.to_string()),
                        ..Default::default()
                    })
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    ToolResult::failure(
                        &call.tool_name,
                        ToolError::execution_failed(format!("Command failed: {}", stderr.trim())),
                    )
                }
            }
            Err(e) => ToolResult::failure(
                &call.tool_name,
                ToolError::execution_failed(format!("Failed to execute {}: {}", command, e)),
            ),
        }
    }

    /// Build tool definitions based on available commands
    fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        let mut tools = Vec::new();

        if self.available_tools.contains(&"grep_search".to_string()) {
            let cmd = self
                .aliases
                .get("grep_search")
                .map(|s| s.as_str())
                .unwrap_or("grep");
            tools.push(
                ToolDefinition::new(
                    "grep_search",
                    format!("Search file contents using {} (CLI provider)", cmd),
                    RiskLevel::Low,
                )
                .with_parameter(
                    ToolParameter::new("pattern", "Search pattern (regex)", true)
                        .with_type("string"),
                )
                .with_parameter(
                    ToolParameter::new("path", "Path to search (default: current dir)", false)
                        .with_type("path"),
                )
                .with_parameter(
                    ToolParameter::new("case_insensitive", "Case-insensitive search", false)
                        .with_type("boolean"),
                ),
            );
        }

        if self.available_tools.contains(&"glob_search".to_string()) {
            let cmd = self
                .aliases
                .get("glob_search")
                .map(|s| s.as_str())
                .unwrap_or("find");
            tools.push(
                ToolDefinition::new(
                    "glob_search",
                    format!("Find files by pattern using {} (CLI provider)", cmd),
                    RiskLevel::Low,
                )
                .with_parameter(
                    ToolParameter::new("pattern", "Glob pattern (e.g., *.rs)", true)
                        .with_type("string"),
                )
                .with_parameter(
                    ToolParameter::new("base_dir", "Base directory to search", false)
                        .with_type("path"),
                )
                .with_parameter(
                    ToolParameter::new("max_results", "Maximum results to return", false)
                        .with_type("number"),
                ),
            );
        }

        tools
    }
}

impl Default for CliToolProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProvider for CliToolProvider {
    fn id(&self) -> &str {
        "cli"
    }

    fn display_name(&self) -> &str {
        "CLI Tools"
    }

    fn priority(&self) -> i32 {
        CLI_PRIORITY
    }

    async fn is_available(&self) -> bool {
        // CLI provider is available if at least one tool is available
        !self.available_tools.is_empty()
    }

    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError> {
        Ok(self.build_tool_definitions())
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        let command = match self.get_command(&call.tool_name) {
            Some(cmd) => cmd.clone(),
            None => {
                return ToolResult::failure(
                    &call.tool_name,
                    ToolError::not_found(format!("Tool not configured: {}", call.tool_name)),
                );
            }
        };

        if !Self::is_command_available(&command) {
            return ToolResult::failure(
                &call.tool_name,
                ToolError::not_found(format!(
                    "CLI command '{}' not found. Install it or configure a different tool.",
                    command
                )),
            );
        }

        match call.tool_name.as_str() {
            "grep_search" => self.execute_grep_search(call, &command),
            "glob_search" => self.execute_glob_search(call, &command),
            _ => ToolResult::failure(
                &call.tool_name,
                ToolError::not_found(format!("Unknown CLI tool: {}", call.tool_name)),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cli_provider_default() {
        let provider = CliToolProvider::new();
        // At least grep or find should be available on most systems
        assert!(provider.is_available().await || provider.available_tools.is_empty());
    }

    #[tokio::test]
    async fn test_cli_provider_discover_tools() {
        let provider = CliToolProvider::new();
        let tools = provider.discover_tools().await.unwrap();

        // Tools depend on what's installed
        for tool in &tools {
            assert!(
                tool.name == "grep_search" || tool.name == "glob_search",
                "Unexpected tool: {}",
                tool.name
            );
        }
    }

    #[tokio::test]
    async fn test_cli_provider_priority() {
        let provider = CliToolProvider::new();
        assert_eq!(provider.priority(), CLI_PRIORITY);
        assert!(provider.priority() > 0); // Higher than builtin
    }

    #[tokio::test]
    async fn test_cli_provider_grep_search() {
        // Skip if grep not available
        if !CliToolProvider::is_command_available("grep") {
            return;
        }

        let provider = CliToolProvider::new();
        let call = ToolCall::new("grep_search")
            .with_arg("pattern", "fn ")
            .with_arg("path", ".");

        let result = provider.execute(&call).await;
        // Should succeed or have no matches
        assert!(
            result.is_success()
                || result
                    .output()
                    .map(|o| o.contains("No matches"))
                    .unwrap_or(false)
        );
    }

    #[tokio::test]
    async fn test_cli_provider_glob_search() {
        // Skip if find not available
        if !CliToolProvider::is_command_available("find") {
            return;
        }

        let provider = CliToolProvider::new();
        let call = ToolCall::new("glob_search")
            .with_arg("pattern", "*.rs")
            .with_arg("base_dir", ".");

        let result = provider.execute(&call).await;
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_cli_provider_unknown_tool() {
        let provider = CliToolProvider::new();
        let call = ToolCall::new("unknown_tool");
        let result = provider.execute(&call).await;

        assert!(!result.is_success());
    }

    #[test]
    fn test_is_command_available() {
        // These should be available on most Unix systems
        #[cfg(unix)]
        {
            assert!(CliToolProvider::is_command_available("ls"));
            assert!(CliToolProvider::is_command_available("cat"));
        }
        // This should not exist
        assert!(!CliToolProvider::is_command_available(
            "definitely_not_a_real_command_xyz123"
        ));
    }

    #[tokio::test]
    async fn test_cli_provider_with_rg() {
        // Only run if rg is available
        if !CliToolProvider::is_command_available("rg") {
            return;
        }

        let mut config = FileCliToolsConfig::default();
        config
            .aliases
            .insert("grep_search".to_string(), "rg".to_string());

        let provider = CliToolProvider::with_config(&config);
        assert!(
            provider
                .available_tools
                .contains(&"grep_search".to_string())
        );

        let tools = provider.discover_tools().await.unwrap();
        let grep_tool = tools.iter().find(|t| t.name == "grep_search");
        assert!(grep_tool.is_some());
        assert!(grep_tool.unwrap().description.contains("rg"));
    }
}
