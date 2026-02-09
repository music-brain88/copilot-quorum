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

    /// Convert this tool definition to a provider-neutral JSON Schema object.
    ///
    /// Returns a JSON object with `name`, `description`, and `input_schema` fields.
    /// This is the intermediate format used by [`ToolSpec::to_api_tools()`] before
    /// provider-specific wrapping (e.g., OpenAI's `{"type": "function", "function": {...}}`).
    ///
    /// # JSON Schema Type Mapping
    ///
    /// | `param_type` | JSON Schema `type` |
    /// |-------------|-------------------|
    /// | `"string"`, `"path"` | `"string"` |
    /// | `"number"` | `"number"` |
    /// | `"integer"` | `"integer"` |
    /// | `"boolean"` | `"boolean"` |
    /// | anything else | `"string"` |
    pub fn to_json_schema(&self) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.parameters {
            let schema_type = match param.param_type.as_str() {
                "string" | "path" => "string",
                "number" => "number",
                "integer" => "integer",
                "boolean" => "boolean",
                _ => "string",
            };

            let mut prop = serde_json::Map::new();
            prop.insert("type".to_string(), serde_json::json!(schema_type));
            prop.insert(
                "description".to_string(),
                serde_json::json!(param.description),
            );
            properties.insert(param.name.clone(), serde_json::Value::Object(prop));

            if param.required {
                required.push(serde_json::json!(param.name));
            }
        }

        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        })
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

/// Registry of available tools and their name aliases.
///
/// `ToolSpec` is the central **Tool Name Alias System** data structure, serving two roles:
///
/// 1. **Tool registry** — stores [`ToolDefinition`]s keyed by canonical name
/// 2. **Alias map** — maps common LLM-hallucinated names to canonical names
///
/// # Alias Resolution
///
/// LLMs frequently emit incorrect tool names (`bash`, `grep`, `view`, etc.).
/// The alias system provides a three-tier resolution strategy:
///
/// ```text
/// resolve_tool_call() in application layer:
///   1. has_tool(exact)      → use as-is        (zero cost)
///   2. resolve_alias(name)  → canonical name    (zero cost, no LLM call)
///   3. LLM retry            → ask model to fix  (1 API round-trip, fallback)
/// ```
///
/// The alias map is also used at **plan time** by `resolve_plan_aliases()` to
/// correct tool names in [`Plan`](crate::agent::Plan) tasks before execution begins.
///
/// # Design: `get()` vs `get_resolved()`
///
/// | Method | Alias-aware? | Use case |
/// |--------|-------------|----------|
/// | [`get()`](Self::get) | No (exact match) | Executor routing (`match call.tool_name`) |
/// | [`get_resolved()`](Self::get_resolved) | Yes | Validation, display, lookups |
///
/// `get()` intentionally ignores aliases so that the executor's `match` dispatch
/// always operates on canonical names. Alias resolution happens once upstream
/// (in `resolve_tool_call`), not at every access point.
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{ToolSpec, ToolDefinition, RiskLevel};
///
/// let spec = ToolSpec::new()
///     .register(ToolDefinition::new("run_command", "Run a shell command", RiskLevel::High))
///     .register_alias("bash", "run_command")
///     .register_alias("shell", "run_command");
///
/// // Exact match works
/// assert!(spec.get("run_command").is_some());
///
/// // Alias resolves to canonical name
/// assert_eq!(spec.resolve("bash"), Some("run_command"));
///
/// // get_resolved works with both canonical and alias names
/// assert_eq!(spec.get_resolved("shell").unwrap().name, "run_command");
/// ```
#[derive(Debug, Clone, Default)]
pub struct ToolSpec {
    tools: HashMap<String, ToolDefinition>,
    /// Alias → canonical name mapping (e.g. `"bash"` → `"run_command"`).
    ///
    /// Populated by [`register_alias`](Self::register_alias) and
    /// [`register_aliases`](Self::register_aliases). Queried by
    /// [`resolve_alias`](Self::resolve_alias) and [`resolve`](Self::resolve).
    aliases: HashMap<String, String>,
}

