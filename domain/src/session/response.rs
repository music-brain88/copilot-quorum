//! LLM Response types for Native Tool Use API support.
//!
//! These types represent structured LLM responses that can contain both
//! text content and tool use requests, enabling the Native Tool Use API
//! workflow where the API enforces tool names and parameter schemas.
//!
//! # Two Response Paths
//!
//! ```text
//! PromptBased (legacy):  send() → String → parse_tool_calls()
//! Native Tool Use:       send_with_tools() → LlmResponse → tool_calls()
//! ```

use crate::tool::entities::ToolCall;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single block of content within an LLM response.
///
/// Native Tool Use APIs (Anthropic, OpenAI) return responses as an array
/// of content blocks, mixing text and tool use requests. This enum models
/// that structure.
///
/// # Examples
///
/// ```
/// use quorum_domain::session::response::ContentBlock;
///
/// let text = ContentBlock::Text("Let me read that file.".to_string());
/// assert!(text.as_text().is_some());
///
/// let tool = ContentBlock::ToolUse {
///     id: "toolu_abc123".to_string(),
///     name: "read_file".to_string(),
///     input: [("path".to_string(), serde_json::json!("/src/main.rs"))]
///         .into_iter().collect(),
/// };
/// assert!(tool.as_tool_use().is_some());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// A text content block from the model.
    Text(String),

    /// A tool use request from the model.
    ///
    /// The API assigns the `id`, enforces `name` against the provided tool
    /// definitions, and validates `input` against the JSON schema.
    ToolUse {
        /// API-assigned ID for correlating with tool results (e.g. "toolu_abc123").
        id: String,
        /// Canonical tool name, guaranteed valid by the API.
        name: String,
        /// Structured arguments validated against the tool's JSON schema.
        input: HashMap<String, serde_json::Value>,
    },
}

impl ContentBlock {
    /// Returns the text content if this is a `Text` block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Returns `(id, name, input)` if this is a `ToolUse` block.
    pub fn as_tool_use(&self) -> Option<(&str, &str, &HashMap<String, serde_json::Value>)> {
        match self {
            ContentBlock::ToolUse { id, name, input } => Some((id, name, input)),
            _ => None,
        }
    }
}

/// Reason the model stopped generating.
///
/// This is critical for the multi-turn tool use loop: when `stop_reason`
/// is `ToolUse`, the caller must execute the requested tools and send
/// results back via `send_tool_results()`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// Natural end of response — the model is done.
    EndTurn,
    /// The model wants to call tools — execute them and return results.
    ToolUse,
    /// Hit the token limit — response may be truncated.
    MaxTokens,
    /// Provider-specific stop reason.
    Other(String),
}

/// A structured response from an LLM, supporting both text and tool use.
///
/// `LlmResponse` is the primary return type for Native Tool Use API calls.
/// It replaces the raw `String` return of the prompt-based path.
///
/// # Helper Methods
///
/// - [`text_content()`](Self::text_content) — concatenate all text blocks
/// - [`tool_calls()`](Self::tool_calls) — extract tool use blocks as `Vec<ToolCall>`
/// - [`has_tool_calls()`](Self::has_tool_calls) — quick check for tool use
/// - [`from_text()`](Self::from_text) — wrap a plain string (fallback compatibility)
///
/// # Examples
///
/// ```
/// use quorum_domain::session::response::{LlmResponse, ContentBlock, StopReason};
///
/// // Fallback: wrap a plain text response
/// let response = LlmResponse::from_text("Hello!");
/// assert_eq!(response.text_content(), "Hello!");
/// assert!(!response.has_tool_calls());
///
/// // Native: response with tool use
/// let response = LlmResponse {
///     content: vec![
///         ContentBlock::Text("Reading file...".to_string()),
///         ContentBlock::ToolUse {
///             id: "toolu_1".to_string(),
///             name: "read_file".to_string(),
///             input: [("path".to_string(), serde_json::json!("/README.md"))]
///                 .into_iter().collect(),
///         },
///     ],
///     stop_reason: Some(StopReason::ToolUse),
///     model: Some("claude-sonnet-4-5-20250929".to_string()),
/// };
/// assert!(response.has_tool_calls());
/// assert_eq!(response.tool_calls().len(), 1);
/// assert_eq!(response.text_content(), "Reading file...");
/// ```
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// Content blocks in the response (text and/or tool use).
    pub content: Vec<ContentBlock>,
    /// Why the model stopped generating.
    pub stop_reason: Option<StopReason>,
    /// Model identifier (if returned by the API).
    pub model: Option<String>,
}

