//! Shared helpers for tool use cases.

use quorum_domain::tool::entities::ToolCall;

/// Extract a short preview string from tool call arguments.
///
/// Looks for well-known keys (`path`, `command`, `pattern`, `query`, `url`)
/// first, then falls back to the first string value found.
pub(crate) fn tool_args_preview(call: &ToolCall) -> String {
    let keys = ["path", "command", "pattern", "query", "url"];
    for key in &keys {
        if let Some(serde_json::Value::String(s)) = call.arguments.get(*key) {
            return truncate_preview(s, 50);
        }
    }
    // Fallback: first string value
    for value in call.arguments.values() {
        if let Some(s) = value.as_str() {
            return truncate_preview(s, 50);
        }
    }
    String::new()
}

fn truncate_preview(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{}…", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_call(args: HashMap<String, serde_json::Value>) -> ToolCall {
        ToolCall {
            tool_name: "test_tool".to_string(),
            arguments: args,
            reasoning: None,
            native_id: None,
        }
    }

    #[test]
    fn test_path_key_preferred() {
        let mut args = HashMap::new();
        args.insert(
            "path".to_string(),
            serde_json::Value::String("src/main.rs".to_string()),
        );
        args.insert(
            "other".to_string(),
            serde_json::Value::String("ignored".to_string()),
        );
        assert_eq!(tool_args_preview(&make_call(args)), "src/main.rs");
    }

    #[test]
    fn test_command_key() {
        let mut args = HashMap::new();
        args.insert(
            "command".to_string(),
            serde_json::Value::String("cargo test".to_string()),
        );
        assert_eq!(tool_args_preview(&make_call(args)), "cargo test");
    }

    #[test]
    fn test_pattern_key() {
        let mut args = HashMap::new();
        args.insert(
            "pattern".to_string(),
            serde_json::Value::String("*.rs".to_string()),
        );
        assert_eq!(tool_args_preview(&make_call(args)), "*.rs");
    }

    #[test]
    fn test_query_key() {
        let mut args = HashMap::new();
        args.insert(
            "query".to_string(),
            serde_json::Value::String("search term".to_string()),
        );
        assert_eq!(tool_args_preview(&make_call(args)), "search term");
    }

    #[test]
    fn test_url_key() {
        let mut args = HashMap::new();
        args.insert(
            "url".to_string(),
            serde_json::Value::String("https://example.com".to_string()),
        );
        assert_eq!(tool_args_preview(&make_call(args)), "https://example.com");
    }

    #[test]
    fn test_fallback_to_first_string() {
        let mut args = HashMap::new();
        args.insert(
            "foo".to_string(),
            serde_json::Value::String("bar".to_string()),
        );
        assert_eq!(tool_args_preview(&make_call(args)), "bar");
    }

    #[test]
    fn test_empty_args() {
        let args = HashMap::new();
        assert_eq!(tool_args_preview(&make_call(args)), "");
    }

    #[test]
    fn test_no_string_values() {
        let mut args = HashMap::new();
        args.insert("count".to_string(), serde_json::Value::Number(42.into()));
        assert_eq!(tool_args_preview(&make_call(args)), "");
    }

    #[test]
    fn test_truncation() {
        let mut args = HashMap::new();
        let long_path = "a".repeat(100);
        args.insert("path".to_string(), serde_json::Value::String(long_path));
        let result = tool_args_preview(&make_call(args));
        assert!(result.chars().count() <= 50);
        assert!(result.ends_with('…'));
    }
}