impl ToolSpec {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            aliases: HashMap::new(),
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

    /// Register a single alias mapping (builder pattern).
    ///
    /// Maps an `alias` name to a `canonical` tool name, enabling the
    /// **Tool Name Alias System** to resolve LLM-hallucinated names.
    ///
    /// If the alias collides with a registered canonical name, the canonical
    /// name takes priority in [`resolve()`](Self::resolve).
    pub fn register_alias(
        mut self,
        alias: impl Into<String>,
        canonical: impl Into<String>,
    ) -> Self {
        self.aliases.insert(alias.into(), canonical.into());
        self
    }

    /// Register multiple aliases at once (builder pattern).
    ///
    /// Convenience method for batch registration. Each `(alias, canonical)` pair
    /// is equivalent to calling [`register_alias`](Self::register_alias).
    ///
    /// # Example
    ///
    /// ```
    /// # use quorum_domain::tool::entities::{ToolSpec, ToolDefinition, RiskLevel};
    /// let spec = ToolSpec::new()
    ///     .register(ToolDefinition::new("grep_search", "Search files", RiskLevel::Low))
    ///     .register_aliases([
    ///         ("grep", "grep_search"),
    ///         ("rg", "grep_search"),
    ///         ("ripgrep", "grep_search"),
    ///     ]);
    /// ```
    pub fn register_aliases(
        mut self,
        mappings: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        for (alias, canonical) in mappings {
            self.aliases.insert(alias.into(), canonical.into());
        }
        self
    }

    /// Resolve an alias to its canonical tool name.
    ///
    /// Returns `Some(canonical_name)` only if `name` is a registered alias.
    /// Returns `None` for canonical names and unknown names alike.
    ///
    /// This is the fast-path used by `resolve_tool_call()` in the application layer
    /// to correct LLM-hallucinated tool names without an LLM API call.
    pub fn resolve_alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    /// Resolve any tool name — canonical or alias — to a canonical name.
    ///
    /// Resolution priority:
    /// 1. If `name` is a registered canonical tool name → returns `name` as-is
    /// 2. If `name` is a registered alias → returns the alias target
    /// 3. Otherwise → `None`
    ///
    /// Canonical names always take priority over aliases, so if a name is both
    /// a registered tool and an alias, the tool's own identity wins.
    pub fn resolve<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        if self.tools.contains_key(name) {
            Some(name)
        } else {
            self.resolve_alias(name)
        }
    }

    /// Get a [`ToolDefinition`] by canonical name **or** alias.
    ///
    /// Unlike [`get()`](Self::get) which requires an exact canonical match,
    /// this method first resolves the name through the alias system.
    /// Used for validation and display contexts where alias-awareness is desired.
    pub fn get_resolved(&self, name: &str) -> Option<&ToolDefinition> {
        self.resolve(name)
            .and_then(|canonical| self.tools.get(canonical))
    }

    /// Get a [`ToolDefinition`] by **exact** canonical name only.
    ///
    /// Does **not** resolve aliases — this is intentional. The executor's routing
    /// logic (`match call.tool_name.as_str()`) relies on exact canonical names.
    /// Use [`get_resolved()`](Self::get_resolved) when alias resolution is needed.
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

    /// Convert all registered tools to a provider-neutral JSON Schema array.
    ///
    /// Each element has `{"name", "description", "input_schema"}` format.
    /// Provider-specific wrapping (e.g., OpenAI's `{"type": "function", "function": {...}}`)
    /// is done in the infrastructure layer.
    ///
    /// This is the bridge between `ToolSpec` and the Native Tool Use API:
    /// ```text
    /// ToolSpec → to_api_tools() → Vec<Value> → LlmSession::send_with_tools()
    /// ```
    pub fn to_api_tools(&self) -> Vec<serde_json::Value> {
        self.tools.values().map(|t| t.to_json_schema()).collect()
    }

    /// Get the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

