//! Tool-call leak detection
//!
//! LLMs sometimes emit a tool invocation as *plain JSON text* instead of
//! calling the tool through the Native Tool Use API (#268). When that text
//! is treated as a normal answer, raw internal JSON leaks into the final
//! response shown to the user.
//!
//! [`looks_like_tool_call_json`] is a pure domain heuristic that detects
//! such responses so the application layer can retry (nudge) or suppress
//! them instead of surfacing them verbatim.

use super::entities::ToolSpec;

/// Keys an LLM typically uses when writing a tool call as a JSON envelope,
/// e.g. `{"tool": "run_command", "args": {...}}`.
const TOOL_NAME_KEYS: &[&str] = &["tool", "tool_name", "name"];

/// Check whether a text response is a *raw tool call written as JSON*
/// rather than a natural-language answer.
///
/// The check is intentionally conservative: it only fires when the **entire**
/// response (after stripping an optional Markdown code fence) parses as a
/// single JSON object that matches one of two shapes:
///
/// 1. **Envelope shape** — the object names a registered tool via a
///    `tool` / `tool_name` / `name` key:
///    `{"tool": "run_command", "args": {"command": "ls"}}`
/// 2. **Bare-arguments shape** — every top-level key matches a parameter of
///    some registered tool, and all of that tool's required parameters are
///    present: `{"command": "cargo metadata --no-deps"}`
///
/// Answers that merely *contain* JSON alongside prose are never flagged.
///
/// # Examples
///
/// ```
/// use quorum_domain::tool::entities::{ToolSpec, ToolDefinition, ToolParameter, RiskLevel};
/// use quorum_domain::tool::detection::looks_like_tool_call_json;
///
/// let spec = ToolSpec::new().register(
///     ToolDefinition::new("run_command", "Run a shell command", RiskLevel::High)
///         .with_parameter(ToolParameter::new("command", "Command to run", true)),
/// );
///
/// assert!(looks_like_tool_call_json("{\"command\": \"ls -la\"}", &spec));
/// assert!(looks_like_tool_call_json("```json\n{\"command\": \"ls\"}\n```", &spec));
/// assert!(!looks_like_tool_call_json("The workspace has 5 crates.", &spec));
/// ```
pub fn looks_like_tool_call_json(text: &str, spec: &ToolSpec) -> bool {
    let Some(candidate) = extract_json_candidate(text) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(candidate) else {
        return false;
    };
    let Some(obj) = value.as_object() else {
        return false;
    };
    if obj.is_empty() {
        return false;
    }

    // Shape 1: envelope with an explicit tool name key
    for key in TOOL_NAME_KEYS {
        if let Some(tool_name) = obj.get(*key).and_then(|v| v.as_str())
            && spec.get(tool_name).is_some()
        {
            return true;
        }
    }

    // Shape 2: bare arguments object matching a registered tool's parameters
    spec.all().any(|tool| {
        !tool.parameters.is_empty()
            && obj
                .keys()
                .all(|k| tool.parameters.iter().any(|p| p.name == *k))
            && tool
                .parameters
                .iter()
                .filter(|p| p.required)
                .all(|p| obj.contains_key(&p.name))
    })
}

