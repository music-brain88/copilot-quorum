//! Custom tool provider — user-defined CLI commands as tools.
//!
//! Reads custom tool definitions from `quorum.toml` and exposes them as
//! first-class tools via the [`ToolProvider`] trait. Each custom tool wraps
//! a shell command template with `{param_name}` placeholders.
//!
//! # Security
//!
//! All parameter values are shell-escaped before substitution to prevent
//! command injection: single-quote wrapping on Unix, double-quote wrapping
//! with character escaping on Windows.
//!
//! # Example Configuration
//!
//! ```toml
//! [tools.custom.gh_create_issue]
//! description = "Create a GitHub issue"
//! command = "gh issue create --title {title} --body {body}"
//! risk_level = "high"
//!
//! [tools.custom.gh_create_issue.parameters.title]
//! type = "string"
//! description = "Issue title"
//! required = true
//! ```

use async_trait::async_trait;
use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    provider::{ProviderError, ToolProvider},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

use crate::config::{FileCustomToolConfig, FileCustomToolParameter};

/// Priority for the custom tool provider (between CLI and MCP)
pub const CUSTOM_PRIORITY: i32 = 75;

/// Maximum output size (1 MB)
const MAX_OUTPUT_SIZE: usize = 1024 * 1024;

/// A custom tool definition with its command template.
#[derive(Debug, Clone)]
struct CustomTool {
    /// Domain tool definition (name, description, risk, parameters)
    definition: ToolDefinition,
    /// Command template with `{param_name}` placeholders
    command_template: String,
}

/// Provider for user-defined custom tools from `quorum.toml`.
///
/// Custom tools are shell commands with typed parameters. The provider
/// handles parameter substitution (with escaping) and execution.
#[derive(Debug, Clone)]
pub struct CustomToolProvider {
    tools: HashMap<String, CustomTool>,
    working_dir: Option<String>,
}

impl CustomToolProvider {
    /// Create a new custom tool provider from config entries.
    ///
    /// Each entry in the map is a tool name → config pair from
    /// `[tools.custom.<name>]` in `quorum.toml`.
    pub fn from_config(configs: &HashMap<String, FileCustomToolConfig>) -> Self {
        let mut tools = HashMap::new();

        for (name, config) in configs {
            let risk_level = match config.risk_level.to_lowercase().as_str() {
                "low" => RiskLevel::Low,
                _ => RiskLevel::High, // Default to high (safe side)
            };

            let mut definition =
                ToolDefinition::new(name.as_str(), config.description.as_str(), risk_level);

            // Sort parameters by name for deterministic ordering
            let mut params: Vec<(&String, &FileCustomToolParameter)> =
                config.parameters.iter().collect();
            params.sort_by_key(|(pname, _)| pname.as_str());

            for (param_name, param_config) in params {
                definition = definition.with_parameter(
                    ToolParameter::new(
                        param_name.as_str(),
                        param_config.description.as_str(),
                        param_config.required,
                    )
                    .with_type(param_config.param_type.as_str()),
                );
            }

            tools.insert(
                name.clone(),
                CustomTool {
                    definition,
                    command_template: config.command.clone(),
                },
            );
        }

        Self {
            tools,
            working_dir: None,
        }
    }