/// A request to invoke a tool, parsed from an LLM response.
///
/// `ToolCall` is produced either by the response parser (`parse_tool_calls()`)
/// in the prompt-based path, or extracted directly from [`LlmResponse::tool_calls()`]
/// in the Native Tool Use path.
///
/// ```text
/// PromptBased:    LLM response → parse_tool_calls() → resolve_tool_call() → execute()
/// Native API:     LlmResponse → tool_calls() → execute()  (no parsing needed)
/// ```
///
/// The `tool_name` field may initially contain an aliased name (e.g. `"bash"`)
/// in the prompt-based path. The **Tool Name Alias System** in `resolve_tool_call()`
/// rewrites it to the canonical name (e.g. `"run_command"`) before execution.
/// In the Native path, the API guarantees correct tool names.
///
/// # Supported LLM Response Formats (Prompt-Based)
///
/// Tool calls are extracted from LLM responses in these formats (highest priority first):
/// 1. `` ```tool `` fenced blocks
/// 2. `` ```json `` fenced blocks
/// 3. Raw JSON (entire response)
/// 4. Embedded JSON (heuristic fallback)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Name of the tool to call.
    ///
    /// May be an alias (e.g. `"bash"`) before resolution; will be rewritten
    /// to the canonical name (e.g. `"run_command"`) by the alias system.
    /// In Native mode, the API guarantees this is a valid canonical name.
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
    /// `None` for prompt-based tool calls.
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
    fn test_tool_spec_aliases() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new(
                "run_command",
                "Run command",
                RiskLevel::High,
            ))
            .register(ToolDefinition::new(
                "read_file",
                "Read file",
                RiskLevel::Low,
            ))
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
        assert_eq!(
            spec.get_resolved("run_command").unwrap().name,
            "run_command"
        );
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
            .register(ToolDefinition::new(
                "read_file",
                "Read file",
                RiskLevel::Low,
            ))
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
    fn test_to_json_schema() {
        let tool = ToolDefinition::new("read_file", "Read file contents", RiskLevel::Low)
            .with_parameter(ToolParameter::new("path", "File path to read", true).with_type("path"))
            .with_parameter(
                ToolParameter::new("max_lines", "Max lines to read", false).with_type("integer"),
            );

        let schema = tool.to_json_schema();

        assert_eq!(schema["name"], "read_file");
        assert_eq!(schema["description"], "Read file contents");
        assert_eq!(schema["input_schema"]["type"], "object");

        // Check path parameter
        let path_prop = &schema["input_schema"]["properties"]["path"];
        assert_eq!(path_prop["type"], "string"); // "path" maps to "string"
        assert_eq!(path_prop["description"], "File path to read");

        // Check max_lines parameter
        let lines_prop = &schema["input_schema"]["properties"]["max_lines"];
        assert_eq!(lines_prop["type"], "integer");

        // Check required
        let required = schema["input_schema"]["required"].as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "path");
    }

    #[test]
    fn test_to_api_tools() {
        let spec = ToolSpec::new()
            .register(
                ToolDefinition::new("read_file", "Read file", RiskLevel::Low)
                    .with_parameter(ToolParameter::new("path", "File path", true)),
            )
            .register(ToolDefinition::new(
                "write_file",
                "Write file",
                RiskLevel::High,
            ));

        let tools = spec.to_api_tools();
        assert_eq!(tools.len(), 2);

        // Check that all tools have the required fields
        for tool in &tools {
            assert!(tool["name"].is_string());
            assert!(tool["description"].is_string());
            assert!(tool["input_schema"]["type"].as_str() == Some("object"));
        }
    }

    #[test]
    fn test_tool_count() {
        let spec = ToolSpec::new()
            .register(ToolDefinition::new("a", "Tool A", RiskLevel::Low))
            .register(ToolDefinition::new("b", "Tool B", RiskLevel::High));
        assert_eq!(spec.tool_count(), 2);
    }
}
