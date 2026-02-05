//! Tool provider abstraction
//!
//! This module defines the [`ToolProvider`] trait, which abstracts external
//! tool providers (CLI, MCP, etc.) that can be plugged into the tool registry.
//!
//! # Architecture
//!
//! The tool system uses a plugin-based architecture where tools come from
//! multiple providers:
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     ToolRegistry                            │
//! │  (aggregates providers, routes by priority)                 │
//! └─────────────────────────────────────────────────────────────┘
//!           │              │              │              │
//!           ▼              ▼              ▼              ▼
//!    ┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
//!    │ Builtin  │   │   CLI    │   │   MCP    │   │  Script  │
//!    │ Provider │   │ Provider │   │ Provider │   │ Provider │
//!    └──────────┘   └──────────┘   └──────────┘   └──────────┘
//!    priority:-100  priority:50   priority:100   priority:75
//! ```
//!
//! # Provider Types
//!
//! - **BuiltinProvider**: Minimal built-in tools (read_file, write_file, etc.)
//!   Always available as a fallback.
//! - **CliToolProvider**: Wraps CLI tools (grep/rg, find/fd, cat/bat).
//!   Uses standard tools by default, enhanced tools when configured.
//! - **McpToolProvider**: Connects to MCP (Model Context Protocol) servers.
//! - **ScriptToolProvider**: User-defined shell scripts as tools.
//!
//! # Priority System
//!
//! When multiple providers offer the same tool, the one with higher priority
//! is used. This allows enhanced tools to override standard ones:
//!
//! | Provider | Priority | Use Case |
//! |----------|----------|----------|
//! | MCP      | 100      | External servers with rich capabilities |
//! | Script   | 75       | User customizations |
//! | CLI      | 50       | System CLI tools |
//! | Builtin  | -100     | Fallback when nothing else available |
//!
//! # Example
//!
//! ```ignore
//! use quorum_domain::tool::ToolProvider;
//!
//! // Create a registry with multiple providers
//! let registry = ToolRegistry::new()
//!     .register(CliToolProvider::new())
//!     .register(BuiltinProvider::new());
//!
//! // Discover all available tools
//! registry.discover().await?;
//!
//! // Execute a tool (routed to the appropriate provider)
//! let result = registry.execute(&call).await;
//! ```

use async_trait::async_trait;
use thiserror::Error;

use super::entities::{ToolCall, ToolDefinition};
use super::value_objects::ToolResult;

/// Error type for tool provider operations
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Provider is not available (e.g., CLI tool not installed)
    #[error("Provider not available: {0}")]
    NotAvailable(String),

    /// Failed to discover tools from the provider
    #[error("Discovery failed: {0}")]
    DiscoveryFailed(String),

    /// Tool not found in this provider
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    /// Tool execution failed
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

/// Tool provider abstraction - external source of tools
///
/// Implementations provide tools from various sources:
/// - `BuiltinProvider`: Minimal built-in tools (read_file, write_file)
/// - `CliToolProvider`: CLI tools (grep, find, rg, fd, gh, etc.)
/// - `McpToolProvider`: MCP server tools
/// - `ScriptToolProvider`: User-defined scripts
#[async_trait]
pub trait ToolProvider: Send + Sync {
    /// Unique identifier for this provider
    ///
    /// Examples: "builtin", "cli", "mcp:filesystem", "script"
    fn id(&self) -> &str;

    /// Display name for user-facing output
    fn display_name(&self) -> &str;

    /// Priority for tool resolution (higher = preferred)
    ///
    /// When multiple providers offer the same tool, the one with
    /// higher priority is used. Default providers:
    /// - Builtin: -100 (fallback)
    /// - CLI: 50
    /// - Script: 75
    /// - MCP: 100 (highest)
    fn priority(&self) -> i32 {
        0
    }

    /// Check if the provider is available and properly configured
    ///
    /// For CLI providers, this checks if the CLI tool is installed.
    /// For MCP providers, this checks if the server can be reached.
    async fn is_available(&self) -> bool;

    /// Discover available tools from this provider
    ///
    /// Returns the list of tools this provider can execute.
    /// For CLI providers, only returns tools whose CLI is installed.
    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError>;

    /// Execute a tool call
    ///
    /// The tool_name in the call must match one of the tools
    /// returned by `discover_tools()`.
    async fn execute(&self, call: &ToolCall) -> ToolResult;

    /// Check if this provider has a specific tool
    async fn has_tool(&self, tool_name: &str) -> bool {
        match self.discover_tools().await {
            Ok(tools) => tools.iter().any(|t| t.name == tool_name),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::entities::RiskLevel;
    use crate::tool::value_objects::ToolError;

    /// A mock provider for testing
    struct MockProvider {
        id: String,
        tools: Vec<ToolDefinition>,
        available: bool,
    }

    impl MockProvider {
        fn new(id: &str, available: bool) -> Self {
            Self {
                id: id.to_string(),
                tools: Vec::new(),
                available,
            }
        }

        fn with_tool(mut self, name: &str) -> Self {
            self.tools.push(ToolDefinition::new(
                name,
                format!("Mock tool: {}", name),
                RiskLevel::Low,
            ));
            self
        }
    }

    #[async_trait]
    impl ToolProvider for MockProvider {
        fn id(&self) -> &str {
            &self.id
        }

        fn display_name(&self) -> &str {
            "Mock Provider"
        }

        fn priority(&self) -> i32 {
            0
        }

        async fn is_available(&self) -> bool {
            self.available
        }

        async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError> {
            if self.available {
                Ok(self.tools.clone())
            } else {
                Err(ProviderError::NotAvailable("Mock not available".into()))
            }
        }

        async fn execute(&self, call: &ToolCall) -> ToolResult {
            if self.tools.iter().any(|t| t.name == call.tool_name) {
                ToolResult::success(&call.tool_name, "Mock output")
            } else {
                ToolResult::failure(&call.tool_name, ToolError::not_found(&call.tool_name))
            }
        }
    }

    #[tokio::test]
    async fn test_provider_discovery() {
        let provider = MockProvider::new("mock", true)
            .with_tool("tool_a")
            .with_tool("tool_b");

        assert!(provider.is_available().await);

        let tools = provider.discover_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert!(tools.iter().any(|t| t.name == "tool_a"));
    }

    #[tokio::test]
    async fn test_provider_not_available() {
        let provider = MockProvider::new("mock", false);

        assert!(!provider.is_available().await);
        assert!(provider.discover_tools().await.is_err());
    }

    #[tokio::test]
    async fn test_provider_has_tool() {
        let provider = MockProvider::new("mock", true).with_tool("read_file");

        assert!(provider.has_tool("read_file").await);
        assert!(!provider.has_tool("unknown").await);
    }

    #[tokio::test]
    async fn test_provider_execute() {
        let provider = MockProvider::new("mock", true).with_tool("read_file");

        let call = ToolCall::new("read_file").with_arg("path", "/test.txt");
        let result = provider.execute(&call).await;

        assert!(result.is_success());
    }
}
