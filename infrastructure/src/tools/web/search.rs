//! `web_search` tool — search the web using DuckDuckGo Instant Answer API.
//!
//! Part of the **Web Tools** feature (`web-tools`), this tool provides agents
//! with a zero-configuration web search capability.
//!
//! # DuckDuckGo Instant Answer API
//!
//! Uses the [DuckDuckGo Instant Answer API](https://api.duckduckgo.com/) which:
//! - Requires **no API key** (no configuration burden)
//! - Returns instant answers, abstracts, definitions, and related topics
//! - Does not return full web search result listings (by design)
//!
//! For full search results, the agent can use `web_fetch` on specific URLs
//! discovered through the instant answer.
//!
//! # Output Format
//!
//! Results are formatted as markdown with sections:
//! - **Summary** — abstract text with source attribution
//! - **Instant Answer** — direct factual answers
//! - **Definition** — dictionary-style definitions
//! - **Related Topics** — up to 10 related links
//! - **Redirect** — for !bang-style queries
//!
//! # Parameters
//!
//! | Name | Type | Required | Description |
//! |------|------|:---:|-------------|
//! | `query` | string | Yes | The search query |
//!
//! # Safety
//!
//! - **Risk level**: [`Low`](quorum_domain::tool::entities::RiskLevel::Low) — read-only
//! - **Timeout**: 30 seconds (from shared `reqwest::Client`)
//!
//! # Aliases
//!
//! `web` → `web_search` (registered in [`default_tool_spec()`](super::super::default_tool_spec))

use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use std::time::Instant;

/// Canonical tool name for the web search tool.
pub const WEB_SEARCH: &str = "web_search";

/// DuckDuckGo Instant Answer API endpoint (no API key required).
const DDG_API_URL: &str = "https://api.duckduckgo.com/";

/// Create the [`ToolDefinition`] for `web_search`.
///
/// Registered in [`default_tool_spec()`](super::super::default_tool_spec) and
/// [`read_only_tool_spec()`](super::super::read_only_tool_spec) when the
/// `web-tools` feature is enabled.
pub fn web_search_definition() -> ToolDefinition {
    ToolDefinition::new(
        WEB_SEARCH,
        "Search the web using DuckDuckGo. Returns instant answers, abstracts, and related topics.",
        RiskLevel::Low,
    )
    .with_parameter(ToolParameter::new("query", "The search query", true).with_type("string"))
}

/// Execute the `web_search` tool — query DuckDuckGo and format results.
///
/// Called by [`LocalToolExecutor::execute_async()`](super::super::LocalToolExecutor)
/// when the agent invokes `web_search` (or its alias `web`).
pub async fn execute_web_search(client: &reqwest::Client, call: &ToolCall) -> ToolResult {
    let start = Instant::now();

    let query = match call.require_string("query") {
        Ok(q) => q,
        Err(e) => {
            return ToolResult::failure(WEB_SEARCH, ToolError::invalid_argument(e));
        }
    };

    // Call DuckDuckGo Instant Answer API
    let response = match client
        .get(DDG_API_URL)
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
        ])
        .header("User-Agent", "CopilotQuorum/0.1 (Agent Tool)")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return ToolResult::failure(
                WEB_SEARCH,
                ToolError::execution_failed(format!("Search request failed: {}", e)),
            );
        }
    };

    if !response.status().is_success() {
        return ToolResult::failure(
            WEB_SEARCH,
            ToolError::execution_failed(format!(
                "Search API returned error: {}",
                response.status()
            )),
        );
    }

    let body: serde_json::Value = match response.json().await {
        Ok(j) => j,
        Err(e) => {
            return ToolResult::failure(
                WEB_SEARCH,
                ToolError::execution_failed(format!("Failed to parse search results: {}", e)),
            );
        }
    };

    let output = format_search_results(query, &body);
    let elapsed = start.elapsed();

    let mut result = ToolResult::success(WEB_SEARCH, output);
    result.metadata = ToolResultMetadata {
        duration_ms: Some(elapsed.as_millis() as u64),
        ..Default::default()
    };
    result
}

