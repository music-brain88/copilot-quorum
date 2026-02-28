//! Local tool executor — the concrete implementation of [`ToolExecutorPort`].
//!
//! [`LocalToolExecutor`] is the infrastructure-layer adapter that bridges the
//! application layer's abstract [`ToolExecutorPort`] with actual system operations:
//! file I/O, process execution, content search, and (optionally) web requests.
//!
//! # Execution Paths
//!
//! ```text
//! ToolExecutorPort::execute()
//!   ├─ is_async_tool?  → execute_async()   (web_fetch, web_search via reqwest)
//!   └─ otherwise       → execute_internal() (file, command, search — synchronous)
//!
//! ToolExecutorPort::execute_sync()
//!   ├─ is_async_tool?  → block_in_place(execute_async())  (tokio bridge)
//!   └─ otherwise       → execute_internal()
//! ```
//!
//! # Web Tools (`web-tools` feature)
//!
//! When the `web-tools` feature is enabled, the executor holds a shared `reqwest::Client`
//! (30s timeout) and routes `web_fetch`/`web_search` calls through the async path.

use async_trait::async_trait;
use quorum_application::ports::tool_executor::ToolExecutorPort;
use quorum_domain::RiskLevel;
use quorum_domain::tool::{
    entities::{ToolCall, ToolDefinition, ToolParameter, ToolSpec},
    provider::ToolProvider,
    value_objects::{ToolError, ToolResult},
};

use quorum_application::ports::scripting_engine::CustomToolDef;

use super::{command, custom_provider::CustomToolProvider, file, search};

/// Executor that runs tools on the local machine.
///
/// Implements [`ToolExecutorPort`] from the application layer, providing the
/// concrete tool execution for the agent system.
///
/// # Configurations
///
/// | Constructor | Tools | Use Case |
/// |-------------|-------|----------|
/// | [`new()`](Self::new) | All tools (5 + 2 web) | Full agent execution |
/// | [`read_only()`](Self::read_only) | Read-only tools only | Context gathering phase |
/// | [`with_tools()`](Self::with_tools) | Custom [`ToolSpec`] | Testing / specialized setups |
///
/// # Custom Tools
///
/// User-defined tools from Lua scripting are integrated via [`with_custom_tool_defs()`](Self::with_custom_tool_defs).
/// Custom tool calls are delegated to the embedded [`CustomToolProvider`].
///
/// # Web Tools Integration
///
/// With the `web-tools` feature, the executor holds a shared [`reqwest::Client`]
/// with a 30-second timeout. Web tool calls are routed through [`execute_async()`](Self)
/// while all other tools use synchronous `execute_internal()`.
#[derive(Debug, Clone)]
pub struct LocalToolExecutor {
    /// Available tools
    tool_spec: ToolSpec,
    /// Working directory for commands (None = current directory)
    working_dir: Option<String>,
    /// Custom tool provider for user-defined tools
    custom_provider: Option<CustomToolProvider>,
    /// HTTP client for web tools (only available with web-tools feature)
    #[cfg(feature = "web-tools")]
    http_client: reqwest::Client,
}

