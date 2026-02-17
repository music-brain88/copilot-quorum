//! Tool domain entities
//!
//! Core entities for the **Tool System**: definitions, invocations, and the
//! tool registry with alias resolution support.
//!
//! See the [module-level documentation](super) for an architectural overview.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Risk level of a tool operation, used by the **Quorum review system** to
/// determine whether multi-model consensus is required before execution.
///
/// This is the core safety mechanism in the agent's tool execution pipeline:
/// high-risk tools are reviewed by the Quorum before running, while low-risk
/// tools (including web tools) execute immediately.
///
/// # Risk Classification
///
/// | Level | Operations | Review |
/// |-------|-----------|--------|
/// | [`Low`](Self::Low) | `read_file`, `glob_search`, `grep_search`, `web_fetch`, `web_search` | Direct execution |
/// | [`High`](Self::High) | `write_file`, `run_command` | Quorum consensus required |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Low risk — read-only operations that don't modify state.
    ///
    /// Includes file reads, searches, and web tools (`web_fetch`, `web_search`).
    Low,
    /// High risk — operations that modify the local environment.
    ///
    /// Requires [Quorum review](crate::quorum) before execution in Ensemble mode.
    High,
}

impl RiskLevel {
    pub fn as_str(&self) -> &str {
        match self {
            RiskLevel::Low => "low",
            RiskLevel::High => "high",
        }
    }

    pub fn requires_quorum(&self) -> bool {
        matches!(self, RiskLevel::High)
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Definition of a tool that can be used by the agent.
///
/// Each tool is registered in [`ToolSpec`] with a unique canonical name, a set of
/// typed parameters, and a [`RiskLevel`] that governs Quorum review requirements.
///
/// Tool definitions are created in the infrastructure layer (e.g. `file::read_file_definition()`,
/// `web::web_fetch_definition()`) and registered via [`ToolSpec::register`].
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{ToolDefinition, ToolParameter, RiskLevel};
///
/// let tool = ToolDefinition::new("web_fetch", "Fetch a web page", RiskLevel::Low)
///     .with_parameter(ToolParameter::new("url", "The URL to fetch", true).with_type("string"))
///     .with_parameter(ToolParameter::new("max_length", "Max output bytes", false).with_type("number"));
///
/// assert!(!tool.is_high_risk());
/// assert_eq!(tool.parameters.len(), 2);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Unique name of the tool (e.g., "read_file")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Risk level of this tool
    pub risk_level: RiskLevel,
    /// Parameter specifications
    pub parameters: Vec<ToolParameter>,
}

/// Parameter specification for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    /// Parameter name
    pub name: String,
    /// Parameter description
    pub description: String,
    /// Whether this parameter is required
    pub required: bool,
    /// Parameter type hint (e.g., "string", "path", "number")
    pub param_type: String,
}

impl ToolDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        risk_level: RiskLevel,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            risk_level,
            parameters: Vec::new(),
        }
    }

    pub fn with_parameter(mut self, param: ToolParameter) -> Self {
        self.parameters.push(param);
        self
    }

    pub fn is_high_risk(&self) -> bool {
        self.risk_level.requires_quorum()
    }
}

impl ToolParameter {
    pub fn new(name: impl Into<String>, description: impl Into<String>, required: bool) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            required,
            param_type: "string".to_string(),
        }
    }

    pub fn with_type(mut self, param_type: impl Into<String>) -> Self {
        self.param_type = param_type.into();
        self
    }
}

/// Registry of available tools.
///
/// `ToolSpec` stores [`ToolDefinition`]s keyed by canonical name. Tools are
/// passed to the LLM via the Native Tool Use API, which enforces valid tool
/// names — no alias resolution needed.
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{ToolSpec, ToolDefinition, RiskLevel};
///
/// let spec = ToolSpec::new()
///     .register(ToolDefinition::new("run_command", "Run a shell command", RiskLevel::High));
///
/// assert!(spec.get("run_command").is_some());
/// assert_eq!(spec.tool_count(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ToolSpec {
    tools: HashMap<String, ToolDefinition>,
}

impl ToolSpec {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool definition by its canonical name.
    ///
    /// This is the primary way to populate the tool registry. Tool definitions
    /// are typically created in the infrastructure layer and registered at startup.
    pub fn register(mut self, tool: ToolDefinition) -> Self {
        self.tools.insert(tool.name.clone(), tool);
        self
    }

    /// Get a [`ToolDefinition`] by canonical name.
    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    /// Iterate over all registered tool definitions.
    pub fn all(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values()
    }

