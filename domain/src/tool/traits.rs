//! Tool domain traits — pure validation logic
//!
//! This module provides the **Tool Validation** component of the Tool System.
//! [`ToolValidator`] ensures that every [`ToolCall`] conforms to
//! its [`ToolDefinition`] before reaching the executor.
//!
//! Validation runs in the infrastructure layer's `LocalToolExecutor` after
//! alias resolution but before actual execution, catching parameter errors
//! early and enabling the retry mechanism.
//!
//! The async `ToolExecutorPort` is defined in the application layer (`ports`)
//! to keep this module free of I/O dependencies.

use super::entities::{ToolCall, ToolDefinition};

/// Pure domain trait for validating [`ToolCall`]s against their [`ToolDefinition`]s.
///
/// Validation checks that:
/// - All **required** parameters are present
/// - No **unknown** parameters are supplied
///
/// This trait is part of the **Tool System's** safety pipeline:
///
/// ```text
/// resolve_tool_call() → ToolValidator::validate() → execute()
/// ```
///
/// Validation errors produce `INVALID_ARGUMENT` errors, which are **retryable** —
/// the agent gets a second chance to fix its tool call.
pub trait ToolValidator {
    /// Validate a tool call against its definition.
    ///
    /// Returns `Ok(())` if valid, or `Err(message)` describing the validation failure.
    fn validate(&self, call: &ToolCall, definition: &ToolDefinition) -> Result<(), String>;
}

/// Default implementation of [`ToolValidator`] with required-parameter and
/// unknown-parameter checks.
///
/// Used by `LocalToolExecutor` in the infrastructure layer for all tool
/// invocations (file, command, search, and web tools).
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
        let valid_params: std::collections::HashSet<&str> = definition
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect();

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
        let definition = ToolDefinition::new("test", "test tool", RiskLevel::Low).with_parameter(
            ToolParameter::new("required_param", "A required param", true),
        );

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
