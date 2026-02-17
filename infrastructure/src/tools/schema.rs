//! JSON Schema tool converter.
//!
//! Default implementation of [`ToolSchemaPort`] that produces provider-neutral
//! JSON Schema for the Native Tool Use API.

use quorum_application::ports::tool_schema::ToolSchemaPort;
use quorum_domain::tool::entities::{ToolDefinition, ToolSpec};

/// Default implementation producing provider-neutral JSON Schema.
///
/// Handles param_type → JSON Schema type mapping:
/// - `"string"`, `"path"` → `"string"`
/// - `"number"` → `"number"`
/// - `"integer"` → `"integer"`
/// - `"boolean"` → `"boolean"`
/// - anything else → `"string"`
pub struct JsonSchemaToolConverter;

impl ToolSchemaPort for JsonSchemaToolConverter {
    fn tool_to_schema(&self, tool: &ToolDefinition) -> serde_json::Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &tool.parameters {
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
            "name": tool.name,
            "description": tool.description,
            "input_schema": {
                "type": "object",
                "properties": properties,
                "required": required,
            }
        })
    }

    fn all_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value> {
        let mut tools: Vec<&ToolDefinition> = spec.all().collect();
        tools.sort_by_key(|t| &t.name);
        tools.into_iter().map(|t| self.tool_to_schema(t)).collect()
    }

    fn low_risk_tools_schema(&self, spec: &ToolSpec) -> Vec<serde_json::Value> {
        let mut tools: Vec<&ToolDefinition> = spec.low_risk_tools().collect();
        tools.sort_by_key(|t| &t.name);
        tools.into_iter().map(|t| self.tool_to_schema(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quorum_domain::tool::entities::{RiskLevel, ToolParameter};

    #[test]
    fn test_tool_to_schema() {
        let converter = JsonSchemaToolConverter;
        let tool = ToolDefinition::new("read_file", "Read file contents", RiskLevel::Low)
            .with_parameter(ToolParameter::new("path", "File path to read", true).with_type("path"))
            .with_parameter(
                ToolParameter::new("max_lines", "Max lines to read", false).with_type("integer"),
            );

        let schema = converter.tool_to_schema(&tool);

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
    fn test_all_tools_schema() {
        let converter = JsonSchemaToolConverter;
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

        let tools = converter.all_tools_schema(&spec);
        assert_eq!(tools.len(), 2);

        // Results are sorted by name
        assert_eq!(tools[0]["name"], "read_file");
        assert_eq!(tools[1]["name"], "write_file");

        // Check that all tools have the required fields
        for tool in &tools {
            assert!(tool["name"].is_string());
            assert!(tool["description"].is_string());
            assert!(tool["input_schema"]["type"].as_str() == Some("object"));
        }
    }

    #[test]
    fn test_low_risk_tools_schema() {
        let converter = JsonSchemaToolConverter;
        let spec = ToolSpec::new()
            .register(
                ToolDefinition::new("read_file", "Read file", RiskLevel::Low)
                    .with_parameter(ToolParameter::new("path", "File path", true)),
            )
            .register(ToolDefinition::new(
                "write_file",
                "Write file",
                RiskLevel::High,
            ))
            .register(ToolDefinition::new("grep_search", "Search", RiskLevel::Low));

        let low_risk_tools = converter.low_risk_tools_schema(&spec);
        assert_eq!(low_risk_tools.len(), 2);

        // Sorted by name
        assert_eq!(low_risk_tools[0]["name"], "grep_search");
        assert_eq!(low_risk_tools[1]["name"], "read_file");

        // High-risk tool excluded
        let names: Vec<&str> = low_risk_tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(!names.contains(&"write_file"));
    }
}
