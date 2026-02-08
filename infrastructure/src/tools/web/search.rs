//! web_search tool: Search the web using DuckDuckGo Instant Answer API

use quorum_domain::tool::{
    entities::{RiskLevel, ToolCall, ToolDefinition, ToolParameter},
    value_objects::{ToolError, ToolResult, ToolResultMetadata},
};
use std::time::Instant;

/// Tool name constant
pub const WEB_SEARCH: &str = "web_search";

/// DuckDuckGo Instant Answer API endpoint
const DDG_API_URL: &str = "https://api.duckduckgo.com/";

/// Get the tool definition for web_search
pub fn web_search_definition() -> ToolDefinition {
    ToolDefinition::new(
        WEB_SEARCH,
        "Search the web using DuckDuckGo. Returns instant answers, abstracts, and related topics.",
        RiskLevel::Low,
    )
    .with_parameter(
        ToolParameter::new("query", "The search query", true).with_type("string"),
    )
}

/// Execute the web_search tool
pub async fn execute_web_search(
    client: &reqwest::Client,
    call: &ToolCall,
) -> ToolResult {
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

/// Format DuckDuckGo API response into readable output
fn format_search_results(query: &str, data: &serde_json::Value) -> String {
    let mut sections: Vec<String> = Vec::new();

    sections.push(format!("## Search Results for: {}", query));

    // Abstract (main answer)
    if let Some(abstract_text) = data["AbstractText"].as_str() {
        if !abstract_text.is_empty() {
            let source = data["AbstractSource"]
                .as_str()
                .unwrap_or("Unknown");
            let url = data["AbstractURL"].as_str().unwrap_or("");
            sections.push(format!(
                "### Summary ({})\n{}\nSource: {}",
                source, abstract_text, url
            ));
        }
    }

    // Answer (instant answer)
    if let Some(answer) = data["Answer"].as_str() {
        if !answer.is_empty() {
            sections.push(format!("### Instant Answer\n{}", answer));
        }
    }

    // Definition
    if let Some(definition) = data["Definition"].as_str() {
        if !definition.is_empty() {
            let source = data["DefinitionSource"]
                .as_str()
                .unwrap_or("Unknown");
            sections.push(format!("### Definition ({})\n{}", source, definition));
        }
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
            sections.push(format!(
                "### Related Topics\n{}",
                topic_texts.join("\n")
            ));
        }
    }

    // Redirect (for !bang queries or similar)
    if let Some(redirect) = data["Redirect"].as_str() {
        if !redirect.is_empty() {
            sections.push(format!("### Redirect\n{}", redirect));
        }
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