impl LocalToolExecutor {
    /// Create a new executor with all available tools.
    ///
    /// Uses [`default_tool_spec()`](super::default_tool_spec) which includes
    /// all 5 core tools and (with `web-tools`) web tools.
    pub fn new() -> Self {
        Self {
            tool_spec: super::default_tool_spec(),
            working_dir: None,
            custom_provider: None,
            #[cfg(feature = "web-tools")]
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Create an executor with only read-only (low-risk) tools.
    ///
    /// Excludes `write_file` and `run_command`. Used during the context
    /// gathering phase where state modification is not allowed.
    pub fn read_only() -> Self {
        Self {
            tool_spec: super::read_only_tool_spec(),
            working_dir: None,
            custom_provider: None,
            #[cfg(feature = "web-tools")]
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Create an executor with a custom tool spec
    pub fn with_tools(tool_spec: ToolSpec) -> Self {
        Self {
            tool_spec,
            working_dir: None,
            custom_provider: None,
            #[cfg(feature = "web-tools")]
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Set the working directory for commands
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Register custom tools from Lua definitions.
    pub fn with_custom_tool_defs(mut self, defs: &[CustomToolDef]) -> Self {
        if defs.is_empty() {
            return self;
        }

        let mut provider = CustomToolProvider::from_custom_tool_defs(defs);
        if let Some(dir) = &self.working_dir {
            provider = provider.with_working_dir(dir.clone());
        }

        for def in defs {
            let risk_level = match def.risk_level.as_str() {
                "low" => RiskLevel::Low,
                _ => RiskLevel::High,
            };
            let mut definition = ToolDefinition::new(&def.name, &def.description, risk_level);
            let mut sorted_params = def.parameters.clone();
            sorted_params.sort_by(|a, b| a.name.cmp(&b.name));
            for p in &sorted_params {
                definition = definition.with_parameter(
                    ToolParameter::new(&p.name, &p.description, p.required)
                        .with_type(&p.param_type),
                );
            }
            self.tool_spec = self.tool_spec.register(definition);
        }

        self.custom_provider = Some(provider);
        self
    }

    /// Internal execute implementation for built-in synchronous tools (file, command, search).
    ///
    /// Routes calls by exact canonical name. Custom tools are handled in the
    /// async `execute()` path and never reach this method.
    fn execute_internal(&self, call: &ToolCall) -> ToolResult {
        // Check if tool exists
        let definition = match self.tool_spec.get(&call.tool_name) {
            Some(d) => d,
            None => {
                return ToolResult::failure(
                    &call.tool_name,
                    ToolError::not_found(format!("Unknown tool: {}", call.tool_name)),
                );
            }
        };

        // Validate the call
        let validator = quorum_domain::tool::traits::DefaultToolValidator;
        if let Err(e) =
            quorum_domain::tool::traits::ToolValidator::validate(&validator, call, definition)
        {
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
                ToolError::execution_failed(format!(
                    "Tool '{}' is not implemented",
                    call.tool_name
                )),
            ),
        }
    }

    /// Check if a tool name belongs to a built-in tool (not custom).
    fn is_builtin_tool(&self, name: &str) -> bool {
        matches!(
            name,
            file::READ_FILE
                | file::WRITE_FILE
                | command::RUN_COMMAND
                | search::GLOB_SEARCH
                | search::GREP_SEARCH
        ) || self.is_web_tool(name)
    }

    /// Check if a tool is a web tool (always false without the feature).
    #[cfg(feature = "web-tools")]
    fn is_web_tool(&self, name: &str) -> bool {
        matches!(name, super::web::WEB_FETCH | super::web::WEB_SEARCH)
    }

    #[cfg(not(feature = "web-tools"))]
    fn is_web_tool(&self, _name: &str) -> bool {
        false
    }

    /// Check if a tool requires async execution (web tools using `reqwest`).
    #[cfg(feature = "web-tools")]
    fn is_async_tool(name: &str) -> bool {
        matches!(name, super::web::WEB_FETCH | super::web::WEB_SEARCH)
    }

    /// Execute async web tools (`web_fetch`, `web_search`) via `reqwest`.
    ///
    /// Validates the call against the tool definition first, then dispatches
    /// to the appropriate web tool executor.
    #[cfg(feature = "web-tools")]
    async fn execute_async(&self, call: &ToolCall) -> ToolResult {
        // Check if tool exists
        let definition = match self.tool_spec.get(&call.tool_name) {
            Some(d) => d,
            None => {
                return ToolResult::failure(
                    &call.tool_name,
                    ToolError::not_found(format!("Unknown tool: {}", call.tool_name)),
                );
            }
        };

        // Validate the call
        let validator = quorum_domain::tool::traits::DefaultToolValidator;
        if let Err(e) =
            quorum_domain::tool::traits::ToolValidator::validate(&validator, call, definition)
        {
            return ToolResult::failure(&call.tool_name, ToolError::invalid_argument(e));
        }

        match call.tool_name.as_str() {
            super::web::WEB_FETCH => super::web::execute_web_fetch(&self.http_client, call).await,
            super::web::WEB_SEARCH => super::web::execute_web_search(&self.http_client, call).await,
            _ => ToolResult::failure(
                &call.tool_name,
                ToolError::execution_failed(format!(
                    "Tool '{}' is not an async tool",
                    call.tool_name
                )),
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
        #[cfg(feature = "web-tools")]
        {
            if Self::is_async_tool(&call.tool_name) {
                return self.execute_async(call).await;
            }
        }
        // Check custom tools first (can await directly in async context)
        if let Some(ref provider) = self.custom_provider
            && self.tool_spec.get(&call.tool_name).is_some()
            && !self.is_builtin_tool(&call.tool_name)
        {
            return provider.execute(call).await;
        }
        self.execute_internal(call)
    }

    fn execute_sync(&self, call: &ToolCall) -> ToolResult {
        #[cfg(feature = "web-tools")]
        {
            if Self::is_async_tool(&call.tool_name) {
                // Web tools need async runtime — use block_on from current runtime
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    return tokio::task::block_in_place(|| {
                        handle.block_on(self.execute_async(call))
                    });
                } else {
                    return ToolResult::failure(
                        &call.tool_name,
                        ToolError::execution_failed(
                            "Web tools require an async runtime".to_string(),
                        ),
                    );
                }
            }
        }
        self.execute_internal(call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::{NamedTempFile, tempdir};

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

    #[cfg(feature = "web-tools")]
    #[test]
    fn test_executor_has_web_tools() {
        let executor = LocalToolExecutor::new();
        assert!(executor.has_tool("web_fetch"));
        assert!(executor.has_tool("web_search"));
    }

    #[cfg(feature = "web-tools")]
    #[test]
    fn test_executor_read_only_has_web_tools() {
        let executor = LocalToolExecutor::read_only();
        assert!(executor.has_tool("web_fetch"));
        assert!(executor.has_tool("web_search"));
    }
}