impl LlmResponse {
    /// Create a text-only response (for fallback / prompt-based path).
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text(text.into())],
            stop_reason: Some(StopReason::EndTurn),
            model: None,
        }
    }

    /// Concatenate all `Text` content blocks into a single string.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }

    /// Extract all `ToolUse` content blocks as `Vec<ToolCall>`.
    ///
    /// Each `ToolUse` block is converted to a `ToolCall` with the
    /// `native_id` field set to the API-assigned ID.
    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => {
                    Some(ToolCall::from_native(id, name, input.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Returns `true` if the response contains any tool use requests.
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_text_creates_text_only_response() {
        let response = LlmResponse::from_text("Hello, world!");
        assert_eq!(response.text_content(), "Hello, world!");
        assert!(!response.has_tool_calls());
        assert!(response.tool_calls().is_empty());
        assert_eq!(response.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(response.model, None);
    }

    #[test]
    fn tool_calls_extraction() {
        let response = LlmResponse {
            content: vec![
                ContentBlock::Text("Let me read that file.".to_string()),
                ContentBlock::ToolUse {
                    id: "toolu_abc123".to_string(),
                    name: "read_file".to_string(),
                    input: [("path".to_string(), serde_json::json!("/src/main.rs"))]
                        .into_iter()
                        .collect(),
                },
                ContentBlock::Text(" And also search.".to_string()),
                ContentBlock::ToolUse {
                    id: "toolu_def456".to_string(),
                    name: "grep_search".to_string(),
                    input: [
                        ("pattern".to_string(), serde_json::json!("TODO")),
                        ("path".to_string(), serde_json::json!("/src")),
                    ]
                    .into_iter()
                    .collect(),
                },
            ],
            stop_reason: Some(StopReason::ToolUse),
            model: Some("claude-sonnet-4-5-20250929".to_string()),
        };

        assert!(response.has_tool_calls());
        assert_eq!(
            response.text_content(),
            "Let me read that file. And also search."
        );

        let calls = response.tool_calls();
        assert_eq!(calls.len(), 2);

        assert_eq!(calls[0].tool_name, "read_file");
        assert_eq!(calls[0].native_id, Some("toolu_abc123".to_string()));
        assert_eq!(calls[0].get_string("path"), Some("/src/main.rs"));

        assert_eq!(calls[1].tool_name, "grep_search");
        assert_eq!(calls[1].native_id, Some("toolu_def456".to_string()));
        assert_eq!(calls[1].get_string("pattern"), Some("TODO"));
    }

    #[test]
    fn empty_response() {
        let response = LlmResponse {
            content: vec![],
            stop_reason: None,
            model: None,
        };

        assert_eq!(response.text_content(), "");
        assert!(!response.has_tool_calls());
        assert!(response.tool_calls().is_empty());
    }

    #[test]
    fn content_block_accessors() {
        let text = ContentBlock::Text("hello".to_string());
        assert_eq!(text.as_text(), Some("hello"));
        assert!(text.as_tool_use().is_none());

        let tool = ContentBlock::ToolUse {
            id: "id1".to_string(),
            name: "read_file".to_string(),
            input: HashMap::new(),
        };
        assert!(tool.as_text().is_none());
        let (id, name, input) = tool.as_tool_use().unwrap();
        assert_eq!(id, "id1");
        assert_eq!(name, "read_file");
        assert!(input.is_empty());
    }

    #[test]
    fn stop_reason_equality() {
        assert_eq!(StopReason::EndTurn, StopReason::EndTurn);
        assert_eq!(StopReason::ToolUse, StopReason::ToolUse);
        assert_eq!(StopReason::MaxTokens, StopReason::MaxTokens);
        assert_ne!(StopReason::EndTurn, StopReason::ToolUse);
        assert_eq!(
            StopReason::Other("custom".to_string()),
            StopReason::Other("custom".to_string())
        );
    }
}