    /// Set the working directory for command execution.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Build the final command string by substituting parameters.
    ///
    /// `{param_name}` placeholders are replaced with shell-escaped argument values.
    /// Missing optional parameters are replaced with empty strings.
    fn build_command(&self, template: &str, call: &ToolCall) -> String {
        let mut command = template.to_string();

        // Replace all {param_name} placeholders
        for (key, value) in &call.arguments {
            let placeholder = format!("{{{}}}", key);
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let escaped = shell_escape(&value_str);
            command = command.replace(&placeholder, &escaped);
        }

        // Remove any remaining unreplaced placeholders (optional params not provided)
        // Simple approach: find {word} patterns and replace with empty string
        let mut result = String::with_capacity(command.len());
        let mut chars = command.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '{' {
                // Check if this looks like a placeholder
                let mut placeholder = String::new();
                let mut found_close = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        found_close = true;
                        break;
                    }
                    placeholder.push(c);
                }
                if !found_close {
                    // Unclosed brace — restore original characters
                    result.push('{');
                    result.push_str(&placeholder);
                } else {
                    let is_placeholder = !placeholder.is_empty()
                        && placeholder.chars().all(|c| c.is_alphanumeric() || c == '_');
                    if !is_placeholder {
                        // Not a valid placeholder, keep original
                        result.push('{');
                        result.push_str(&placeholder);
                        result.push('}');
                    }
                    // Valid placeholder with no value → omit (empty string)
                }
            } else {
                result.push(ch);
            }
        }

        result
    }

    /// Execute a custom tool command.
    fn execute_command(&self, tool_name: &str, command_str: &str) -> ToolResult {
        let start = Instant::now();

        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", command_str]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", command_str]);
            c
        };

        // Set working directory if specified
        if let Some(dir) = &self.working_dir {
            let path = std::path::Path::new(dir);
            if path.exists() && path.is_dir() {
                cmd.current_dir(path);
            }
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let output = match cmd.output() {
            Ok(output) => output,
            Err(e) => {
                return ToolResult::failure(
                    tool_name,
                    ToolError::execution_failed(format!("Failed to execute command: {}", e)),
                );
            }
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result_text = String::new();
        if !stdout.is_empty() {
            let truncated = if stdout.len() > MAX_OUTPUT_SIZE {
                &stdout[..MAX_OUTPUT_SIZE]
            } else {
                &stdout
            };
            result_text.push_str(truncated);
        }
        if !stderr.is_empty() {
            if !result_text.is_empty() {
                result_text.push_str("\n--- stderr ---\n");
            }
            let truncated = if stderr.len() > MAX_OUTPUT_SIZE {
                &stderr[..MAX_OUTPUT_SIZE]
            } else {
                &stderr
            };
            result_text.push_str(truncated);
        }

        if result_text.is_empty() {
            result_text = if output.status.success() {
                "Command completed successfully (no output)".to_string()
            } else {
                format!("Command failed with exit code: {:?}", output.status.code())
            };
        }

        let metadata = ToolResultMetadata {
            duration_ms: Some(duration_ms),
            bytes: Some(result_text.len()),
            exit_code: output.status.code(),
            ..Default::default()
        };

        if output.status.success() {
            ToolResult::success(tool_name, result_text).with_metadata(metadata)
        } else {
            ToolResult::failure(tool_name, ToolError::execution_failed(result_text))
                .with_metadata(metadata)
        }
    }
}

/// Escape a string for safe shell substitution.
///
/// Uses OS-appropriate escaping:
/// - **Unix**: Single-quote wrapping (`hello 'world'` → `'hello '\''world'\'''`)
/// - **Windows**: Double-quote wrapping with `"` → `\"`, `%` → `%%`, `!` → `^!`
fn shell_escape(s: &str) -> String {
    // If the string contains no special characters, return as-is
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '/')
    {
        return s.to_string();
    }

    if cfg!(target_os = "windows") {
        shell_escape_windows(s)
    } else {
        shell_escape_unix(s)
    }
}

/// Unix shell escape: wrap in single quotes, escape internal single quotes.
fn shell_escape_unix(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 4);
    escaped.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(ch);
        }
    }
    escaped.push('\'');
    escaped
}

/// Windows cmd.exe escape: wrap in double quotes, escape `"`, `%`, and `!`.
fn shell_escape_windows(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len() + 4);
    escaped.push('"');
    for ch in s.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '%' => escaped.push_str("%%"),
            '!' => escaped.push_str("^!"),
            _ => escaped.push(ch),
        }
    }
    escaped.push('"');
    escaped
}

#[async_trait]
impl ToolProvider for CustomToolProvider {
    fn id(&self) -> &str {
        "custom"
    }

    fn display_name(&self) -> &str {
        "Custom Tools"
    }

    fn priority(&self) -> i32 {
        CUSTOM_PRIORITY
    }

    async fn is_available(&self) -> bool {
        !self.tools.is_empty()
    }

    async fn discover_tools(&self) -> Result<Vec<ToolDefinition>, ProviderError> {
        Ok(self.tools.values().map(|t| t.definition.clone()).collect())
    }