/// Extract the JSON body if the whole text is a JSON object, optionally
/// wrapped in a Markdown code fence (```json ... ``` or ``` ... ```).
///
/// Returns `None` when the text has prose outside the JSON/fence.
fn extract_json_candidate(text: &str) -> Option<&str> {
    let trimmed = text.trim();

    if let Some(rest) = trimmed.strip_prefix("```") {
        // Drop the fence header line (e.g. "json") and require a closing fence
        let after_header = rest.split_once('\n')?.1;
        let body = after_header.strip_suffix("```")?.trim();
        if body.starts_with('{') && body.ends_with('}') {
            return Some(body);
        }
        return None;
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Some(trimmed);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::entities::{RiskLevel, ToolDefinition, ToolParameter};

    fn test_spec() -> ToolSpec {
        ToolSpec::new()
            .register(
                ToolDefinition::new("run_command", "Run a shell command", RiskLevel::High)
                    .with_parameter(ToolParameter::new("command", "Command to run", true)),
            )
            .register(
                ToolDefinition::new("read_file", "Read a file", RiskLevel::Low)
                    .with_parameter(ToolParameter::new("path", "File path", true))
                    .with_parameter(ToolParameter::new("max_bytes", "Byte limit", false)),
            )
    }

    #[test]
    fn detects_bare_arguments_object() {
        let spec = test_spec();
        assert!(looks_like_tool_call_json(
            r#"{"command": "cargo metadata --no-deps --format-version 1"}"#,
            &spec
        ));
        assert!(looks_like_tool_call_json(
            r#"{"path": "src/main.rs"}"#,
            &spec
        ));
    }

    #[test]
    fn detects_fenced_json_block() {
        // Reproduction case from #268
        let spec = test_spec();
        let text = "```json\n{\n  \"command\": \"cargo metadata --no-deps --format-version 1 | jq -r '.packages'\"\n}\n```";
        assert!(looks_like_tool_call_json(text, &spec));
    }

    #[test]
    fn detects_plain_fence_without_language_tag() {
        let spec = test_spec();
        let text = "```\n{\"command\": \"ls\"}\n```";
        assert!(looks_like_tool_call_json(text, &spec));
    }

    #[test]
    fn detects_envelope_shape() {
        let spec = test_spec();
        assert!(looks_like_tool_call_json(
            r#"{"tool": "run_command", "args": {"command": "ls"}}"#,
            &spec
        ));
        assert!(looks_like_tool_call_json(
            r#"{"tool_name": "read_file", "arguments": {"path": "a.rs"}}"#,
            &spec
        ));
        assert!(looks_like_tool_call_json(
            r#"{"name": "run_command", "input": {"command": "pwd"}}"#,
            &spec
        ));
    }

    #[test]
    fn ignores_envelope_with_unknown_tool() {
        let spec = test_spec();
        assert!(!looks_like_tool_call_json(
            r#"{"tool": "no_such_tool", "args": {}}"#,
            &spec
        ));
        // "name" key with a non-tool value is a common data shape, not a call
        assert!(!looks_like_tool_call_json(
            r#"{"name": "copilot-quorum", "version": "0.1.0"}"#,
            &spec
        ));
    }

    #[test]
    fn ignores_natural_language_answers() {
        let spec = test_spec();
        assert!(!looks_like_tool_call_json(
            "The workspace contains 5 crates: domain, application, ...",
            &spec
        ));
        assert!(!looks_like_tool_call_json("", &spec));
    }

    #[test]
    fn ignores_json_embedded_in_prose() {
        let spec = test_spec();
        let text = "Here is the members list:\n```json\n{\"command\": \"ls\"}\n```\nas requested.";
        assert!(!looks_like_tool_call_json(text, &spec));
    }

    #[test]
    fn ignores_json_not_matching_any_tool() {
        let spec = test_spec();
        // Keys don't match any registered tool's parameters
        assert!(!looks_like_tool_call_json(
            r#"{"crates": ["quorum-domain", "quorum-application"]}"#,
            &spec
        ));
        // Partial match but missing required parameter
        assert!(!looks_like_tool_call_json(r#"{"max_bytes": 100}"#, &spec));
    }

    #[test]
    fn ignores_empty_object_and_arrays() {
        let spec = test_spec();
        assert!(!looks_like_tool_call_json("{}", &spec));
        assert!(!looks_like_tool_call_json(r#"[{"command": "ls"}]"#, &spec));
    }

    #[test]
    fn ignores_unclosed_fence() {
        let spec = test_spec();
        assert!(!looks_like_tool_call_json(
            "```json\n{\"command\": \"ls\"}",
            &spec
        ));
    }
}