/// Format DuckDuckGo API response into a readable markdown document.
///
/// Extracts and formats: AbstractText, Answer, Definition, RelatedTopics (up to 10),
/// and Redirect sections. Falls back to a "no instant answer" message if no
/// sections are populated.
fn format_search_results(query: &str, data: &serde_json::Value) -> String {
    let mut sections: Vec<String> = Vec::new();

    sections.push(format!("## Search Results for: {}", query));

    // Abstract (main answer)
    if let Some(abstract_text) = data["AbstractText"].as_str()
        && !abstract_text.is_empty()
    {
        let source = data["AbstractSource"].as_str().unwrap_or("Unknown");
        let url = data["AbstractURL"].as_str().unwrap_or("");
        sections.push(format!(
            "### Summary ({})\n{}\nSource: {}",
            source, abstract_text, url
        ));
    }

    // Answer (instant answer)
    if let Some(answer) = data["Answer"].as_str()
        && !answer.is_empty()
    {
        sections.push(format!("### Instant Answer\n{}", answer));
    }

    // Definition
    if let Some(definition) = data["Definition"].as_str()
        && !definition.is_empty()
    {
        let source = data["DefinitionSource"].as_str().unwrap_or("Unknown");
        sections.push(format!("### Definition ({})\n{}", source, definition));
    }

    // Related Topics
    if let Some(topics) = data["RelatedTopics"].as_array() {
        let topic_texts: Vec<String> = topics
            .iter()
            .filter_map(|t| {
                if let Some(text) = t["Text"].as_str() {
                    let url = t["FirstURL"].as_str().unwrap_or("");
                    if !text.is_empty() {
                        Some(format!("- {} ({})", text, url))
                    } else {
                        None
                    }
                } else {
                    // Nested topic group
                    None
                }
            })
            .take(10)
            .collect();

        if !topic_texts.is_empty() {
            sections.push(format!("### Related Topics\n{}", topic_texts.join("\n")));
        }
    }

    // Redirect (for !bang queries or similar)
    if let Some(redirect) = data["Redirect"].as_str()
        && !redirect.is_empty()
    {
        sections.push(format!("### Redirect\n{}", redirect));
    }

    if sections.len() == 1 {
        // Only the header, no results found
        sections.push(
            "No instant answer available. Try using `web_fetch` to visit a specific URL for more detailed information.".to_string(),
        );
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_search_results_with_abstract() {
        let data = serde_json::json!({
            "AbstractText": "Rust is a systems programming language.",
            "AbstractSource": "Wikipedia",
            "AbstractURL": "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            "Answer": "",
            "Definition": "",
            "RelatedTopics": [],
            "Redirect": ""
        });

        let output = format_search_results("Rust programming", &data);
        assert!(output.contains("Rust programming"));
        assert!(output.contains("systems programming language"));
        assert!(output.contains("Wikipedia"));
    }

    #[test]
    fn test_format_search_results_empty() {
        let data = serde_json::json!({
            "AbstractText": "",
            "Answer": "",
            "Definition": "",
            "RelatedTopics": [],
            "Redirect": ""
        });

        let output = format_search_results("obscure query", &data);
        assert!(output.contains("No instant answer available"));
    }

    #[test]
    fn test_format_search_results_with_related_topics() {
        let data = serde_json::json!({
            "AbstractText": "",
            "Answer": "",
            "Definition": "",
            "RelatedTopics": [
                {
                    "Text": "Topic 1 description",
                    "FirstURL": "https://example.com/1"
                },
                {
                    "Text": "Topic 2 description",
                    "FirstURL": "https://example.com/2"
                }
            ],
            "Redirect": ""
        });

        let output = format_search_results("test", &data);
        assert!(output.contains("Related Topics"));
        assert!(output.contains("Topic 1 description"));
        assert!(output.contains("Topic 2 description"));
    }
}
