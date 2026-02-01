//! Tool Executor port
//!
//! Defines the interface for executing tools (file operations, commands, searches).

use async_trait::async_trait;
use quorum_domain::tool::{
    entities::{ToolCall, ToolDefinition, ToolSpec},
    value_objects::ToolResult,
};

/// Port for tool execution
///
/// This port defines how the application layer executes tools.
/// Implementations (adapters) live in the infrastructure layer.
#[async_trait]
pub trait ToolExecutorPort: Send + Sync {
    /// Get the specification of all available tools
    fn tool_spec(&self) -> &ToolSpec;

    /// Check if a tool is available
    fn has_tool(&self, name: &str) -> bool {
        self.tool_spec().get(name).is_some()
    }

    /// Get the definition of a specific tool
    fn get_tool(&self, name: &str) -> Option<&ToolDefinition> {
        self.tool_spec().get(name)
    }

    /// Get names of all available tools
    fn available_tools(&self) -> Vec<&str> {
        self.tool_spec().names().collect()
    }

    /// Execute a tool call asynchronously
    async fn execute(&self, call: &ToolCall) -> ToolResult;

    /// Execute a tool call synchronously (blocking)
    ///
    /// Default implementation wraps the async version.
    /// Implementations may override for better performance.
    fn execute_sync(&self, call: &ToolCall) -> ToolResult;
}
