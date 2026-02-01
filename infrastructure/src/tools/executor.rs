//! Local tool executor that runs tools on the local machine

use async_trait::async_trait;
use quorum_application::ports::tool_executor::ToolExecutorPort;
use quorum_domain::tool::{
    entities::{ToolCall, ToolSpec},
    value_objects::{ToolError, ToolResult},
};

use super::{command, file, search};

/// Executor that runs tools on the local machine
///
/// Implements the `ToolExecutorPort` trait from the application layer.
#[derive(Debug, Clone)]
pub struct LocalToolExecutor {
    /// Available tools
    tool_spec: ToolSpec,
    /// Working directory for commands (None = current directory)
    working_dir: Option<String>,
}

impl LocalToolExecutor {
    /// Create a new executor with all available tools
    pub fn new() -> Self {
        Self {
            tool_spec: super::default_tool_spec(),
            working_dir: None,
        }
    }

    /// Create an executor with only read-only tools
    pub fn read_only() -> Self {
        Self {
            tool_spec: super::read_only_tool_spec(),
            working_dir: None,
        }
    }

    /// Create an executor with a custom tool spec
    pub fn with_tools(tool_spec: ToolSpec) -> Self {
        Self {
            tool_spec,
            working_dir: None,
        }
    }

    /// Set the working directory for commands
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Internal execute implementation
    fn execute_internal(&self, call: &ToolCall) -> ToolResult {
        // Check if tool exists
        let definition = match self.tool_spec.get(&call.tool_name) {
            Some(d) => d,
            None => {
                return ToolResult::failure(
                    &call.tool_name,
                    ToolError::not_found(format!("Unknown tool: {}", call.tool_name)),
                )
            }
        };

        // Validate the call
        let validator = quorum_domain::tool::traits::DefaultToolValidator;
        if let Err(e) = quorum_domain::tool::traits::ToolValidator::validate(
            &validator,
            call,
            definition,
        ) {
            return ToolResult::failure(&call.tool_name, ToolError::invalid_argument(e));
        }

        // Execute the appropriate tool
        match call.tool_name.as_str() {
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
            _ => ToolResult::failure(
                &call.tool_name,
                ToolError::execution_failed(format!("Tool '{}' is not implemented", call.tool_name)),
            ),
        }
    }
}

impl Default for LocalToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolExecutorPort for LocalToolExecutor {
    fn tool_spec(&self) -> &ToolSpec {
        &self.tool_spec
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        // For now, execute synchronously
        // In the future, we could make file I/O and command execution truly async
        self.execute_internal(call)
    }

    fn execute_sync(&self, call: &ToolCall) -> ToolResult {
        self.execute_internal(call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_executor_has_all_tools() {
        let executor = LocalToolExecutor::new();
        assert!(executor.has_tool("read_file"));
        assert!(executor.has_tool("write_file"));
        assert!(executor.has_tool("run_command"));
        assert!(executor.has_tool("glob_search"));
        assert!(executor.has_tool("grep_search"));
    }

    #[test]
    fn test_executor_read_only() {
        let executor = LocalToolExecutor::read_only();
        assert!(executor.has_tool("read_file"));
        assert!(!executor.has_tool("write_file"));
        assert!(!executor.has_tool("run_command"));
        assert!(executor.has_tool("glob_search"));
        assert!(executor.has_tool("grep_search"));
    }

    #[test]
    fn test_executor_unknown_tool() {
        let executor = LocalToolExecutor::new();
        let call = ToolCall::new("unknown_tool");
        let result = executor.execute_sync(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "NOT_FOUND");
    }

    #[test]
    fn test_executor_read_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "test content").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let executor = LocalToolExecutor::new();
        let call = ToolCall::new("read_file").with_arg("path", path);
        let result = executor.execute_sync(&call);

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("test content"));
    }

    #[test]
    fn test_executor_write_file() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");
        let path_str = path.to_str().unwrap();

        let executor = LocalToolExecutor::new();
        let call = ToolCall::new("write_file")
            .with_arg("path", path_str)
            .with_arg("content", "written content");
        let result = executor.execute_sync(&call);

        assert!(result.is_success());
        assert_eq!(fs::read_to_string(&path).unwrap(), "written content");
    }

    #[test]
    fn test_executor_with_working_dir() {
        let temp_dir = tempdir().unwrap();
        let executor = LocalToolExecutor::new().with_working_dir(temp_dir.path().to_str().unwrap());

        let call = ToolCall::new("run_command").with_arg("command", "pwd");
        let result = executor.execute_sync(&call);

        assert!(result.is_success());
        // Output should contain the temp dir
        let output = result.output().unwrap();
        assert!(output.contains(temp_dir.path().file_name().unwrap().to_str().unwrap()));
    }

    #[test]
    fn test_executor_validation_error() {
        let executor = LocalToolExecutor::new();
        // Missing required 'path' parameter
        let call = ToolCall::new("read_file");
        let result = executor.execute_sync(&call);

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "INVALID_ARGUMENT");
    }

    #[tokio::test]
    async fn test_executor_async() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "async content").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let executor = LocalToolExecutor::new();
        let call = ToolCall::new("read_file").with_arg("path", path);
        let result = executor.execute(&call).await;

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("async content"));
    }

    #[test]
    fn test_available_tools() {
        let executor = LocalToolExecutor::new();
        let tools = executor.available_tools();

        assert!(tools.contains(&"read_file"));
        assert!(tools.contains(&"write_file"));
        assert!(tools.contains(&"run_command"));
        assert!(tools.contains(&"glob_search"));
        assert!(tools.contains(&"grep_search"));
    }
}
