//! Built-in tool provider
//!
//! Provides minimal built-in tools as a fallback when no external
//! providers are available. Uses the existing tool implementations.

use async_trait::async_trait;
use quorum_domain::tool::{
    entities::{ToolCall, ToolDefinition, ToolSpec},
    provider::{ProviderError, ToolProvider},
    value_objects::ToolResult,
    DefaultToolValidator, ToolValidator,
};

use crate::tools::{command, file, search};

/// Priority for the built-in provider (lowest, used as fallback)
pub const BUILTIN_PRIORITY: i32 = -100;

/// Built-in tool provider
///
/// Wraps the existing tool implementations (read_file, write_file, etc.)
/// as a ToolProvider. This is the fallback provider when no external
/// tools are available.
#[derive(Debug, Clone)]
pub struct BuiltinProvider {
    /// Available tools
    tool_spec: ToolSpec,
    /// Working directory for commands
    working_dir: Option<String>,
}

impl BuiltinProvider {
    /// Create a new built-in provider with all tools
    pub fn new() -> Self {
        Self {
            tool_spec: crate::tools::default_tool_spec(),
            working_dir: None,
        }
    }

    /// Create a built-in provider with only read-only tools
    pub fn read_only() -> Self {
        Self {
            tool_spec: crate::tools::read_only_tool_spec(),
            working_dir: None,
        }
    }

    /// Set the working directory for command execution
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Internal execute implementation
    fn execute_internal(&self, call: &ToolCall) -> Result<ToolResult, ProviderError> {
        // Check if tool exists
        let definition = self
            .tool_spec
            .get(&call.tool_name)
            .ok_or_else(|| ProviderError::ToolNotFound(call.tool_name.clone()))?;

        // Validate the call
        let validator = DefaultToolValidator;
        if let Err(e) = validator.validate(call, definition) {
            return Ok(ToolResult::failure(
                &call.tool_name,
                quorum_domain::tool::ToolError::invalid_argument(e),
            ));
        }

        // Execute the appropriate tool
        let result = match call.tool_name.as_str() {
            file::READ_FILE => file::execute_read_file(call),
            file::WRITE_FILE => file::execute_write_file(call),
            command::RUN_COMMAND => {
                // Inject working directory if set and not already specified
                if self.working_dir.is_some() && call.get_string("working_dir").is_none() {
                    let mut modified_call = call.clone();
                    if let Some(dir) = &self.working_dir {
                        modified_call = modified_call.with_arg("working_dir", dir.as_str());
                    }
                    command::execute_run_command(&modified_call)
                } else {
                    command::execute_run_command(call)
                }
            }
            search::GLOB_SEARCH => search::execute_glob_search(call),
            search::GREP_SEARCH => search::execute_grep_search(call),
            _ => {
                return Err(ProviderError::ToolNotFound(call.tool_name.clone()));
            }
        };

        Ok(result)
    }
}

impl Default for BuiltinProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolProvider for BuiltinProvider {
    fn id(&self) -> &str {
        "builtin"
    }

    fn display_name(&self) -> &str {
        "Built-in Tools"
    }

    fn priority(&self) -> i32 {
        BUILTIN_PRIORITY
    }

    async fn is_available(&self) -> bool {
        // Built-in tools are always available
        true
    }

    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError> {
        Ok(self.tool_spec.all().cloned().collect())
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        match self.execute_internal(call) {
            Ok(result) => result,
            Err(e) => ToolResult::failure(
                &call.tool_name,
                quorum_domain::tool::ToolError::execution_failed(e.to_string()),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_builtin_provider_is_available() {
        let provider = BuiltinProvider::new();
        assert!(provider.is_available().await);
    }

    #[tokio::test]
    async fn test_builtin_provider_discover_tools() {
        let provider = BuiltinProvider::new();
        let tools = provider.discover_tools().await.unwrap();

        let tool_names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"read_file"));
        assert!(tool_names.contains(&"write_file"));
        assert!(tool_names.contains(&"run_command"));
        assert!(tool_names.contains(&"glob_search"));
        assert!(tool_names.contains(&"grep_search"));
    }

    #[tokio::test]
    async fn test_builtin_provider_read_only() {
        let provider = BuiltinProvider::read_only();
        let tools = provider.discover_tools().await.unwrap();

        let tool_names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(tool_names.contains(&"read_file"));
        assert!(!tool_names.contains(&"write_file"));
        assert!(!tool_names.contains(&"run_command"));
    }

    #[tokio::test]
    async fn test_builtin_provider_execute() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let provider = BuiltinProvider::new();
        let call = ToolCall::new("read_file").with_arg("path", path);
        let result = provider.execute(&call).await;

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("test content"));
    }

    #[tokio::test]
    async fn test_builtin_provider_has_tool() {
        let provider = BuiltinProvider::new();

        assert!(provider.has_tool("read_file").await);
        assert!(provider.has_tool("write_file").await);
        assert!(!provider.has_tool("unknown_tool").await);
    }

    #[tokio::test]
    async fn test_builtin_provider_priority() {
        let provider = BuiltinProvider::new();
        assert_eq!(provider.priority(), BUILTIN_PRIORITY);
        assert!(provider.priority() < 0); // Should be low priority (fallback)
    }

    #[tokio::test]
    async fn test_builtin_provider_unknown_tool() {
        let provider = BuiltinProvider::new();
        let call = ToolCall::new("unknown_tool");
        let result = provider.execute(&call).await;

        assert!(!result.is_success());
    }
}
