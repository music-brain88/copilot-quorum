//! Tool schema conversion port.
//!
//! Separates "which tools to use" (domain) from "how to serialize for API"
//! (infrastructure). The domain layer defines [`ToolDefinition`] and
//! [`ToolSpec`] with filtering logic; this port handles the JSON Schema
//! conversion that the LLM API requires.

use quorum_domain::tool::entities::{ToolDefinition, ToolSpec};

/// Port for converting tool definitions to LLM API format (JSON Schema).
///
/// Separates "which tools to use" (domain) from "how to serialize for API" (infrastructure).
pub trait ToolSchemaPort: Send + Sync {
    /// Convert a single tool definition to provider-neutral JSON Schema.
    fn tool_to_schema(&self, tool: &ToolDefinition) -> serde_json::Value;

    /// Convert all tools to JSON Schema array (sorted by name).
    fn all_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value>;

    /// Convert low-risk tools only to JSON Schema array (sorted by name).
    fn low_risk_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value>;
}
