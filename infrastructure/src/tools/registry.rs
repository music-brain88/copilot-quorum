//! Tool Registry
//!
//! The [`ToolRegistry`] aggregates multiple tool providers and implements
//! [`ToolExecutorPort`]. It handles tool discovery, provider resolution,
//! and execution routing based on priority.
//!
//! # Usage
//!
//! ```ignore
//! use quorum_infrastructure::tools::{ToolRegistry, BuiltinProvider, CliToolProvider};
//!
//! // Create registry with providers
//! let mut registry = ToolRegistry::new()
//!     .register(CliToolProvider::new())      // priority: 50
//!     .register(BuiltinProvider::new());     // priority: -100
//!
//! // Discover available tools from all providers
//! registry.discover().await?;
//!
//! // Check available tools
//! assert!(registry.has_tool("read_file"));
//! assert!(registry.has_tool("grep_search"));
//!
//! // Execute tools (automatically routed to correct provider)
//! let call = ToolCall::new("read_file").with_arg("path", "README.md");
//! let result = registry.execute(&call).await;
//! ```
//!
//! # Priority-Based Resolution
//!
//! When multiple providers offer the same tool, the registry uses the provider
//! with the highest priority. This allows:
//!
//! - CLI tools (rg) to override builtin tools (grep_search)
//! - MCP servers to provide enhanced functionality
//! - User scripts to customize behavior
//!
//! # Discovery Process
//!
//! The `discover()` method must be called before using the registry:
//!
//! 1. Providers are sorted by priority (highest first)
//! 2. Each provider's `discover_tools()` is called
//! 3. Tools are registered, with higher-priority providers winning conflicts
//! 4. A unified `ToolSpec` is built for the agent

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use quorum_application::ports::tool_executor::ToolExecutorPort;
use quorum_domain::tool::{
    entities::{ToolCall, ToolSpec},
    provider::ToolProvider,
    value_objects::{ToolError, ToolResult},
};

/// Tool registry that aggregates multiple providers
///
/// Implements `ToolExecutorPort` by routing tool calls to the appropriate
/// provider based on priority. When multiple providers offer the same tool,
/// the one with higher priority is used.
pub struct ToolRegistry {
    /// Registered providers
    providers: Vec<Arc<dyn ToolProvider>>,
    /// Tool name -> provider ID mapping (cached after discovery)
    tool_mapping: HashMap<String, String>,
    /// Merged tool specification
    tool_spec: ToolSpec,
    /// Whether discovery has been run
    discovered: bool,
}