    /// Iterate over all registered canonical tool names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(|s| s.as_str())
    }

    /// Iterate over tools that require Quorum review ([`RiskLevel::High`]).
    pub fn high_risk_tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().filter(|t| t.is_high_risk())
    }

    /// Iterate over tools that execute directly without review ([`RiskLevel::Low`]).
    pub fn low_risk_tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().filter(|t| !t.is_high_risk())
    }

    /// Get the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

/// A request to invoke a tool, extracted from an LLM response.
///
/// `ToolCall` is extracted from [`LlmResponse::tool_calls()`] via the
/// Native Tool Use API. The API guarantees valid tool names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call (canonical name, guaranteed valid by the API).
    pub tool_name: String,
    /// Arguments passed to the tool, validated against [`ToolDefinition::parameters`].
    pub arguments: HashMap<String, serde_json::Value>,
    /// Optional reasoning for why this tool is being called.
    ///
    /// Used for Quorum review context — reviewers can see *why* the agent
    /// chose this tool, improving consensus quality.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// API-assigned tool use ID for Native Tool Use.
    ///
    /// Set when the tool call originates from a Native API response
    /// (e.g. Anthropic `tool_use` content block). Used to correlate
    /// tool results back to the original request via `send_tool_results()`.
    ///
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
}

impl ToolCall {
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            arguments: HashMap::new(),
            reasoning: None,
            native_id: None,
        }
    }

    /// Create a tool call from a Native Tool Use API response.
    ///
    /// The `id` is the API-assigned identifier used to correlate tool results
    /// back to this request. The `name` is guaranteed valid by the API.
    pub fn from_native(
        id: impl Into<String>,
        name: impl Into<String>,
        input: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            tool_name: name.into(),
            arguments: input,
            reasoning: None,
            native_id: Some(id.into()),
        }
    }

    pub fn with_arg(mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) -> Self {
        self.arguments.insert(key.into(), value.into());
        self
    }

    pub fn with_reasoning(mut self, reasoning: impl Into<String>) -> Self {
        self.reasoning = Some(reasoning.into());
        self
    }

    /// Get a string argument
    pub fn get_string(&self, key: &str) -> Option<&str> {
        self.arguments.get(key).and_then(|v| v.as_str())
    }

    /// Get a required string argument or return an error message
    pub fn require_string(&self, key: &str) -> Result<&str, String> {
        self.get_string(key)
            .ok_or_else(|| format!("Missing required argument: {}", key))
    }

    /// Get an optional i64 argument
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.arguments.get(key).and_then(|v| v.as_i64())
    }

    /// Get an optional bool argument
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.arguments.get(key).and_then(|v| v.as_bool())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level() {
        assert!(!RiskLevel::Low.requires_quorum());
        assert!(RiskLevel::High.requires_quorum());
    }

    #[test]
    fn test_tool_definition() {
        let tool = ToolDefinition::new("read_file", "Read file contents", RiskLevel::Low)
            .with_parameter(
                ToolParameter::new("path", "File path to read", true).with_type("path"),
            );

        assert_eq!(tool.name, "read_file");
        assert!(!tool.is_high_risk());
        assert_eq!(tool.parameters.len(), 1);
        assert_eq!(tool.parameters[0].name, "path");
    }

    #[test]
    fn test_tool_spec() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new(
                "read_file",
                "Read file",
                RiskLevel::Low,
            ))
            .register(ToolDefinition::new(
                "write_file",
                "Write file",
                RiskLevel::High,
            ));

        assert!(spec.get("read_file").is_some());
        assert!(spec.get("write_file").is_some());
        assert!(spec.get("unknown").is_none());

        assert_eq!(spec.high_risk_tools().count(), 1);
        assert_eq!(spec.low_risk_tools().count(), 1);
    }

    #[test]
    fn test_tool_call() {
        let call = ToolCall::new("read_file")
            .with_arg("path", "/test/file.txt")
            .with_reasoning("Need to read the config");

        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.get_string("path"), Some("/test/file.txt"));
        assert_eq!(call.require_string("path").unwrap(), "/test/file.txt");
        assert!(call.require_string("missing").is_err());
        assert_eq!(call.native_id, None);
    }

    #[test]
    fn test_tool_call_from_native() {
        let input: HashMap<String, serde_json::Value> =
            [("path".to_string(), serde_json::json!("/src/main.rs"))]
                .into_iter()
                .collect();

        let call = ToolCall::from_native("toolu_abc123", "read_file", input);

        assert_eq!(call.tool_name, "read_file");
        assert_eq!(call.native_id, Some("toolu_abc123".to_string()));
        assert_eq!(call.get_string("path"), Some("/src/main.rs"));
        assert_eq!(call.reasoning, None);
    }

    #[test]
    fn test_tool_count() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("a", "Tool A", RiskLevel::Low))
            .register(ToolDefinition::new("b", "Tool B", RiskLevel::High));
        assert_eq!(spec.tool_count(), 2);
    }
}
