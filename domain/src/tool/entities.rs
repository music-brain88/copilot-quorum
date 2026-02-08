//! Tool domain entities

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Risk level of a tool operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// Low risk - read-only operations (e.g., read_file, glob, grep)
    Low,
    /// High risk - operations that modify state (e.g., write_file, run_command)
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

/// Definition of a tool that can be used by the agent
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

/// Specification of available tools for the agent
#[derive(Debug, Clone, Default)]
pub struct ToolSpec {
    tools: HashMap<String, ToolDefinition>,
    /// Alias → canonical name mapping (e.g. "bash" → "run_command")
    aliases: HashMap<String, String>,
}

impl ToolSpec {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    pub fn register(mut self, tool: ToolDefinition) -> Self {
        self.tools.insert(tool.name.clone(), tool);
        self
    }

    /// Register a single alias mapping (builder pattern)
    pub fn register_alias(mut self, alias: impl Into<String>, canonical: impl Into<String>) -> Self {
        self.aliases.insert(alias.into(), canonical.into());
        self
    }

    /// Register multiple aliases at once (builder pattern)
    pub fn register_aliases(mut self, mappings: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>) -> Self {
        for (alias, canonical) in mappings {
            self.aliases.insert(alias.into(), canonical.into());
        }
        self
    }

    /// Resolve an alias to its canonical name (aliases only, not canonical names)
    pub fn resolve_alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    /// Resolve a name: returns canonical name if it's a registered tool,
    /// or resolves alias, or None if unknown
    pub fn resolve<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if self.tools.contains_key(name) {
            Some(name)
        } else {
            self.resolve_alias(name)
        }
    }

    /// Get tool definition by canonical name or alias
    pub fn get_resolved(&self, name: &str) -> Option<&ToolDefinition> {
        self.resolve(name).and_then(|canonical| self.tools.get(canonical))
    }

    pub fn get(&self, name: &str) -> Option<&ToolDefinition> {
        self.tools.get(name)
    }

    pub fn all(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(|s| s.as_str())
    }

    pub fn high_risk_tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().filter(|t| t.is_high_risk())
    }

    pub fn low_risk_tools(&self) -> impl Iterator<Item = &ToolDefinition> {
        self.tools.values().filter(|t| !t.is_high_risk())
    }
}

/// A call to a tool with arguments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call
    pub tool_name: String,
    /// Arguments passed to the tool
    pub arguments: HashMap<String, serde_json::Value>,
    /// Optional reasoning for why this tool is being called
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl ToolCall {
    pub fn new(tool_name: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            arguments: HashMap::new(),
            reasoning: None,
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
    fn test_tool_spec_aliases() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("run_command", "Run command", RiskLevel::High))
            .register(ToolDefinition::new("read_file", "Read file", RiskLevel::Low))
            .register_alias("bash", "run_command")
            .register_alias("shell", "run_command")
            .register_alias("view", "read_file");

        // resolve_alias only resolves aliases, not canonical names
        assert_eq!(spec.resolve_alias("bash"), Some("run_command"));
        assert_eq!(spec.resolve_alias("shell"), Some("run_command"));
        assert_eq!(spec.resolve_alias("view"), Some("read_file"));
        assert_eq!(spec.resolve_alias("run_command"), None);
        assert_eq!(spec.resolve_alias("unknown"), None);

        // resolve returns canonical for both registered tools and aliases
        assert_eq!(spec.resolve("run_command"), Some("run_command"));
        assert_eq!(spec.resolve("bash"), Some("run_command"));
        assert_eq!(spec.resolve("read_file"), Some("read_file"));
        assert_eq!(spec.resolve("view"), Some("read_file"));
        assert_eq!(spec.resolve("unknown"), None);

        // get_resolved returns tool definition via alias
        assert_eq!(spec.get_resolved("bash").unwrap().name, "run_command");
        assert_eq!(spec.get_resolved("run_command").unwrap().name, "run_command");
        assert!(spec.get_resolved("unknown").is_none());
    }

    #[test]
    fn test_tool_spec_register_aliases_batch() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("grep_search", "Grep", RiskLevel::Low))
            .register_aliases([
                ("grep", "grep_search"),
                ("rg", "grep_search"),
                ("search", "grep_search"),
            ]);

        assert_eq!(spec.resolve("grep"), Some("grep_search"));
        assert_eq!(spec.resolve("rg"), Some("grep_search"));
        assert_eq!(spec.resolve("search"), Some("grep_search"));
    }

    #[test]
    fn test_canonical_name_takes_priority_over_alias() {
        // If a canonical name and alias collide, canonical wins in resolve()
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("read_file", "Read file", RiskLevel::Low))
            .register(ToolDefinition::new("view", "View tool", RiskLevel::Low))
            .register_alias("view", "read_file"); // alias points to read_file, but "view" is also a tool

        // resolve should return "view" as canonical since it's a registered tool
        assert_eq!(spec.resolve("view"), Some("view"));
        // get_resolved should return the "view" tool, not "read_file"
        assert_eq!(spec.get_resolved("view").unwrap().name, "view");
    }

    #[test]
    fn test_get_is_not_affected_by_aliases() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("run_command", "Run", RiskLevel::High))
            .register_alias("bash", "run_command");

        // get() is exact match only - aliases don't work
        assert!(spec.get("run_command").is_some());
        assert!(spec.get("bash").is_none());
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
    }
}