impl ToolRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            tool_mapping: HashMap::new(),
            tool_spec: ToolSpec::new(),
            discovered: false,
        }
    }

    /// Register a tool provider
    pub fn register<P: ToolProvider + 'static>(mut self, provider: P) -> Self {
        self.providers.push(Arc::new(provider));
        self.discovered = false; // Invalidate cache
        self
    }

    /// Register a tool provider (Arc version)
    pub fn register_arc(mut self, provider: Arc<dyn ToolProvider>) -> Self {
        self.providers.push(provider);
        self.discovered = false;
        self
    }

    /// Discover tools from all providers
    ///
    /// This must be called before using the registry.
    /// Tools are merged with priority-based conflict resolution.
    pub async fn discover(&mut self) -> Result<(), String> {
        // Sort providers by priority (descending)
        self.providers
            .sort_by_key(|p| std::cmp::Reverse(p.priority()));

        let mut tool_spec = ToolSpec::new();
        let mut tool_mapping = HashMap::new();

        for provider in &self.providers {
            if !provider.is_available().await {
                tracing::debug!(provider = provider.id(), "Provider not available, skipping");
                continue;
            }

            match provider.discover_tools().await {
                Ok(tools) => {
                    for tool in tools {
                        // Only add if not already registered (higher priority wins)
                        if !tool_mapping.contains_key(&tool.name) {
                            tracing::debug!(
                                tool = %tool.name,
                                provider = provider.id(),
                                "Registered tool"
                            );
                            tool_mapping.insert(tool.name.clone(), provider.id().to_string());
                            tool_spec = tool_spec.register(tool);
                        } else {
                            tracing::trace!(
                                tool = %tool.name,
                                provider = provider.id(),
                                "Tool already registered by higher priority provider"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        provider = provider.id(),
                        error = %e,
                        "Failed to discover tools from provider"
                    );
                }
            }
        }

        self.tool_spec = tool_spec;
        self.tool_mapping = tool_mapping;
        self.discovered = true;

        Ok(())
    }

    /// Get the provider for a specific tool
    fn provider_for(&self, tool_name: &str) -> Option<&Arc<dyn ToolProvider>> {
        let provider_id = self.tool_mapping.get(tool_name)?;
        self.providers.iter().find(|p| p.id() == provider_id)
    }

    /// Get a list of registered provider IDs
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    /// Get statistics about registered tools
    pub fn stats(&self) -> RegistryStats {
        let mut tools_per_provider = HashMap::new();
        for provider_id in self.tool_mapping.values() {
            *tools_per_provider.entry(provider_id.clone()).or_insert(0) += 1;
        }

        RegistryStats {
            total_providers: self.providers.len(),
            total_tools: self.tool_mapping.len(),
            tools_per_provider,
        }
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about the registry
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total_providers: usize,
    pub total_tools: usize,
    pub tools_per_provider: HashMap<String, usize>,
}

#[async_trait]
impl ToolExecutorPort for ToolRegistry {
    fn tool_spec(&self) -> &ToolSpec {
        &self.tool_spec
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        if !self.discovered {
            return ToolResult::failure(
                &call.tool_name,
                ToolError::execution_failed("Registry not initialized. Call discover() first."),
            );
        }

        match self.provider_for(&call.tool_name) {
            Some(provider) => provider.execute(call).await,
            None => ToolResult::failure(
                &call.tool_name,
                ToolError::not_found(format!("Tool not found: {}", call.tool_name)),
            ),
        }
    }

    fn execute_sync(&self, call: &ToolCall) -> ToolResult {
        // Create a runtime for sync execution
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();

        match rt {
            Ok(rt) => rt.block_on(self.execute(call)),
            Err(e) => ToolResult::failure(
                &call.tool_name,
                ToolError::execution_failed(format!("Failed to create runtime: {}", e)),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::builtin::BuiltinProvider;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_registry_with_builtin() {
        let mut registry = ToolRegistry::new().register(BuiltinProvider::new());

        registry.discover().await.unwrap();

        assert!(registry.has_tool("read_file"));
        assert!(registry.has_tool("write_file"));
        assert!(registry.has_tool("run_command"));
    }

    #[tokio::test]
    async fn test_registry_execute() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "registry test").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let mut registry = ToolRegistry::new().register(BuiltinProvider::new());
        registry.discover().await.unwrap();

        let call = ToolCall::new("read_file").with_arg("path", path);
        let result = registry.execute(&call).await;

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("registry test"));
    }

    #[tokio::test]
    async fn test_registry_unknown_tool() {
        let mut registry = ToolRegistry::new().register(BuiltinProvider::new());
        registry.discover().await.unwrap();

        let call = ToolCall::new("unknown_tool");
        let result = registry.execute(&call).await;

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "NOT_FOUND");
    }

    #[tokio::test]
    async fn test_registry_not_discovered() {
        let registry = ToolRegistry::new().register(BuiltinProvider::new());

        let call = ToolCall::new("read_file").with_arg("path", "/test");
        let result = registry.execute(&call).await;

        assert!(!result.is_success());
        assert!(result.error().unwrap().message.contains("not initialized"));
    }

    #[tokio::test]
    async fn test_registry_stats() {
        let mut registry = ToolRegistry::new().register(BuiltinProvider::new());
        registry.discover().await.unwrap();

        let stats = registry.stats();
        assert_eq!(stats.total_providers, 1);
        assert_eq!(stats.total_tools, 5); // 5 builtin tools
        assert!(stats.tools_per_provider.contains_key("builtin"));
    }

    #[tokio::test]
    async fn test_registry_provider_ids() {
        let registry = ToolRegistry::new().register(BuiltinProvider::new());

        let ids = registry.provider_ids();
        assert!(ids.contains(&"builtin"));
    }

    #[test]
    fn test_registry_execute_sync() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "sync test").unwrap();
        let path = temp_file.path().to_str().unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut registry = ToolRegistry::new().register(BuiltinProvider::new());
        rt.block_on(registry.discover()).unwrap();

        let call = ToolCall::new("read_file").with_arg("path", path);
        let result = registry.execute_sync(&call);

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("sync test"));
    }
}
