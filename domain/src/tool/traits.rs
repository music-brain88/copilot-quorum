//! Tool domain traits
//!
//! Contains pure domain logic traits for tool validation.
//! The async ToolExecutorPort is defined in the application layer (ports).

use super::entities::{ToolCall, ToolDefinition};

/// Validator for tool calls
///
/// This is a pure domain trait that validates tool calls
/// against their definitions without any I/O operations.
pub trait ToolValidator {
    /// Validate a tool call against its definition
    fn validate(&self, call: &ToolCall, definition: &ToolDefinition) -> Result<(), String>;
}

/// Default implementation of ToolValidator
#[derive(Debug, Clone, Default)]
pub struct DefaultToolValidator;

impl ToolValidator for DefaultToolValidator {
    fn validate(&self, call: &ToolCall, definition: &ToolDefinition) -> Result<(), String> {
        // Check that all required parameters are present
        for param in &definition.parameters {
            if param.required && !call.arguments.contains_key(&param.name) {
                return Err(format!(
                    "Missing required parameter '{}' for tool '{}'",
                    param.name, definition.name
                ));
            }
        }

        // Check that all provided arguments are valid parameters
        let valid_params: std::collections::HashSet<&str> =
            definition.parameters.iter().map(|p| p.name.as_str()).collect();

        for arg_name in call.arguments.keys() {
            if !valid_params.contains(arg_name.as_str()) {
                return Err(format!(
                    "Unknown parameter '{}' for tool '{}'",
                    arg_name, definition.name
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::entities::{RiskLevel, ToolParameter};

    #[test]
    fn test_validator_missing_required() {
        let validator = DefaultToolValidator;
        let definition = ToolDefinition::new("test", "test tool", RiskLevel::Low)
            .with_parameter(ToolParameter::new("required_param", "A required param", true));

        let call = ToolCall::new("test");
        let result = validator.validate(&call, &definition);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Missing required parameter"));
    }

    #[test]
    fn test_validator_unknown_param() {
        let validator = DefaultToolValidator;
        let definition = ToolDefinition::new("test", "test tool", RiskLevel::Low)
            .with_parameter(ToolParameter::new("known_param", "A known param", false));

        let call = ToolCall::new("test").with_arg("unknown_param", "value");
        let result = validator.validate(&call, &definition);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown parameter"));
    }

    #[test]
    fn test_validator_valid_call() {
        let validator = DefaultToolValidator;
        let definition = ToolDefinition::new("test", "test tool", RiskLevel::Low)
            .with_parameter(ToolParameter::new("param1", "First param", true))
            .with_parameter(ToolParameter::new("param2", "Second param", false));

        let call = ToolCall::new("test")
            .with_arg("param1", "value1")
            .with_arg("param2", "value2");

        let result = validator.validate(&call, &definition);
        assert!(result.is_ok());
    }
}