    async fn execute(&self, call: &ToolCall) -> ToolResult {
        let tool = match self.tools.get(&call.tool_name) {
            Some(t) => t,
            None => {
                return ToolResult::failure(
                    &call.tool_name,
                    ToolError::not_found(format!("Custom tool not found: {}", call.tool_name)),
                );
            }
        };

        // Validate required parameters
        for param in &tool.definition.parameters {
            if param.required && call.get_string(&param.name).is_none() {
                // Check non-string types too
                if !call.arguments.contains_key(&param.name) {
                    return ToolResult::failure(
                        &call.tool_name,
                        ToolError::invalid_argument(format!(
                            "Missing required parameter: {}",
                            param.name
                        )),
                    );
                }
            }
        }

        let command_str = self.build_command(&tool.command_template, call);
        self.execute_command(&call.tool_name, &command_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FileCustomToolParameter;

    fn make_config(
        description: &str,
        command: &str,
        risk_level: &str,
        params: Vec<(&str, &str, bool)>,
    ) -> FileCustomToolConfig {
        let mut parameters = HashMap::new();
        for (name, desc, required) in params {
            parameters.insert(
                name.to_string(),
                FileCustomToolParameter {
                    param_type: "string".to_string(),
                    description: desc.to_string(),
                    required,
                },
            );
        }
        FileCustomToolConfig {
            description: description.to_string(),
            command: command.to_string(),
            risk_level: risk_level.to_string(),
            parameters,
        }
    }

    #[test]
    fn test_build_command_unclosed_brace() {
        let mut configs = HashMap::new();
        configs.insert(
            "tool".to_string(),
            make_config("Tool", "echo {msg", "low", vec![("msg", "Msg", true)]),
        );
        let provider = CustomToolProvider::from_config(&configs);

        // The {msg never closes, so it should be preserved as-is
        let call = ToolCall::new("tool").with_arg("msg", "hello");
        let cmd = provider.build_command("echo {msg", &call);
        assert_eq!(cmd, "echo {msg");
    }

    #[test]
    fn test_shell_escape_windows() {
        assert_eq!(shell_escape_windows("hello world"), "\"hello world\"");
        assert_eq!(shell_escape_windows("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(shell_escape_windows("100%"), "\"100%%\"");
        assert_eq!(shell_escape_windows("wow!"), "\"wow^!\"");
    }

    #[test]
    fn test_shell_escape_safe_string() {
        assert_eq!(shell_escape("hello"), "hello");
        assert_eq!(shell_escape("my-file.txt"), "my-file.txt");
        assert_eq!(shell_escape("/path/to/file"), "/path/to/file");
    }

    #[test]
    fn test_shell_escape_special_chars() {
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
        assert_eq!(shell_escape("$(rm -rf /)"), "'$(rm -rf /)'");
        assert_eq!(shell_escape("a; rm -rf /"), "'a; rm -rf /'");
        assert_eq!(shell_escape("foo`bar`"), "'foo`bar`'");
    }

    #[test]
    fn test_from_config() {
        let mut configs = HashMap::new();
        configs.insert(
            "my_tool".to_string(),
            make_config(
                "A test tool",
                "echo {message}",
                "low",
                vec![("message", "The message", true)],
            ),
        );

        let provider = CustomToolProvider::from_config(&configs);
        assert_eq!(provider.tools.len(), 1);

        let tool = provider.tools.get("my_tool").unwrap();
        assert_eq!(tool.definition.name, "my_tool");
        assert_eq!(tool.definition.description, "A test tool");
        assert_eq!(tool.definition.risk_level, RiskLevel::Low);
        assert_eq!(tool.definition.parameters.len(), 1);
        assert_eq!(tool.command_template, "echo {message}");
    }

    #[test]
    fn test_from_config_default_high_risk() {
        let mut configs = HashMap::new();
        configs.insert(
            "risky".to_string(),
            make_config(
                "Risky tool",
                "rm {path}",
                "high",
                vec![("path", "Path", true)],
            ),
        );

        let provider = CustomToolProvider::from_config(&configs);
        let tool = provider.tools.get("risky").unwrap();
        assert_eq!(tool.definition.risk_level, RiskLevel::High);
    }

    #[test]
    fn test_build_command_simple() {
        let mut configs = HashMap::new();
        configs.insert(
            "echo_tool".to_string(),
            make_config(
                "Echo",
                "echo {message}",
                "low",
                vec![("message", "Msg", true)],
            ),
        );
        let provider = CustomToolProvider::from_config(&configs);

        let call = ToolCall::new("echo_tool").with_arg("message", "hello");
        let cmd = provider.build_command("echo {message}", &call);
        assert_eq!(cmd, "echo hello");
    }

    #[test]
    fn test_build_command_escaping() {
        let mut configs = HashMap::new();
        configs.insert(
            "echo_tool".to_string(),
            make_config(
                "Echo",
                "echo {message}",
                "low",
                vec![("message", "Msg", true)],
            ),
        );
        let provider = CustomToolProvider::from_config(&configs);

        let call = ToolCall::new("echo_tool").with_arg("message", "hello world; rm -rf /");
        let cmd = provider.build_command("echo {message}", &call);
        assert_eq!(cmd, "echo 'hello world; rm -rf /'");
    }

    #[test]
    fn test_build_command_multiple_params() {
        let mut configs = HashMap::new();
        configs.insert(
            "gh_issue".to_string(),
            make_config(
                "Create issue",
                "gh issue create --title {title} --body {body}",
                "high",
                vec![("title", "Title", true), ("body", "Body", true)],
            ),
        );
        let provider = CustomToolProvider::from_config(&configs);

        let call = ToolCall::new("gh_issue")
            .with_arg("title", "Bug fix")
            .with_arg("body", "Fixed the bug");
        let cmd = provider.build_command("gh issue create --title {title} --body {body}", &call);
        assert_eq!(
            cmd,
            "gh issue create --title 'Bug fix' --body 'Fixed the bug'"
        );
    }

    #[test]
    fn test_build_command_missing_optional() {
        let mut configs = HashMap::new();
        configs.insert(
            "tool".to_string(),
            make_config(
                "Tool",
                "cmd {required} {optional}",
                "low",
                vec![("required", "Req", true), ("optional", "Opt", false)],
            ),
        );
        let provider = CustomToolProvider::from_config(&configs);

        let call = ToolCall::new("tool").with_arg("required", "value");
        let cmd = provider.build_command("cmd {required} {optional}", &call);
        // Optional placeholder should be removed
        assert_eq!(cmd, "cmd value ");
    }

    #[tokio::test]
    async fn test_provider_is_available() {
        let configs = HashMap::new();
        let empty = CustomToolProvider::from_config(&configs);
        assert!(!empty.is_available().await);

        let mut configs = HashMap::new();
        configs.insert("t".to_string(), make_config("T", "echo hi", "low", vec![]));
        let with_tool = CustomToolProvider::from_config(&configs);
        assert!(with_tool.is_available().await);
    }

    #[tokio::test]
    async fn test_provider_discover_tools() {
        let mut configs = HashMap::new();
        configs.insert(
            "tool_a".to_string(),
            make_config("Tool A", "echo a", "low", vec![]),
        );
        configs.insert(
            "tool_b".to_string(),
            make_config("Tool B", "echo b", "high", vec![]),
        );

        let provider = CustomToolProvider::from_config(&configs);
        let tools = provider.discover_tools().await.unwrap();
        assert_eq!(tools.len(), 2);

        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[tokio::test]
    async fn test_provider_execute_echo() {
        let mut configs = HashMap::new();
        configs.insert(
            "echo_tool".to_string(),
            make_config(
                "Echo tool",
                "echo {message}",
                "low",
                vec![("message", "Message", true)],
            ),
        );

        let provider = CustomToolProvider::from_config(&configs);
        let call = ToolCall::new("echo_tool").with_arg("message", "hello");
        let result = provider.execute(&call).await;

        assert!(result.is_success());
        assert!(result.output().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_provider_execute_unknown_tool() {
        let configs = HashMap::new();
        let provider = CustomToolProvider::from_config(&configs);
        let call = ToolCall::new("nonexistent");
        let result = provider.execute(&call).await;

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "NOT_FOUND");
    }

    #[tokio::test]
    async fn test_provider_execute_missing_required_param() {
        let mut configs = HashMap::new();
        configs.insert(
            "tool".to_string(),
            make_config(
                "Tool",
                "echo {message}",
                "low",
                vec![("message", "Msg", true)],
            ),
        );

        let provider = CustomToolProvider::from_config(&configs);
        let call = ToolCall::new("tool"); // Missing 'message'
        let result = provider.execute(&call).await;

        assert!(!result.is_success());
        assert_eq!(result.error().unwrap().code, "INVALID_ARGUMENT");
    }

    #[tokio::test]
    async fn test_provider_priority() {
        let configs = HashMap::new();
        let provider = CustomToolProvider::from_config(&configs);
        assert_eq!(provider.priority(), CUSTOM_PRIORITY);
    }

    #[tokio::test]
    async fn test_to_api_tools_includes_custom() {
        let mut configs = HashMap::new();
        configs.insert(
            "my_custom_tool".to_string(),
            make_config(
                "My custom tool",
                "echo {msg}",
                "low",
                vec![("msg", "Message", true)],
            ),
        );

        let provider = CustomToolProvider::from_config(&configs);
        let tools = provider.discover_tools().await.unwrap();

        // Build a ToolSpec that includes the custom tool
        let mut spec = quorum_domain::tool::entities::ToolSpec::new();
        for tool in tools {
            spec = spec.register(tool);
        }

        let converter = crate::tools::JsonSchemaToolConverter;
        use quorum_application::ports::tool_schema::ToolSchemaPort;
        let api_tools = converter.all_tools_schema(&spec);
        assert_eq!(api_tools.len(), 1);
        assert_eq!(api_tools[0]["name"], "my_custom_tool");
        assert_eq!(api_tools[0]["description"], "My custom tool");
    }
}
